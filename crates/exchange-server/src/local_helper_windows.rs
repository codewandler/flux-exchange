//! Windows execution boundary for the one-shot Service Account helper.
//!
//! The inherited writer is a process capability, not protocol data. This module validates the
//! designated anonymous-pipe write end, clears inheritance, and keeps owner authentication,
//! process pinning, `DuplicateHandle`, MINT, and its terminal response on one typed session. It
//! deliberately does not claim to enumerate the process handle table: launch-time
//! `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` closure is proved by the native launcher fixture.

use std::fmt;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr::null_mut;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{
    GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
use windows_sys::Win32::System::Pipes::{GetNamedPipeInfo, PIPE_SERVER_END};

use crate::local_helper::{
    ExpiresAt, HelperExit, ServiceAccountId, WindowsHandle, HELPER_SETUP_DEADLINE,
    MAX_HELPER_FRAME_BYTES,
};

const HEADER_BYTES: usize = 12;
const MAX_CONTROL_BYTES: usize = 65_536;
const CLIENT_DIRECTION: u8 = 1;
const SERVER_DIRECTION: u8 = 2;
const SERVICE_ACCOUNT_MINT: u16 = 0x0020;
const SERVICE_ACCOUNT_RECEIPT: u16 = 0x0022;
const ERROR: u16 = 0x7fff;
const RECEIPT_SCHEMA: &str = "exchange.service-account-mint-receipt.v1";
const ERROR_SCHEMA: &str = "exchange.local-management-error.v1";

/// One owner-authenticated Windows mint session.
///
/// Implementations connect to the account-derived named pipe, pin that exact server process, and
/// compare its `TokenUser` SID and session with the helper before returning `Session`. The
/// `duplicate_writer` implementation must call `DuplicateHandle` from the helper into that pinned
/// process. Because `exchange_mint` consumes the same `Session` and `DuplicatedWriter`, a writer
/// from another connection or process cannot be substituted by the orchestration seam.
pub(crate) trait AuthenticatedMintPort {
    type Error;
    type Session;
    type DuplicatedWriter;

    /// Open, owner-authenticate, and process-pin one local-management connection.
    fn open_owner_session(&mut self, ready_by: Instant) -> Result<Self::Session, Self::Error>;

    /// Duplicate the validated writer into the exact server process pinned by `session`.
    fn duplicate_writer(
        &mut self,
        session: &mut Self::Session,
        writer: HANDLE,
    ) -> Result<Self::DuplicatedWriter, Self::Error>;

    /// Send exact MINT plus the duplicated capability and read its sole terminal response.
    fn exchange_mint(
        &mut self,
        session: Self::Session,
        writer: Self::DuplicatedWriter,
        mint_frame: &[u8],
        ready_by: Instant,
    ) -> Result<Vec<u8>, Self::Error>;
}

/// Execute the Windows Service Account helper through one authenticated, process-pinned session.
pub(crate) fn run_mint<P: AuthenticatedMintPort>(
    id: &ServiceAccountId,
    expires_at: ExpiresAt,
    writer: WindowsHandle,
    port: &mut P,
) -> HelperExit {
    let writer = match InheritedWriter::take(writer) {
        Ok(writer) => writer,
        Err(_) => return HelperExit::CapabilityOrTransportFailure,
    };
    let ready_by = Instant::now()
        .checked_add(HELPER_SETUP_DEADLINE)
        .unwrap_or_else(Instant::now);
    let mut session = match port.open_owner_session(ready_by) {
        Ok(session) if Instant::now() < ready_by => session,
        _ => return HelperExit::CapabilityOrTransportFailure,
    };
    let duplicated =
        match port.duplicate_writer(&mut session, writer.descriptor.as_raw_handle() as HANDLE) {
            Ok(duplicated) if Instant::now() < ready_by => duplicated,
            _ => return HelperExit::CapabilityOrTransportFailure,
        };
    // The helper's source handle closes after duplication. Only the pinned server process retains
    // the writer that can receive one FXSA frame.
    drop(writer);

    let payload = format!(
        "{{\"expires_at\":\"{}\",\"id\":\"{}\"}}",
        expires_at.value(),
        id.as_str()
    );
    let mint = encode_frame(CLIENT_DIRECTION, SERVICE_ACCOUNT_MINT, payload.as_bytes());
    let terminal = match port.exchange_mint(session, duplicated, &mint, ready_by) {
        Ok(terminal) if Instant::now() < ready_by => terminal,
        _ => return HelperExit::CapabilityOrTransportFailure,
    };
    if validate_terminal(&terminal, id.as_str()) {
        HelperExit::TerminalFrameWritten
    } else {
        HelperExit::CapabilityOrTransportFailure
    }
}

struct InheritedWriter {
    descriptor: OwnedHandle,
}

impl InheritedWriter {
    fn take(writer: WindowsHandle) -> Result<Self, WindowsMintRefusal> {
        let raw = writer.native_value() as HANDLE;
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(WindowsMintRefusal::WriterInvalid);
        }
        let mut handle_flags = 0_u32;
        if unsafe { GetHandleInformation(raw, &mut handle_flags) } == 0 {
            return Err(WindowsMintRefusal::WriterInvalid);
        }
        // SAFETY: successful GetHandleInformation established one live non-pseudo handle. The
        // closed helper ABI transfers it to this mode exactly once; every later refusal closes it.
        let descriptor = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        if handle_flags & HANDLE_FLAG_INHERIT == 0 || unsafe { GetFileType(raw) } != FILE_TYPE_PIPE
        {
            return Err(WindowsMintRefusal::WriterInvalid);
        }
        let mut pipe_flags = 0_u32;
        if unsafe { GetNamedPipeInfo(raw, &mut pipe_flags, null_mut(), null_mut(), null_mut()) }
            == 0
            || pipe_flags & PIPE_SERVER_END != 0
        {
            return Err(WindowsMintRefusal::WriterInvalid);
        }
        if unsafe { SetHandleInformation(raw, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(WindowsMintRefusal::WriterInvalid);
        }
        let mut cleared_flags = HANDLE_FLAG_INHERIT;
        if unsafe { GetHandleInformation(raw, &mut cleared_flags) } == 0
            || cleared_flags & HANDLE_FLAG_INHERIT != 0
        {
            return Err(WindowsMintRefusal::WriterInvalid);
        }
        Ok(Self { descriptor })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsMintRefusal {
    WriterInvalid,
}

impl fmt::Display for WindowsMintRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Windows mint writer capability is invalid")
    }
}

impl std::error::Error for WindowsMintRefusal {}

fn encode_frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_BYTES + payload.len());
    frame.extend_from_slice(b"FXLM");
    frame.extend_from_slice(&[1, direction]);
    frame.extend_from_slice(&opcode.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn validate_terminal(frame: &[u8], expected_id: &str) -> bool {
    if !(HEADER_BYTES..=MAX_HELPER_FRAME_BYTES).contains(&frame.len())
        || &frame[..4] != b"FXLM"
        || frame[4] != 1
        || frame[5] != SERVER_DIRECTION
    {
        return false;
    }
    let opcode = u16::from_be_bytes([frame[6], frame[7]]);
    let declared = u32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
    if declared > MAX_CONTROL_BYTES || frame.len() != HEADER_BYTES + declared {
        return false;
    }
    let payload = &frame[HEADER_BYTES..];
    match opcode {
        SERVICE_ACCOUNT_RECEIPT => validate_receipt(payload, expected_id),
        ERROR => validate_error(payload),
        _ => false,
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MintReceipt {
    commit: MintCommit,
    id: String,
    receipt_id: String,
    replayed: bool,
    schema: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MintCommit {
    frame_written: bool,
    verifier: String,
}

fn validate_receipt(payload: &[u8], expected_id: &str) -> bool {
    let Ok(receipt) = serde_json::from_slice::<MintReceipt>(payload) else {
        return false;
    };
    receipt.schema == RECEIPT_SCHEMA
        && receipt.id == expected_id
        && receipt.commit.frame_written
        && receipt.commit.verifier == "committed"
        && valid_receipt_id(&receipt.receipt_id)
        && serde_json::to_vec(&receipt).is_ok_and(|canonical| canonical == payload)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreDecisionError {
    code: String,
    commit: String,
    retry: String,
    schema: String,
    status: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PostDecisionError {
    code: String,
    commit: String,
    receipt_id: String,
    retry: String,
    schema: String,
    status: u16,
}

fn validate_error(payload: &[u8]) -> bool {
    if let Ok(error) = serde_json::from_slice::<PreDecisionError>(payload) {
        return error.schema == ERROR_SCHEMA
            && error.commit == "none"
            && predecision_tuple(&error.code) == Some((error.status, error.retry.as_str()))
            && serde_json::to_vec(&error).is_ok_and(|canonical| canonical == payload);
    }
    if let Ok(error) = serde_json::from_slice::<PostDecisionError>(payload) {
        return error.schema == ERROR_SCHEMA
            && error.commit == "query_receipt"
            && error.retry == "same_proposal"
            && valid_receipt_id(&error.receipt_id)
            && postdecision_tuple(&error.code) == Some(error.status)
            && serde_json::to_vec(&error).is_ok_and(|canonical| canonical == payload);
    }
    false
}

fn predecision_tuple(code: &str) -> Option<(u16, &'static str)> {
    Some(match code {
        "invalid_frame" => (400, "never"),
        "unsupported_version" => (426, "never"),
        "wrong_direction" => (400, "never"),
        "unexpected_frame" => (409, "never"),
        "frame_too_large" => (413, "never"),
        "truncated_frame" => (400, "never"),
        "surplus_data" => (400, "never"),
        "deadline_exceeded" => (408, "refresh"),
        "peer_unverified" => (403, "never"),
        "unsafe_root" | "local_management_unavailable" => (503, "operator"),
        "invalid_request" => (400, "never"),
        "unknown_connector" | "unknown_label" => (404, "refresh"),
        "invalid_label" => (422, "never"),
        "secret_json_forbidden" => (415, "never"),
        "unknown_target" => (422, "refresh"),
        "stale_plan"
        | "stale_credential_revision"
        | "credential_state_conflict"
        | "proposal_conflict"
        | "connect_busy"
        | "grant_stale"
        | "grant_digest_mismatch"
        | "service_account_conflict" => (409, "refresh"),
        "grant_unexpressible" => (409, "operator"),
        "writer_invalid" => (400, "never"),
        "writer_closed" => (409, "operator"),
        "store_unavailable" | "audit_unavailable" => (503, "operator"),
        "internal_refusal" => (500, "operator"),
        _ => return None,
    })
}

fn postdecision_tuple(code: &str) -> Option<u16> {
    match code {
        "store_unavailable" | "audit_unavailable" => Some(503),
        "internal_refusal" => Some(500),
        _ => None,
    }
}

fn valid_receipt_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}
