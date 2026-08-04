#![cfg(windows)]

#[path = "../src/local_helper.rs"]
#[allow(dead_code)]
mod local_helper;
#[path = "../src/local_helper_windows.rs"]
mod local_helper_windows;

use std::ffi::OsString;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr::null_mut;
use std::time::Instant;

use local_helper::{
    parse_local_helper, HelperExit, HelperPlatform, LocalHelperInvocation, MintWriterCapability,
};
use local_helper_windows::{run_mint, AuthenticatedMintPort};
use windows_sys::Win32::Foundation::{
    DuplicateHandle, GetHandleInformation, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE,
    HANDLE_FLAG_INHERIT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

fn invocation(handle: HANDLE) -> LocalHelperInvocation {
    parse_local_helper(
        HelperPlatform::Windows,
        &[
            OsString::from("local"),
            OsString::from("service-account-mint"),
            OsString::from("--id"),
            OsString::from("worker"),
            OsString::from("--expires-at"),
            OsString::from("1800000000"),
            OsString::from("--writer-handle"),
            OsString::from((handle as usize).to_string()),
        ],
    )
    .expect("closed Windows mint grammar")
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
        unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
        0
    );
    unsafe {
        (
            OwnedHandle::from_raw_handle(read.cast()),
            OwnedHandle::from_raw_handle(write.cast()),
        )
    }
}

struct Session;

struct RecordingPort {
    calls: Vec<&'static str>,
    mint: Vec<u8>,
    terminal: Vec<u8>,
}

impl RecordingPort {
    fn successful() -> Self {
        Self {
            calls: Vec::new(),
            mint: Vec::new(),
            terminal: terminal(
                0x0022,
                br#"{"commit":{"frame_written":true,"verifier":"committed"},"id":"worker","receipt_id":"1111111111111111111111111111111111111111111111111111111111111111","replayed":false,"schema":"exchange.service-account-mint-receipt.v1"}"#,
            ),
        }
    }
}

impl AuthenticatedMintPort for RecordingPort {
    type Error = ();
    type Session = Session;
    type DuplicatedWriter = OwnedHandle;

    fn open_owner_session(&mut self, _ready_by: Instant) -> Result<Self::Session, Self::Error> {
        self.calls.push("open-owner-session");
        Ok(Session)
    }

