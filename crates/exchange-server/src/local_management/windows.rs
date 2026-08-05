use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr::{null_mut, NonNull};
use std::sync::Arc;

use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use windows_sys::Win32::Foundation::{
    CompareObjectHandles, DuplicateHandle, GetHandleInformation, GetLastError, LocalFree,
    SetHandleInformation, DUPLICATE_SAME_ACCESS, ERROR_INSUFFICIENT_BUFFER, FILETIME, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, RevertToSelf, TokenSessionId, TokenUser,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, GetFileType, WriteFile, FILE_TYPE_PIPE, SYNCHRONIZE,
};
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeClientProcessId, GetNamedPipeInfo, ImpersonateNamedPipeClient, PIPE_SERVER_END,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetProcessTimes, OpenProcess, OpenProcessToken,
    OpenThreadToken, WaitForSingleObject, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::codec::{Direction, Frame, Opcode, StreamDecoder};
use super::dispatcher::{expired_reply, native_frame_refusal, Transport};
use super::service_account::{OneShotWriter, WriterRefusal};
use super::{
    deadline::{finalize_native_connection, write_native_terminal},
    ActiveSession, DeadlineController, Dispatcher, SessionAdvance, SessionBegin,
    TransactionCoordinator,
};
use crate::state::AppState;

#[cfg(any(test, feature = "native-deadline-test-seam"))]
use super::deadline::finalize_native_terminal;

const PIPE_PREFIX: &str = r"\\.\pipe\flux-exchange-local-management-v1-";
const MAX_FRAME_BYTES: u32 = 65_548;
const ATTACHMENT_BYTES: usize = 16;

/// One first-instance, owner-authenticated local-management pipe endpoint.
///
/// The endpoint retains the process `TokenUser` captured at startup. A connected pipe is returned
/// only after named-pipe impersonation proves that the client has that exact SID and the server has
/// reverted to its own token. Callers can therefore read one FXLM operation without constructing a
/// second identity mechanism or trusting a client-supplied spelling of an account.
pub(crate) struct WindowsEndpoint {
    pipe_name: String,
    owner_sid: OwnedSid,
    waiting: Option<NamedPipeServer>,
}

struct AuthenticatedPipe {
    pipe: NamedPipeServer,
    client: PinnedClient,
}

struct PinnedClient {
    process_id: u32,
    creation: u64,
    process: OwnedHandle,
}

impl WindowsEndpoint {
    /// Bind identity to the authenticated account that started this process.
    pub(crate) fn bind() -> Result<Self, WindowsEndpointRefusal> {
        let owner_sid = process_token_sid()?;
        // Windows uses the kernel named-pipe namespace directly. No inherited filesystem path,
        // profile value or caller-controlled component exists here to traverse as a reparse point.
        let pipe_name = pipe_name_for_sid(owner_sid.bytes());
        let mut endpoint = Self {
            pipe_name,
            owner_sid,
            waiting: None,
        };
        // Holding the first instance is part of startup admission, not deferred work in the serve
        // task. Readiness can therefore never precede ownership of the authenticated endpoint.
        endpoint.waiting = Some(endpoint.create_instance(true)?);
        Ok(endpoint)
    }

    /// Accept one pipe while retaining the authenticated client's pinned process identity.
    async fn accept_authenticated(&mut self) -> Result<AuthenticatedPipe, WindowsEndpointRefusal> {
        let pipe = self.waiting.take().ok_or(WindowsEndpointRefusal::Bind)?;
        pipe.connect()
            .await
            .map_err(|_| WindowsEndpointRefusal::Connect)?;
        let raw = pipe.as_raw_handle() as HANDLE;
        authenticate_owner(raw, self.owner_sid.bytes())?;
        let client = PinnedClient::open(raw, self.owner_sid.bytes())?;
        Ok(AuthenticatedPipe { pipe, client })
    }

    fn rearm(&mut self) -> Result<(), WindowsEndpointRefusal> {
        if self.waiting.is_some() {
            return Err(WindowsEndpointRefusal::Bind);
        }
        // `FILE_FLAG_FIRST_PIPE_INSTANCE` is a startup namespace-ownership assertion. After
        // `DisconnectNamedPipe`, the old client can still hold its now-disconnected handle while
        // this server rearms; repeating the flag would make that harmless handle terminate the
        // service. The original successful bind already established ownership, and the same
        // protected descriptor plus single-instance limit remains authoritative here.
        self.waiting = Some(self.create_instance(false)?);
        Ok(())
    }

    fn create_instance(&self, first: bool) -> Result<NamedPipeServer, WindowsEndpointRefusal> {
        let descriptor = SecurityDescriptor::owner_and_system(&self.owner_sid)?;
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .map_err(|_| WindowsEndpointRefusal::Security)?,
            lpSecurityDescriptor: descriptor.as_ptr(),
            bInheritHandle: 0,
        };
        let mut options = ServerOptions::new();
        options
            .pipe_mode(PipeMode::Byte)
            .access_inbound(true)
            .access_outbound(true)
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(1)
            .in_buffer_size(MAX_FRAME_BYTES)
            .out_buffer_size(MAX_FRAME_BYTES);

        // SAFETY: `attributes` and its descriptor allocation remain live until CreateNamedPipeW
        // returns. Tokio always adds FILE_FLAG_OVERLAPPED; the options additionally select byte
        // mode and PIPE_REJECT_REMOTE_CLIENTS. Startup additionally selects
        // FILE_FLAG_FIRST_PIPE_INSTANCE; rearm cannot repeat that startup-only namespace claim.
        unsafe {
            options.create_with_security_attributes_raw(
                std::ffi::OsStr::new(&self.pipe_name),
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
            )
        }
        .map_err(|_| WindowsEndpointRefusal::Bind)
    }

    #[cfg(any(test, feature = "native-deadline-test-seam"))]
    fn pipe_name(&self) -> &str {
        &self.pipe_name
    }
}

