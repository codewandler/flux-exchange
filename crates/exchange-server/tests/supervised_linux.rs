#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::ffi::OsStr;
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const SENTINEL: &str = "X128_VENDOR_TOKEN_DO_NOT_SERIALIZE_7c96d9";

struct PipeEnds {
    read: RawFd,
    write: RawFd,
}

impl PipeEnds {
    fn new() -> Self {
        let mut fds = [-1; 2];
        // SAFETY: the output array is valid and receives two owned descriptors on success.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        for fd in fds {
            // SAFETY: the fresh pipe descriptors are valid and only their descriptor flags change.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(flags >= 0);
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) },
                0
            );
        }
        Self {
            read: fds[0],
            write: fds[1],
        }
    }
}

struct SupervisedChild {
    child: Child,
    readiness_read: RawFd,
    liveness_write: RawFd,
    state_root: PathBuf,
}

impl SupervisedChild {
    fn spawn(root_mode: u32) -> Self {
        Self::spawn_with(root_mode, false)
    }

    fn spawn_with(root_mode: u32, wedge: bool) -> Self {
        Self::spawn_config(root_mode, wedge, None)
    }

    fn spawn_config(root_mode: u32, wedge: bool, bind: Option<&str>) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x128-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&root).expect("private state fixture root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(root_mode))
            .expect("fixture root mode");
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let readiness_source = duplicate_high(readiness.write);
        let liveness_source = duplicate_high(liveness.read);

        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("--supervised")
            .env("FLUX_EXCHANGE_STATE", &root)
            .env("X128_VENDOR_TOKEN", SENTINEL)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if wedge {
            command.env("FLUX_EXCHANGE_TEST_WEDGE_AFTER_READY", "1");
        }
        if let Some(bind) = bind {
            command.env("FLUX_EXCHANGE_BIND", bind);
        }
        // SAFETY: the closure uses only async-signal-safe descriptor operations before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(readiness_source, 3) < 0 || libc::dup2(liveness_source, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for fd in 5..256 {
                    libc::close(fd);
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("real supervised server");
        close_fd(readiness.write);
        close_fd(liveness.read);
        close_fd(readiness_source);
        close_fd(liveness_source);
        Self {
            child,
            readiness_read: readiness.read,
            liveness_write: liveness.write,
            state_root: root,
        }
    }

    fn readiness(&mut self) -> Vec<u8> {
        let fd = std::mem::replace(&mut self.readiness_read, -1);
        let reader = std::thread::spawn(move || {
            use std::os::fd::FromRawFd;
            // SAFETY: the descriptor ownership moves to this reader exactly once.
            let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).expect("readiness pipe read");
            bytes
        });
        reader.join().expect("readiness reader")
    }

    fn close_liveness(&mut self) {
        if self.liveness_write >= 0 {
            close_fd(self.liveness_write);
            self.liveness_write = -1;
        }
    }

    fn finish(mut self) -> std::process::Output {
        self.close_liveness();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.child.try_wait().expect("child state").is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "liveness EOF did not stop Exchange"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let status = self.child.wait().expect("child output status");
        let mut stdout = Vec::new();
        self.child
            .stdout
            .take()
            .expect("captured stdout")
            .read_to_end(&mut stdout)
            .expect("stdout bytes");
        let mut stderr = Vec::new();
        self.child
            .stderr
            .take()
            .expect("captured stderr")
            .read_to_end(&mut stderr)
            .expect("stderr bytes");
        let _ = std::fs::remove_dir_all(&self.state_root);
        std::process::Output {
            status,
            stdout,
            stderr,
        }
    }
}

#[test]
fn native_liveness_exits_an_exchange_whose_tokio_main_future_is_wedged() {
    let mut server = SupervisedChild::spawn_with(0o700, true);
    let readiness = server.readiness();
    assert!(
        !readiness.is_empty(),
        "wedged child reached readiness first"
    );
    let ready: serde_json::Value = serde_json::from_slice(&readiness).expect("readiness object");
    let address: SocketAddr = format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("host"),
        ready["bind"]["port"].as_u64().expect("port")
    )
    .parse()
    .expect("reported address");
    let output = server.finish();
    assert!(!output.status.success());
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_err());
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        self.close_liveness();
        if self.readiness_read >= 0 {
            close_fd(self.readiness_read);
            self.readiness_read = -1;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state_root);
    }
}

fn duplicate_high(fd: RawFd) -> RawFd {
    // SAFETY: F_DUPFD_CLOEXEC creates a distinct owned descriptor or returns -1.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 32) };
    assert!(duplicate >= 32, "duplicate inherited capability");
    duplicate
}

