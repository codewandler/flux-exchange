//! The closed inherited-capability ABI for a Flux-owned local Exchange process.
//!
//! Readiness proves one successful startup. Liveness proves continuing ownership. They are
//! deliberately different pipes: neither carries a credential or becomes a later control channel.

use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::protocol::{ProtocolVersions, PROTOCOL_VERSIONS, SUPERVISOR_READY_V1};

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
        schema: "exchange.compatibility.v1",
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
        if !bind.ip().is_loopback() || bind.port() == 0 {
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
            schema: SUPERVISOR_READY_V1.as_str(),
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
    let readiness_stat = fd_stat(3)?;
    let liveness_stat = fd_stat(4)?;
    if readiness_stat.st_dev == liveness_stat.st_dev
        && readiness_stat.st_ino == liveness_stat.st_ino
    {
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
fn discover_capabilities(
    arguments: &[std::ffi::OsString],
) -> Result<(ReadyWriter, LivenessReader), String> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileType, ReadFile, WriteFile, FILE_TYPE_PIPE,
    };

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

    fn validate(handle: HANDLE, read: bool, name: &str) -> Result<(), String> {
        let mut inherited = 0_u32;
        // SAFETY: output storage is valid and the numeric capability is only observed.
        if unsafe { GetHandleInformation(handle, &mut inherited) } == 0
            || inherited & HANDLE_FLAG_INHERIT == 0
        {
            return Err(format!(
                "supervisor {name} HANDLE is absent or not inherited"
            ));
        }
        // SAFETY: the call only identifies the validated inherited handle.
        if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
            return Err(format!("supervisor {name} HANDLE is not a pipe"));
        }
        let mut transferred = 0_u32;
        // A zero-byte operation returns immediately but still validates the handle's access mask.
        // SAFETY: no buffer is accessed for a zero-byte operation and the output count is valid.
        let usable = unsafe {
            if read {
                ReadFile(
                    handle,
                    std::ptr::null_mut(),
                    0,
                    &mut transferred,
                    std::ptr::null_mut(),
                )
            } else {
                WriteFile(
                    handle,
                    std::ptr::null(),
                    0,
                    &mut transferred,
                    std::ptr::null_mut(),
                )
            }
        };
        // SAFETY: the opposite zero-byte operation likewise carries no payload and cannot block.
        let opposite_usable = unsafe {
            if read {
                WriteFile(
                    handle,
                    std::ptr::null(),
                    0,
                    &mut transferred,
                    std::ptr::null_mut(),
                )
            } else {
                ReadFile(
                    handle,
                    std::ptr::null_mut(),
                    0,
                    &mut transferred,
                    std::ptr::null_mut(),
                )
            }
        };
        if usable == 0 || opposite_usable != 0 {
            return Err(format!("supervisor {name} HANDLE has the wrong direction"));
        }
        // SAFETY: clearing inheritance on the discovered capability cannot widen authority.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(format!(
                "cannot protect supervisor {name} HANDLE from child processes"
            ));
        }
        Ok(())
    }

    validate(readiness, false, "readiness")?;
    validate(liveness, true, "liveness")?;
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

    #[test]
    fn compatibility_is_exact_canonical_json_from_the_six_field_source() {
        let bytes = compatibility_json().expect("compatibility JSON");
        let build_id = serde_json::to_string(env!("FLUX_EXCHANGE_COMPILED_BUILD_ID"))
            .expect("build id string");
        let source_commit = serde_json::to_string(env!("FLUX_EXCHANGE_COMPILED_SOURCE_COMMIT"))
            .expect("source commit string");
        let version = env!("CARGO_PKG_VERSION");
        let expected = format!(
            "{{\"protocols\":{{\"connection_plan\":\"exchange.connection-plan.v1\",\"effective_catalogue_response\":\"exchange.effective-catalogue-response.v1\",\"exchange_api\":\"exchange.api.v1\",\"invoke_request\":\"exchange.invoke-request.v1\",\"invoke_response\":\"exchange.invoke-response.v1\",\"supervisor\":\"exchange.supervisor-ready.v1\"}},\"release\":{{\"build_id\":{build_id},\"source_commit\":{source_commit},\"tag\":\"refs/tags/v{version}\",\"version\":\"{version}\"}},\"schema\":\"exchange.compatibility.v1\"}}"
        );
        assert_eq!(bytes, expected.as_bytes());
        assert!(!bytes.ends_with(b"\n"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("compatibility value");
        assert_eq!(value["schema"], "exchange.compatibility.v1");
        assert_eq!(
            value["protocols"]["supervisor"],
            SUPERVISOR_READY_V1.as_str()
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

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_start_identity_comes_from_proc_and_has_the_closed_domain() {
        let StartIdentity::Linux { boot_id, ticks } =
            native_start_identity().expect("native start identity");
        assert!(valid_lower_uuid(&boot_id));
        assert!(parse_decimal(&ticks, u64::MAX, false).is_ok());
    }
}