impl PinnedClient {
    fn open(pipe: HANDLE, owner_sid: &[u8]) -> Result<Self, WindowsEndpointRefusal> {
        let mut process_id = 0_u32;
        if unsafe { GetNamedPipeClientProcessId(pipe, &mut process_id) } == 0 || process_id == 0 {
            return Err(WindowsEndpointRefusal::PeerIdentity);
        }
        let raw = unsafe {
            OpenProcess(
                PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(WindowsEndpointRefusal::PeerIdentity);
        }
        // SAFETY: successful OpenProcess returns one owned non-pseudo handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let expected = process_token_identity(unsafe { GetCurrentProcess() })?;
        let actual = process_token_identity(process.as_raw_handle() as HANDLE)?;
        if actual.sid != owner_sid
            || actual.session != expected.session
            || unsafe { WaitForSingleObject(process.as_raw_handle() as HANDLE, 0) } != WAIT_TIMEOUT
        {
            return Err(WindowsEndpointRefusal::PeerIdentity);
        }
        let creation = process_creation(process.as_raw_handle() as HANDLE)?;
        let mut confirmed = 0_u32;
        if unsafe { GetNamedPipeClientProcessId(pipe, &mut confirmed) } == 0
            || confirmed != process_id
            || process_creation(process.as_raw_handle() as HANDLE)? != creation
        {
            return Err(WindowsEndpointRefusal::PeerIdentity);
        }
        Ok(Self {
            process_id,
            creation,
            process,
        })
    }

    fn duplicate_writer(
        &self,
        pipe: HANDLE,
        source: u64,
    ) -> Result<WindowsWriter, AttachmentRefusal> {
        let source = usize::try_from(source).map_err(|_| AttachmentRefusal::WriterInvalid)?;
        if source == 0 {
            return Err(AttachmentRefusal::WriterInvalid);
        }
        let mut confirmed = 0_u32;
        let expected = process_token_identity(unsafe { GetCurrentProcess() })
            .map_err(|_| AttachmentRefusal::WriterInvalid)?;
        let actual = process_token_identity(self.process.as_raw_handle() as HANDLE)
            .map_err(|_| AttachmentRefusal::WriterInvalid)?;
        let creation = process_creation(self.process.as_raw_handle() as HANDLE)
            .map_err(|_| AttachmentRefusal::WriterInvalid)?;
        let live = unsafe { WaitForSingleObject(self.process.as_raw_handle() as HANDLE, 0) }
            == WAIT_TIMEOUT;
        if unsafe { GetNamedPipeClientProcessId(pipe, &mut confirmed) } == 0
            || !pinned_client_matches(
                self.process_id,
                confirmed,
                self.creation,
                creation,
                &expected,
                &actual,
                live,
            )
        {
            return Err(AttachmentRefusal::WriterInvalid);
        }
        let mut duplicate: HANDLE = null_mut();
        if unsafe {
            DuplicateHandle(
                self.process.as_raw_handle() as HANDLE,
                source as HANDLE,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
            || duplicate.is_null()
            || duplicate == INVALID_HANDLE_VALUE
        {
            return Err(AttachmentRefusal::WriterInvalid);
        }
        // SAFETY: DuplicateHandle returned one target-process-owned handle.
        let writer = unsafe { OwnedHandle::from_raw_handle(duplicate.cast()) };
        let raw = writer.as_raw_handle() as HANDLE;
        if unsafe { CompareObjectHandles(raw, pipe) } != 0
            || unsafe { CompareObjectHandles(raw, self.process.as_raw_handle() as HANDLE) } != 0
            || unsafe { GetFileType(raw) } != FILE_TYPE_PIPE
        {
            return Err(AttachmentRefusal::WriterInvalid);
        }
        let mut pipe_flags = 0_u32;
        if unsafe { GetNamedPipeInfo(raw, &mut pipe_flags, null_mut(), null_mut(), null_mut()) }
            == 0
            || pipe_flags & PIPE_SERVER_END != 0
            || unsafe { SetHandleInformation(raw, HANDLE_FLAG_INHERIT, 0) } == 0
        {
            return Err(AttachmentRefusal::WriterInvalid);
        }
        let mut flags = HANDLE_FLAG_INHERIT;
        if unsafe { GetHandleInformation(raw, &mut flags) } == 0 || flags & HANDLE_FLAG_INHERIT != 0
        {
            return Err(AttachmentRefusal::WriterInvalid);
        }
        Ok(WindowsWriter(writer))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachmentRefusal {
    Truncated,
    InvalidFrame,
    UnexpectedFrame,
    WriterInvalid,
}

impl AttachmentRefusal {
    const fn body(self) -> &'static [u8] {
        match self {
            Self::Truncated => br#"{"code":"truncated_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#,
            Self::InvalidFrame => br#"{"code":"invalid_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#,
            Self::UnexpectedFrame => br#"{"code":"unexpected_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":409}"#,
            Self::WriterInvalid => br#"{"code":"writer_invalid","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#,
        }
    }
}

fn parse_attachment(bytes: &[u8; ATTACHMENT_BYTES]) -> Result<u64, AttachmentRefusal> {
    if &bytes[..4] != b"FXHA" || bytes[4] != 1 || bytes[5] != 1 || bytes[6] != 1 || bytes[7] != 0 {
        return Err(AttachmentRefusal::WriterInvalid);
    }
    let source = u64::from_be_bytes(
        bytes[8..]
            .try_into()
            .map_err(|_| AttachmentRefusal::WriterInvalid)?,
    );
    (source != 0)
        .then_some(source)
        .ok_or(AttachmentRefusal::WriterInvalid)
}

struct WindowsWriter(OwnedHandle);

impl OneShotWriter for WindowsWriter {
    fn write_once(self: Box<Self>, frame: &[u8]) -> Result<(), WriterRefusal> {
        let raw = self.0.as_raw_handle() as HANDLE;
        let mut offset = 0_usize;
        while offset < frame.len() {
            let mut written = 0_u32;
            let remaining =
                u32::try_from(frame.len() - offset).map_err(|_| WriterRefusal::Invalid)?;
            if unsafe {
                WriteFile(
                    raw,
                    frame[offset..].as_ptr(),
                    remaining,
                    &mut written,
                    null_mut(),
                )
            } == 0
                || written == 0
            {
                return Err(WriterRefusal::Closed);
            }
            offset += written as usize;
        }
        if unsafe { FlushFileBuffers(raw) } == 0 {
            return Err(WriterRefusal::Closed);
        }
        Ok(())
    }
}

/// Supervised production composition of the authenticated Windows pipe and shared dispatcher.
pub(crate) struct LocalManagement {
    endpoint: WindowsEndpoint,
    dispatcher: Dispatcher,
    tenant: exchange_host::Tenant,
    #[cfg(any(test, feature = "native-deadline-test-seam"))]
    deadline_override: Option<DeadlineController>,
}

impl LocalManagement {
    /// Bind the first pipe instance before readiness only for supervised single-tenant startup.
    pub(crate) fn bind_for_mode(
        supervised: bool,
        state: AppState,
        coordinator: Option<Arc<TransactionCoordinator>>,
    ) -> Result<Option<Self>, String> {
        if !supervised {
            return Ok(None);
        }
        let coordinator = coordinator.ok_or_else(|| {
            "the supervised local-management endpoint has no transaction coordinator".to_owned()
        })?;
        let tenant = exchange_host::Tenant::new("local")
            .map_err(|_| "the fixed native owner tenant is invalid".to_owned())?;
        let dispatcher = Dispatcher::new(state, coordinator);
        let endpoint = WindowsEndpoint::bind().map_err(|refusal| refusal.to_string())?;
        Ok(Some(Self {
            endpoint,
            dispatcher,
            tenant,
            #[cfg(any(test, feature = "native-deadline-test-seam"))]
            deadline_override: None,
        }))
    }

    /// Accept authenticated owner operations until shutdown or an endpoint integrity refusal.
    pub(crate) async fn serve(mut self) {
        #[cfg(any(test, feature = "native-deadline-test-seam"))]
        let deadline_override = self.deadline_override.clone();
        loop {
            let Ok(mut connection) = self.endpoint.accept_authenticated().await else {
                return;
            };
            #[cfg(any(test, feature = "native-deadline-test-seam"))]
            let deadline = deadline_override
                .clone()
                .unwrap_or_else(DeadlineController::start);
            #[cfg(not(any(test, feature = "native-deadline-test-seam")))]
            let deadline = DeadlineController::start();
            if let Err(expired) = deadline
                .race(dispatch_one(
                    &mut connection,
                    &self.dispatcher,
                    &self.tenant,
                    &deadline,
                ))
                .await
            {
                let (reply, _) = expired_reply(expired).into_parts();
                finalize_native_connection(&mut connection.pipe, Some(&reply)).await;
            }
            // DisconnectNamedPipe is the bounded Windows analogue of closing the Unix read half:
            // it rejects raced client writes without a read loop and makes the completed server
            // shutdown observable before this authenticated connection is dropped.
            let _ = connection.pipe.disconnect();
            // The endpoint is rearmed only after both the pipe and its pinned client process have
            // been dropped, so no attachment can be associated with the next connection.
            drop(connection);
            if self.endpoint.rearm().is_err() {
                return;
            }
        }
    }
}

async fn dispatch_one(
    connection: &mut AuthenticatedPipe,
    dispatcher: &Dispatcher,
    tenant: &exchange_host::Tenant,
    deadline: &DeadlineController,
) -> std::io::Result<()> {
    let mut prefix = [0_u8; 4];
    let prefix_length = read_prefix(&mut connection.pipe, &mut prefix).await?;
    if prefix_length == 0 {
        return Ok(());
    }

    let mut writer: Option<Box<dyn OneShotWriter>> = None;
    if prefix_length < prefix.len() {
        if b"FXHA".starts_with(&prefix[..prefix_length]) {
            refuse_attachment(&mut connection.pipe, AttachmentRefusal::Truncated, deadline).await?;
        }
        return Ok(());
    }
    if &prefix == b"FXHA" {
        let mut attachment = [0_u8; ATTACHMENT_BYTES];
        attachment[..4].copy_from_slice(&prefix);
        if read_prefix(&mut connection.pipe, &mut attachment[4..]).await? < ATTACHMENT_BYTES - 4 {
            refuse_attachment(&mut connection.pipe, AttachmentRefusal::Truncated, deadline).await?;
            return Ok(());
        }
        let source = match parse_attachment(&attachment) {
            Ok(source) => source,
            Err(refusal) => {
                refuse_attachment(&mut connection.pipe, refusal, deadline).await?;
                return Ok(());
            }
        };
        let duplicate = match connection
            .client
            .duplicate_writer(connection.pipe.as_raw_handle() as HANDLE, source)
        {
            Ok(writer) => writer,
            Err(refusal) => {
                refuse_attachment(&mut connection.pipe, refusal, deadline).await?;
                return Ok(());
            }
        };
        writer = Some(Box::new(duplicate));
        let following = read_prefix(&mut connection.pipe, &mut prefix).await?;
        if following < prefix.len() {
            refuse_attachment(&mut connection.pipe, AttachmentRefusal::Truncated, deadline).await?;
            return Ok(());
        }
        let refusal = if &prefix == b"FXHA" {
            Some(AttachmentRefusal::UnexpectedFrame)
        } else if &prefix != b"FXLM" {
            Some(AttachmentRefusal::InvalidFrame)
        } else {
            None
        };
        if let Some(refusal) = refusal {
            refuse_attachment(&mut connection.pipe, refusal, deadline).await?;
            return Ok(());
        }
    }

    let mut decoder = StreamDecoder::new(Direction::ClientToServer);
    let mut bytes = [0_u8; 4096];
    let mut active: Option<Box<ActiveSession>> = None;
    if let Err(error) = decoder.push(&prefix) {
        let response = native_frame_refusal(error);
        write_native_terminal(&mut connection.pipe, &response, deadline).await;
        return Ok(());
    }
    let mut initial_pending = true;
    loop {
        let used_initial = initial_pending;
        let received = if used_initial {
            initial_pending = false;
            prefix.len()
        } else {
            connection.pipe.read(&mut bytes).await?
        };
        if received == 0 {
            if deadline.may_abort() {
                if let Some(session) = active.as_mut() {
                    session.abort().await;
                }
            }
            if let Err(error) = decoder.finish() {
                let response = native_frame_refusal(error);
                write_native_terminal(&mut connection.pipe, &response, deadline).await;
            }
            return Ok(());
        }
        if !used_initial {
            if let Err(error) = decoder.push(&bytes[..received]) {
                if let Some(session) = active.as_mut() {
                    session.abort().await;
                }
                let response = native_frame_refusal(error);
                write_native_terminal(&mut connection.pipe, &response, deadline).await;
                return Ok(());
            }
        }
        while let Some(request) = match decoder.next_frame() {
            Ok(frame) => frame,
            Err(error) => {
                if deadline.may_abort() {
                    if let Some(session) = active.as_mut() {
                        session.abort().await;
                    }
                }
                let response = native_frame_refusal(error);
                write_native_terminal(&mut connection.pipe, &response, deadline).await;
                return Ok(());
            }
        } {
            if let Some(session) = active.as_mut() {
                match session.accept_frame(request).await {
                    SessionAdvance::Awaiting => {}
                    SessionAdvance::Terminal(reply) => {
                        let (response, _) = reply.into_parts();
                        write_native_terminal(&mut connection.pipe, &response, deadline).await;
                        return Ok(());
                    }
                }
            } else {
                if writer.is_some() {
                    let refusal = if request.opcode() != Opcode::ServiceAccountMint {
                        Some(AttachmentRefusal::UnexpectedFrame)
                    } else if decoder.buffered_bytes().starts_with(b"FXHA") {
                        Some(AttachmentRefusal::UnexpectedFrame)
                    } else if !decoder.buffered_bytes().is_empty() {
                        Some(AttachmentRefusal::InvalidFrame)
                    } else {
                        // The attachment is consumed by exactly this immediate MINT.
                        // Later reads cannot inherit the writer because `take` clears it.
                        None
                    };
                    if let Some(refusal) = refusal {
                        refuse_attachment(&mut connection.pipe, refusal, deadline).await?;
                        return Ok(());
                    }
                }
                match dispatcher
                    .begin_frame_with_writer(
                        Transport::Native,
                        tenant,
                        request,
                        writer.take(),
                        deadline,
                    )
                    .await
                {
                    SessionBegin::Terminal(reply) => {
                        let (response, _) = reply.into_parts();
                        write_native_terminal(&mut connection.pipe, &response, deadline).await;
                        return Ok(());
                    }
                    SessionBegin::Active { response, session } => {
                        deadline
                            .race_response(connection.pipe.write_all(&response))
                            .await
                            .map_err(|()| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
                        active = Some(session);
                    }
                }
            }
        }
    }
}

async fn read_prefix(stream: &mut NamedPipeServer, bytes: &mut [u8]) -> std::io::Result<usize> {
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let received = stream.read(&mut bytes[offset..]).await?;
        if received == 0 {
            break;
        }
        offset += received;
    }
    Ok(offset)
}

async fn refuse_attachment(
    stream: &mut NamedPipeServer,
    refusal: AttachmentRefusal,
    deadline: &DeadlineController,
) -> std::io::Result<()> {
    if let Ok(frame) = Frame::control(
        Direction::ServerToClient,
        Opcode::Error,
        refusal.body().to_vec(),
    ) {
        write_native_terminal(stream, &frame.encode(), deadline).await;
        return Ok(());
    }
    finalize_native_connection(stream, None).await;
    Ok(())
}

/// A fixed, value-free native endpoint refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowsEndpointRefusal {
    OwnerIdentity,
    Security,
    Bind,
    Connect,
    PeerIdentity,
    Revert,
}

impl WindowsEndpointRefusal {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::OwnerIdentity => "owner_identity_unavailable",
            Self::Security => "local_management_security_refused",
            Self::Bind => "local_management_bind_refused",
            Self::Connect => "local_management_connect_refused",
            Self::PeerIdentity => "local_management_peer_refused",
            Self::Revert => "local_management_revert_refused",
        }
    }
}

impl fmt::Display for WindowsEndpointRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

struct OwnedSid {
    storage: Vec<usize>,
    offset: usize,
    length: usize,
}

impl OwnedSid {
    fn bytes(&self) -> &[u8] {
        let base = self.storage.as_ptr().cast::<u8>();
        // SAFETY: construction validates that `[offset, offset + length)` lies within storage.
        unsafe { std::slice::from_raw_parts(base.add(self.offset), self.length) }
    }

