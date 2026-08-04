//! Windows execution boundary for the verified local helper modes.
//!
//! The production vendor client authenticates and process-pins the owner endpoint across its PLAN
//! and mutation connections. The inherited Service Account writer remains a process capability,
//! not protocol data: its typed test seam validates the write end and models `DuplicateHandle`, but
//! production refuses until the named-pipe endpoint defines a capability receiver association. It
//! deliberately does not serialize a HANDLE or claim to enumerate the process handle table.

// Windows-only integration targets include this source directly. Production also compiles the
// module and enters it only through the closed `LocalHelperInvocation` parsed in `main`.
#![cfg_attr(test, allow(dead_code))]

use std::fmt;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, IntoRawHandle as _, OwnedHandle};
use std::ptr::null_mut;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use windows_sys::Wdk::Foundation::{NtQueryObject, OBJECT_NAME_INFORMATION};
use windows_sys::Win32::Foundation::{
    CloseHandle, CompareObjectHandles, GetHandleInformation, GetLastError, SetHandleInformation,
    ERROR_BROKEN_PIPE, ERROR_INSUFFICIENT_BUFFER, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, TokenSessionId, TokenUser, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileType, ReadFile, WriteFile, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TYPE_PIPE, OPEN_EXISTING, SYNCHRONIZE,
};
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeInfo, GetNamedPipeServerProcessId, PeekNamedPipe, PIPE_SERVER_END,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::local_helper::{
    ExpiresAt, HelperExit, LocalHelperInvocation, MintWriterCapability, ServiceAccountId,
    VendorSecretCapabilities, WindowsHandle, HELPER_RESULT_DEADLINE, HELPER_SETUP_DEADLINE,
    MAX_HELPER_FRAME_BYTES,
};

const PIPE_PREFIX: &str = r"\\.\pipe\flux-exchange-local-management-v1-";
const HEADER_BYTES: usize = 12;
const MAX_CONTROL_BYTES: usize = 65_536;
const MAX_SECRET_BYTES: usize = 8_192;
const CLIENT_DIRECTION: u8 = 1;
const SERVER_DIRECTION: u8 = 2;
const SERVICE_ACCOUNT_MINT: u16 = 0x0020;
const SERVICE_ACCOUNT_RECEIPT: u16 = 0x0022;
const CONNECT_BEGIN: u16 = 0x0001;
const NEED_SECRETS: u16 = 0x0002;
const SECRET: u16 = 0x0003;
const CONNECT_COMMIT: u16 = 0x0004;
const CONNECT_RECEIPT: u16 = 0x0006;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;
const CREDENTIAL_BEGIN: u16 = 0x0030;
const CREDENTIAL_COMMIT: u16 = 0x0031;
const CREDENTIAL_RECEIPT: u16 = 0x0032;
const ERROR: u16 = 0x7fff;
const RECEIPT_SCHEMA: &str = "exchange.service-account-mint-receipt.v1";
const ERROR_SCHEMA: &str = "exchange.local-management-error.v1";
const ENABLE_ECHO_INPUT: u32 = 0x0004;

/// Execute one already-closed Windows helper invocation before tracing or the server runtime.
pub(crate) fn run(invocation: LocalHelperInvocation) -> HelperExit {
    match invocation {
        LocalHelperInvocation::VendorSecret(VendorSecretCapabilities::Windows {
            request,
            response,
        }) => match NativeVendorCeremony::new() {
            Ok(mut ceremony) => run_vendor(request, response, &mut ceremony),
            Err(_) => HelperExit::CapabilityOrTransportFailure,
        },
        LocalHelperInvocation::ServiceAccountMint {
            writer: MintWriterCapability::Windows(writer),
            ..
        } => {
            // The accepted contract does not define how the target-process HANDLE returned by
            // DuplicateHandle crosses the named-pipe boundary, and the production server consumes
            // only FXLM bytes. Take and close the inherited capability rather than serializing a
            // HANDLE into MINT JSON or inventing an unauthenticated transport preface.
            let _ = InheritedWriter::take(writer);
            HelperExit::CapabilityOrTransportFailure
        }
        _ => HelperExit::CapabilityOrTransportFailure,
    }
}

/// One request whose frame and EOF passed the Windows Flux-to-helper capability boundary.
///
/// The bytes have no text conversion or debug representation. They can contain non-secret
/// settings, but only the authenticated native ceremony receives them.
pub(crate) struct VendorRequest {
    bytes: Vec<u8>,
    kind: VendorRequestKind,
}

impl VendorRequest {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) const fn kind(&self) -> VendorRequestKind {
        self.kind
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VendorRequestKind {
    Connect,
    Credential,
}

/// Secret bytes read from the current Windows console with echo disabled.
///
/// The buffer is zeroized on drop and intentionally has no formatting implementation.
pub(crate) struct ConsoleSecret(Vec<u8>);

impl ConsoleSecret {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for ConsoleSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// Private input port passed to the production Windows vendor ceremony.
pub(crate) struct PrivateConsole;

impl PrivateConsole {
    /// Open `CONIN$` directly and read one non-empty, bounded line without terminal echo.
    pub(crate) fn read_secret(&mut self) -> Result<ConsoleSecret, WindowsHelperError> {
        read_console_secret_with(&mut KernelConsole)
    }
}

/// Production plan-first vendor ceremony behind the closed Windows process-capability boundary.
pub(crate) trait VendorCeremony {
    type Error;
    type Session;

