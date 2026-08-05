//! Unix execution boundary for the verified local helper modes.
//!
//! This module owns only process capabilities and transport. The vendor ceremony remains behind a
//! typed port because the production dispatcher owns its plan/coordinator state machine, while the
//! Service Account writer remains the separately validated `HelperWriter` capability.

use std::ffi::CString;
use std::fmt;
use std::io::{self, Read as _, Write as _};
use std::mem::MaybeUninit;
use std::net::Shutdown;
use std::os::fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _, OwnedFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::local_helper::{
    validate_unix_vendor_capabilities, ExpiresAt, HelperDeadlineSchedule, HelperExit,
    LocalHelperInvocation, MintWriterCapability, PipeCapabilityFacts, PipeDirection,
    ServiceAccountId, UnixVendorCapabilityFacts, VendorSecretCapabilities, HELPER_RESULT_DEADLINE,
    HELPER_SETUP_DEADLINE, MAX_HELPER_FRAME_BYTES, UNIX_MINT_WRITER_FD, UNIX_VENDOR_REQUEST_FD,
    UNIX_VENDOR_RESPONSE_FD,
};
use crate::local_helper_plan::{VendorBegin, VendorOperation};
use crate::local_management::service_account_handoff::unix_transfer::{
    HelperWriter, UnixHandoffError,
};

const RUN_DIRECTORY: &str = "run";
const SOCKET_NAME: &str = "local-management-v1.sock";
const HEADER_BYTES: usize = 12;
const MAX_CONTROL_BYTES: usize = 65_536;
const CLIENT_DIRECTION: u8 = 1;
const SERVER_DIRECTION: u8 = 2;
const CONNECT_BEGIN: u16 = 0x0001;
const NEED_SECRETS: u16 = 0x0002;
const SECRET: u16 = 0x0003;
const CONNECT_COMMIT: u16 = 0x0004;
const CONNECT_RECEIPT: u16 = 0x0006;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;
const SERVICE_ACCOUNT_MINT: u16 = 0x0020;
const SERVICE_ACCOUNT_RECEIPT: u16 = 0x0022;
const CREDENTIAL_BEGIN: u16 = 0x0030;
const CREDENTIAL_COMMIT: u16 = 0x0031;
const CREDENTIAL_RECEIPT: u16 = 0x0032;
const ERROR: u16 = 0x7fff;
const MAX_SECRET_BYTES: usize = 8_192;

/// Execute one already-closed Unix helper invocation before tracing or an async runtime exists.
pub(crate) fn run(invocation: LocalHelperInvocation) -> HelperExit {
    match invocation {
        LocalHelperInvocation::VendorSecret(VendorSecretCapabilities::Unix) => {
            run_vendor(&mut NativeVendorCeremony::new())
        }
        LocalHelperInvocation::ServiceAccountMint {
            id,
            expires_at,
            writer: MintWriterCapability::UnixFd5,
        } => match HelperWriter::inherited_fd5() {
            Ok(writer) => run_mint(&id, expires_at, writer),
            Err(_) => HelperExit::CapabilityOrTransportFailure,
        },
        _ => HelperExit::CapabilityOrTransportFailure,
    }
}

#[cfg(feature = "native-helper-deadline-test-seam")]
pub(crate) fn run_with_result_budget_for_test(
    invocation: LocalHelperInvocation,
    result_budget: Duration,
) -> u8 {
    match invocation {
        LocalHelperInvocation::VendorSecret(VendorSecretCapabilities::Unix) => {
            let mut ceremony = NativeVendorCeremony::new();
            let exit = run_vendor_with(
                PinnedEndpoint::authenticated,
                &mut ceremony,
                HELPER_SETUP_DEADLINE,
                HELPER_SETUP_DEADLINE,
                result_budget,
            );
            if std::env::var_os("FLUX_EXCHANGE_TEST_HELPER_STAGE_EXIT").is_some()
                && ceremony.stage != NativeVendorStage::Complete
            {
                ceremony.stage as u8
            } else {
                exit.code()
            }
        }
        _ => HelperExit::CapabilityOrTransportFailure.code(),
    }
}

/// One request whose complete frame and EOF passed the Flux-to-helper transport contract.
///
/// It intentionally has no `Debug` or text conversion implementation: the payload can contain
/// non-secret settings that still do not belong in helper diagnostics.
pub(crate) struct VendorRequest {
    bytes: Vec<u8>,
    kind: VendorRequestKind,
}

impl VendorRequest {
    /// Byte-identical initiating frame to send only on the authenticated ceremony connection.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Closed initiating operation admitted on the Flux-to-helper pipe.
    pub(crate) const fn kind(&self) -> VendorRequestKind {
        self.kind
    }
}

/// The only two operations the vendor helper accepts from Flux.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VendorRequestKind {
    Connect,
    Credential,
}

