#![cfg(windows)]

#[path = "../src/local_helper.rs"]
#[allow(dead_code)]
mod local_helper;
#[path = "../src/local_helper_plan.rs"]
mod local_helper_plan;
#[path = "../src/local_helper_windows.rs"]
mod local_helper_windows;

use std::io::Read as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use local_helper::{HelperDeadlineSchedule, HelperExit};
use local_helper_windows::{
    blocking_read_before_for_test, finish_response_before_for_test, WindowsHelperError,
};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::WriteFile;
use windows_sys::Win32::System::Pipes::CreatePipe;

#[test]
fn supervised_windows_helper_outer_deadline_is_exact() {
    let (blocked_read, _held_write) = inheritable_pipe();
    let started = Instant::now();
    let schedule = HelperDeadlineSchedule::from_request_eof_with_budgets(
        started,
        Duration::from_millis(25),
        Duration::from_millis(25),
    )
    .expect("test helper deadlines");
    assert!(matches!(
        blocking_read_before_for_test(blocked_read, schedule.result_by()),
        Err(WindowsHelperError::Deadline)
    ));
    assert!(started.elapsed() < Duration::from_secs(1));

    for terminal in [
        frame(2, 0x0006, &vec![b'x'; 65_536]),
        frame(
            2,
            0x7fff,
            br#"{"code":"local_management_unavailable","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":503}"#,
        ),
    ] {
        let (read, write) = inheritable_pipe();
        fill_pipe(&write);
        let started = Instant::now();
        let schedule = HelperDeadlineSchedule::from_request_eof_with_budgets(
            started,
            Duration::from_millis(25),
            Duration::from_millis(25),
        )
        .expect("test helper deadlines");
        assert!(
            finish_response_before_for_test(write, terminal, schedule.result_by())
                == HelperExit::CapabilityOrTransportFailure,
            "a blocked terminal cannot claim frame plus EOF"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        let mut drained = Vec::new();
        std::fs::File::from(read)
            .read_to_end(&mut drained)
            .expect("cancelled writer closes for EOF");
        assert_eq!(drained, vec![0x5a; 4_096]);
    }
}

fn inheritable_pipe() -> (OwnedHandle, OwnedHandle) {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read = null_mut();
    let mut write = null_mut();
    assert_ne!(
        unsafe { CreatePipe(&mut read, &mut write, &attributes, 4_096) },
        0
    );
    unsafe {
        (
            OwnedHandle::from_raw_handle(read.cast()),
            OwnedHandle::from_raw_handle(write.cast()),
        )
    }
}

fn fill_pipe(write: &OwnedHandle) {
    let bytes = [0x5a_u8; 4_096];
    let mut written = 0_u32;
    assert_ne!(
        unsafe {
            WriteFile(
                write.as_raw_handle() as HANDLE,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                null_mut(),
            )
        },
        0
    );
    assert_eq!(written as usize, bytes.len());
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
