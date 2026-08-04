//! Closed command and capability contract for verified local helper processes.
//!
//! This module deliberately stops at the native-port boundary. In particular, Windows writer
//! handles are capabilities transferred by the endpoint implementation, never bytes added to an
//! FXLM or FXSA payload.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::time::Duration;

/// Fixed Unix request-read descriptor for the vendor helper.
pub const UNIX_VENDOR_REQUEST_FD: u32 = 6;
/// Fixed Unix terminal-response-write descriptor for the vendor helper.
pub const UNIX_VENDOR_RESPONSE_FD: u32 = 7;
/// Fixed Unix writer descriptor reserved for Service Account mint.
pub const UNIX_MINT_WRITER_FD: u32 = 5;
/// Largest complete FXLM frame admitted on either helper pipe, including its header.
pub const MAX_HELPER_FRAME_BYTES: usize = 65_548;
/// Time allowed for request completion and, independently, pre-ceremony readiness.
pub const HELPER_SETUP_DEADLINE: Duration = Duration::from_secs(5);
/// Flux's absolute result deadline after request EOF.
pub const HELPER_RESULT_DEADLINE: Duration = Duration::from_secs(335);

/// Platform whose exact helper argv contract is being parsed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HelperPlatform {
    /// Unix fixed-descriptor ABI.
    Unix,
    /// Windows explicit-handle ABI.
    Windows,
}

/// One accepted invocation of `flux-exchange local`.
pub enum LocalHelperInvocation {
    /// Secret-bearing vendor ceremony, with its request and result capabilities.
    VendorSecret(VendorSecretCapabilities),
    /// Service Account mint with its separately transferred writer capability.
    ServiceAccountMint {
        /// Closed Service Account identifier.
        id: ServiceAccountId,
        /// Canonical positive Unix timestamp spelling from argv.
        expires_at: ExpiresAt,
        /// Platform writer capability; it never enters the FXSA body.
        writer: MintWriterCapability,
    },
}

/// Vendor-helper capabilities selected solely by the platform ABI.
pub enum VendorSecretCapabilities {
    /// Unix uses only fixed descriptors 6 and 7; descriptor 5 must be closed.
    Unix,
    /// Windows receives exactly two explicit, distinct handles.
    Windows {
        /// Readable anonymous request pipe.
        request: WindowsHandle,
        /// Writable anonymous terminal-response pipe.
        response: WindowsHandle,
    },
}

/// Service Account writer capability transferred independently from protocol bytes.
pub enum MintWriterCapability {
    /// The closed Unix ABI always names descriptor 5.
    UnixFd5,
    /// Windows carries one explicit nonzero handle.
    Windows(WindowsHandle),
}

/// Canonical nonzero Windows pointer-width handle spelling.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WindowsHandle(usize);

impl WindowsHandle {
    /// Native value to pass to platform capability validation, never protocol serialization.
    pub const fn native_value(self) -> usize {
        self.0
    }
}

/// A closed Service Account identifier (1..=64 ASCII label bytes).
#[derive(Clone, PartialEq, Eq)]
pub struct ServiceAccountId(String);

impl ServiceAccountId {
    /// Validated identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical positive `i64` expiry spelling.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ExpiresAt(i64);

impl ExpiresAt {
    /// Validated timestamp value.
    pub const fn value(self) -> i64 {
        self.0
    }
}

/// Value-free parser refusal. It never echoes an argv capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperGrammarRefusal {
    /// The option set, option order, or argument count is not the sole accepted grammar.
    Grammar,
    /// The Service Account identifier is outside its closed grammar.
    ServiceAccountId,
    /// The expiry is not canonical positive decimal in the admitted range.
    ExpiresAt,
    /// A Windows handle is zero, noncanonical, or outside pointer width.
    Handle,
    /// Request and response name the same Windows handle.
    DuplicateHandle,
}

impl fmt::Display for HelperGrammarRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Grammar => "local helper arguments do not match the closed grammar",
            Self::ServiceAccountId => "service account id is invalid",
            Self::ExpiresAt => "service account expiry is invalid",
            Self::Handle => "local helper handle is invalid",
            Self::DuplicateHandle => "local helper handles must be distinct",
        })
    }
}

impl std::error::Error for HelperGrammarRefusal {}