/// Exact owner endpoint pinned before either native ceremony connection.
///
/// Each call to [`connect_before`](Self::connect_before) rechecks the same socket device/inode and
/// authenticates the peer UID. A ceremony can therefore open plan connection 1 and mutation
/// connection 2 without accepting a pathname replacement between them.
pub(crate) struct PinnedEndpoint {
    socket: PathBuf,
    device: u64,
    inode: u64,
    expected_uid: u32,
}

impl PinnedEndpoint {
    fn authenticated() -> Result<Self, UnixHelperError> {
        #[cfg(feature = "native-root-test-seam")]
        if let Some(root) = std::env::var_os("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT") {
            return Self::at(Path::new(&root));
        }
        let root = crate::native_root::authenticated_account_state_root()
            .map_err(|_| UnixHelperError::EndpointUnavailable)?;
        Self::at(&root)
    }

    fn at(root: &Path) -> Result<Self, UnixHelperError> {
        let expected_uid = effective_uid();
        inspect_private_directory(root, expected_uid, 0o700)?;
        let run = root.join(RUN_DIRECTORY);
        inspect_private_directory(&run, expected_uid, 0o700)?;
        let socket = run.join(SOCKET_NAME);
        let metadata = inspect_socket(&socket, expected_uid)?;
        Ok(Self {
            socket,
            device: metadata.dev(),
            inode: metadata.ino(),
            expected_uid,
        })
    }

    /// Connect to the pinned endpoint before the helper's absolute pre-ceremony deadline.
    pub(crate) fn connect_before(&self, deadline: Instant) -> Result<UnixStream, UnixHelperError> {
        if Instant::now() >= deadline {
            return Err(UnixHelperError::Deadline);
        }
        self.revalidate()?;
        let stream =
            UnixStream::connect(&self.socket).map_err(|_| UnixHelperError::EndpointUnavailable)?;
        if Instant::now() >= deadline {
            return Err(UnixHelperError::Deadline);
        }
        self.revalidate()?;
        authenticate_peer(&stream, self.expected_uid)?;
        Ok(stream)
    }

    fn revalidate(&self) -> Result<(), UnixHelperError> {
        let metadata = inspect_socket(&self.socket, self.expected_uid)?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(UnixHelperError::EndpointChanged);
        }
        Ok(())
    }
}

/// Production port for the plan-first, two-connection vendor ceremony.
///
/// Implementations must use `endpoint.connect_before(ready_by)` for both owner-authenticated
/// connections and return only the terminal receipt/error frame. Transaction ids, ordinals and
/// secret bytes therefore never enter this port's result or the Flux response pipe.
pub(crate) trait VendorCeremony {
    type Error;

    fn execute(
        &mut self,
        endpoint: &PinnedEndpoint,
        request: &VendorRequest,
        deadlines: HelperDeadlineSchedule,
    ) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum NativeVendorStage {
    ParseBegin = 20,
    ConnectPlan = 21,
    WritePlan = 22,
    ReadPlan = 23,
    PlanRefusal = 24,
    ValidatePlan = 25,
    ConnectMutation = 26,
    WriteBegin = 27,
    ReadNeedSecrets = 28,
    MutationRefusal = 29,
    ValidateNeedSecrets = 30,
    ReadPrivateTerminal = 31,
    WriteSecret = 32,
    WriteCommit = 33,
    ReadTerminal = 34,
    Complete = 35,
    ReadPlanDeadline = 36,
    ReadPlanProtocol = 37,
    ReadPlanTransport = 38,
}

struct NativeVendorCeremony {
    stage: NativeVendorStage,
}

impl NativeVendorCeremony {
    const fn new() -> Self {
        Self {
            stage: NativeVendorStage::ParseBegin,
        }
    }
}

impl VendorCeremony for NativeVendorCeremony {
    type Error = UnixHelperError;