    fn duplicate_writer(
        &mut self,
        _session: &mut Self::Session,
        writer: HANDLE,
    ) -> Result<Self::DuplicatedWriter, Self::Error> {
        self.calls.push("duplicate-writer");
        let mut flags = 0;
        assert_ne!(unsafe { GetHandleInformation(writer, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0, "inheritance was cleared");

        let mut duplicate = null_mut();
        assert_ne!(
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    writer,
                    GetCurrentProcess(),
                    &mut duplicate,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            },
            0
        );
        Ok(unsafe { OwnedHandle::from_raw_handle(duplicate.cast()) })
    }

    fn exchange_mint(
        &mut self,
        _session: Self::Session,
        _writer: Self::DuplicatedWriter,
        mint_frame: &[u8],
        _ready_by: Instant,
    ) -> Result<Vec<u8>, Self::Error> {
        self.calls.push("exchange-mint");
        self.mint.extend_from_slice(mint_frame);
        Ok(self.terminal.clone())
    }
}

fn terminal(opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(12 + payload.len());
    frame.extend_from_slice(b"FXLM");
    frame.extend_from_slice(&[1, 2]);
    frame.extend_from_slice(&opcode.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

#[test]
fn mint_clears_inheritance_duplicates_only_the_declared_writer_and_accepts_exact_receipt() {
    let (_read, write) = inheritable_pipe();
    let (_canary_read, canary_write) = inheritable_pipe();
    let mut canary_flags = 0;
    assert_ne!(
        unsafe { GetHandleInformation(canary_write.as_raw_handle() as HANDLE, &mut canary_flags,) },
        0
    );
    assert_ne!(canary_flags & HANDLE_FLAG_INHERIT, 0);

    let LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::Windows(writer),
    } = invocation(write.as_raw_handle() as HANDLE)
    else {
        panic!("mint invocation");
    };
    std::mem::forget(write);
    let mut port = RecordingPort::successful();
    assert!(run_mint(&id, expires_at, writer, &mut port) == HelperExit::TerminalFrameWritten);
    assert_eq!(
        port.calls,
        ["open-owner-session", "duplicate-writer", "exchange-mint"]
    );
    assert_eq!(
        port.mint,
        terminal(0x0020, br#"{"expires_at":"1800000000","id":"worker"}"#)
    );

    let mut still_inherited = 0;
    assert_ne!(
        unsafe {
            GetHandleInformation(canary_write.as_raw_handle() as HANDLE, &mut still_inherited)
        },
        0
    );
    assert_ne!(still_inherited & HANDLE_FLAG_INHERIT, 0);
}

#[test]
fn reversed_or_noninheritable_capability_refuses_before_owner_session() {
    let (read, _write) = inheritable_pipe();
    let LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::Windows(writer),
    } = invocation(read.as_raw_handle() as HANDLE)
    else {
        panic!("mint invocation");
    };
    std::mem::forget(read);
    let mut port = RecordingPort::successful();
    assert!(
        run_mint(&id, expires_at, writer, &mut port) == HelperExit::CapabilityOrTransportFailure
    );
    assert!(port.calls.is_empty());

    let (_read, write) = inheritable_pipe();
    assert_ne!(
        unsafe { SetHandleInformation(write.as_raw_handle() as HANDLE, HANDLE_FLAG_INHERIT, 0,) },
        0
    );
    let LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::Windows(writer),
    } = invocation(write.as_raw_handle() as HANDLE)
    else {
        panic!("mint invocation");
    };
    std::mem::forget(write);
    let mut port = RecordingPort::successful();
    assert!(
        run_mint(&id, expires_at, writer, &mut port) == HelperExit::CapabilityOrTransportFailure
    );
    assert!(port.calls.is_empty());
}

#[test]
fn altered_receipt_or_wrong_terminal_opcode_is_transport_failure() {
    for terminal in [
        terminal(
            0x0022,
            br#"{"commit":{"frame_written":true,"verifier":"committed"},"extra":true,"id":"worker","receipt_id":"1111111111111111111111111111111111111111111111111111111111111111","replayed":false,"schema":"exchange.service-account-mint-receipt.v1"}"#,
        ),
        terminal(
            0x0022,
            br#"{"commit":{"frame_written":true,"verifier":"committed"},"id":"another","receipt_id":"1111111111111111111111111111111111111111111111111111111111111111","replayed":false,"schema":"exchange.service-account-mint-receipt.v1"}"#,
        ),
        terminal(0x0006, br#"{}"#),
    ] {
        let (_read, write) = inheritable_pipe();
        let LocalHelperInvocation::ServiceAccountMint {
            id,
            expires_at,
            writer: MintWriterCapability::Windows(writer),
        } = invocation(write.as_raw_handle() as HANDLE)
        else {
            panic!("mint invocation");
        };
        std::mem::forget(write);
        let mut port = RecordingPort {
            terminal,
            ..RecordingPort::successful()
        };
        assert!(
            run_mint(&id, expires_at, writer, &mut port)
                == HelperExit::CapabilityOrTransportFailure
        );
    }
}

#[test]
fn canonical_application_error_is_a_written_terminal_result() {
    let (_read, write) = inheritable_pipe();
    let LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::Windows(writer),
    } = invocation(write.as_raw_handle() as HANDLE)
    else {
        panic!("mint invocation");
    };
    std::mem::forget(write);
    let mut port = RecordingPort {
        terminal: terminal(
            0x7fff,
            br#"{"code":"service_account_conflict","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":409}"#,
        ),
        ..RecordingPort::successful()
    };
    assert!(run_mint(&id, expires_at, writer, &mut port) == HelperExit::TerminalFrameWritten);
}