/// Parse the complete argv tail after the executable name.
pub fn parse_local_helper(
    platform: HelperPlatform,
    arguments: &[OsString],
) -> Result<LocalHelperInvocation, HelperGrammarRefusal> {
    let strings = arguments
        .iter()
        .map(OsString::as_os_str)
        .collect::<Vec<_>>();

    match platform {
        HelperPlatform::Unix => parse_unix(&strings),
        HelperPlatform::Windows => parse_windows(&strings),
    }
}

fn parse_unix(arguments: &[&OsStr]) -> Result<LocalHelperInvocation, HelperGrammarRefusal> {
    if arguments == [OsStr::new("local"), OsStr::new("vendor-secret")] {
        return Ok(LocalHelperInvocation::VendorSecret(
            VendorSecretCapabilities::Unix,
        ));
    }

    let [local, mode, id_flag, id, expiry_flag, expiry, writer_flag, writer] = arguments else {
        return Err(HelperGrammarRefusal::Grammar);
    };
    if *local != "local"
        || *mode != "service-account-mint"
        || *id_flag != "--id"
        || *expiry_flag != "--expires-at"
        || *writer_flag != "--writer-fd"
        || *writer != "5"
    {
        return Err(HelperGrammarRefusal::Grammar);
    }

    mint_invocation(id, expiry, MintWriterCapability::UnixFd5)
}

fn parse_windows(arguments: &[&OsStr]) -> Result<LocalHelperInvocation, HelperGrammarRefusal> {
    if let [local, mode, request_flag, request, response_flag, response] = arguments {
        if *local == "local"
            && *mode == "vendor-secret"
            && *request_flag == "--request-handle"
            && *response_flag == "--response-handle"
        {
            let request = parse_handle(request)?;
            let response = parse_handle(response)?;
            if request == response {
                return Err(HelperGrammarRefusal::DuplicateHandle);
            }
            return Ok(LocalHelperInvocation::VendorSecret(
                VendorSecretCapabilities::Windows { request, response },
            ));
        }
    }

    let [local, mode, id_flag, id, expiry_flag, expiry, writer_flag, writer] = arguments else {
        return Err(HelperGrammarRefusal::Grammar);
    };
    if *local != "local"
        || *mode != "service-account-mint"
        || *id_flag != "--id"
        || *expiry_flag != "--expires-at"
        || *writer_flag != "--writer-handle"
    {
        return Err(HelperGrammarRefusal::Grammar);
    }

    mint_invocation(
        id,
        expiry,
        MintWriterCapability::Windows(parse_handle(writer)?),
    )
}

fn mint_invocation(
    id: &OsStr,
    expires_at: &OsStr,
    writer: MintWriterCapability,
) -> Result<LocalHelperInvocation, HelperGrammarRefusal> {
    let id = id
        .to_str()
        .filter(|id| {
            (1..=64).contains(&id.len())
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or(HelperGrammarRefusal::ServiceAccountId)?;
    let expires_at = expires_at
        .to_str()
        .and_then(|value| parse_canonical_decimal(value, i64::MAX as u64))
        .ok_or(HelperGrammarRefusal::ExpiresAt)?;

    Ok(LocalHelperInvocation::ServiceAccountMint {
        id: ServiceAccountId(id.to_owned()),
        expires_at: ExpiresAt(expires_at as i64),
        writer,
    })
}

fn parse_handle(value: &OsStr) -> Result<WindowsHandle, HelperGrammarRefusal> {
    value
        .to_str()
        .and_then(|value| parse_canonical_decimal(value, usize::MAX as u64))
        .and_then(|value| usize::try_from(value).ok())
        .map(WindowsHandle)
        .ok_or(HelperGrammarRefusal::Handle)
}

fn parse_canonical_decimal(value: &str, maximum: u64) -> Option<u64> {
    if value.is_empty()
        || value == "0"
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse::<u64>().ok().filter(|value| *value <= maximum)
}

/// Exit codes are a two-value capability contract, not an application-error channel.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HelperExit {
    /// One complete terminal receipt or application error was written, closed, and delivered.
    TerminalFrameWritten = 0,
    /// Capability, native transport, response-write, or response-close prevented that contract.
    CapabilityOrTransportFailure = 1,
}

impl HelperExit {
    /// Sole process status admitted by the helper ABI.
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// Direction observed for one native anonymous-pipe capability.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PipeDirection {
    /// Read end only.
    Read,
    /// Write end only.
    Write,
    /// Not a usable unidirectional endpoint.
    Other,
}

/// Value-free facts obtained from an OS capability probe.
///
/// `pipe_identity` is an opaque equality token and is intentionally neither formatted nor
/// serializable.
pub struct PipeCapabilityFacts {
    /// Whether the OS object is an anonymous pipe.
    pub anonymous_pipe: bool,
    /// Usable direction of this endpoint.
    pub direction: PipeDirection,
    /// Opaque same-object identity from the platform probe.
    pub pipe_identity: u128,
}

/// Complete Unix closure facts measured on helper entry.
pub struct UnixVendorCapabilityFacts {
    /// Fixed descriptor 6.
    pub request: PipeCapabilityFacts,
    /// Fixed descriptor 7.
    pub response: PipeCapabilityFacts,
    /// Descriptor 5 remains closed and reserved for the other helper mode.
    pub fd5_closed: bool,
    /// Every descriptor at or above 3 except 6 and 7 is closed.
    pub all_other_nonstandard_fds_closed: bool,
}

/// Complete Windows inheritance-list facts measured on helper entry.
pub struct WindowsVendorCapabilityFacts {
    /// Capability named by `--request-handle`.
    pub request: PipeCapabilityFacts,
    /// Capability named by `--response-handle`.
    pub response: PipeCapabilityFacts,
    /// Number of handles in the launcher's explicit inheritance list.
    pub inherited_handle_count: usize,
}

/// Value-free native capability refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityRefusal {
    /// Required capability closure was not established.
    Closure,
    /// An endpoint is not a correctly directed anonymous pipe.
    Direction,
    /// Request and response are two names for one pipe endpoint/object.
    Duplicate,
}

