//! The closed inherited-capability ABI for a Flux-owned local Exchange process.
//!
//! Readiness proves one successful startup. Liveness proves continuing ownership. They are
//! deliberately different pipes: neither carries a credential or becomes a later control channel.

use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::protocol_identity::{
    ProtocolVersions, CONNECTION_PLAN_V2, EFFECTIVE_CATALOGUE_RESPONSE_V1, EXCHANGE_API_V1,
    INVOKE_REQUEST_V1, INVOKE_RESPONSE_V1, LOCAL_MANAGEMENT_V1, PROTOCOL_VERSIONS,
    SERVICE_ACCOUNT_HANDOFF_V1, SUPERVISOR_READY_V2,
};

/// Maximum accepted size of the complete one-shot readiness object.
pub const MAX_READINESS_BYTES: usize = 16 * 1024;

/// Release identity compiled into both compatibility and readiness output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseIdentity {
    build_id: &'static str,
    source_commit: &'static str,
    tag: String,
    version: &'static str,
}

impl ReleaseIdentity {
    fn compiled() -> Self {
        let version = env!("CARGO_PKG_VERSION");
        Self {
            build_id: env!("FLUX_EXCHANGE_COMPILED_BUILD_ID"),
            source_commit: env!("FLUX_EXCHANGE_COMPILED_SOURCE_COMMIT"),
            tag: format!("refs/tags/v{version}"),
            version,
        }
    }
}

#[derive(Serialize)]
struct Compatibility {
    protocols: ProtocolVersions,
    release: ReleaseIdentity,
    schema: &'static str,
}

/// Return the exact side-effect-free compatibility document.
pub fn compatibility_json() -> Result<Vec<u8>, String> {
    canonical_json(&Compatibility {
        protocols: PROTOCOL_VERSIONS,
        release: ReleaseIdentity::compiled(),
        schema: "exchange.compatibility.v2",
    })
}

#[derive(Serialize)]
struct ReadyRelease {
    build_id: &'static str,
    executable_sha256: String,
    source_commit: &'static str,
    tag: String,
    version: &'static str,
}

#[derive(Serialize)]
struct ReadyBind {
    host: IpAddr,
    port: u16,
    scheme: &'static str,
}

#[derive(Serialize)]
struct ReadyProcess {
    pid: u32,
    start_identity: StartIdentity,
}

#[derive(Serialize)]
struct Readiness {
    bind: ReadyBind,
    process: ReadyProcess,
    protocols: ProtocolVersions,
    release: ReadyRelease,
    schema: &'static str,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum StartIdentity {
    #[cfg(target_os = "linux")]
    #[serde(rename = "linux-proc-start")]
    Linux { boot_id: String, ticks: String },
    #[cfg(target_os = "macos")]
    #[serde(rename = "macos-proc-start")]
    Macos { microseconds: u32, seconds: String },
    #[cfg(windows)]
    #[serde(rename = "windows-process-creation")]
    Windows { filetime: String },
}

/// The platform selected by the already-open child process handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePlatform {
    /// Linux `/proc` boot id and process start ticks.
    Linux,
    /// macOS `proc_pidinfo(PROC_PIDTBSDINFO)` start time.
    Macos,
    /// Windows `GetProcessTimes` creation FILETIME.
    Windows,
}

/// A native process-start identity obtained from an already-open child handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum VerifiedStartIdentity {
    /// Linux boot identity and `/proc/<pid>/stat` field 22.
    #[serde(rename = "linux-proc-start")]
    Linux {
        /// Lowercase RFC 4122 boot UUID.
        boot_id: String,
        /// Canonical nonzero `u64` decimal start ticks.
        ticks: String,
    },
    /// macOS process start timeval.
    #[serde(rename = "macos-proc-start")]
    Macos {
        /// Native microseconds, `0..=999999`.
        microseconds: u32,
        /// Canonical nonzero `i64::MAX`-bounded decimal seconds.
        seconds: String,
    },
    /// Windows process creation time.
    #[serde(rename = "windows-process-creation")]
    Windows {
        /// Canonical nonzero `u64` decimal FILETIME.
        filetime: String,
    },
}

/// Release facts already established from the verified executable and compatibility output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedRelease {
    /// Release tag reported by compatibility.
    pub tag: String,
    /// Stable package version reported by compatibility.
    pub version: String,
    /// Exact source commit reported by compatibility.
    pub source_commit: String,
    /// Exact build identity reported by compatibility.
    pub build_id: String,
    /// SHA-256 of the executable bytes the parent verified and spawned.
    pub executable_sha256: String,
}

/// Facts tied to the parent's already-open child handle before lifecycle ownership is committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessExpectation {
    /// Exact release and executable identity expected from the child.
    pub release: ExpectedRelease,
    /// PID from the open child handle. Diagnostic only unless start identity also agrees.
    pub pid: u32,
    /// Native platform of that open child handle.
    pub platform: NativePlatform,
    /// Native start identity read through that handle's platform source.
    pub start_identity: VerifiedStartIdentity,
}

/// A fully validated readiness record which may be committed as lifecycle ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedReadiness {
    /// Actual one-time bound listener.
    pub bind: VerifiedBind,
    /// Diagnostic PID plus native anti-reuse identity.
    pub process: VerifiedProcess,
    /// Eight exact provider protocol identities.
    pub protocols: VerifiedProtocols,
    /// Release and executable identity.
    pub release: VerifiedRelease,
    /// Exact `exchange.supervisor-ready.v2` schema identity.
    pub schema: String,
}

/// Closed readiness bind object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedBind {
    /// Literal `127.0.0.1` or `::1`.
    pub host: String,
    /// OS-selected nonzero port.
    pub port: u16,
    /// Literal `http`.
    pub scheme: String,
}