    /// Pin the owner endpoint, complete plan connection 1, and make connection 2 ready for BEGIN.
    fn prepare(
        &mut self,
        request: &VendorRequest,
        ready_by: Instant,
    ) -> Result<VendorPreparation<Self::Session>, Self::Error>;

    /// Own connection 2 through its server-bounded prompt/decision/roll-forward phases.
    fn exchange(
        &mut self,
        session: Self::Session,
        input: &mut PrivateConsole,
    ) -> Result<Vec<u8>, Self::Error>;
}

pub(crate) enum VendorPreparation<Session> {
    /// Plan connection 1 produced a terminal application error; connection 2 was not opened.
    Terminal(Vec<u8>),
    /// The pinned, revalidated connection 2 is ready to receive the byte-identical BEGIN.
    Ready(Session),
}

struct NativeVendorCeremony {
    runtime: tokio::runtime::Runtime,
}

impl NativeVendorCeremony {
    fn new() -> Result<Self, WindowsHelperError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| WindowsHelperError::Transport)?;
        Ok(Self { runtime })
    }
}

struct NativeVendorSession {
    // Retaining the pinned process handle through the terminal frame makes a server exit observable
    // and prevents the second connection from silently becoming a different same-owner process.
    _endpoint: PinnedEndpoint,
    pipe: NamedPipeClient,
    kind: VendorRequestKind,
}

impl VendorCeremony for NativeVendorCeremony {
    type Error = WindowsHelperError;
    type Session = NativeVendorSession;

    fn prepare(
        &mut self,
        request: &VendorRequest,
        ready_by: Instant,
    ) -> Result<VendorPreparation<Self::Session>, Self::Error> {
        let begin = BeginFacts::parse(request)?;
        self.runtime.block_on(async {
            let mut endpoint = PinnedEndpoint::authenticated()?;
            let mut plan_pipe = endpoint.connect_before(ready_by).await?;
            let plan_query = serde_json::to_vec(&PlanQuery {
                connector: &begin.connector,
                selection: match request.kind() {
                    VendorRequestKind::Connect => None,
                    VendorRequestKind::Credential => Some(begin.label.as_str()),
                },
            })
            .map_err(|_| WindowsHelperError::Protocol)?;
            write_before_async(
                &mut plan_pipe,
                &encode_frame(CLIENT_DIRECTION, PLAN_QUERY, &plan_query),
                ready_by,
            )
            .await?;
            let plan = read_terminal_before_async(&mut plan_pipe, ready_by).await?;
            if plan.opcode == ERROR {
                return Ok(VendorPreparation::Terminal(plan.bytes));
            }
            if plan.opcode != PLAN_RESPONSE || !begin.admits_plan(&plan.payload, request.kind()) {
                return Err(WindowsHelperError::Protocol);
            }
            drop(plan_pipe);

            let mut mutation = endpoint.connect_before(ready_by).await?;
            write_before_async(&mut mutation, request.bytes(), ready_by).await?;
            Ok(VendorPreparation::Ready(NativeVendorSession {
                _endpoint: endpoint,
                pipe: mutation,
                kind: request.kind(),
            }))
        })
    }

    fn exchange(
        &mut self,
        mut session: Self::Session,
        input: &mut PrivateConsole,
    ) -> Result<Vec<u8>, Self::Error> {
        self.runtime.block_on(async {
            let need = read_frame_before_async(
                &mut session.pipe,
                Instant::now()
                    .checked_add(Duration::from_secs(300))
                    .ok_or(WindowsHelperError::Deadline)?,
            )
            .await?;
            if need.opcode == ERROR {
                return Ok(need.bytes);
            }
            if need.opcode != NEED_SECRETS {
                return Err(WindowsHelperError::Protocol);
            }
            let need_payload = need.payload;
            let need: NeedSecrets =
                serde_json::from_slice(&need_payload).map_err(|_| WindowsHelperError::Protocol)?;
            if !need.is_canonical(&need_payload) {
                return Err(WindowsHelperError::Protocol);
            }

            let predecision = Instant::now()
                .checked_add(Duration::from_secs(300))
                .ok_or(WindowsHelperError::Deadline)?;
            for (index, secret) in need.secrets.iter().enumerate() {
                if usize::from(secret.ordinal) != index + 1 || secret.target.is_empty() {
                    return Err(WindowsHelperError::Protocol);
                }
                if Instant::now() >= predecision {
                    return Err(WindowsHelperError::Deadline);
                }
                let value = input.read_secret()?;
                let mut frame = encode_secret(secret.ordinal, value.bytes())?;
                let result = write_before_async(&mut session.pipe, &frame, predecision).await;
                frame.fill(0);
                result?;
            }
            let commit = serde_json::to_vec(&Commit {
                proposal_digest: &need.proposal_digest,
                transaction_id: &need.transaction_id,
            })
            .map_err(|_| WindowsHelperError::Protocol)?;
            let opcode = match session.kind {
                VendorRequestKind::Connect => CONNECT_COMMIT,
                VendorRequestKind::Credential => CREDENTIAL_COMMIT,
            };
            write_before_async(
                &mut session.pipe,
                &encode_frame(CLIENT_DIRECTION, opcode, &commit),
                predecision,
            )
            .await?;
            let postdecision = Instant::now()
                .checked_add(Duration::from_secs(30))
                .ok_or(WindowsHelperError::Deadline)?;
            Ok(read_terminal_before_async(&mut session.pipe, postdecision)
                .await?
                .bytes)
        })
    }
}