    fn as_ptr(&self) -> PSID {
        self.bytes().as_ptr().cast_mut().cast()
    }
}

fn process_token_sid() -> Result<OwnedSid, WindowsEndpointRefusal> {
    let mut raw: HANDLE = null_mut();
    // SAFETY: the pseudo-handle remains process-owned; success initializes one owned token handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(WindowsEndpointRefusal::OwnerIdentity);
    }
    let token = owned_handle(raw).ok_or(WindowsEndpointRefusal::OwnerIdentity)?;
    token_sid(
        token.as_raw_handle() as HANDLE,
        WindowsEndpointRefusal::OwnerIdentity,
    )
}

#[derive(Clone, PartialEq, Eq)]
struct TokenIdentity {
    sid: Vec<u8>,
    session: u32,
}

fn pinned_client_matches(
    expected_process: u32,
    confirmed_process: u32,
    expected_creation: u64,
    confirmed_creation: u64,
    expected_token: &TokenIdentity,
    confirmed_token: &TokenIdentity,
    live: bool,
) -> bool {
    expected_process == confirmed_process
        && expected_creation == confirmed_creation
        && expected_token == confirmed_token
        && live
}

fn process_token_identity(process: HANDLE) -> Result<TokenIdentity, WindowsEndpointRefusal> {
    let mut raw: HANDLE = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } == 0 {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    let token = owned_handle(raw).ok_or(WindowsEndpointRefusal::PeerIdentity)?;
    let sid = token_sid(
        token.as_raw_handle() as HANDLE,
        WindowsEndpointRefusal::PeerIdentity,
    )?
    .bytes()
    .to_vec();
    let mut session = 0_u32;
    let mut length = size_of::<u32>() as u32;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenSessionId,
            (&mut session as *mut u32).cast(),
            length,
            &mut length,
        )
    } == 0
        || length as usize != size_of::<u32>()
    {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    Ok(TokenIdentity { sid, session })
}