fn close_fd(fd: RawFd) {
    // SAFETY: test ownership ensures each descriptor is closed at most once.
    unsafe {
        libc::close(fd);
    }
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[test]
fn supervisor_helper_process() {
    if std::env::var_os("X128_RUN_SUPERVISOR_HELPER").is_none() {
        return;
    }
    let wedge = std::env::var_os("X128_HELPER_WEDGE").is_some();
    let mut server = SupervisedChild::spawn_with(0o700, wedge);
    let readiness = server.readiness();
    println!(
        "X128_READY\t{}\t{}\t{}",
        server.child.id(),
        server.state_root.display(),
        String::from_utf8(readiness).expect("UTF-8 readiness")
    );
    std::io::stdout().flush().expect("publish helper child");
    loop {
        std::thread::park();
    }
}

#[test]
fn sigkill_of_the_real_supervisor_kills_a_tokio_wedged_exchange_and_releases_its_port() {
    let mut helper = Command::new(std::env::current_exe().expect("integration test executable"))
        .args(["--exact", "supervisor_helper_process", "--nocapture"])
        .env("X128_RUN_SUPERVISOR_HELPER", "1")
        .env("X128_HELPER_WEDGE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("outer supervisor helper");
    let mut reader = std::io::BufReader::new(helper.stdout.take().expect("helper stdout"));
    let line = loop {
        let mut line = String::new();
        assert_ne!(
            reader.read_line(&mut line).expect("helper readiness line"),
            0,
            "helper exited before readiness"
        );
        if let Some((_, ready)) = line.split_once("X128_READY\t") {
            break ready.to_owned();
        }
    };
    let mut fields = line.trim_end().splitn(3, '\t');
    let exchange_pid = fields
        .next()
        .expect("exchange pid")
        .parse::<i32>()
        .expect("numeric Exchange pid");
    let state_root = PathBuf::from(fields.next().expect("state root"));
    let ready: serde_json::Value =
        serde_json::from_str(fields.next().expect("readiness JSON")).expect("readiness object");
    let address: SocketAddr = format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("host"),
        ready["bind"]["port"].as_u64().expect("port")
    )
    .parse()
    .expect("reported address");

    // SAFETY: this PID is the still-open helper child returned by `spawn`, not a recorded name.
    assert_eq!(unsafe { libc::kill(helper.id() as i32, libc::SIGKILL) }, 0);
    let status = helper.wait().expect("killed helper status");
    assert!(!status.success());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: signal zero only observes whether the exact reported child PID remains.
        let process_gone = unsafe { libc::kill(exchange_pid, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        let port_gone = TcpStream::connect_timeout(&address, Duration::from_millis(20)).is_err();
        if process_gone && port_gone {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Exchange survived SIGKILL of its supervisor"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(state_root);
}

#[test]
fn real_server_emits_one_canonical_record_after_bind_and_dies_on_liveness_eof() {
    let mut server = SupervisedChild::spawn(0o700);
    let readiness = server.readiness();
    assert!(
        !readiness.is_empty(),
        "successful startup emitted no readiness"
    );
    assert!(readiness.len() <= 16 * 1024);
    assert!(!readiness.ends_with(b"\n"));
    assert!(!readiness
        .windows(SENTINEL.len())
        .any(|bytes| bytes == SENTINEL.as_bytes()));
    let ready: serde_json::Value = serde_json::from_slice(&readiness).expect("readiness object");

    let compatibility = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .args(["compatibility", "--json"])
        .output()
        .expect("compatibility process");
    assert!(compatibility.status.success());
    assert!(compatibility.stderr.is_empty());
    let compatibility: serde_json::Value =
        serde_json::from_slice(&compatibility.stdout).expect("compatibility object");
    assert_eq!(ready["protocols"], compatibility["protocols"]);
    for field in ["tag", "version", "source_commit", "build_id"] {
        assert_eq!(ready["release"][field], compatibility["release"][field]);
    }
    let executable = std::fs::read(env!("CARGO_BIN_EXE_flux-exchange")).expect("server executable");
    let digest = Sha256::digest(&executable)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(ready["release"]["executable_sha256"], digest);
    assert_eq!(ready["schema"], ready["protocols"]["supervisor"]);
    assert_eq!(ready["process"]["pid"], server.child.id());
    assert_native_start_identity(server.child.id(), &ready["process"]["start_identity"]);

    let host = ready["bind"]["host"].as_str().expect("bind host");
    let port = ready["bind"]["port"].as_u64().expect("bind port") as u16;
    let address: SocketAddr = format!("{host}:{port}").parse().expect("reported address");
    let mut connection = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("reported listener accepts HTTP");
    connection
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("health request");

    let output = server.finish();
    assert!(!output
        .stdout
        .windows(SENTINEL.len())
        .any(|bytes| bytes == SENTINEL.as_bytes()));
    assert!(!output
        .stderr
        .windows(SENTINEL.len())
        .any(|bytes| bytes == SENTINEL.as_bytes()));
    let deadline = Instant::now() + Duration::from_secs(2);
    while TcpStream::connect_timeout(&address, Duration::from_millis(20)).is_ok() {
        assert!(
            Instant::now() < deadline,
            "port survived supervisor liveness loss"
        );
    }
}

#[cfg(target_os = "linux")]
fn assert_native_start_identity(pid: u32, identity: &serde_json::Value) {
    assert_eq!(identity["kind"], "linux-proc-start");
    let boot_id =
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id").expect("kernel boot identity");
    assert_eq!(identity["boot_id"], boot_id.trim());
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("open child stat");
    let after_name = &stat[stat.rfind(") ").expect("closed command field") + 2..];
    let ticks = after_name
        .split_ascii_whitespace()
        .nth(19)
        .expect("child start ticks");
    assert_eq!(identity["ticks"], ticks);
}

#[cfg(target_os = "macos")]
fn assert_native_start_identity(pid: u32, identity: &serde_json::Value) {
    assert_eq!(identity["kind"], "macos-proc-start");
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    // SAFETY: the exact native output buffer remains live for the complete call.
    assert_eq!(
        unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size as i32,
            )
        },
        size as i32
    );
    // SAFETY: the full structure was initialized by the successful call.
    let info = unsafe { info.assume_init() };
    assert_eq!(identity["seconds"], info.pbi_start_tvsec.to_string());
    assert_eq!(identity["microseconds"], info.pbi_start_tvusec);
}

#[test]
fn unsafe_store_refusal_emits_no_readiness_or_sentinel() {
    let mut server = SupervisedChild::spawn(0o777);
    let readiness = server.readiness();
    assert!(readiness.is_empty(), "store refusal emitted readiness");
    let output = server.finish();
    assert!(!output.status.success());
    assert!(!output
        .stdout
        .windows(SENTINEL.len())
        .any(|bytes| bytes == SENTINEL.as_bytes()));
    assert!(!output
        .stderr
        .windows(SENTINEL.len())
        .any(|bytes| bytes == SENTINEL.as_bytes()));
}

#[test]
fn preselected_or_nonloopback_bind_refuses_before_readiness() {
    for bind in ["127.0.0.1:8080", "0.0.0.0:0"] {
        let mut server = SupervisedChild::spawn_config(0o700, false, Some(bind));
        assert!(server.readiness().is_empty(), "{bind} emitted readiness");
        let output = server.finish();
        assert!(!output.status.success());
    }
}

#[test]
fn exact_unix_abi_refuses_missing_and_wrong_capabilities() {
    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .arg("--supervised")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("missing ABI process");
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "readiness was redirected to stdout"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("FD 3"));

    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .args([
            OsStr::new("--supervised"),
            OsStr::new("--readiness-fd"),
            OsStr::new("9"),
        ])
        .output()
        .expect("arbitrary FD option process");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn unix_abi_refuses_alias_wrong_direction_and_extra_inherited_fd() {
    fn refusal(mode: &str) -> std::process::Output {
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let extra = PipeEnds::new();
        let (fd3, fd4) = match mode {
            "alias" => (
                duplicate_high(readiness.write),
                duplicate_high(readiness.read),
            ),
            "wrong" => (
                duplicate_high(readiness.read),
                duplicate_high(liveness.write),
            ),
            "extra" => (
                duplicate_high(readiness.write),
                duplicate_high(liveness.read),
            ),
            _ => unreachable!(),
        };
        let fd5 = (mode == "extra").then(|| duplicate_high(extra.read));
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("--supervised")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: only async-signal-safe descriptor operations run before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(fd3, 3) < 0 || libc::dup2(fd4, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(fd5) = fd5 {
                    if libc::dup2(fd5, 5) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                for fd in 6..256 {
                    libc::close(fd);
                }
                Ok(())
            });
        }
        let output = command.output().expect("ABI refusal process");
        for fd in [
            readiness.read,
            readiness.write,
            liveness.read,
            liveness.write,
            extra.read,
            extra.write,
            fd3,
            fd4,
        ] {
            close_fd(fd);
        }
        if let Some(fd) = fd5 {
            close_fd(fd);
        }
        output
    }

    for (mode, diagnostic) in [
        ("alias", "alias one pipe"),
        ("wrong", "wrong direction"),
        ("extra", "unexpected inherited nonstandard FD 5"),
    ] {
        let output = refusal(mode);
        assert!(!output.status.success(), "{mode}");
        assert!(output.stdout.is_empty(), "{mode} wrote stdout readiness");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn compatibility_is_exact_and_never_opens_a_store_or_listener() {
    let root = std::env::temp_dir().join(format!(
        "flux-exchange-x128-compatibility-{}-{}",
        std::process::id(),
        unique_counter()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .args(["compatibility", "--json"])
        .env("FLUX_EXCHANGE_STATE", &root)
        .env("FLUX_EXCHANGE_BIND", "127.0.0.1:1")
        .output()
        .expect("compatibility process");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!root.exists(), "compatibility opened the configured store");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("exact compatibility object");
    assert_eq!(value["schema"], "exchange.compatibility.v1");

    let wrong = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .args(["compatibility", "--json", "extra"])
        .env("FLUX_EXCHANGE_STATE", &root)
        .output()
        .expect("invalid compatibility process");
    assert!(!wrong.status.success());
    assert!(wrong.stdout.is_empty());
    assert!(!root.exists());
}