struct PinnedEndpoint {
    pipe_name: String,
    owner: ProcessIdentity,
    process: Option<OwnedHandle>,
}

impl PinnedEndpoint {
    fn authenticated() -> Result<Self, WindowsHelperError> {
        let owner = process_token_identity(unsafe { GetCurrentProcess() })?;
        Ok(Self {
            pipe_name: pipe_name_for_sid(&owner.sid),
            owner,
            process: None,
        })
    }

    async fn connect_before(
        &mut self,
        deadline: Instant,
    ) -> Result<NamedPipeClient, WindowsHelperError> {
        let pipe = loop {
            if Instant::now() >= deadline {
                return Err(WindowsHelperError::Deadline);
            }
            match ClientOptions::new()
                .read(true)
                .write(true)
                .open(&self.pipe_name)
            {
                Ok(pipe) => break pipe,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        };
        self.authenticate_and_pin(&pipe)?;
        if Instant::now() >= deadline {
            return Err(WindowsHelperError::Deadline);
        }
        Ok(pipe)
    }

    fn authenticate_and_pin(&mut self, pipe: &NamedPipeClient) -> Result<(), WindowsHelperError> {
        let mut process_id = 0_u32;
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut process_id) }
            == 0
            || process_id == 0
        {
            return Err(WindowsHelperError::OwnerIdentity);
        }
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(WindowsHelperError::OwnerIdentity);
        }
        // SAFETY: successful OpenProcess returned one owned non-pseudo process handle.
        let candidate = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let mut confirmed_process_id = 0_u32;
        if process_token_identity(candidate.as_raw_handle() as HANDLE)? != self.owner
            || unsafe { WaitForSingleObject(candidate.as_raw_handle() as HANDLE, 0) }
                != WAIT_TIMEOUT
            || unsafe {
                GetNamedPipeServerProcessId(
                    pipe.as_raw_handle() as HANDLE,
                    &mut confirmed_process_id,
                )
            } == 0
            || confirmed_process_id != process_id
        {
            return Err(WindowsHelperError::OwnerIdentity);
        }
        match &self.process {
            Some(process)
                if unsafe {
                    CompareObjectHandles(
                        process.as_raw_handle() as HANDLE,
                        candidate.as_raw_handle() as HANDLE,
                    )
                } == 0 =>
            {
                Err(WindowsHelperError::EndpointChanged)
            }
            Some(_) => Ok(()),
            None => {
                self.process = Some(candidate);
                Ok(())
            }
        }
    }
}

#[derive(PartialEq, Eq)]
struct ProcessIdentity {
    sid: Vec<u8>,
    session: u32,
}

fn process_token_identity(process: HANDLE) -> Result<ProcessIdentity, WindowsHelperError> {
    let mut raw: HANDLE = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } == 0 {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    // SAFETY: successful OpenProcessToken returned one owned token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
    let mut length = 0_u32;
    let sized = unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenUser,
            null_mut(),
            0,
            &mut length,
        )
    };
    if sized != 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    let capacity = length as usize;
    if capacity < std::mem::size_of::<TOKEN_USER>() {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    let mut storage = vec![0_usize; capacity.div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
        || length as usize > capacity
    {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    let base = storage.as_ptr().cast::<u8>();
    // SAFETY: the successful query initialized TOKEN_USER at the aligned buffer start.
    let sid = unsafe { (*(base.cast::<TOKEN_USER>())).User.Sid.cast::<u8>() };
    let offset = (sid as usize)
        .checked_sub(base as usize)
        .filter(|offset| *offset < capacity)
        .ok_or(WindowsHelperError::OwnerIdentity)?;
    if unsafe { IsValidSid(sid.cast()) } == 0 {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    let sid_length = unsafe { GetLengthSid(sid.cast()) } as usize;
    if sid_length == 0
        || offset
            .checked_add(sid_length)
            .is_none_or(|end| end > capacity)
    {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    // SAFETY: the validated SID range lies wholly inside the live query allocation.
    let sid = unsafe { std::slice::from_raw_parts(sid, sid_length) }.to_vec();
    let mut session = 0_u32;
    let mut session_length =
        u32::try_from(std::mem::size_of::<u32>()).map_err(|_| WindowsHelperError::OwnerIdentity)?;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenSessionId,
            (&mut session as *mut u32).cast(),
            session_length,
            &mut session_length,
        )
    } == 0
        || session_length as usize != std::mem::size_of::<u32>()
    {
        return Err(WindowsHelperError::OwnerIdentity);
    }
    Ok(ProcessIdentity { sid, session })
}

fn pipe_name_for_sid(sid: &[u8]) -> String {
    let digest = Sha256::digest(sid);
    let mut suffix = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write as _;
        let _ = write!(suffix, "{byte:02x}");
    }
    format!("{PIPE_PREFIX}{suffix}")
}

#[derive(Serialize)]
struct PlanQuery<'a> {
    connector: &'a str,
    selection: Option<&'a str>,
}