fn process_creation(process: HANDLE) -> Result<u64, WindowsEndpointRefusal> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn thread_token_sid() -> Result<OwnedSid, WindowsEndpointRefusal> {
    let mut raw: HANDLE = null_mut();
    // `OpenAsSelf = FALSE` asks for the impersonated client's thread token, never the process token.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut raw) } == 0 {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    let token = owned_handle(raw).ok_or(WindowsEndpointRefusal::PeerIdentity)?;
    token_sid(
        token.as_raw_handle() as HANDLE,
        WindowsEndpointRefusal::PeerIdentity,
    )
}

fn token_sid(
    token: HANDLE,
    refusal: WindowsEndpointRefusal,
) -> Result<OwnedSid, WindowsEndpointRefusal> {
    let mut length = 0_u32;
    // SAFETY: the null-buffer sizing call writes only the required byte count.
    let sized = unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut length) };
    if sized != 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(refusal);
    }
    let capacity = length as usize;
    if capacity < size_of::<TOKEN_USER>() {
        return Err(refusal);
    }
    let mut storage = vec![0_usize; capacity.div_ceil(size_of::<usize>())];
    // SAFETY: the aligned allocation is at least `capacity` bytes and remains live below.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
        || length as usize > capacity
    {
        return Err(refusal);
    }
    let base = storage.as_ptr().cast::<u8>();
    // SAFETY: the buffer holds a TOKEN_USER after successful GetTokenInformation.
    let sid = unsafe { (*(base.cast::<TOKEN_USER>())).User.Sid.cast::<u8>() };
    let offset = (sid as usize)
        .checked_sub(base as usize)
        .filter(|offset| *offset < capacity)
        .ok_or(refusal)?;
    if unsafe { IsValidSid(sid.cast()) } == 0 {
        return Err(refusal);
    }
    let sid_length = unsafe { GetLengthSid(sid.cast()) } as usize;
    if offset
        .checked_add(sid_length)
        .is_none_or(|end| end > capacity)
    {
        return Err(refusal);
    }
    Ok(OwnedSid {
        storage,
        offset,
        length: sid_length,
    })
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