    fn execute(
        &mut self,
        endpoint: &PinnedEndpoint,
        request: &VendorRequest,
        deadlines: HelperDeadlineSchedule,
    ) -> Result<Vec<u8>, Self::Error> {
        self.stage = NativeVendorStage::ParseBegin;
        let ready_by = deadlines.setup_by();
        let operation = match request.kind() {
            VendorRequestKind::Connect => VendorOperation::Connect,
            VendorRequestKind::Credential => VendorOperation::Credential,
        };
        let begin = VendorBegin::parse(
            request
                .bytes()
                .get(HEADER_BYTES..)
                .ok_or(UnixHelperError::Protocol)?,
            operation,
        )
        .ok_or(UnixHelperError::Protocol)?;
        self.stage = NativeVendorStage::ConnectPlan;
        let mut plan_stream = endpoint.connect_before(ready_by)?;
        let plan_query = serde_json::to_vec(&PlanQuery {
            connector: begin.connector(),
            selection: match request.kind() {
                VendorRequestKind::Connect => None,
                VendorRequestKind::Credential => Some(begin.label()),
            },
        })
        .map_err(|_| UnixHelperError::Protocol)?;
        self.stage = NativeVendorStage::WritePlan;
        write_before(
            &mut plan_stream,
            &encode_frame(CLIENT_DIRECTION, PLAN_QUERY, &plan_query),
            ready_by,
        )?;
        plan_stream
            .shutdown(Shutdown::Write)
            .map_err(|_| UnixHelperError::Transport)?;
        self.stage = NativeVendorStage::ReadPlan;
        let plan = match read_terminal_before(&mut plan_stream, ready_by) {
            Ok(plan) => plan,
            Err(error) => {
                self.stage = match error {
                    UnixHelperError::Deadline => NativeVendorStage::ReadPlanDeadline,
                    UnixHelperError::Protocol => NativeVendorStage::ReadPlanProtocol,
                    _ => NativeVendorStage::ReadPlanTransport,
                };
                return Err(error);
            }
        };
        if plan.opcode == ERROR {
            self.stage = NativeVendorStage::PlanRefusal;
            return Ok(plan.bytes);
        }
        self.stage = NativeVendorStage::ValidatePlan;
        if plan.opcode != PLAN_RESPONSE || !begin.admits_plan(&plan.payload) {
            return Err(UnixHelperError::Protocol);
        }

        self.stage = NativeVendorStage::ConnectMutation;
        let mut mutation = endpoint.connect_before(ready_by)?;
        self.stage = NativeVendorStage::WriteBegin;
        write_before(&mut mutation, request.bytes(), ready_by)?;
        let result_by = deadlines.result_by();
        self.stage = NativeVendorStage::ReadNeedSecrets;
        let need = read_frame_before(&mut mutation, result_by)?;
        if need.opcode == ERROR {
            self.stage = NativeVendorStage::MutationRefusal;
            mutation
                .shutdown(Shutdown::Write)
                .map_err(|_| UnixHelperError::Transport)?;
            return Ok(need.bytes);
        }
        if need.opcode != NEED_SECRETS {
            return Err(UnixHelperError::Protocol);
        }
        self.stage = NativeVendorStage::ValidateNeedSecrets;
        let need_payload = need.payload;
        let need: NeedSecrets =
            serde_json::from_slice(&need_payload).map_err(|_| UnixHelperError::Protocol)?;
        if !need.is_canonical(&need_payload) {
            return Err(UnixHelperError::Protocol);
        }

        for (index, secret) in need.secrets.iter().enumerate() {
            if usize::from(secret.ordinal) != index + 1 || secret.target.is_empty() {
                return Err(UnixHelperError::Protocol);
            }
            self.stage = NativeVendorStage::ReadPrivateTerminal;
            let mut value = read_private_tty_line(result_by)?;
            let frame = encode_secret(secret.ordinal, &value)?;
            self.stage = NativeVendorStage::WriteSecret;
            let result = write_before(&mut mutation, &frame, result_by);
            value.fill(0);
            result?;
        }
        let commit = serde_json::to_vec(&Commit {
            proposal_digest: &need.proposal_digest,
            transaction_id: &need.transaction_id,
        })
        .map_err(|_| UnixHelperError::Protocol)?;
        let opcode = match request.kind() {
            VendorRequestKind::Connect => CONNECT_COMMIT,
            VendorRequestKind::Credential => CREDENTIAL_COMMIT,
        };
        self.stage = NativeVendorStage::WriteCommit;
        write_before(
            &mut mutation,
            &encode_frame(CLIENT_DIRECTION, opcode, &commit),
            result_by,
        )?;
        mutation
            .shutdown(Shutdown::Write)
            .map_err(|_| UnixHelperError::Transport)?;
        self.stage = NativeVendorStage::ReadTerminal;
        let terminal = read_terminal_before(&mut mutation, result_by)?.bytes;
        self.stage = NativeVendorStage::Complete;
        Ok(terminal)
    }
}

impl MintTransfer for HelperWriter {
    type Error = UnixHandoffError;

    fn transfer(self, stream: &UnixStream, mint_frame: &[u8]) -> Result<(), Self::Error> {
        self.transfer_mint(stream, mint_frame)
    }
}

#[derive(Serialize)]
struct PlanQuery<'a> {
    connector: &'a str,
    selection: Option<&'a str>,
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

fn write_before(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), UnixHelperError> {
    stream
        .set_write_timeout(Some(remaining(deadline)?))
        .map_err(|_| UnixHelperError::Transport)?;
    stream
        .write_all(bytes)
        .map_err(|error| timeout_or_transport(&error))
}

fn read_frame_before(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<NativeFrame, UnixHelperError> {
    stream
        .set_read_timeout(Some(remaining(deadline)?))
        .map_err(|_| UnixHelperError::Transport)?;
    let mut header = [0_u8; HEADER_BYTES];
    stream
        .read_exact(&mut header)
        .map_err(|error| timeout_or_transport(&error))?;
    if &header[..4] != b"FXLM" || header[4] != 1 || header[5] != SERVER_DIRECTION {
        return Err(UnixHelperError::Protocol);
    }
    let payload_length =
        u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if payload_length > MAX_CONTROL_BYTES {
        return Err(UnixHelperError::Protocol);
    }
    let mut payload = vec![0_u8; payload_length];
    stream
        .read_exact(&mut payload)
        .map_err(|error| timeout_or_transport(&error))?;
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&payload);
    Ok(NativeFrame {
        bytes,
        opcode: u16::from_be_bytes([header[6], header[7]]),
        payload,
    })
}

fn read_terminal_before(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<NativeFrame, UnixHelperError> {
    let frame = read_frame_before(stream, deadline)?;
    stream
        .set_read_timeout(Some(remaining(deadline)?))
        .map_err(|_| UnixHelperError::Transport)?;
    let mut extra = [0_u8; 1];
    match stream.read(&mut extra) {
        Ok(0) => Ok(frame),
        Ok(_) => Err(UnixHelperError::Protocol),
        Err(error) => Err(timeout_or_transport(&error)),
    }
}

fn remaining(deadline: Instant) -> Result<Duration, UnixHelperError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(UnixHelperError::Deadline)
}

fn timeout_or_transport(error: &io::Error) -> UnixHelperError {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        UnixHelperError::Deadline
    } else {
        UnixHelperError::Transport
    }
}