struct BeginFacts {
    connector: String,
    credential_revision: Option<String>,
    label: String,
    plan_revision: String,
}

impl BeginFacts {
    fn parse(request: &VendorRequest) -> Result<Self, WindowsHelperError> {
        let payload = request
            .bytes()
            .get(HEADER_BYTES..)
            .ok_or(WindowsHelperError::Protocol)?;
        let value: serde_json::Value =
            serde_json::from_slice(payload).map_err(|_| WindowsHelperError::Protocol)?;
        let object = value.as_object().ok_or(WindowsHelperError::Protocol)?;
        let connector = required_string(object, "connector")?;
        let label = required_string(object, "label")?;
        let plan_revision = required_string(object, "plan_revision")?;
        let credential_revision = match object.get("credential_revision") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(value)) => Some(value.clone()),
            _ => return Err(WindowsHelperError::Protocol),
        };
        match request.kind() {
            VendorRequestKind::Connect if credential_revision.is_some() => {
                return Err(WindowsHelperError::Protocol);
            }
            VendorRequestKind::Credential
                if !credential_revision
                    .as_deref()
                    .is_some_and(is_nonzero_lowerhex_32) =>
            {
                return Err(WindowsHelperError::Protocol);
            }
            _ => {}
        }
        Ok(Self {
            connector,
            credential_revision,
            label,
            plan_revision,
        })
    }

    fn admits_plan(&self, payload: &[u8], kind: VendorRequestKind) -> bool {
        let Ok(plan) = serde_json::from_slice::<serde_json::Value>(payload) else {
            return false;
        };
        plan.get("version").and_then(serde_json::Value::as_str)
            == Some("exchange.connection-plan.v2")
            && plan.get("connector").and_then(serde_json::Value::as_str)
                == Some(self.connector.as_str())
            && plan
                .get("plan_revision")
                .and_then(serde_json::Value::as_str)
                == Some(self.plan_revision.as_str())
            && match kind {
                VendorRequestKind::Connect => {
                    plan.get("selection") == Some(&serde_json::Value::Null)
                        && plan.get("credential_revision") == Some(&serde_json::Value::Null)
                }
                VendorRequestKind::Credential => {
                    plan.get("selection").and_then(serde_json::Value::as_str)
                        == Some(self.label.as_str())
                        && plan
                            .get("credential_revision")
                            .and_then(serde_json::Value::as_str)
                            == self.credential_revision.as_deref()
                }
            }
    }
}

fn required_string(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, WindowsHelperError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(WindowsHelperError::Protocol)
}

fn is_nonzero_lowerhex_32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && value.bytes().any(|byte| byte != b'0')
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NeedSecrets {
    proposal_digest: String,
    secrets: Vec<NeedSecret>,
    transaction_id: String,
}

impl NeedSecrets {
    fn is_canonical(&self, payload: &[u8]) -> bool {
        is_nonzero_lowerhex_32(&self.proposal_digest)
            && self.transaction_id.len() == 64
            && self
                .transaction_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && self.transaction_id.bytes().any(|byte| byte != b'0')
            && serde_json::to_vec(self).is_ok_and(|canonical| canonical == payload)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NeedSecret {
    ordinal: u16,
    target: String,
}

#[derive(Serialize)]
struct Commit<'a> {
    proposal_digest: &'a str,
    transaction_id: &'a str,
}

struct NativeFrame {
    bytes: Vec<u8>,
    opcode: u16,
    payload: Vec<u8>,
}

async fn write_before_async(
    pipe: &mut NamedPipeClient,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), WindowsHelperError> {
    let mut written = 0_usize;
    while written < bytes.len() {
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), pipe.writable())
            .await
            .map_err(|_| WindowsHelperError::Deadline)?
            .map_err(|_| WindowsHelperError::Transport)?;
        match pipe.try_write(&bytes[written..]) {
            Ok(0) => return Err(WindowsHelperError::Transport),
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(WindowsHelperError::Transport),
        }
    }
    Ok(())
}

async fn read_frame_before_async(
    pipe: &mut NamedPipeClient,
    deadline: Instant,
) -> Result<NativeFrame, WindowsHelperError> {
    let mut header = [0_u8; HEADER_BYTES];
    read_exact_before_async(pipe, &mut header, deadline).await?;
    if &header[..4] != b"FXLM" || header[4] != 1 || header[5] != SERVER_DIRECTION {
        return Err(WindowsHelperError::Protocol);
    }
    let payload_length =
        u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if payload_length > MAX_CONTROL_BYTES {
        return Err(WindowsHelperError::Protocol);
    }
    let mut payload = vec![0_u8; payload_length];
    read_exact_before_async(pipe, &mut payload, deadline).await?;
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&payload);
    Ok(NativeFrame {
        bytes,
        opcode: u16::from_be_bytes([header[6], header[7]]),
        payload,
    })
}