fn authenticate_owner(pipe: HANDLE, expected_sid: &[u8]) -> Result<(), WindowsEndpointRefusal> {
    authenticated_flow(
        expected_sid,
        || {
            if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
                Err(WindowsEndpointRefusal::PeerIdentity)
            } else {
                Ok(())
            }
        },
        || thread_token_sid().map(|sid| sid.bytes().to_vec()),
        || unsafe { RevertToSelf() } != 0,
    )
}

fn authenticated_flow<Begin, Query, Revert>(
    expected_sid: &[u8],
    begin: Begin,
    query: Query,
    revert: Revert,
) -> Result<(), WindowsEndpointRefusal>
where
    Begin: FnOnce() -> Result<(), WindowsEndpointRefusal>,
    Query: FnOnce() -> Result<Vec<u8>, WindowsEndpointRefusal>,
    Revert: FnMut() -> bool,
{
    begin()?;
    let mut guard = RevertGuard::new(revert);
    let peer = query();
    if !guard.finish() {
        return Err(WindowsEndpointRefusal::Revert);
    }
    let peer = peer?;
    if peer != expected_sid {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    Ok(())
}

struct RevertGuard<Revert: FnMut() -> bool> {
    revert: Option<Revert>,
}

impl<Revert: FnMut() -> bool> RevertGuard<Revert> {
    fn new(revert: Revert) -> Self {
        Self {
            revert: Some(revert),
        }
    }

    fn finish(&mut self) -> bool {
        self.revert.take().is_some_and(|mut revert| revert())
    }
}

impl<Revert: FnMut() -> bool> Drop for RevertGuard<Revert> {
    fn drop(&mut self) {
        if let Some(mut revert) = self.revert.take() {
            let _ = revert();
        }
    }
}

struct SecurityDescriptor(NonNull<c_void>);

impl SecurityDescriptor {
    fn owner_and_system(owner: &OwnedSid) -> Result<Self, WindowsEndpointRefusal> {
        let owner = sid_string(owner.as_ptr())?;
        // `P` marks the DACL protected. These are the only two ACEs: LocalSystem and the startup
        // owner. There is deliberately no Administrators, BUILTIN Users, Everyone or AppContainer
        // grant and the handle is non-inheritable.
        let sddl = format!("O:{owner}D:P(A;;GA;;;SY)(A;;GA;;;{owner})");
        let mut encoded: Vec<u16> = std::ffi::OsStr::new(&sddl).encode_wide().collect();
        encoded.push(0);
        let mut raw: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: the UTF-16 input is NUL-terminated; success returns a LocalAlloc descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_ptr(),
                SDDL_REVISION_1,
                &mut raw,
                null_mut(),
            )
        } == 0
        {
            return Err(WindowsEndpointRefusal::Security);
        }
        NonNull::new(raw.cast())
            .map(Self)
            .ok_or(WindowsEndpointRefusal::Security)
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: this is the exact LocalAlloc pointer returned by the SDDL converter.
        unsafe {
            let _ = LocalFree(self.0.as_ptr());
        }
    }
}

fn sid_string(sid: PSID) -> Result<String, WindowsEndpointRefusal> {
    let mut raw = null_mut();
    // SAFETY: callers pass a validated live SID; success returns a LocalAlloc UTF-16 string.
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 || raw.is_null() {
        return Err(WindowsEndpointRefusal::Security);
    }
    let length = unsafe { (0..).take_while(|offset| *raw.add(*offset) != 0).count() };
    let rendered = String::from_utf16(unsafe { std::slice::from_raw_parts(raw, length) })
        .map_err(|_| WindowsEndpointRefusal::Security);
    unsafe {
        let _ = LocalFree(raw.cast());
    }
    rendered
}

fn owned_handle(raw: HANDLE) -> Option<OwnedHandle> {
    if raw.is_null() {
        None
    } else {
        // SAFETY: successful token APIs return a new handle owned by the caller.
        Some(unsafe { OwnedHandle::from_raw_handle(raw.cast()) })
    }
}

#[cfg(feature = "native-deadline-test-seam")]
pub(crate) fn run_deadline_process_fixture() -> Result<(), String> {
    tests::run_deadline_process_fixture()
}