/// Closed readiness process object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedProcess {
    /// Diagnostic process id.
    pub pid: u32,
    /// Native anti-reuse process-start identity.
    pub start_identity: VerifiedStartIdentity,
}

/// Closed eight-field protocol object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedProtocols {
    /// Connection plan identity.
    pub connection_plan: String,
    /// Effective catalogue response identity.
    pub effective_catalogue_response: String,
    /// Exchange API identity.
    pub exchange_api: String,
    /// Invocation request identity.
    pub invoke_request: String,
    /// Invocation response identity.
    pub invoke_response: String,
    /// Owner-authenticated FXLM local-management identity.
    pub local_management: String,
    /// One-frame Service Account handoff identity.
    pub service_account_handoff: String,
    /// Supervision identity.
    pub supervisor: String,
}

/// Closed readiness release object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedRelease {
    /// Build identity.
    pub build_id: String,
    /// Digest of the exact executable bytes.
    pub executable_sha256: String,
    /// Source commit.
    pub source_commit: String,
    /// Immutable release tag.
    pub tag: String,
    /// Stable release version.
    pub version: String,
}

/// Why parent-side readiness verification refused lifecycle ownership.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReadinessRefusal {
    /// EOF arrived before a complete object.
    #[error("readiness ended before one complete object")]
    Incomplete,
    /// The bounded channel exceeded its protocol maximum.
    #[error("readiness exceeded the {MAX_READINESS_BYTES}-byte limit")]
    TooLarge,
    /// Bytes were not valid UTF-8.
    #[error("readiness is not UTF-8")]
    InvalidUtf8,
    /// JSON syntax, members or types did not match the closed schema.
    #[error("readiness is not one closed exchange.supervisor-ready.v2 object: {0}")]
    InvalidObject(String),
    /// Bytes did not equal the RFC 8785-equivalent canonical serialization.
    #[error("readiness is not canonical JSON")]
    NonCanonical,
    /// A typed value was outside its closed domain or disagreed with verified parent state.
    #[error("readiness identity mismatch: {0}")]
    Mismatch(String),
}

/// Verify one complete readiness channel before lifecycle ownership is committed.
///
/// HTTP health, PID files and a PID alone are deliberately absent inputs. The caller supplies the
/// release facts it verified and the native identity read through its already-open child handle;
/// only a returned [`VerifiedReadiness`] is eligible for an ownership commit.
pub fn verify_readiness(
    bytes: &[u8],
    expected: &ReadinessExpectation,
) -> Result<VerifiedReadiness, ReadinessRefusal> {
    if bytes.is_empty() {
        return Err(ReadinessRefusal::Incomplete);
    }
    if bytes.len() > MAX_READINESS_BYTES {
        return Err(ReadinessRefusal::TooLarge);
    }
    std::str::from_utf8(bytes).map_err(|_| ReadinessRefusal::InvalidUtf8)?;
    let record: VerifiedReadiness = serde_json::from_slice(bytes)
        .map_err(|error| ReadinessRefusal::InvalidObject(error.to_string()))?;
    let canonical = canonical_json(&record).map_err(ReadinessRefusal::InvalidObject)?;
    if canonical != bytes {
        return Err(ReadinessRefusal::NonCanonical);
    }
    validate_record(&record, expected)?;
    Ok(record)
}

fn validate_record(
    record: &VerifiedReadiness,
    expected: &ReadinessExpectation,
) -> Result<(), ReadinessRefusal> {
    let mismatch = |reason: &str| ReadinessRefusal::Mismatch(reason.to_owned());
    if record.schema != SUPERVISOR_READY_V2.as_str() {
        return Err(mismatch("schema"));
    }
    if record.bind.scheme != "http"
        || !matches!(record.bind.host.as_str(), "127.0.0.1" | "::1")
        || record.bind.port == 0
    {
        return Err(mismatch("bind"));
    }
    if record.process.pid == 0 || record.process.pid != expected.pid {
        return Err(mismatch("pid"));
    }
    validate_start_identity(&record.process.start_identity)?;
    let platform_agrees = matches!(
        (expected.platform, &record.process.start_identity),
        (NativePlatform::Linux, VerifiedStartIdentity::Linux { .. })
            | (NativePlatform::Macos, VerifiedStartIdentity::Macos { .. })
            | (
                NativePlatform::Windows,
                VerifiedStartIdentity::Windows { .. }
            )
    );
    if !platform_agrees || record.process.start_identity != expected.start_identity {
        return Err(mismatch("native process-start identity"));
    }
    let protocols = &record.protocols;
    if protocols.connection_plan != CONNECTION_PLAN_V2.as_str()
        || protocols.exchange_api != EXCHANGE_API_V1.as_str()
        || protocols.effective_catalogue_response != EFFECTIVE_CATALOGUE_RESPONSE_V1.as_str()
        || protocols.invoke_request != INVOKE_REQUEST_V1.as_str()
        || protocols.invoke_response != INVOKE_RESPONSE_V1.as_str()
        || protocols.local_management != LOCAL_MANAGEMENT_V1.as_str()
        || protocols.service_account_handoff != SERVICE_ACCOUNT_HANDOFF_V1.as_str()
        || protocols.supervisor != SUPERVISOR_READY_V2.as_str()
        || protocols.supervisor != record.schema
    {
        return Err(mismatch("protocols"));
    }
    validate_release(&record.release)?;
    let release = &expected.release;
    if record.release.tag != release.tag
        || record.release.version != release.version
        || record.release.source_commit != release.source_commit
        || record.release.build_id != release.build_id
        || record.release.executable_sha256 != release.executable_sha256
    {
        return Err(mismatch("release or executable"));
    }
    Ok(())
}