async fn read_terminal_before_async(
    pipe: &mut NamedPipeClient,
    deadline: Instant,
) -> Result<NativeFrame, WindowsHelperError> {
    let frame = read_frame_before_async(pipe, deadline).await?;
    let mut extra = [0_u8; 1];
    loop {
        match pipe.try_read(&mut extra) {
            Ok(0) => return Ok(frame),
            Ok(_) => return Err(WindowsHelperError::Protocol),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                match tokio::time::timeout_at(
                    tokio::time::Instant::from_std(deadline),
                    pipe.readable(),
                )
                .await
                {
                    Err(_) => return Err(WindowsHelperError::Deadline),
                    Ok(Err(error)) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                        return Ok(frame);
                    }
                    Ok(Err(_)) => return Err(WindowsHelperError::Transport),
                    Ok(Ok(())) => {}
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => return Ok(frame),
            Err(_) => return Err(WindowsHelperError::Transport),
        }
    }
}

async fn read_exact_before_async(
    pipe: &mut NamedPipeClient,
    bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), WindowsHelperError> {
    let mut received = 0_usize;
    while received < bytes.len() {
        match pipe.try_read(&mut bytes[received..]) {
            Ok(0) => return Err(WindowsHelperError::Transport),
            Ok(count) => received += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), pipe.readable())
                    .await
                    .map_err(|_| WindowsHelperError::Deadline)?
                    .map_err(|_| WindowsHelperError::Transport)?;
            }
            Err(_) => return Err(WindowsHelperError::Transport),
        }
    }
    Ok(())
}

fn encode_secret(ordinal: u16, secret: &[u8]) -> Result<Vec<u8>, WindowsHelperError> {
    if ordinal == 0 || !(1..=MAX_SECRET_BYTES).contains(&secret.len()) {
        return Err(WindowsHelperError::Protocol);
    }
    let mut payload = Vec::with_capacity(2 + secret.len());
    payload.extend_from_slice(&ordinal.to_be_bytes());
    payload.extend_from_slice(secret);
    Ok(encode_frame(CLIENT_DIRECTION, SECRET, &payload))
}

/// Execute the Windows vendor helper from its two validated inherited anonymous pipes.
///
/// This is the production-callable helper seam. The executable composition supplies the already
/// parsed handles and the native plan/dispatcher adapter; this module owns pipe direction,
/// identity, inheritance clearing, one-frame-plus-EOF, private-console input and terminal output.
pub(crate) fn run_vendor<C: VendorCeremony>(
    request: WindowsHandle,
    response: WindowsHandle,
    ceremony: &mut C,
) -> HelperExit {
    let capabilities = match VendorCapabilities::take(request, response) {
        Ok(capabilities) => capabilities,
        Err(_) => return HelperExit::CapabilityOrTransportFailure,
    };
    let VendorCapabilities { request, response } = capabilities;
    let request = match read_request(&request, HELPER_SETUP_DEADLINE) {
        Ok(request) => request,
        Err(refusal) => return finish_response(response, &refusal.frame()),
    };
    // EOF starts both caps. Setup covers endpoint/plan/connection-2 readiness; result completion
    // separately admits the server's 300-second pre-decision and 30-second post-decision budgets.
    let result_by = Instant::now()
        .checked_add(HELPER_RESULT_DEADLINE)
        .unwrap_or_else(Instant::now);
    let ready_by = Instant::now()
        .checked_add(HELPER_SETUP_DEADLINE)
        .unwrap_or_else(Instant::now);
    let preparation = match ceremony.prepare(&request, ready_by) {
        Ok(preparation) if Instant::now() < ready_by => preparation,
        _ => return finish_response(response, &Refusal::LocalManagementUnavailable.frame()),
    };
    let terminal = match preparation {
        VendorPreparation::Terminal(terminal) => terminal,
        VendorPreparation::Ready(session) => {
            let mut input = PrivateConsole;
            match ceremony.exchange(session, &mut input) {
                Ok(terminal) if Instant::now() < result_by => terminal,
                _ => return HelperExit::CapabilityOrTransportFailure,
            }
        }
    };
    let terminal = match validate_vendor_terminal(&terminal, request.kind()) {
        Ok(()) => terminal,
        Err(refusal) => refusal.frame(),
    };
    finish_response(response, &terminal)
}

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

struct VendorCapabilities {
    request: OwnedHandle,
    response: OwnedHandle,
}

impl VendorCapabilities {
    fn take(request: WindowsHandle, response: WindowsHandle) -> Result<Self, WindowsHelperError> {
        let request = inherited_pipe(request, PipeEnd::Read)?;
        let response = inherited_pipe(response, PipeEnd::Write)?;
        let request_raw = request.as_raw_handle() as HANDLE;
        let response_raw = response.as_raw_handle() as HANDLE;
        // CompareObjectHandles catches aliases even when a launcher supplied two numeric values.
        // File identity catches the opposite ends of one anonymous pipe: the ABI requires two
        // distinct pipes, so Flux can never read the terminal result through its request channel.
        if unsafe { CompareObjectHandles(request_raw, response_raw) } != 0
            || pipe_identity(request_raw)? == pipe_identity(response_raw)?
        {
            return Err(WindowsHelperError::Capability);
        }
        clear_inheritance(request_raw)?;
        clear_inheritance(response_raw)?;
        Ok(Self { request, response })
    }
}

#[derive(Clone, Copy)]
enum PipeEnd {
    Read,
    Write,
}