impl fmt::Display for CapabilityRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Closure => "local helper capability set is not closed",
            Self::Direction => "local helper pipe direction is invalid",
            Self::Duplicate => "local helper pipe capabilities must be distinct",
        })
    }
}

impl std::error::Error for CapabilityRefusal {}

/// Validate the portable portion of the Unix FD contract before setting `FD_CLOEXEC` on 6 and 7.
pub fn validate_unix_vendor_capabilities(
    facts: &UnixVendorCapabilityFacts,
) -> Result<(), CapabilityRefusal> {
    if !facts.fd5_closed || !facts.all_other_nonstandard_fds_closed {
        return Err(CapabilityRefusal::Closure);
    }
    validate_pipe_pair(&facts.request, &facts.response)
}

/// Validate the portable portion of the Windows handle contract before clearing inheritance.
pub fn validate_windows_vendor_capabilities(
    facts: &WindowsVendorCapabilityFacts,
) -> Result<(), CapabilityRefusal> {
    if facts.inherited_handle_count != 2 {
        return Err(CapabilityRefusal::Closure);
    }
    validate_pipe_pair(&facts.request, &facts.response)
}

fn validate_pipe_pair(
    request: &PipeCapabilityFacts,
    response: &PipeCapabilityFacts,
) -> Result<(), CapabilityRefusal> {
    if !request.anonymous_pipe
        || request.direction != PipeDirection::Read
        || !response.anonymous_pipe
        || response.direction != PipeDirection::Write
    {
        return Err(CapabilityRefusal::Direction);
    }
    if request.pipe_identity == response.pipe_identity {
        return Err(CapabilityRefusal::Duplicate);
    }
    Ok(())
}

/// Refuse a truncated or oversized complete helper frame before any endpoint operation.
pub fn validate_complete_frame_size(size: usize) -> Result<(), FrameSizeRefusal> {
    if size < 12 {
        Err(FrameSizeRefusal::Truncated)
    } else if size > MAX_HELPER_FRAME_BYTES {
        Err(FrameSizeRefusal::TooLarge)
    } else {
        Ok(())
    }
}

/// Value-free helper frame-bound refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSizeRefusal {
    /// Fewer than the fixed 12-byte FXLM header.
    Truncated,
    /// More than one maximum-size complete FXLM frame.
    TooLarge,
}

impl fmt::Display for FrameSizeRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Truncated => "local helper frame is truncated",
            Self::TooLarge => "local helper frame is too large",
        })
    }
}

impl std::error::Error for FrameSizeRefusal {}

/// Typed endpoint seam. Implementations own native authentication and capability transfer.
///
/// This trait has no byte-valued handle parameter: the writer remains the typed capability in the
/// parsed invocation and is never invented inside FXLM/FXSA serialization.
pub trait LocalHelperEndpointPort {
    /// Platform-specific, value-free transport refusal.
    type Error;

    /// Execute the already-validated invocation through its owner-authenticated native endpoint.
    fn execute(&mut self, invocation: LocalHelperInvocation) -> Result<HelperExit, Self::Error>;
}
