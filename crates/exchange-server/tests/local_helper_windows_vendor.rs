#![cfg(windows)]

#[path = "../src/local_helper.rs"]
#[allow(dead_code)]
mod local_helper;
#[path = "../src/local_helper_plan.rs"]
mod local_helper_plan;
#[path = "../src/local_helper_windows.rs"]
mod local_helper_windows;

use std::ffi::OsString;
use std::io::Read as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr::null_mut;
use std::time::Instant;

use local_helper::{
    parse_local_helper, HelperDeadlineSchedule, HelperExit, HelperPlatform, LocalHelperInvocation,
    VendorSecretCapabilities,
};
use local_helper_windows::{
    blocking_read_before_for_test, read_console_secret_with, ConsolePort, VendorCeremony,
    VendorPreparation, VendorRequest, WindowsHelperError,
};
use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::WriteFile;
use windows_sys::Win32::System::Pipes::CreatePipe;

fn inheritable_pipe() -> (OwnedHandle, OwnedHandle) {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read = null_mut();
    let mut write = null_mut();
    assert_ne!(
        unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
        0
    );
    // SAFETY: CreatePipe returned two newly owned handles.
    unsafe {
        (
            OwnedHandle::from_raw_handle(read.cast()),
            OwnedHandle::from_raw_handle(write.cast()),
        )
    }
}

fn invocation(request: &OwnedHandle, response: &OwnedHandle) -> LocalHelperInvocation {
    parse_local_helper(
        HelperPlatform::Windows,
        &[
            OsString::from("local"),
            OsString::from("vendor-secret"),
            OsString::from("--request-handle"),
            OsString::from((request.as_raw_handle() as usize).to_string()),
            OsString::from("--response-handle"),
            OsString::from((response.as_raw_handle() as usize).to_string()),
        ],
    )
    .expect("closed Windows vendor grammar")
}

fn frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12 + payload.len());
    bytes.extend_from_slice(b"FXLM");
    bytes.extend_from_slice(&[1, direction]);
    bytes.extend_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn write_all(handle: &OwnedHandle, bytes: &[u8]) {
    let mut written = 0;
    while written < bytes.len() {
        let mut count = 0;
        assert_ne!(
            unsafe {
                WriteFile(
                    handle.as_raw_handle() as HANDLE,
                    bytes[written..].as_ptr(),
                    (bytes.len() - written) as u32,
                    &mut count,
                    null_mut(),
                )
            },
            0
        );
        written += count as usize;
    }
}

struct RecordingCeremony {
    request: Vec<u8>,
    terminal: Vec<u8>,
}

impl VendorCeremony for RecordingCeremony {
    type Error = ();
    type Session = ();

    fn prepare(
        &mut self,
        request: &VendorRequest,
        deadlines: HelperDeadlineSchedule,
    ) -> Result<VendorPreparation<Self::Session>, Self::Error> {
        assert!(Instant::now() < deadlines.setup_by());
        self.request.extend_from_slice(request.bytes());
        Ok(VendorPreparation::Ready(()))
    }

    fn exchange(
        &mut self,
        _session: Self::Session,
        _input: &mut local_helper_windows::PrivateConsole,
    ) -> Result<Vec<u8>, Self::Error> {
        Ok(self.terminal.clone())
    }
}