fn inherited_pipe(
    handle: WindowsHandle,
    expected: PipeEnd,
) -> Result<OwnedHandle, WindowsHelperError> {
    let raw = handle.native_value() as HANDLE;
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(WindowsHelperError::Capability);
    }
    let mut handle_flags = 0_u32;
    if unsafe { GetHandleInformation(raw, &mut handle_flags) } == 0
        || handle_flags & HANDLE_FLAG_INHERIT == 0
        || unsafe { GetFileType(raw) } != FILE_TYPE_PIPE
    {
        return Err(WindowsHelperError::Capability);
    }
    let mut pipe_flags = 0_u32;
    if unsafe { GetNamedPipeInfo(raw, &mut pipe_flags, null_mut(), null_mut(), null_mut()) } == 0 {
        return Err(WindowsHelperError::Capability);
    }
    let direction_matches = match expected {
        PipeEnd::Read => pipe_flags & PIPE_SERVER_END != 0,
        PipeEnd::Write => pipe_flags & PIPE_SERVER_END == 0,
    };
    if !direction_matches {
        return Err(WindowsHelperError::Capability);
    }
    // SAFETY: the closed helper grammar transfers each live non-pseudo handle exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw.cast()) })
}

fn pipe_identity(handle: HANDLE) -> Result<Vec<u16>, WindowsHelperError> {
    const OBJECT_NAME_INFORMATION_CLASS: i32 = 1;
    let mut required = 0_u32;
    unsafe {
        NtQueryObject(
            handle,
            OBJECT_NAME_INFORMATION_CLASS,
            null_mut(),
            0,
            &mut required,
        )
    };
    if required < std::mem::size_of::<OBJECT_NAME_INFORMATION>() as u32 {
        return Err(WindowsHelperError::Capability);
    }
    let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
    let mut storage = vec![0_usize; words];
    let status = unsafe {
        NtQueryObject(
            handle,
            OBJECT_NAME_INFORMATION_CLASS,
            storage.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    if status < 0 {
        return Err(WindowsHelperError::Capability);
    }
    // SAFETY: a successful query initialized OBJECT_NAME_INFORMATION at the aligned buffer start.
    let information = unsafe { &*(storage.as_ptr().cast::<OBJECT_NAME_INFORMATION>()) };
    let length = usize::from(information.Name.Length);
    let address = information.Name.Buffer as usize;
    let start = storage.as_ptr() as usize;
    let end = start
        .checked_add(storage.len() * std::mem::size_of::<usize>())
        .ok_or(WindowsHelperError::Capability)?;
    if length == 0
        || length % std::mem::size_of::<u16>() != 0
        || address < start
        || address
            .checked_add(length)
            .is_none_or(|name_end| name_end > end)
    {
        return Err(WindowsHelperError::Capability);
    }
    // SAFETY: the validated UNICODE_STRING range lies wholly inside `storage`.
    Ok(unsafe { std::slice::from_raw_parts(information.Name.Buffer, length / 2) }.to_vec())
}

fn clear_inheritance(handle: HANDLE) -> Result<(), WindowsHelperError> {
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(WindowsHelperError::Capability);
    }
    let mut flags = HANDLE_FLAG_INHERIT;
    if unsafe { GetHandleInformation(handle, &mut flags) } == 0 || flags & HANDLE_FLAG_INHERIT != 0
    {
        Err(WindowsHelperError::Capability)
    } else {
        Ok(())
    }
}

fn read_request(descriptor: &OwnedHandle, budget: Duration) -> Result<VendorRequest, Refusal> {
    let deadline = Instant::now()
        .checked_add(budget)
        .unwrap_or_else(Instant::now);
    let raw = descriptor.as_raw_handle() as HANDLE;
    let mut bytes = Vec::with_capacity(HEADER_BYTES);
    let mut expected = None;
    loop {
        if Instant::now() >= deadline {
            return Err(Refusal::Deadline);
        }
        let mut available = 0_u32;
        if unsafe { PeekNamedPipe(raw, null_mut(), 0, null_mut(), &mut available, null_mut()) } == 0
        {
            if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
                break;
            }
            return Err(Refusal::Truncated);
        }
        if available == 0 {
            thread::sleep(Duration::from_millis(1));
            continue;
        }
        let remaining = MAX_HELPER_FRAME_BYTES
            .saturating_add(1)
            .saturating_sub(bytes.len());
        let amount = usize::try_from(available)
            .unwrap_or(usize::MAX)
            .min(remaining)
            .min(4096);
        let mut chunk = [0_u8; 4096];
        let mut received = 0_u32;
        if unsafe {
            ReadFile(
                raw,
                chunk.as_mut_ptr(),
                amount as u32,
                &mut received,
                null_mut(),
            )
        } == 0
        {
            return Err(Refusal::Truncated);
        }
        bytes.extend_from_slice(&chunk[..received as usize]);
        if expected.is_none() && bytes.len() >= HEADER_BYTES {
            expected = Some(parse_request_header(&bytes[..HEADER_BYTES])?);
        }
        if bytes.len() > expected.unwrap_or(MAX_HELPER_FRAME_BYTES) {
            return Err(Refusal::Surplus);
        }
        if bytes.len() > MAX_HELPER_FRAME_BYTES {
            return Err(Refusal::FrameTooLarge);
        }
    }
    let expected = expected.ok_or(Refusal::Truncated)?;
    if bytes.len() < expected {
        return Err(Refusal::Truncated);
    }
    if bytes.len() > expected {
        return Err(Refusal::Surplus);
    }
    let kind = match u16::from_be_bytes([bytes[6], bytes[7]]) {
        CONNECT_BEGIN => VendorRequestKind::Connect,
        CREDENTIAL_BEGIN => VendorRequestKind::Credential,
        _ => return Err(Refusal::UnexpectedFrame),
    };
    Ok(VendorRequest { bytes, kind })
}