fn validate_release(release: &VerifiedRelease) -> Result<(), ReadinessRefusal> {
    let mismatch = |reason: &str| ReadinessRefusal::Mismatch(reason.to_owned());
    if !valid_stable_version(&release.version)
        || release.tag != format!("refs/tags/v{}", release.version)
        || release.source_commit.len() != 40
        || !lower_hex(&release.source_commit)
        || release.executable_sha256.len() != 64
        || !lower_hex(&release.executable_sha256)
        || release.build_id.is_empty()
        || release.build_id.len() > 128
        || !release
            .build_id
            .bytes()
            .all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(mismatch("release field domain"));
    }
    Ok(())
}

fn validate_start_identity(identity: &VerifiedStartIdentity) -> Result<(), ReadinessRefusal> {
    let mismatch = |reason: &str| ReadinessRefusal::Mismatch(reason.to_owned());
    match identity {
        VerifiedStartIdentity::Linux { boot_id, ticks } => {
            if !valid_lower_uuid_any_target(boot_id)
                || parse_decimal(ticks, u64::MAX, false).is_err()
            {
                return Err(mismatch("Linux process-start domain"));
            }
        }
        VerifiedStartIdentity::Macos {
            microseconds,
            seconds,
        } => {
            if *microseconds > 999_999 || parse_decimal(seconds, i64::MAX as u64, false).is_err() {
                return Err(mismatch("macOS process-start domain"));
            }
        }
        VerifiedStartIdentity::Windows { filetime } => {
            if parse_decimal(filetime, u64::MAX, false).is_err() {
                return Err(mismatch("Windows process-start domain"));
            }
        }
    }
    Ok(())
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_stable_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 9
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    parts.next().is_some_and(valid_part)
        && parts.next().is_some_and(valid_part)
        && parts.next().is_some_and(valid_part)
        && parts.next().is_none()
}

fn valid_lower_uuid_any_target(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

/// The validated readiness writer retained after the liveness thread starts.
pub struct Supervision {
    readiness: ReadyWriter,
}

impl Supervision {
    /// Discover the platform's exact inherited capabilities and start native liveness monitoring.
    ///
    /// This is synchronous by design: the caller invokes it before constructing a Tokio runtime or
    /// opening any durable store.
    pub fn discover(arguments: &[std::ffi::OsString]) -> Result<Self, String> {
        let (readiness, liveness) = discover_capabilities(arguments)?;
        std::thread::Builder::new()
            .name("flux-exchange-supervisor-liveness".to_owned())
            .spawn(move || liveness_wait(liveness))
            .map_err(|error| format!("cannot start supervisor liveness thread: {error}"))?;
        Ok(Self { readiness })
    }

    /// Emit exactly one bounded canonical object after the already-bound listener is ready.
    pub fn ready(mut self, bind: SocketAddr) -> Result<(), String> {
        if !matches!(
            bind.ip(),
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) | IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
        ) || bind.port() == 0
        {
            return Err(format!(
                "refusing supervised readiness for non-bound loopback address {bind}"
            ));
        }
        let release = ReleaseIdentity::compiled();
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot identify the running executable: {error}"))?;
        let bytes = canonical_json(&Readiness {
            bind: ReadyBind {
                host: bind.ip(),
                port: bind.port(),
                scheme: "http",
            },
            process: ReadyProcess {
                pid: std::process::id(),
                start_identity: native_start_identity()?,
            },
            protocols: PROTOCOL_VERSIONS,
            release: ReadyRelease {
                build_id: release.build_id,
                executable_sha256: sha256_file(&executable)?,
                source_commit: release.source_commit,
                tag: release.tag,
                version: release.version,
            },
            schema: SUPERVISOR_READY_V2.as_str(),
        })?;
        if bytes.len() > MAX_READINESS_BYTES {
            return Err(format!(
                "supervisor readiness record is {} bytes; the limit is {MAX_READINESS_BYTES}",
                bytes.len()
            ));
        }
        self.readiness.write_once(&bytes)
    }
}

fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    // `serde_json::Value` uses a sorted map without `preserve_order`. Rendering that normalized
    // tree gives the RFC 8785 member order and JSON escaping needed by these integer-only schemas.
    let value = serde_json::to_value(value)
        .map_err(|error| format!("cannot construct canonical protocol JSON: {error}"))?;
    serde_json::to_vec(&value).map_err(|error| format!("cannot serialize protocol JSON: {error}"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("cannot read executable `{}`: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash executable `{}`: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(target_os = "linux")]
fn native_start_identity() -> Result<StartIdentity, String> {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|error| format!("cannot read Linux boot identity: {error}"))?;
    let boot_id = boot_id.trim();
    if !valid_lower_uuid(boot_id) {
        return Err("Linux boot identity is not a lowercase RFC 4122 UUID".to_owned());
    }
    let stat = std::fs::read_to_string("/proc/self/stat")
        .map_err(|error| format!("cannot read Linux process start identity: {error}"))?;
    let after_name = stat
        .rfind(") ")
        .and_then(|index| stat.get(index + 2..))
        .ok_or_else(|| "Linux process stat has no closed command field".to_owned())?;
    // After the command field the first token is field 3; starttime is field 22.
    let ticks = after_name
        .split_ascii_whitespace()
        .nth(19)
        .ok_or_else(|| "Linux process stat has no starttime field".to_owned())?;
    let ticks = parse_decimal(ticks, u64::MAX, false)?;
    Ok(StartIdentity::Linux {
        boot_id: boot_id.to_owned(),
        ticks: ticks.to_string(),
    })
}

#[cfg(target_os = "macos")]
fn native_start_identity() -> Result<StartIdentity, String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    // SAFETY: `info` points to a buffer of the exact `PROC_PIDTBSDINFO` size and remains local.
    let read = unsafe {
        libc::proc_pidinfo(
            std::process::id() as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if read != size as i32 {
        return Err(format!(
            "cannot read macOS process start identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `proc_pidinfo` filled the complete structure above.
    let info = unsafe { info.assume_init() };
    if info.pbi_start_tvsec == 0
        || info.pbi_start_tvsec > i64::MAX as u64
        || info.pbi_start_tvusec > 999_999
    {
        return Err("macOS returned an out-of-domain process start identity".to_owned());
    }
    Ok(StartIdentity::Macos {
        microseconds: info.pbi_start_tvusec as u32,
        seconds: info.pbi_start_tvsec.to_string(),
    })
}

#[cfg(windows)]
fn native_start_identity() -> Result<StartIdentity, String> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all four output pointers are valid for the duration of the call.
    let success = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if success == 0 {
        return Err(format!(
            "cannot read Windows process creation identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    let filetime = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if filetime == 0 {
        return Err("Windows returned a zero process creation identity".to_owned());
    }
    Ok(StartIdentity::Windows {
        filetime: filetime.to_string(),
    })
}

fn parse_decimal(value: &str, maximum: u64, allow_zero: bool) -> Result<u64, String> {
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("noncanonical decimal value {value:?}"));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("decimal value {value:?} is out of range"))?;
    if parsed > maximum || (!allow_zero && parsed == 0) {
        return Err(format!("decimal value {value:?} is out of range"));
    }
    Ok(parsed)
}

#[cfg(target_os = "linux")]
fn valid_lower_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

#[cfg(unix)]
struct ReadyWriter(std::fs::File);

#[cfg(unix)]
struct LivenessReader(std::fs::File);

#[cfg(unix)]
fn discover_capabilities(
    arguments: &[std::ffi::OsString],
) -> Result<(ReadyWriter, LivenessReader), String> {
    use std::os::fd::FromRawFd;

    if arguments != [std::ffi::OsString::from("--supervised")] {
        return Err("Unix supervised usage is exactly `flux-exchange --supervised`".to_owned());
    }
    validate_unix_fd(3, libc::O_WRONLY, "readiness")?;
    validate_unix_fd(4, libc::O_RDONLY, "liveness")?;
    if unix_fds_alias(3, 4)? {
        return Err("supervisor readiness FD 3 and liveness FD 4 alias one pipe".to_owned());
    }
    refuse_extra_unix_fds()?;
    set_cloexec(3)?;
    set_cloexec(4)?;
    // SAFETY: the exact inherited descriptors were validated above and ownership transfers once.
    let readiness = unsafe { std::fs::File::from_raw_fd(3) };
    // SAFETY: as above, for the independently validated liveness descriptor.
    let liveness = unsafe { std::fs::File::from_raw_fd(4) };
    Ok((ReadyWriter(readiness), LivenessReader(liveness)))
}

#[cfg(target_os = "linux")]
fn unix_fds_alias(left: libc::c_int, right: libc::c_int) -> Result<bool, String> {
    let left = fd_stat(left)?;
    let right = fd_stat(right)?;
    Ok(left.st_dev == right.st_dev && left.st_ino == right.st_ino)
}

#[cfg(target_os = "macos")]
fn unix_fds_alias(left: libc::c_int, right: libc::c_int) -> Result<bool, String> {
    let left = macos_pipe_identity(left)?;
    let right = macos_pipe_identity(right)?;
    Ok(
        (left.handle == right.handle && left.peer_handle == right.peer_handle)
            || (left.handle == right.peer_handle && left.peer_handle == right.handle),
    )
}

#[cfg(target_os = "macos")]
struct MacosPipeIdentity {
    handle: u64,
    peer_handle: u64,
}

#[cfg(target_os = "macos")]
fn macos_pipe_identity(fd: libc::c_int) -> Result<MacosPipeIdentity, String> {
    const PROC_PIDFDPIPEINFO: libc::c_int = 6;

    #[repr(C)]
    struct ProcFileInfo {
        _open_flags: u32,
        _status: u32,
        _offset: i64,
        _file_type: i32,
        _guard_flags: u32,
    }

    #[repr(C)]
    struct VinfoStat {
        _device: u32,
        _mode: u16,
        _links: u16,
        _inode: u64,
        _user: u32,
        _group: u32,
        _access_time: i64,
        _access_time_nanoseconds: i64,
        _modification_time: i64,
        _modification_time_nanoseconds: i64,
        _change_time: i64,
        _change_time_nanoseconds: i64,
        _birth_time: i64,
        _birth_time_nanoseconds: i64,
        _size: i64,
        _blocks: i64,
        _block_size: i32,
        _flags: u32,
        _generation: u32,
        _raw_device: u32,
        _spare: [i64; 2],
    }

    #[repr(C)]
    struct PipeInfo {
        _stat: VinfoStat,
        handle: u64,
        peer_handle: u64,
        _status: i32,
        _reserved: i32,
    }

    #[repr(C)]
    struct PipeFdInfo {
        _file: ProcFileInfo,
        pipe: PipeInfo,
    }

    let mut info = std::mem::MaybeUninit::<PipeFdInfo>::zeroed();
    let size = std::mem::size_of::<PipeFdInfo>();
    let returned = unsafe {
        // SAFETY: the buffer has the exact public `pipe_fdinfo` layout and remains valid for the
        // duration of this read-only query about one descriptor owned by this process.
        libc::proc_pidfdinfo(
            libc::getpid(),
            fd,
            PROC_PIDFDPIPEINFO,
            info.as_mut_ptr().cast(),
            size as libc::c_int,
        )
    };
    if returned != size as libc::c_int {
        return Err(format!(
            "cannot identify required supervisor FD {fd} as one native pipe endpoint: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `proc_pidfdinfo` returned the exact complete structure size.
    let info = unsafe { info.assume_init() };
    if info.pipe.handle == 0 || info.pipe.peer_handle == 0 {
        return Err(format!(
            "cannot identify required supervisor FD {fd} as one native pipe endpoint"
        ));
    }
    Ok(MacosPipeIdentity {
        handle: info.pipe.handle,
        peer_handle: info.pipe.peer_handle,
    })
}

#[cfg(unix)]
fn fd_stat(fd: libc::c_int) -> Result<libc::stat, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `stat` is a valid output buffer and `fd` is only observed.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(format!(
            "required supervisor FD {fd} is absent: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: successful `fstat` initialized the complete value.
    Ok(unsafe { stat.assume_init() })
}

#[cfg(unix)]
fn validate_unix_fd(fd: libc::c_int, direction: libc::c_int, name: &str) -> Result<(), String> {
    let stat = fd_stat(fd)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err(format!("supervisor {name} FD {fd} is not a pipe"));
    }
    // SAFETY: `F_GETFL` only observes the validated descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || flags & libc::O_ACCMODE != direction {
        return Err(format!("supervisor {name} FD {fd} has the wrong direction"));
    }
    Ok(())
}

#[cfg(unix)]
fn set_cloexec(fd: libc::c_int) -> Result<(), String> {
    // SAFETY: both operations only inspect/update the descriptor flag word.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(format!(
            "cannot protect inherited supervisor FD {fd}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn refuse_extra_unix_fds() -> Result<(), String> {
    let entries = std::fs::read_dir("/proc/self/fd")
        .map_err(|error| format!("cannot enumerate inherited descriptors: {error}"))?
        .map(|entry| {
            entry
                .map_err(|error| error.to_string())?
                .file_name()
                .into_string()
                .map_err(|_| "non-UTF-8 descriptor number".to_owned())?
                .parse::<i32>()
                .map_err(|_| "non-numeric descriptor entry".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The directory descriptor used for enumeration is closed before this check and is therefore
    // ignored by `F_GETFD`; every inherited descriptor remains open and is refused.
    for fd in entries.into_iter().filter(|fd| *fd > 4) {
        // SAFETY: `F_GETFD` observes whether this integer currently names an open descriptor.
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0 {
            return Err(format!("unexpected inherited nonstandard FD {fd}"));
        }
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn refuse_extra_unix_fds() -> Result<(), String> {
    // SAFETY: `sysconf` has no pointer arguments or side effects.
    let maximum = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    if maximum < 0 {
        return Err("cannot determine inherited descriptor bound".to_owned());
    }
    for fd in 5..maximum.min(i64::from(i32::MAX)) {
        // SAFETY: `F_GETFD` only observes this possible descriptor number.
        if unsafe { libc::fcntl(fd as i32, libc::F_GETFD) } >= 0 {
            return Err(format!("unexpected inherited nonstandard FD {fd}"));
        }
    }
    Ok(())
}

#[cfg(unix)]
impl ReadyWriter {
    fn write_once(&mut self, bytes: &[u8]) -> Result<(), String> {
        use std::os::fd::AsRawFd;
        // SAFETY: `bytes` stays live, and the owned pipe FD remains open for this one call.
        let written =
            unsafe { libc::write(self.0.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 || written as usize != bytes.len() {
            return Err(format!(
                "cannot write the complete supervisor readiness record: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(unix)]
fn liveness_wait(reader: LivenessReader) -> ! {
    use std::os::fd::AsRawFd;
    let mut byte = 0_u8;
    // SAFETY: the buffer and owned pipe FD remain live. Every outcome deliberately exits.
    let _ = unsafe { libc::read(reader.0.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
    // SAFETY: liveness loss must terminate without unwinding, even if Tokio is wedged.
    unsafe { libc::_exit(1) }
}

#[cfg(windows)]
struct ReadyWriter(std::fs::File);

#[cfg(windows)]
struct LivenessReader(std::fs::File);

#[cfg(windows)]
use crate::windows_handle::validate_supervisor_handle as validate_windows_handle;

#[cfg(windows)]
fn discover_capabilities(
    arguments: &[std::ffi::OsString],
) -> Result<(ReadyWriter, LivenessReader), String> {
    use std::os::windows::io::FromRawHandle;

    let expected = [
        "--supervised",
        "--supervisor-readiness-handle",
        "<readiness>",
        "--supervisor-liveness-handle",
        "<liveness>",
    ];
    if arguments.len() != expected.len()
        || arguments[0] != expected[0]
        || arguments[1] != expected[1]
        || arguments[3] != expected[3]
    {
        return Err("Windows supervised usage is exactly `flux-exchange.exe --supervised --supervisor-readiness-handle <H> --supervisor-liveness-handle <H>`".to_owned());
    }
    let readiness = parse_windows_handle(&arguments[2], "readiness")?;
    let liveness = parse_windows_handle(&arguments[4], "liveness")?;
    if readiness == liveness {
        return Err("supervisor readiness and liveness HANDLE values are identical".to_owned());
    }

    validate_windows_handle(readiness, false, "readiness")?;
    validate_windows_handle(liveness, true, "liveness")?;
    // SAFETY: the two distinct validated HANDLE capabilities transfer ownership exactly once.
    let readiness = unsafe { std::fs::File::from_raw_handle(readiness.cast()) };
    // SAFETY: as above for liveness.
    let liveness = unsafe { std::fs::File::from_raw_handle(liveness.cast()) };
    Ok((ReadyWriter(readiness), LivenessReader(liveness)))
}

#[cfg(windows)]
fn parse_windows_handle(
    value: &std::ffi::OsStr,
    name: &str,
) -> Result<windows_sys::Win32::Foundation::HANDLE, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("supervisor {name} HANDLE is not UTF-8 decimal"))?;
    let parsed = parse_decimal(value, usize::MAX as u64, false)?;
    Ok(parsed as usize as windows_sys::Win32::Foundation::HANDLE)
}

#[cfg(windows)]
impl ReadyWriter {
    fn write_once(&mut self, bytes: &[u8]) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::WriteFile;
        let mut written = 0_u32;
        // SAFETY: the byte slice and output count remain live for this single synchronous write.
        let success = unsafe {
            WriteFile(
                self.0.as_raw_handle().cast(),
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if success == 0 || written as usize != bytes.len() {
            return Err(format!(
                "cannot write the complete supervisor readiness record: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn liveness_wait(mut reader: LivenessReader) -> ! {
    use windows_sys::Win32::System::Threading::ExitProcess;
    let mut byte = [0_u8; 1];
    let _ = reader.0.read(&mut byte);
    // SAFETY: liveness loss must terminate without unwinding, even if Tokio is wedged.
    unsafe { ExitProcess(1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConformanceFixture {
        record: VerifiedReadiness,
        expected: ReadinessExpectation,
    }

    impl ConformanceFixture {
        fn linux() -> Self {
            let start_identity = VerifiedStartIdentity::Linux {
                boot_id: "00000000-0000-0000-0000-000000000001".to_owned(),
                ticks: "1".to_owned(),
            };
            let release = VerifiedRelease {
                build_id: "fixture-build".to_owned(),
                executable_sha256: "11".repeat(32),
                source_commit: "22".repeat(20),
                tag: "refs/tags/v1.2.3".to_owned(),
                version: "1.2.3".to_owned(),
            };
            Self {
                record: VerifiedReadiness {
                    bind: VerifiedBind {
                        host: "127.0.0.1".to_owned(),
                        port: 43123,
                        scheme: "http".to_owned(),
                    },
                    process: VerifiedProcess {
                        pid: 42,
                        start_identity: start_identity.clone(),
                    },
                    protocols: VerifiedProtocols {
                        connection_plan: CONNECTION_PLAN_V2.as_str().to_owned(),
                        effective_catalogue_response: EFFECTIVE_CATALOGUE_RESPONSE_V1
                            .as_str()
                            .to_owned(),
                        exchange_api: EXCHANGE_API_V1.as_str().to_owned(),
                        invoke_request: INVOKE_REQUEST_V1.as_str().to_owned(),
                        invoke_response: INVOKE_RESPONSE_V1.as_str().to_owned(),
                        local_management: LOCAL_MANAGEMENT_V1.as_str().to_owned(),
                        service_account_handoff: SERVICE_ACCOUNT_HANDOFF_V1.as_str().to_owned(),
                        supervisor: SUPERVISOR_READY_V2.as_str().to_owned(),
                    },
                    release: release.clone(),
                    schema: SUPERVISOR_READY_V2.as_str().to_owned(),
                },
                expected: ReadinessExpectation {
                    release: ExpectedRelease {
                        tag: release.tag,
                        version: release.version,
                        source_commit: release.source_commit,
                        build_id: release.build_id,
                        executable_sha256: release.executable_sha256,
                    },
                    pid: 42,
                    platform: NativePlatform::Linux,
                    start_identity,
                },
            }
        }

        fn bytes(&self) -> Vec<u8> {
            canonical_json(&self.record).expect("canonical readiness fixture")
        }
    }

    #[derive(Default)]
    struct OwnershipState {
        committed: Option<VerifiedReadiness>,
    }

    impl OwnershipState {
        fn verify_then_commit(&mut self, bytes: &[u8], expected: &ReadinessExpectation) {
            if let Ok(verified) = verify_readiness(bytes, expected) {
                self.committed = Some(verified);
            }
        }
    }

    fn changed(
        fixture: &ConformanceFixture,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> Vec<u8> {
        let mut value = serde_json::to_value(&fixture.record).expect("fixture value");
        mutate(&mut value);
        canonical_json(&value).expect("canonical mutated fixture")
    }

    fn assert_refused(bytes: &[u8], expected: &ReadinessExpectation) {
        let mut ownership = OwnershipState::default();
        ownership.verify_then_commit(bytes, expected);
        assert!(ownership.committed.is_none(), "refusal committed ownership");
        assert!(verify_readiness(bytes, expected).is_err());
    }

    #[test]
    fn strict_parent_verifier_commits_only_one_complete_canonical_matching_record() {
        let fixture = ConformanceFixture::linux();
        let bytes = fixture.bytes();
        let mut ownership = OwnershipState::default();
        ownership.verify_then_commit(&bytes, &fixture.expected);
        assert_eq!(ownership.committed, Some(fixture.record.clone()));

        assert_eq!(
            verify_readiness(b"", &fixture.expected),
            Err(ReadinessRefusal::Incomplete)
        );
        assert_refused(b"{", &fixture.expected);
        assert_eq!(
            verify_readiness(&vec![b' '; MAX_READINESS_BYTES + 1], &fixture.expected),
            Err(ReadinessRefusal::TooLarge)
        );
        assert_eq!(
            verify_readiness(&[0xff], &fixture.expected),
            Err(ReadinessRefusal::InvalidUtf8)
        );

        let mut duplicate = bytes[..bytes.len() - 1].to_vec();
        duplicate.extend_from_slice(b",\"schema\":\"exchange.supervisor-ready.v2\"}");
        assert_refused(&duplicate, &fixture.expected);
        assert_refused(
            &changed(&fixture, |value| value["unknown"] = serde_json::json!(true)),
            &fixture.expected,
        );
        let mut trailing = bytes.clone();
        trailing.extend_from_slice(b"x");
        assert_refused(&trailing, &fixture.expected);
        let mut second = bytes.clone();
        second.extend_from_slice(&bytes);
        assert_refused(&second, &fixture.expected);
        let mut noncanonical = b" ".to_vec();
        noncanonical.extend_from_slice(&bytes);
        assert_eq!(
            verify_readiness(&noncanonical, &fixture.expected),
            Err(ReadinessRefusal::NonCanonical)
        );
    }

    #[test]
    fn closed_readiness_serializer_has_no_authority_value_slot() {
        let _closed_emitter: fn(Supervision, SocketAddr) -> Result<(), String> = Supervision::ready;
        // These represent the six value classes the production server can hold or receive. The
        // emitter consumes only its validated capability plus the actual socket address, and the
        // readiness serializer accepts only `VerifiedReadiness`; unlike a map or flattened state
        // object, neither has a state/store input through which any of them can enter.
        let authority_values = [
            "credential-secret-7c96d9",
            "setting-value-7c96d9",
            "grant-body-7c96d9",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "session-value-7c96d9",
            "control-credential-7c96d9",
        ];
        let bytes = ConformanceFixture::linux().bytes();
        let object: serde_json::Value = serde_json::from_slice(&bytes).expect("readiness object");
        assert_eq!(
            object
                .as_object()
                .expect("top-level object")
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            ["bind", "process", "protocols", "release", "schema"]
        );
        for value in authority_values {
            assert!(!bytes
                .windows(value.len())
                .any(|window| window == value.as_bytes()));
        }
    }

    #[test]
    fn every_identity_axis_and_unknown_or_duplicate_nested_member_refuses_commit() {
        let fixture = ConformanceFixture::linux();
        let mutations: Vec<Vec<u8>> = vec![
            changed(&fixture, |v| v["schema"] = serde_json::json!("other.v1")),
            changed(&fixture, |v| {
                v["bind"]["scheme"] = serde_json::json!("https")
            }),
            changed(&fixture, |v| {
                v["bind"]["host"] = serde_json::json!("127.0.0.2")
            }),
            changed(&fixture, |v| v["bind"]["port"] = serde_json::json!(0)),
            changed(&fixture, |v| v["process"]["pid"] = serde_json::json!(0)),
            changed(&fixture, |v| v["process"]["pid"] = serde_json::json!(43)),
            changed(&fixture, |v| {
                v["release"]["tag"] = serde_json::json!("refs/tags/v1.2.4")
            }),
            changed(&fixture, |v| {
                v["release"]["version"] = serde_json::json!("01.2.3")
            }),
            changed(&fixture, |v| {
                v["release"]["source_commit"] = serde_json::json!("A".repeat(40))
            }),
            changed(&fixture, |v| {
                v["release"]["build_id"] = serde_json::json!("")
            }),
            changed(&fixture, |v| {
                v["release"]["executable_sha256"] = serde_json::json!("g".repeat(64))
            }),
            changed(&fixture, |v| {
                v["protocols"]["exchange_api"] = serde_json::json!("exchange.api.v2")
            }),
            changed(&fixture, |v| {
                v["protocols"]["supervisor"] = serde_json::json!("exchange.supervisor-ready.v3")
            }),
            changed(&fixture, |v| {
                v["protocols"]
                    .as_object_mut()
                    .expect("protocol object")
                    .remove("local_management");
            }),
            changed(&fixture, |v| {
                v["protocols"]
                    .as_object_mut()
                    .expect("protocol object")
                    .remove("service_account_handoff");
            }),
            changed(&fixture, |v| {
                v["process"]["unknown"] = serde_json::json!(true)
            }),
            changed(&fixture, |v| {
                v["release"]["unknown"] = serde_json::json!(true)
            }),
            changed(&fixture, |v| {
                v["protocols"]["unknown"] = serde_json::json!(true)
            }),
        ];
        for bytes in mutations {
            assert_refused(&bytes, &fixture.expected);
        }
        for field in [
            "connection_plan",
            "effective_catalogue_response",
            "exchange_api",
            "invoke_request",
            "invoke_response",
            "local_management",
            "service_account_handoff",
            "supervisor",
        ] {
            assert_refused(
                &changed(&fixture, |value| {
                    value["protocols"][field] = serde_json::json!("exchange.wrong.v1");
                }),
                &fixture.expected,
            );
        }

        let bytes = fixture.bytes();
        let duplicate_nested = String::from_utf8(bytes).expect("UTF-8 fixture").replacen(
            "\"scheme\":\"http\"",
            "\"scheme\":\"http\",\"scheme\":\"http\"",
            1,
        );
        assert_refused(duplicate_nested.as_bytes(), &fixture.expected);
        let out_of_range_port = String::from_utf8(fixture.bytes())
            .expect("UTF-8 fixture")
            .replacen("\"port\":43123", "\"port\":65536", 1);
        assert_refused(out_of_range_port.as_bytes(), &fixture.expected);
        let out_of_range_pid = String::from_utf8(fixture.bytes())
            .expect("UTF-8 fixture")
            .replacen("\"pid\":42", "\"pid\":4294967296", 1);
        assert_refused(out_of_range_pid.as_bytes(), &fixture.expected);
    }

    #[test]
    fn every_native_start_tag_encoding_and_domain_mutation_refuses() {
        let fixture = ConformanceFixture::linux();
        let identities = [
            serde_json::json!({"kind":"unknown","value":"1"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-00000000000A","ticks":"1"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"not-a-uuid","ticks":"1"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"0"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"01"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"+1"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"-1"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"18446744073709551616"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"111111111111111111111"}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"1","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"0","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"01","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"+1","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"-1","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"111111111111111111111","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"1","microseconds":-1}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"1","microseconds":1.5}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"9223372036854775808","microseconds":0}),
            serde_json::json!({"kind":"macos-proc-start","seconds":"1","microseconds":1000000}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"1"}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"0"}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"01"}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"+1"}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"-1"}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"18446744073709551616"}),
            serde_json::json!({"kind":"windows-process-creation","filetime":"111111111111111111111"}),
            serde_json::json!({"kind":"linux-proc-start","boot_id":"00000000-0000-0000-0000-000000000001","ticks":"1","unknown":true}),
        ];
        for identity in identities {
            let bytes = changed(&fixture, |value| {
                value["process"]["start_identity"] = identity;
            });
            assert_refused(&bytes, &fixture.expected);
        }
    }

    #[test]
    fn health_pid_files_and_pid_reuse_never_substitute_for_verified_readiness() {
        let fixture = ConformanceFixture::linux();
        let foreign_health = std::net::TcpListener::bind("127.0.0.1:0").expect("foreign health");
        let pid_file = std::env::temp_dir().join(format!(
            "flux-exchange-x128-planted-pid-{}",
            std::process::id()
        ));
        std::fs::write(&pid_file, fixture.expected.pid.to_string()).expect("planted PID file");
        assert!(foreign_health.local_addr().is_ok());
        assert!(pid_file.exists());
        assert_refused(b"", &fixture.expected);

        let reused = changed(&fixture, |value| {
            value["process"]["start_identity"]["ticks"] = serde_json::json!("2");
        });
        assert_refused(&reused, &fixture.expected);
        std::fs::remove_file(pid_file).expect("remove planted PID fixture");
    }

    #[test]
    fn compatibility_is_exact_canonical_json_from_the_eight_field_source() {
        let bytes = compatibility_json().expect("compatibility JSON");
        let build_id = serde_json::to_string(env!("FLUX_EXCHANGE_COMPILED_BUILD_ID"))
            .expect("build id string");
        let source_commit = serde_json::to_string(env!("FLUX_EXCHANGE_COMPILED_SOURCE_COMMIT"))
            .expect("source commit string");
        let version = env!("CARGO_PKG_VERSION");
        let expected = format!(
            "{{\"protocols\":{{\"connection_plan\":\"exchange.connection-plan.v2\",\"effective_catalogue_response\":\"exchange.effective-catalogue-response.v1\",\"exchange_api\":\"exchange.api.v1\",\"invoke_request\":\"exchange.invoke-request.v1\",\"invoke_response\":\"exchange.invoke-response.v1\",\"local_management\":\"exchange.local-management.v1\",\"service_account_handoff\":\"exchange.service-account-handoff.v1\",\"supervisor\":\"exchange.supervisor-ready.v2\"}},\"release\":{{\"build_id\":{build_id},\"source_commit\":{source_commit},\"tag\":\"refs/tags/v{version}\",\"version\":\"{version}\"}},\"schema\":\"exchange.compatibility.v2\"}}"
        );
        assert_eq!(bytes, expected.as_bytes());
        assert!(!bytes.ends_with(b"\n"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("compatibility value");
        assert_eq!(value["schema"], "exchange.compatibility.v2");
        assert_eq!(
            value["protocols"]["supervisor"],
            SUPERVISOR_READY_V2.as_str()
        );
        assert_eq!(canonical_json(&value).expect("canonical value"), bytes);
        assert_ne!(value["release"]["source_commit"], "unknown");
        assert_ne!(value["release"]["build_id"], "0");
    }

    #[test]
    fn decimal_capabilities_are_closed_and_canonical() {
        for invalid in [
            "",
            "0",
            "00",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "18446744073709551616",
        ] {
            assert!(
                parse_decimal(invalid, u64::MAX, false).is_err(),
                "{invalid:?}"
            );
        }
        assert_eq!(parse_decimal("1", u64::MAX, false), Ok(1));
        assert_eq!(
            parse_decimal("18446744073709551615", u64::MAX, false),
            Ok(u64::MAX)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_validator_refuses_noninherited_nonpipe_and_each_wrong_direction() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{
            CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
        };
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::System::Pipes::CreatePipe;

        fn pipe() -> (HANDLE, HANDLE) {
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };
            let mut read = std::ptr::null_mut();
            let mut write = std::ptr::null_mut();
            // SAFETY: output pointers and security attributes remain live for the call.
            assert_ne!(
                unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
                0
            );
            (read, write)
        }

        fn inheritable(handle: HANDLE, inherited: bool) {
            // SAFETY: only the inheritance bit of this owned fixture handle changes.
            assert_ne!(
                unsafe {
                    SetHandleInformation(
                        handle,
                        HANDLE_FLAG_INHERIT,
                        if inherited { HANDLE_FLAG_INHERIT } else { 0 },
                    )
                },
                0
            );
        }

        let (read, write) = pipe();
        assert!(validate_windows_handle(write, false, "readiness").is_ok());
        inheritable(write, true);
        assert!(validate_windows_handle(read, true, "liveness").is_ok());

        inheritable(read, true);
        assert!(validate_windows_handle(read, false, "readiness")
            .expect_err("read end is not readiness")
            .contains("wrong direction"));
        inheritable(write, true);
        assert!(validate_windows_handle(write, true, "liveness")
            .expect_err("write end is not liveness")
            .contains("wrong direction"));

        inheritable(write, false);
        assert!(validate_windows_handle(write, false, "readiness")
            .expect_err("noninherited handle")
            .contains("not inherited"));

        let null = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("NUL")
            .expect("non-pipe Windows fixture");
        let null_handle = null.as_raw_handle().cast();
        inheritable(null_handle, true);
        assert!(validate_windows_handle(null_handle, false, "readiness")
            .expect_err("non-pipe handle")
            .contains("not a pipe"));

        // SAFETY: the two pipe handles are owned by this fixture and closed once.
        unsafe {
            CloseHandle(read);
            CloseHandle(write);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_start_identity_comes_from_proc_and_has_the_closed_domain() {
        let StartIdentity::Linux { boot_id, ticks } =
            native_start_identity().expect("native start identity");
        assert!(valid_lower_uuid(&boot_id));
        assert!(parse_decimal(&ticks, u64::MAX, false).is_ok());
    }
}