#[test]
fn vendor_seam_consumes_only_the_two_directed_pipes_and_writes_one_terminal_frame() {
    let (request_read, request_write) = inheritable_pipe();
    let (response_read, response_write) = inheritable_pipe();
    let (_canary_read, canary_write) = inheritable_pipe();
    let request = frame(1, 0x0001, br#"{"connector":"fixture"}"#);
    let terminal = frame(2, 0x0006, br#"{}"#);
    write_all(&request_write, &request);
    drop(request_write);

    let LocalHelperInvocation::VendorSecret(VendorSecretCapabilities::Windows {
        request: parsed_request,
        response: parsed_response,
    }) = invocation(&request_read, &response_write)
    else {
        panic!("vendor invocation");
    };
    // The production seam takes ownership of exactly these parsed handles.
    std::mem::forget(request_read);
    std::mem::forget(response_write);
    let mut ceremony = RecordingCeremony {
        request: Vec::new(),
        terminal: terminal.clone(),
    };
    assert!(
        local_helper_windows::run_vendor(parsed_request, parsed_response, &mut ceremony)
            == HelperExit::TerminalFrameWritten
    );
    assert_eq!(ceremony.request, request);

    let mut result = Vec::new();
    std::fs::File::from(response_read)
        .read_to_end(&mut result)
        .expect("terminal frame plus EOF");
    assert_eq!(result, terminal);

    let mut canary_flags = 0;
    assert_ne!(
        unsafe { GetHandleInformation(canary_write.as_raw_handle() as HANDLE, &mut canary_flags,) },
        0
    );
    assert_ne!(canary_flags & HANDLE_FLAG_INHERIT, 0);
}

#[test]
fn production_run_enters_the_native_vendor_adapter_and_refuses_an_invalid_begin() {
    let (request_read, request_write) = inheritable_pipe();
    let (response_read, response_write) = inheritable_pipe();
    write_all(&request_write, &frame(1, 0x0001, b"{}"));
    drop(request_write);

    let invocation = invocation(&request_read, &response_write);
    // The closed grammar transfers exactly these two capabilities to production `run`.
    std::mem::forget(request_read);
    std::mem::forget(response_write);
    assert!(
        local_helper_windows::run(invocation) == HelperExit::TerminalFrameWritten,
        "production run writes one value-free refusal"
    );

    let mut result = Vec::new();
    std::fs::File::from(response_read)
        .read_to_end(&mut result)
        .expect("terminal refusal plus EOF");
    assert_eq!(
        result,
        frame(
            2,
            0x7fff,
            br#"{"code":"local_management_unavailable","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":503}"#,
        )
    );
}

struct FakeConsole {
    calls: Vec<String>,
    original_mode: u32,
    line: Result<Vec<u16>, WindowsHelperError>,
}

impl ConsolePort for FakeConsole {
    type Input = ();

    fn open_current_input(&mut self) -> Result<Self::Input, WindowsHelperError> {
        self.calls.push("open-CONIN$".into());
        Ok(())
    }

    fn mode(&mut self, _input: &Self::Input) -> Result<u32, WindowsHelperError> {
        self.calls.push("get-mode".into());
        Ok(self.original_mode)
    }

    fn set_mode(&mut self, _input: &Self::Input, mode: u32) -> Result<(), WindowsHelperError> {
        self.calls.push(format!("set-mode-{mode}"));
        Ok(())
    }

    fn read_line(
        &mut self,
        _input: &Self::Input,
        maximum_utf16_units: usize,
        _deadline: Instant,
    ) -> Result<Vec<u16>, WindowsHelperError> {
        self.calls.push(format!("read-{maximum_utf16_units}"));
        self.line.clone()
    }
}

#[test]
fn private_input_opens_current_console_disables_echo_and_restores_it() {
    let mut console = FakeConsole {
        calls: Vec::new(),
        original_mode: 0x0007,
        line: Ok("sëcret\r\n".encode_utf16().collect()),
    };
    let secret = read_console_secret_with(
        &mut console,
        Instant::now() + std::time::Duration::from_secs(1),
    )
    .expect("private console secret");
    assert_eq!(secret.bytes(), "sëcret".as_bytes());
    assert_eq!(
        console.calls,
        [
            "open-CONIN$",
            "get-mode",
            "set-mode-3",
            "read-8194",
            "set-mode-7"
        ]
    );
}

#[test]
fn private_input_restores_echo_after_console_read_failure() {
    let mut console = FakeConsole {
        calls: Vec::new(),
        original_mode: 0x0007,
        line: Err(WindowsHelperError::Console),
    };
    assert!(read_console_secret_with(
        &mut console,
        Instant::now() + std::time::Duration::from_secs(1)
    )
    .is_err());
    assert_eq!(console.calls.last().map(String::as_str), Some("set-mode-7"));
}

#[test]
fn blocked_console_read_is_cancelled_at_the_unchanged_outer_deadline() {
    let (read, _held_write) = inheritable_pipe();
    let started = Instant::now();
    assert!(matches!(
        blocking_read_before_for_test(read, started + std::time::Duration::from_millis(25)),
        Err(WindowsHelperError::Deadline)
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}