fn parse_request_header(header: &[u8]) -> Result<usize, Refusal> {
    if &header[..4] != b"FXLM" {
        return Err(Refusal::InvalidFrame);
    }
    if header[4] != 1 {
        return Err(Refusal::UnsupportedVersion);
    }
    if header[5] != CLIENT_DIRECTION {
        return Err(Refusal::WrongDirection);
    }
    let opcode = u16::from_be_bytes([header[6], header[7]]);
    if !known_opcode(opcode) {
        return Err(Refusal::InvalidFrame);
    }
    if !matches!(opcode, CONNECT_BEGIN | CREDENTIAL_BEGIN) {
        return Err(Refusal::UnexpectedFrame);
    }
    let payload = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if payload > MAX_CONTROL_BYTES {
        return Err(Refusal::FrameTooLarge);
    }
    HEADER_BYTES
        .checked_add(payload)
        .ok_or(Refusal::FrameTooLarge)
}

fn validate_vendor_terminal(bytes: &[u8], request: VendorRequestKind) -> Result<(), Refusal> {
    if bytes.len() < HEADER_BYTES {
        return Err(Refusal::Truncated);
    }
    if bytes.len() > MAX_HELPER_FRAME_BYTES {
        return Err(Refusal::FrameTooLarge);
    }
    if &bytes[..4] != b"FXLM" {
        return Err(Refusal::InvalidFrame);
    }
    if bytes[4] != 1 {
        return Err(Refusal::UnsupportedVersion);
    }
    if bytes[5] != SERVER_DIRECTION {
        return Err(Refusal::WrongDirection);
    }
    let opcode = u16::from_be_bytes([bytes[6], bytes[7]]);
    if !known_opcode(opcode) {
        return Err(Refusal::InvalidFrame);
    }
    let permitted = match request {
        VendorRequestKind::Connect => matches!(opcode, CONNECT_RECEIPT | ERROR),
        VendorRequestKind::Credential => matches!(opcode, CREDENTIAL_RECEIPT | ERROR),
    };
    if !permitted {
        return Err(Refusal::UnexpectedFrame);
    }
    let payload = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if payload > MAX_CONTROL_BYTES {
        return Err(Refusal::FrameTooLarge);
    }
    match bytes.len().cmp(&(HEADER_BYTES + payload)) {
        std::cmp::Ordering::Less => Err(Refusal::Truncated),
        std::cmp::Ordering::Greater => Err(Refusal::Surplus),
        std::cmp::Ordering::Equal => Ok(()),
    }
}

fn known_opcode(opcode: u16) -> bool {
    matches!(
        opcode,
        0x0001..=0x0008 | 0x0010..=0x0014 | 0x0020..=0x0022 | 0x0030..=0x0033 | ERROR
    )
}

fn finish_response(response: OwnedHandle, bytes: &[u8]) -> HelperExit {
    let raw = response.into_raw_handle() as HANDLE;
    let mut written = 0_usize;
    while written < bytes.len() {
        let mut count = 0_u32;
        let remaining = (bytes.len() - written).min(u32::MAX as usize);
        if unsafe {
            WriteFile(
                raw,
                bytes[written..].as_ptr(),
                remaining as u32,
                &mut count,
                null_mut(),
            )
        } == 0
            || count == 0
        {
            // SAFETY: ownership was extracted once and this branch terminates its use.
            unsafe { CloseHandle(raw) };
            return HelperExit::CapabilityOrTransportFailure;
        }
        written += count as usize;
    }
    // SAFETY: ownership was extracted once; successful close produces the required EOF.
    if unsafe { CloseHandle(raw) } != 0 {
        HelperExit::TerminalFrameWritten
    } else {
        HelperExit::CapabilityOrTransportFailure
    }
}

#[derive(Clone, Copy)]
enum Refusal {
    InvalidFrame,
    UnsupportedVersion,
    WrongDirection,
    UnexpectedFrame,
    FrameTooLarge,
    Truncated,
    Surplus,
    Deadline,
    LocalManagementUnavailable,
}