fn encode_secret(ordinal: u16, secret: &[u8]) -> Result<Vec<u8>, UnixHelperError> {
    if ordinal == 0 || !(1..=MAX_SECRET_BYTES).contains(&secret.len()) {
        return Err(UnixHelperError::Protocol);
    }
    let mut payload = Vec::with_capacity(2 + secret.len());
    payload.extend_from_slice(&ordinal.to_be_bytes());
    payload.extend_from_slice(secret);
    Ok(encode_frame(CLIENT_DIRECTION, SECRET, &payload))
}

fn read_private_tty_line(deadline: Instant) -> Result<Vec<u8>, UnixHelperError> {
    let tty = CString::new("/dev/tty").expect("fixed terminal path");
    // SAFETY: the fixed NUL-terminated path is valid and open returns one owned descriptor.
    let descriptor = unsafe { libc::open(tty.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if descriptor < 0 {
        return Err(UnixHelperError::Terminal);
    }
    // SAFETY: successful open transfers this descriptor exactly once.
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let mut original = MaybeUninit::<libc::termios>::zeroed();
    // SAFETY: the descriptor is a live terminal and the output region is exact.
    if unsafe { libc::tcgetattr(descriptor.as_raw_fd(), original.as_mut_ptr()) } != 0 {
        return Err(UnixHelperError::Terminal);
    }
    // SAFETY: tcgetattr succeeded and initialized the complete termios value.
    let original = unsafe { original.assume_init() };
    let mut private = original;
    private.c_lflag &= !libc::ECHO;
    // SAFETY: only the terminal's echo flag changes; this process restores it through the guard.
    if unsafe { libc::tcsetattr(descriptor.as_raw_fd(), libc::TCSANOW, &private) } != 0 {
        return Err(UnixHelperError::Terminal);
    }
    let guard = TerminalEchoGuard {
        descriptor: descriptor.as_raw_fd(),
        original,
    };
    let mut secret = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(UnixHelperError::Deadline);
        }
        if !wait_fd_readable(descriptor.as_raw_fd(), deadline)? {
            return Err(UnixHelperError::Deadline);
        }
        let mut byte = 0_u8;
        // SAFETY: the descriptor is a validated terminal and the one-byte output is live.
        let received =
            unsafe { libc::read(descriptor.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
        if received < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if received <= 0 {
            return Err(UnixHelperError::Terminal);
        }
        if byte == b'\n' {
            break;
        }
        if secret.len() == MAX_SECRET_BYTES {
            secret.fill(0);
            return Err(UnixHelperError::Protocol);
        }
        secret.push(byte);
    }
    drop(guard);
    if secret.last() == Some(&b'\r') {
        secret.pop();
    }
    if secret.is_empty() {
        Err(UnixHelperError::Protocol)
    } else {
        Ok(secret)
    }
}

#[cfg(test)]
#[allow(dead_code)] // Called by the native integration harness through this source module.
pub(crate) fn read_private_tty_line_before_for_test(
    deadline: Instant,
) -> Result<Vec<u8>, UnixHelperError> {
    read_private_tty_line(deadline)
}

fn wait_fd_readable(descriptor: RawFd, deadline: Instant) -> Result<bool, UnixHelperError> {
    let mut poll = libc::pollfd {
        fd: descriptor,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    loop {
        let milliseconds = remaining(deadline)?
            .as_millis()
            .max(1)
            .min(i32::MAX as u128) as i32;
        // SAFETY: poll receives one live descriptor record for a bounded duration.
        let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
        if result > 0 {
            return Ok(true);
        }
        if result == 0 {
            if Instant::now() >= deadline {
                return Ok(false);
            }
            // `poll` accepts whole milliseconds and may wake before the absolute instant after a
            // rounded timeout. Preserve the one outer deadline instead of turning that early wake
            // into time to write a terminal frame after a private-input timeout.
            continue;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(UnixHelperError::Terminal);
        }
    }
}

struct TerminalEchoGuard {
    descriptor: RawFd,
    original: libc::termios,
}

impl Drop for TerminalEchoGuard {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains owned by the caller until after this guard drops.
        unsafe {
            libc::tcsetattr(self.descriptor, libc::TCSANOW, &self.original);
        }
    }
}

/// Typed adapter for the existing Unix `HelperWriter` transfer.
///
/// The implementation consumes its FD5 capability and attaches it to these exact validated MINT
/// bytes. This module never accepts a descriptor number as a byte or reconstructs the capability.
pub(crate) trait MintTransfer {
    type Error;

    fn transfer(self, stream: &UnixStream, mint_frame: &[u8]) -> Result<(), Self::Error>;
}

/// Execute the fixed-descriptor vendor helper against the authenticated account endpoint.
pub(crate) fn run_vendor<C: VendorCeremony>(ceremony: &mut C) -> HelperExit {
    run_vendor_with(
        PinnedEndpoint::authenticated,
        ceremony,
        HELPER_SETUP_DEADLINE,
        HELPER_SETUP_DEADLINE,
        HELPER_RESULT_DEADLINE,
    )
}

/// Execute the Unix Service Account helper and transfer its exact MINT frame with FD5.
pub(crate) fn run_mint<T: MintTransfer>(
    id: &ServiceAccountId,
    expires_at: ExpiresAt,
    transfer: T,
) -> HelperExit {
    run_mint_with(
        PinnedEndpoint::authenticated,
        id,
        expires_at,
        transfer,
        HELPER_SETUP_DEADLINE,
    )
}

#[cfg(test)]
#[allow(dead_code)] // Called by the integration runtime harness through this source module.
pub(crate) fn run_vendor_at_for_test<C: VendorCeremony>(
    root: &Path,
    ceremony: &mut C,
    request_deadline: Duration,
    setup_deadline: Duration,
) -> HelperExit {
    run_vendor_with(
        || PinnedEndpoint::at(root),
        ceremony,
        request_deadline,
        setup_deadline,
        HELPER_RESULT_DEADLINE,
    )
}

#[cfg(test)]
#[allow(dead_code)] // Called by the integration runtime harness through this source module.
pub(crate) fn run_mint_at_for_test<T: MintTransfer>(
    root: &Path,
    id: &ServiceAccountId,
    expires_at: ExpiresAt,
    transfer: T,
    setup_deadline: Duration,
) -> HelperExit {
    run_mint_with(
        || PinnedEndpoint::at(root),
        id,
        expires_at,
        transfer,
        setup_deadline,
    )
}

fn run_vendor_with<C, E>(
    endpoint: E,
    ceremony: &mut C,
    request_budget: Duration,
    setup_budget: Duration,
    result_budget: Duration,
) -> HelperExit
where
    C: VendorCeremony,
    E: FnOnce() -> Result<PinnedEndpoint, UnixHelperError>,
{
    let capabilities = match acquire_vendor_capabilities() {
        Ok(capabilities) => capabilities,
        Err(_) => return HelperExit::CapabilityOrTransportFailure,
    };
    let VendorCapabilities { request, response } = capabilities;
    let request_by = Instant::now()
        .checked_add(request_budget)
        .unwrap_or_else(Instant::now);
    let parsed_request = read_request(&request, request_by);
    drop(request);
    let deadlines = HelperDeadlineSchedule::from_request_eof_with_budgets(
        Instant::now(),
        setup_budget,
        result_budget,
    );
    let Some(deadlines) = deadlines else {
        return HelperExit::CapabilityOrTransportFailure;
    };
    let request = match parsed_request {
        Ok(request) => request,
        Err(refusal) => {
            return finish_response(response, refusal.frame(), deadlines.result_by());
        }
    };
    // Request EOF starts a separate absolute pre-ceremony budget; traffic never resets it.
    let ready_by = deadlines.setup_by();
    let endpoint = match endpoint() {
        Ok(endpoint) if Instant::now() < ready_by => endpoint,
        Ok(_) => {
            return finish_response(response, Refusal::Deadline.frame(), deadlines.result_by());
        }
        Err(_) => {
            return finish_response(
                response,
                Refusal::LocalManagementUnavailable.frame(),
                deadlines.result_by(),
            );
        }
    };
    let terminal = match ceremony.execute(&endpoint, &request, deadlines) {
        Ok(bytes) if HelperDeadlineSchedule::permits(deadlines.result_by(), Instant::now()) => {
            bytes
        }
        Ok(_) | Err(_) => {
            return finish_response(
                response,
                Refusal::LocalManagementUnavailable.frame(),
                deadlines.result_by(),
            );
        }
    };
    let terminal = match validate_terminal(&terminal, request.kind) {
        Ok(()) => terminal,
        Err(refusal) => refusal.frame(),
    };
    finish_response(response, terminal, deadlines.result_by())
}

fn run_mint_with<T, E>(
    endpoint: E,
    id: &ServiceAccountId,
    expires_at: ExpiresAt,
    transfer: T,
    setup_budget: Duration,
) -> HelperExit
where
    T: MintTransfer,
    E: FnOnce() -> Result<PinnedEndpoint, UnixHelperError>,
{
    let Some(deadlines) =
        HelperDeadlineSchedule::from_request_eof_with_setup(Instant::now(), setup_budget)
    else {
        return HelperExit::CapabilityOrTransportFailure;
    };
    let ready_by = deadlines.setup_by();
    let endpoint = match endpoint() {
        Ok(endpoint) if Instant::now() < ready_by => endpoint,
        _ => return HelperExit::CapabilityOrTransportFailure,
    };
    let stream = match endpoint.connect_before(ready_by) {
        Ok(stream) => stream,
        Err(_) => return HelperExit::CapabilityOrTransportFailure,
    };
    let payload = format!(
        "{{\"expires_at\":\"{}\",\"id\":\"{}\"}}",
        expires_at.value(),
        id.as_str()
    );
    let frame = encode_frame(CLIENT_DIRECTION, SERVICE_ACCOUNT_MINT, payload.as_bytes());
    if transfer.transfer(&stream, &frame).is_err() || Instant::now() >= ready_by {
        return HelperExit::CapabilityOrTransportFailure;
    }
    let mut stream = stream;
    match read_terminal_before(&mut stream, deadlines.result_by()) {
        Ok(frame) if matches!(frame.opcode, SERVICE_ACCOUNT_RECEIPT | ERROR) => {
            HelperExit::TerminalFrameWritten
        }
        _ => HelperExit::CapabilityOrTransportFailure,
    }
}

struct VendorCapabilities {
    request: OwnedFd,
    response: OwnedFd,
}

fn acquire_vendor_capabilities() -> Result<VendorCapabilities, UnixHelperError> {
    let request_fd = UNIX_VENDOR_REQUEST_FD as RawFd;
    let response_fd = UNIX_VENDOR_RESPONSE_FD as RawFd;
    let request = inspect_pipe(request_fd)?;
    let response = inspect_pipe(response_fd)?;
    let facts = UnixVendorCapabilityFacts {
        request,
        response,
        fd5_closed: descriptor_is_closed(UNIX_MINT_WRITER_FD as RawFd),
        all_other_nonstandard_fds_closed: all_other_descriptors_closed(request_fd, response_fd)?,
    };
    validate_unix_vendor_capabilities(&facts).map_err(|_| UnixHelperError::Capability)?;
    set_close_on_exec(request_fd)?;
    set_close_on_exec(response_fd)?;
    // SAFETY: the closed helper ABI gives this mode sole ownership of descriptors 6 and 7.
    let request = unsafe { OwnedFd::from_raw_fd(request_fd) };
    // SAFETY: as above, descriptor 7 is distinct and transferred exactly once.
    let response = unsafe { OwnedFd::from_raw_fd(response_fd) };
    Ok(VendorCapabilities { request, response })
}

fn inspect_pipe(descriptor: RawFd) -> Result<PipeCapabilityFacts, UnixHelperError> {
    // SAFETY: fstat writes only the exact live output structure.
    let mut metadata = unsafe { MaybeUninit::<libc::stat>::zeroed().assume_init() };
    if unsafe { libc::fstat(descriptor, &mut metadata) } != 0 {
        return Err(UnixHelperError::Capability);
    }
    // SAFETY: F_GETFL reads descriptor flags without mutation.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(UnixHelperError::Capability);
    }
    let direction = match flags & libc::O_ACCMODE {
        libc::O_RDONLY => PipeDirection::Read,
        libc::O_WRONLY => PipeDirection::Write,
        _ => PipeDirection::Other,
    };
    let anonymous_pipe =
        metadata.st_mode & libc::S_IFMT == libc::S_IFIFO && is_anonymous_pipe(descriptor);
    let device = metadata.st_dev as u128;
    let inode = metadata.st_ino as u128;
    Ok(PipeCapabilityFacts {
        anonymous_pipe,
        direction,
        pipe_identity: (device << 64) | inode,
    })
}

#[cfg(target_os = "linux")]
fn is_anonymous_pipe(descriptor: RawFd) -> bool {
    std::fs::read_link(format!("/proc/self/fd/{descriptor}"))
        .map(|target| {
            let bytes = target.into_os_string().into_vec();
            bytes.starts_with(b"pipe:[") && bytes.ends_with(b"]")
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn is_anonymous_pipe(descriptor: RawFd) -> bool {
    let mut path = [0_u8; libc::PATH_MAX as usize];
    // An anonymous pipe has no path; F_GETPATH therefore must refuse it.
    (unsafe { libc::fcntl(descriptor, libc::F_GETPATH, path.as_mut_ptr()) }) == -1
}

#[cfg(target_os = "linux")]
fn all_other_descriptors_closed(request: RawFd, response: RawFd) -> Result<bool, UnixHelperError> {
    let path = CString::new("/proc/self/fd").expect("fixed proc descriptor directory");
    // SAFETY: the fixed path is NUL terminated and opendir returns one owned directory stream.
    let directory = unsafe { libc::opendir(path.as_ptr()) };
    if directory.is_null() {
        return Err(UnixHelperError::Capability);
    }
    // SAFETY: `directory` remains live until the matching closedir below.
    let inspection = unsafe { libc::dirfd(directory) };
    let mut closed = true;
    loop {
        // SAFETY: readdir returns either null or one entry owned by the live directory stream.
        let entry = unsafe { libc::readdir(directory) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL-terminated within the returned dirent.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let Ok(name) = name.to_str() else {
            continue;
        };
        let Ok(name) = name.parse::<RawFd>() else {
            continue;
        };
        if name > 2 && name != request && name != response && name != inspection {
            closed = false;
            break;
        }
    }
    // SAFETY: this closes exactly the directory stream opened above.
    if unsafe { libc::closedir(directory) } != 0 {
        return Err(UnixHelperError::Capability);
    }
    Ok(closed)
}

#[cfg(target_os = "macos")]
fn all_other_descriptors_closed(request: RawFd, response: RawFd) -> Result<bool, UnixHelperError> {
    // SAFETY: getrlimit writes one exact output structure.
    let mut limit = unsafe { MaybeUninit::<libc::rlimit>::zeroed().assume_init() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(UnixHelperError::Capability);
    }
    let maximum = limit.rlim_cur.min(i32::MAX as libc::rlim_t) as RawFd;
    for descriptor in 3..maximum {
        if descriptor != request && descriptor != response && !descriptor_is_closed(descriptor) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn descriptor_is_closed(descriptor: RawFd) -> bool {
    // SAFETY: F_GETFD has no pointer argument and does not mutate the descriptor.
    if unsafe { libc::fcntl(descriptor, libc::F_GETFD) } != -1 {
        return false;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EBADF)
}

fn set_close_on_exec(descriptor: RawFd) -> Result<(), UnixHelperError> {
    // SAFETY: F_GETFD has no pointer argument.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags == -1
        // SAFETY: F_SETFD changes only flags on the live descriptor.
        || unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
    {
        Err(UnixHelperError::Capability)
    } else {
        Ok(())
    }
}

fn read_request(descriptor: &OwnedFd, deadline: Instant) -> Result<VendorRequest, Refusal> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES);
    let mut expected = None;
    loop {
        if !wait_readable(descriptor.as_raw_fd(), deadline)? {
            return Err(Refusal::Deadline);
        }
        let mut chunk = [0_u8; 4096];
        // SAFETY: the descriptor is a validated read end and the output buffer is live.
        let received = unsafe {
            libc::read(
                descriptor.as_raw_fd(),
                chunk.as_mut_ptr().cast(),
                chunk.len(),
            )
        };
        if received == 0 {
            break;
        }
        if received < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
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
    let opcode = u16::from_be_bytes([bytes[6], bytes[7]]);
    let kind = match opcode {
        CONNECT_BEGIN => VendorRequestKind::Connect,
        CREDENTIAL_BEGIN => VendorRequestKind::Credential,
        _ => return Err(Refusal::UnexpectedFrame),
    };
    Ok(VendorRequest { bytes, kind })
}

fn wait_readable(descriptor: RawFd, deadline: Instant) -> Result<bool, Refusal> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        let remaining = deadline.duration_since(now);
        let milliseconds = remaining
            .as_nanos()
            .div_ceil(1_000_000)
            .min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd: descriptor,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        // SAFETY: poll references one live pollfd for the bounded timeout.
        let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
        if result > 0 {
            return Ok(true);
        }
        if result == 0 {
            return Ok(false);
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(Refusal::Truncated);
        }
    }
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

fn validate_terminal(bytes: &[u8], request: VendorRequestKind) -> Result<(), Refusal> {
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
    let permitted = match request {
        VendorRequestKind::Connect => matches!(opcode, CONNECT_RECEIPT | ERROR),
        VendorRequestKind::Credential => matches!(opcode, CREDENTIAL_RECEIPT | ERROR),
    };
    if !known_opcode(opcode) {
        return Err(Refusal::InvalidFrame);
    }
    if !permitted {
        return Err(Refusal::UnexpectedFrame);
    }
    let payload = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if payload > MAX_CONTROL_BYTES {
        return Err(Refusal::FrameTooLarge);
    }
    let expected = HEADER_BYTES
        .checked_add(payload)
        .ok_or(Refusal::FrameTooLarge)?;
    match bytes.len().cmp(&expected) {
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

fn finish_response(response: OwnedFd, bytes: Vec<u8>, deadline: Instant) -> HelperExit {
    let descriptor = response.into_raw_fd();
    // SAFETY: F_GETFL reads flags from one live descriptor.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1
        // SAFETY: F_SETFL updates status flags on the same live descriptor.
        || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        // SAFETY: ownership was extracted exactly once.
        unsafe { libc::close(descriptor) };
        return HelperExit::CapabilityOrTransportFailure;
    }
    let mut written = 0;
    while written < bytes.len() {
        if !wait_writable(descriptor, deadline) {
            // SAFETY: ownership was extracted exactly once.
            unsafe { libc::close(descriptor) };
            return HelperExit::CapabilityOrTransportFailure;
        }
        // SAFETY: the descriptor is a validated write end and the remaining bytes are live.
        let result = unsafe {
            libc::write(
                descriptor,
                bytes[written..].as_ptr().cast(),
                bytes.len() - written,
            )
        };
        if result > 0 {
            written += result as usize;
            continue;
        }
        if result < 0 {
            let kind = io::Error::last_os_error().kind();
            if matches!(kind, io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) {
                continue;
            }
        }
        // SAFETY: ownership was extracted exactly once and the helper exits after this failure.
        unsafe { libc::close(descriptor) };
        return HelperExit::CapabilityOrTransportFailure;
    }
    // A pipe close itself is non-blocking; checking immediately beforehand keeps EOF inside the
    // unchanged result deadline rather than accepting a write that completed on the boundary.
    if Instant::now() < deadline && unsafe { libc::close(descriptor) } == 0 {
        HelperExit::TerminalFrameWritten
    } else {
        HelperExit::CapabilityOrTransportFailure
    }
}

#[cfg(test)]
#[allow(dead_code)] // Called by the integration runtime harness through this source module.
pub(crate) fn finish_response_before_for_test(
    response: OwnedFd,
    bytes: Vec<u8>,
    deadline: Instant,
) -> HelperExit {
    finish_response(response, bytes, deadline)
}

fn wait_writable(descriptor: RawFd, deadline: Instant) -> bool {
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        if remaining.is_zero() {
            return false;
        }
        let milliseconds = remaining
            .as_nanos()
            .div_ceil(1_000_000)
            .min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd: descriptor,
            events: libc::POLLOUT | libc::POLLHUP,
            revents: 0,
        };
        // SAFETY: poll references one live pollfd for the bounded timeout.
        let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
        if result > 0 {
            return poll.revents & libc::POLLOUT != 0;
        }
        if result == 0 {
            return false;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return false;
        }
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

fn encode_frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES + payload.len());
    bytes.extend_from_slice(b"FXLM");
    bytes.push(1);
    bytes.push(direction);
    bytes.extend_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn inspect_private_directory(
    path: &Path,
    expected_uid: u32,
    expected_mode: u32,
) -> Result<(), UnixHelperError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| UnixHelperError::UnsafeEndpoint)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != expected_uid
        || metadata.permissions().mode() & 0o7777 != expected_mode
    {
        return Err(UnixHelperError::UnsafeEndpoint);
    }
    Ok(())
}

fn inspect_socket(path: &Path, expected_uid: u32) -> Result<std::fs::Metadata, UnixHelperError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| UnixHelperError::UnsafeEndpoint)?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != expected_uid
        || metadata.permissions().mode() & 0o7777 != 0o600
    {
        return Err(UnixHelperError::UnsafeEndpoint);
    }
    Ok(metadata)
}

fn authenticate_peer(stream: &UnixStream, expected_uid: u32) -> Result<(), UnixHelperError> {
    if peer_uid(stream)? == expected_uid {
        Ok(())
    } else {
        Err(UnixHelperError::PeerUnverified)
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Result<u32, UnixHelperError> {
    let mut credential = MaybeUninit::<libc::ucred>::zeroed();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the stream is live and the output region has the exact ucred size.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credential.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(UnixHelperError::PeerUnverified);
    }
    // SAFETY: getsockopt succeeded with the complete output size.
    Ok(unsafe { credential.assume_init() }.uid)
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Result<u32, UnixHelperError> {
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: both output pointers are live for getpeereid.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        Err(UnixHelperError::PeerUnverified)
    } else {
        Ok(uid)
    }
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no pointer arguments or preconditions.
    unsafe { libc::geteuid() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnixHelperError {
    Capability,
    Protocol,
    Transport,
    Terminal,
    UnsafeEndpoint,
    EndpointUnavailable,
    EndpointChanged,
    PeerUnverified,
    Deadline,
}

impl fmt::Display for UnixHelperError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Capability => "local helper capability contract refused",
            Self::Protocol => "local helper protocol contract refused",
            Self::Transport => "local helper transport refused",
            Self::Terminal => "local helper private terminal input refused",
            Self::UnsafeEndpoint => "local helper endpoint metadata refused",
            Self::EndpointUnavailable => "local management is unavailable",
            Self::EndpointChanged => "local helper endpoint identity changed",
            Self::PeerUnverified => "local helper endpoint peer was not verified",
            Self::Deadline => "local helper deadline exceeded",
        })
    }
}

impl std::error::Error for UnixHelperError {}