#[cfg(any(test, feature = "native-deadline-test-seam"))]
#[cfg_attr(not(test), allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    use exchange_host::{
        GrantReceiptId, GrantSelector, GrantStore, GrantTransactions as _, Tenant,
    };
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows_sys::Win32::Foundation::{
        GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_NO_TOKEN, ERROR_PIPE_NOT_CONNECTED,
        ERROR_SUCCESS, HANDLE,
    };
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows_sys::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SE_DACL_PRESENT,
        SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

    use crate::audit::AuditJournal;
    use crate::local_management::codec::Opcode;
    use crate::local_management::{Expired, ReceiptIdentity, Unresolved};

    static ENDPOINT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn private_root(name: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x135-windows-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        exchange_host::ensure_private_state_directory(&root).expect("private test root");
        root
    }

    fn test_dispatcher(root: &std::path::Path) -> Dispatcher {
        let store = exchange_host::CredentialStore::bind(root.join("credentials/store"))
            .expect("credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("transactions/journal.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("transaction coordinator"),
        );
        Dispatcher::new(AppState::without_identity(), coordinator)
    }

    fn grant_dispatcher(root: &std::path::Path, seed: bool) -> (Dispatcher, GrantReceiptId) {
        let store = exchange_host::CredentialStore::bind(root.join("credentials/store"))
            .expect("credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("transactions/journal.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("transaction coordinator"),
        );
        let audit =
            Arc::new(AuditJournal::bind(root.join("audit/events.sqlite3")).expect("audit journal"));
        let grants = Arc::new(GrantStore::bind(root.join("grants.json")).expect("grant store"));
        let receipt = GrantReceiptId::from_protocol_bytes([0x71; 32]).expect("receipt");
        if seed {
            let tenant = Tenant::new("local").expect("tenant");
            let selector: GrantSelector = serde_json::from_slice(
                br#"{"effects_within":null,"idempotency":null,"max_risk":"low"}"#,
            )
            .expect("selector");
            let candidate = grants
                .preview(&tenant, "github", selector)
                .expect("grant preview");
            grants
                .apply(
                    &tenant,
                    &candidate.candidate,
                    candidate.revision,
                    candidate.proposal_digest,
                    receipt,
                )
                .expect("durable grant decision");
        }
        let state = AppState::without_identity()
            .with_transaction_coordinator(coordinator.clone())
            .with_audit(audit)
            .with_grant_transactions(grants);
        (Dispatcher::new(state, coordinator), receipt)
    }

    fn local_management(
        dispatcher: Dispatcher,
        deadline: DeadlineController,
    ) -> (LocalManagement, String) {
        let endpoint = WindowsEndpoint::bind().expect("owner endpoint");
        let pipe_name = endpoint.pipe_name().to_owned();
        (
            LocalManagement {
                endpoint,
                dispatcher,
                tenant: Tenant::new("local").expect("tenant"),
                deadline_override: Some(deadline),
            },
            pipe_name,
        )
    }

    async fn read_named_pipe_to_end<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
        bytes: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        let mut chunk = [0_u8; 4096];
        loop {
            match tokio::io::AsyncReadExt::read(reader, &mut chunk).await {
                Ok(0) => return Ok(()),
                Ok(received) => bytes.extend_from_slice(&chunk[..received]),
                Err(error)
                    if error.raw_os_error().is_some_and(|code| {
                        code == ERROR_BROKEN_PIPE as i32 || code == ERROR_PIPE_NOT_CONNECTED as i32
                    }) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn deadline_process_fixture() {
        let _endpoint_guard = ENDPOINT_TEST_LOCK.lock().expect("endpoint test lock");

        let root = private_root("predecision");
        let deadline = DeadlineController::start();
        let (local, pipe_name) = local_management(test_dispatcher(&root), deadline.clone());
        let server = tokio::spawn(local.serve());
        let client = ClientOptions::new().open(&pipe_name).expect("owner pipe");
        let (mut reader, mut writer) = tokio::io::split(client);
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"FXLM")
            .await
            .expect("partial header");
        let response = tokio::spawn(async move {
            let mut bytes = Vec::new();
            read_named_pipe_to_end(&mut reader, &mut bytes)
                .await
                .expect("pre-decision response EOF");
            bytes
        });
        tokio::time::advance(std::time::Duration::from_secs(299)).await;
        tokio::io::AsyncWriteExt::write_all(&mut writer, &[1, 0, 0, 1])
            .await
            .expect("partial traffic at 299 seconds");
        tokio::task::yield_now().await;
        assert!(!response.is_finished());
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            response.await.expect("pre-decision response"),
            crate::local_management::deadline_frame()
        );
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(root);

        let post_root = private_root("postdecision");
        let (dispatcher, receipt) = grant_dispatcher(&post_root, true);
        let receipt_identity = ReceiptIdentity::from_protocol_bytes(receipt.protocol_bytes())
            .expect("receipt identity");
        let deadline = DeadlineController::start();
        let (local, pipe_name) = local_management(dispatcher, deadline.clone());
        let server = tokio::spawn(local.serve());
        let client = ClientOptions::new().open(&pipe_name).expect("owner pipe");
        let (mut reader, mut writer) = tokio::io::split(client);
        tokio::task::yield_now().await;
        deadline
            .decided(receipt_identity, Unresolved::Store)
            .expect("durable decision");
        let response = tokio::spawn(async move {
            let mut bytes = Vec::new();
            read_named_pipe_to_end(&mut reader, &mut bytes)
                .await
                .expect("post-decision response EOF");
            bytes
        });
        tokio::time::advance(std::time::Duration::from_secs(29)).await;
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"FX")
            .await
            .expect("partial traffic at 29 seconds");
        tokio::task::yield_now().await;
        assert!(!response.is_finished());
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        let (expected, close_code) =
            crate::local_management::expired_reply(Expired::PostDecision {
                receipt: receipt_identity,
                unresolved: Unresolved::Store,
            })
            .into_parts();
        assert_eq!(close_code, 1000);
        assert_eq!(
            response.await.expect("post-decision response"),
            expected,
            "the Windows native stream ends in clean EOF"
        );
        server.abort();
        let _ = server.await;

        let replay_root = private_root("replay-endpoint");
        let (dispatcher, reopened_receipt) = grant_dispatcher(&post_root, false);
        assert_eq!(reopened_receipt, receipt);
        let replay_deadline = DeadlineController::start();
        let (local, pipe_name) = local_management(dispatcher, replay_deadline);
        let server = tokio::spawn(local.serve());
        let mut client = ClientOptions::new().open(&pipe_name).expect("replay pipe");
        let query = format!(r#"{{"receipt_id":"{receipt}"}}"#);
        let query = Frame::control(
            Direction::ClientToServer,
            Opcode::GrantQuery,
            query.into_bytes(),
        )
        .expect("grant QUERY")
        .encode();
        tokio::io::AsyncWriteExt::write_all(&mut client, &query)
            .await
            .expect("grant QUERY write");
        tokio::io::AsyncWriteExt::shutdown(&mut client)
            .await
            .expect("grant QUERY EOF");
        let mut replayed = Vec::new();
        read_named_pipe_to_end(&mut client, &mut replayed)
            .await
            .expect("grant replay EOF");
        let replayed = String::from_utf8_lossy(&replayed);
        assert!(replayed.contains(&receipt.to_string()));
        assert!(replayed.contains("\"replayed\":true"));
        server.abort();
        let _ = server.await;

        // A real named pipe with its production output buffer proves that a blocked terminal frame
        // cannot consume the EOF reservation. The authenticated client is retained through the
        // finalizer exactly as it is by `LocalManagement::serve`.
        let mut endpoint = WindowsEndpoint::bind().expect("terminal endpoint");
        let pipe_name = endpoint.pipe_name().to_owned();
        let accepting = tokio::spawn(async move {
            endpoint
                .accept_authenticated()
                .await
                .expect("authenticated terminal pipe")
        });
        let mut terminal_reader = ClientOptions::new()
            .open(&pipe_name)
            .expect("terminal pipe client");
        let mut terminal_connection = accepting.await.expect("terminal accept task");
        terminal_connection
            .pipe
            .writable()
            .await
            .expect("terminal pipe initially writable");
        let filler = [0x5a_u8; 65_536];
        let mut filled = 0_usize;
        loop {
            match terminal_connection.pipe.try_write(&filler) {
                Ok(0) => panic!("a writable Windows pipe accepted zero bytes"),
                Ok(written) => {
                    filled += written;
                    assert!(
                        filled <= 64 * 1024 * 1024,
                        "the production Windows pipe never established backpressure"
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("fill authenticated terminal pipe: {error}"),
            }
        }
        assert_ne!(filled, 0, "the production pipe accepted no fixture bytes");
        let response = crate::local_management::deadline_frame();
        let terminal = tokio::spawn(async move {
            finalize_native_terminal(&mut terminal_connection.pipe, Some(&response)).await;
            // `NamedPipeServer` queues an overlapped write as though the complete buffer was
            // accepted. Production immediately disconnects after the bounded frame attempt; that
            // boundary cancels the queued write before a newly-readable peer can complete it.
            let _ = terminal_connection.pipe.disconnect();
        });
        tokio::task::yield_now().await;
        assert!(
            !terminal.is_finished(),
            "the canonical frame must backpressure an unread full Windows pipe"
        );
        tokio::time::advance(std::time::Duration::from_millis(250)).await;
        tokio::task::yield_now().await;
        terminal.await.expect("bounded Windows terminal finalizer");
        let mut partial = Vec::new();
        read_named_pipe_to_end(&mut terminal_reader, &mut partial)
            .await
            .expect("backpressured Windows terminal EOF");
        assert!(
            partial.len() <= filled,
            "terminal frame crossed backpressure"
        );
        assert!(
            partial.iter().all(|byte| *byte == 0x5a),
            "the blocked canonical frame must be abandoned before EOF"
        );

        let _ = std::fs::remove_dir_all(replay_root);
        let _ = std::fs::remove_dir_all(post_root);
    }

    #[cfg(test)]
    #[tokio::test(start_paused = true)]
    async fn supervised_windows_local_management_deadlines_are_phase_exact() {
        deadline_process_fixture().await;
    }

    #[cfg(feature = "native-deadline-test-seam")]
    pub(super) fn run_deadline_process_fixture() -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async {
            tokio::time::pause();
            deadline_process_fixture().await;
        });
        Ok(())
    }

    #[test]
    fn sid_bytes_select_exact_lowerhex_pipe_name() {
        assert_eq!(
            pipe_name_for_sid(&[0, 1, 2]),
            r"\\.\pipe\flux-exchange-local-management-v1-ae4b3280e56e2faf83f414a6e3dabe9d"
        );
    }

    #[test]
    fn fxha_is_exactly_one_closed_sixteen_byte_attachment() {
        let mut attachment = *b"FXHA\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x07";
        assert_eq!(parse_attachment(&attachment), Ok(7));

        for index in 0..8 {
            let original = attachment[index];
            attachment[index] ^= 0xff;
            assert_eq!(
                parse_attachment(&attachment),
                Err(AttachmentRefusal::WriterInvalid),
                "field byte {index} must be closed"
            );
            attachment[index] = original;
        }
        attachment[8..].fill(0);
        assert_eq!(
            parse_attachment(&attachment),
            Err(AttachmentRefusal::WriterInvalid)
        );
    }

    #[test]
    fn pinned_fxha_client_refuses_each_pid_creation_sid_session_and_liveness_substitution() {
        let expected = TokenIdentity {
            sid: b"owner-sid".to_vec(),
            session: 7,
        };
        assert!(pinned_client_matches(
            41, 41, 99, 99, &expected, &expected, true
        ));
        let substitutions = [
            (42, 99, expected.clone(), true),
            (41, 100, expected.clone(), true),
            (
                41,
                99,
                TokenIdentity {
                    sid: b"other-sid".to_vec(),
                    session: 7,
                },
                true,
            ),
            (
                41,
                99,
                TokenIdentity {
                    sid: b"owner-sid".to_vec(),
                    session: 8,
                },
                true,
            ),
            (41, 99, expected.clone(), false),
        ];
        for (process, creation, token, live) in substitutions {
            assert!(!pinned_client_matches(
                41,
                41.max(process),
                99,
                creation,
                &expected,
                &token,
                live
            ));
        }
    }

    #[test]
    fn attachment_refusals_have_the_frozen_status_and_retry_rows() {
        assert_eq!(
            AttachmentRefusal::Truncated.body(),
            br#"{"code":"truncated_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#
        );
        assert_eq!(
            AttachmentRefusal::InvalidFrame.body(),
            br#"{"code":"invalid_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#
        );
        assert_eq!(
            AttachmentRefusal::UnexpectedFrame.body(),
            br#"{"code":"unexpected_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":409}"#
        );
        assert_eq!(
            AttachmentRefusal::WriterInvalid.body(),
            br#"{"code":"writer_invalid","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#
        );
    }

    #[test]
    fn peer_mismatch_refuses_after_exactly_one_revert() {
        let reverts = Cell::new(0_u8);
        let refusal = authenticated_flow(
            b"startup-owner",
            || Ok(()),
            || Ok(b"different-owner".to_vec()),
            || {
                reverts.set(reverts.get() + 1);
                true
            },
        )
        .expect_err("another account must refuse");
        assert_eq!(refusal, WindowsEndpointRefusal::PeerIdentity);
        assert_eq!(reverts.get(), 1);
    }

    #[test]
    fn token_query_failure_still_reverts_before_refusal() {
        let reverts = Cell::new(0_u8);
        let refusal = authenticated_flow(
            b"startup-owner",
            || Ok(()),
            || Err(WindowsEndpointRefusal::PeerIdentity),
            || {
                reverts.set(reverts.get() + 1);
                true
            },
        )
        .expect_err("token query failure must refuse");
        assert_eq!(refusal, WindowsEndpointRefusal::PeerIdentity);
        assert_eq!(reverts.get(), 1);
    }

    #[test]
    fn unwinding_token_query_still_reverts_exactly_once() {
        let reverts = Cell::new(0_u8);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = authenticated_flow(
                b"startup-owner",
                || Ok(()),
                || -> Result<Vec<u8>, WindowsEndpointRefusal> { panic!("planted token panic") },
                || {
                    reverts.set(reverts.get() + 1);
                    true
                },
            );
        }));
        assert!(unwind.is_err());
        assert_eq!(reverts.get(), 1);
    }

    #[test]
    fn revert_failure_is_a_distinct_value_free_refusal() {
        let refusal = authenticated_flow(
            b"startup-owner",
            || Ok(()),
            || Ok(b"startup-owner".to_vec()),
            || false,
        )
        .expect_err("failed RevertToSelf must refuse");
        assert_eq!(refusal, WindowsEndpointRefusal::Revert);
        assert_eq!(refusal.to_string(), "local_management_revert_refused");
    }

    #[tokio::test]
    async fn native_pipe_has_only_owner_and_system_in_a_protected_dacl() {
        let _endpoint_guard = ENDPOINT_TEST_LOCK.lock().expect("endpoint test lock");
        let mut endpoint = WindowsEndpoint::bind().expect("startup TokenUser");
        assert!(endpoint.pipe_name().starts_with(PIPE_PREFIX));
        assert_eq!(endpoint.pipe_name().len(), PIPE_PREFIX.len() + 32);
        let owner_text = sid_string(endpoint.owner_sid.as_ptr()).expect("owner SID text");
        let pipe = endpoint.waiting.as_ref().expect("owner pipe");
        assert_eq!(
            endpoint.create_instance(true).err(),
            Some(WindowsEndpointRefusal::Bind),
            "the exact first-instance name cannot be preempted"
        );
        let info = pipe.info().expect("named-pipe metadata");
        assert_eq!(info.mode, PipeMode::Byte);
        assert_eq!(info.max_instances, 1);

        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let status = unsafe {
            GetSecurityInfo(
                pipe.as_raw_handle() as HANDLE,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let descriptor_allocation = NonNull::new(descriptor.cast()).expect("descriptor");
        assert_eq!(sid_string(owner).expect("pipe owner"), owner_text);

        let mut control = 0_u16;
        let mut revision = 0_u32;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        assert_eq!(
            control & (SE_DACL_PRESENT | SE_DACL_PROTECTED),
            SE_DACL_PRESENT | SE_DACL_PROTECTED
        );

        let mut size = ACL_SIZE_INFORMATION::default();
        assert_ne!(
            unsafe {
                GetAclInformation(
                    dacl,
                    (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                    u32::try_from(size_of::<ACL_SIZE_INFORMATION>()).expect("ACL size"),
                    AclSizeInformation,
                )
            },
            0
        );
        assert_eq!(size.AceCount, 2);
        let mut trustees = Vec::new();
        for index in 0..size.AceCount {
            let mut raw_ace = null_mut();
            assert_ne!(unsafe { GetAce(dacl, index, &mut raw_ace) }, 0);
            let ace = unsafe { &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>()) };
            assert_eq!(u32::from(ace.Header.AceType), ACCESS_ALLOWED_ACE_TYPE);
            trustees.push(
                sid_string((&ace.SidStart as *const u32).cast_mut().cast())
                    .expect("allowed trustee"),
            );
        }
        trustees.sort();
        let mut expected = vec!["S-1-5-18".to_owned(), owner_text];
        expected.sort();
        assert_eq!(trustees, expected);

        unsafe {
            let _ = LocalFree(descriptor_allocation.as_ptr());
        }

        let client = ClientOptions::new()
            .open(endpoint.pipe_name())
            .expect("same-account local client");
        let accepted = endpoint
            .accept_authenticated()
            .await
            .expect("connected owner pipe");
        let pipe = accepted.pipe;
        let mut unexpected_token: HANDLE = null_mut();
        assert_eq!(
            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut unexpected_token,) },
            0,
            "the server thread remained impersonated after peer authentication"
        );
        assert_eq!(unsafe { GetLastError() }, ERROR_NO_TOKEN);
        drop(client);
        drop(pipe);
    }

    #[tokio::test]
    async fn authenticated_client_identity_is_pinned_before_source_handle_duplication() {
        let _endpoint_guard = ENDPOINT_TEST_LOCK.lock().expect("endpoint test lock");
        let mut endpoint = WindowsEndpoint::bind().expect("startup TokenUser");
        let pipe_name = endpoint.pipe_name().to_owned();
        let accepting = endpoint.accept_authenticated();
        let connecting = async {
            loop {
                match ClientOptions::new().open(&pipe_name) {
                    Ok(client) => break client,
                    Err(_) => tokio::task::yield_now().await,
                }
            }
        };
        let (accepted, client) = tokio::join!(accepting, connecting);
        let accepted = accepted.expect("authenticated same-account client");

        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 0,
        };
        let mut read: HANDLE = null_mut();
        let mut write: HANDLE = null_mut();
        assert_ne!(
            unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
            0
        );
        // SAFETY: CreatePipe returned two newly owned handles.
        let read = unsafe { OwnedHandle::from_raw_handle(read.cast()) };
        // SAFETY: CreatePipe returned two newly owned handles.
        let write = unsafe { OwnedHandle::from_raw_handle(write.cast()) };
        let duplicate = accepted
            .client
            .duplicate_writer(
                accepted.pipe.as_raw_handle() as HANDLE,
                write.as_raw_handle() as usize as u64,
            )
            .expect("source handle from pinned client process");
        let mut flags = HANDLE_FLAG_INHERIT;
        assert_ne!(
            unsafe { GetHandleInformation(duplicate.0.as_raw_handle() as HANDLE, &mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        assert_eq!(
            unsafe { GetFileType(duplicate.0.as_raw_handle() as HANDLE) },
            FILE_TYPE_PIPE
        );
        drop(duplicate);
        drop(write);
        drop(read);
        drop(client);
        drop(accepted);
    }
}