impl Refusal {
    fn frame(self) -> Vec<u8> {
        let (code, status, retry) = match self {
            Self::InvalidFrame => ("invalid_frame", 400, "never"),
            Self::UnsupportedVersion => ("unsupported_version", 426, "never"),
            Self::WrongDirection => ("wrong_direction", 400, "never"),
            Self::UnexpectedFrame => ("unexpected_frame", 409, "never"),
            Self::FrameTooLarge => ("frame_too_large", 413, "never"),
            Self::Truncated => ("truncated_frame", 400, "never"),
            Self::Surplus => ("surplus_data", 400, "never"),
            Self::Deadline => ("deadline_exceeded", 408, "refresh"),
            Self::LocalManagementUnavailable => ("local_management_unavailable", 503, "operator"),
        };
        let payload = format!(
            "{{\"code\":\"{code}\",\"commit\":\"none\",\"retry\":\"{retry}\",\"schema\":\"exchange.local-management-error.v1\",\"status\":{status}}}"
        );
        encode_frame(SERVER_DIRECTION, ERROR, payload.as_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowsHelperError {
    Capability,
    Console,
    Deadline,
    EndpointChanged,
    OwnerIdentity,
    Protocol,
    Secret,
    Transport,
}

impl fmt::Display for WindowsHelperError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Capability => "Windows helper capability is invalid",
            Self::Console => "Windows console is unavailable",
            Self::Deadline => "Windows helper deadline elapsed",
            Self::EndpointChanged => "Windows local-management endpoint changed",
            Self::OwnerIdentity => "Windows local-management owner identity refused",
            Self::Protocol => "Windows local-management protocol refused",
            Self::Secret => "Windows console input is invalid",
            Self::Transport => "Windows local-management transport failed",
        })
    }
}

impl std::error::Error for WindowsHelperError {}

pub(crate) trait ConsolePort {
    type Input;

    fn open_current_input(&mut self) -> Result<Self::Input, WindowsHelperError>;
    fn mode(&mut self, input: &Self::Input) -> Result<u32, WindowsHelperError>;
    fn set_mode(&mut self, input: &Self::Input, mode: u32) -> Result<(), WindowsHelperError>;
    fn read_line(
        &mut self,
        input: &Self::Input,
        maximum_utf16_units: usize,
    ) -> Result<Vec<u16>, WindowsHelperError>;
}

/// Testable core for the production `CONIN$` reader.
///
/// Standard handles are absent from the port by design, so null stdin/stdout/stderr cannot become
/// input. Echo is restored on every return path after the console mode was changed.
pub(crate) fn read_console_secret_with<P: ConsolePort>(
    port: &mut P,
) -> Result<ConsoleSecret, WindowsHelperError> {
    let input = port.open_current_input()?;
    let original = port.mode(&input)?;
    port.set_mode(&input, original & !ENABLE_ECHO_INPUT)?;
    let result = port.read_line(&input, 8_194);
    let restored = port.set_mode(&input, original);
    let mut units = match result {
        Ok(units) => units,
        Err(refusal) => {
            restored?;
            return Err(refusal);
        }
    };
    if restored.is_err() {
        units.fill(0);
        return Err(WindowsHelperError::Console);
    }
    while units
        .last()
        .is_some_and(|unit| *unit == u16::from(b'\r') || *unit == u16::from(b'\n'))
    {
        units.pop();
    }
    let decoded = String::from_utf16(&units).map_err(|_| WindowsHelperError::Secret);
    units.fill(0);
    let mut bytes = decoded?.into_bytes();
    if bytes.is_empty() || bytes.len() > 8_192 || bytes.contains(&0) {
        bytes.fill(0);
        return Err(WindowsHelperError::Secret);
    }
    Ok(ConsoleSecret(bytes))
}

struct KernelConsole;

impl ConsolePort for KernelConsole {
    type Input = OwnedHandle;

    fn open_current_input(&mut self) -> Result<Self::Input, WindowsHelperError> {
        let path = "CONIN$\0".encode_utf16().collect::<Vec<_>>();
        let raw = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            Err(WindowsHelperError::Console)
        } else {
            // SAFETY: successful CreateFileW returned a newly owned console handle.
            Ok(unsafe { OwnedHandle::from_raw_handle(raw.cast()) })
        }
    }

    fn mode(&mut self, input: &Self::Input) -> Result<u32, WindowsHelperError> {
        let mut mode = 0_u32;
        if unsafe { GetConsoleMode(input.as_raw_handle() as HANDLE, &mut mode) } == 0 {
            Err(WindowsHelperError::Console)
        } else {
            Ok(mode)
        }
    }

    fn set_mode(&mut self, input: &Self::Input, mode: u32) -> Result<(), WindowsHelperError> {
        if unsafe { SetConsoleMode(input.as_raw_handle() as HANDLE, mode) } == 0 {
            Err(WindowsHelperError::Console)
        } else {
            Ok(())
        }
    }

    fn read_line(
        &mut self,
        input: &Self::Input,
        maximum_utf16_units: usize,
    ) -> Result<Vec<u16>, WindowsHelperError> {
        let mut units = vec![0_u16; maximum_utf16_units];
        let mut read = 0_u32;
        if unsafe {
            ReadConsoleW(
                input.as_raw_handle() as HANDLE,
                units.as_mut_ptr(),
                maximum_utf16_units as u32,
                &mut read,
                null_mut(),
            )
        } == 0
        {
            units.fill(0);
            return Err(WindowsHelperError::Console);
        }
        units.truncate(read as usize);
        Ok(units)
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleMode(console: HANDLE, mode: *mut u32) -> i32;
    fn SetConsoleMode(console: HANDLE, mode: u32) -> i32;
    fn ReadConsoleW(
        console: HANDLE,
        buffer: *mut u16,
        characters_to_read: u32,
        characters_read: *mut u32,
        input_control: *mut std::ffi::c_void,
    ) -> i32;
}

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
