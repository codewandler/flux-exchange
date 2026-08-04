#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "native-root-test-seam"
))]

use std::ffi::OsStr;
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use flux_exchange::supervisor::{
    verify_readiness, ExpectedRelease, NativePlatform, ReadinessExpectation, VerifiedStartIdentity,
};

const SENTINELS: [(&str, &str); 6] = [
    ("X128_CREDENTIAL_VALUE", "credential-secret-7c96d9"),
    ("X128_SETTING_VALUE", "setting-value-7c96d9"),
    ("X128_GRANT_BODY", "grant-body-7c96d9"),
    (
        "X128_SERVICE_ACCOUNT_VERIFIER",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ),
    ("X128_SESSION_VALUE", "session-value-7c96d9"),
    ("X128_CONTROL_CREDENTIAL", "control-credential-7c96d9"),
];

fn exchange_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
}

fn seed_production_authority_stores(root: &std::path::Path) -> Vec<String> {
    use exchange_host::{ConnectionSettings, Grants};

    let tenant = exchange_host::Tenant::new("dev").expect("sentinel tenant");
    let credential = exchange_host::CredentialStore::bind(root.join("credentials/store.txt"))
        .expect("production credential store");
    let reference =
        exchange_host::CredentialRef::new("dev", "com.zendesk.api", "support", "api_token")
            .expect("sentinel credential address");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("sentinel store runtime")
        .block_on(
            credential
                .secrets()
                .put(&reference, &exchange_host::Secret::new(SENTINELS[0].1)),
        )
        .expect("production credential write");

    let settings = exchange_host::SettingsStore::bind(root.join("settings/store.json"))
        .expect("production settings store");
    let declared = exchange_host::DeclaredSetting::parse("default", "endpoint.subdomain")
        .expect("declared sentinel setting");
    settings
        .set(&tenant, "zendesk", &declared, SENTINELS[1].1)
        .expect("production setting write");

    let grants = exchange_host::GrantStore::bind(root.join("grants/store.json"))
        .expect("production grant store");
    let grant = exchange_host::Grant::for_connector(
        SENTINELS[2].1,
        exchange_host::Selector::at_most(exchange_host::Risk::Low),
    );
    grants
        .set(&tenant, &[grant])
        .expect("production grant write");

    let service_account_path = root.join("service-accounts/store.json");
    let service_accounts =
        flux_exchange::service_account::ServiceAccountStore::open(&service_account_path)
            .expect("production Service Account store");
    let minted = service_accounts
        .mint(
            &exchange_host::Principal::new(
                exchange_host::PrincipalKind::User,
                "sentinel-user",
                tenant,
            ),
            "sentinel-service",
            flux_exchange::service_account::Expiry {
                as_of: 1_800_000_000,
                expires_at: 1_800_000_001,
            },
        )
        .expect("production Service Account mint");
    let token = minted.token.as_str().to_owned();
    drop(service_accounts);
    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(service_account_path).expect("persisted Service Account store"),
    )
    .expect("production Service Account JSON");
    let verifier = stored["agents"]
        .as_object()
        .and_then(|agents| agents.keys().next())
        .expect("persisted Service Account verifier")
        .to_owned();
    vec![token, verifier]
}

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
    dynamic_authority_values: Vec<String>,
}

#[derive(Clone, Copy)]
enum EndpointFixture {
    Clean,
    SymlinkRun,
    StaleSocket,
}

impl SupervisedChild {
    fn spawn(root_mode: u32) -> Self {
        Self::spawn_with(root_mode, false)
    }

    fn spawn_with(root_mode: u32, wedge: bool) -> Self {
        Self::spawn_config(root_mode, wedge, None, None)
    }

    fn spawn_endpoint_fixture(fixture: EndpointFixture, expected_peer_uid: Option<u32>) -> Self {
        Self::spawn_config_full(0o700, false, None, None, fixture, expected_peer_uid)
    }

    fn spawn_config(
        root_mode: u32,
        wedge: bool,
        bind: Option<&OsStr>,
        occupied_bind: Option<SocketAddr>,
    ) -> Self {
        Self::spawn_config_full(
            root_mode,
            wedge,
            bind,
            occupied_bind,
            EndpointFixture::Clean,
            None,
        )
    }

    fn spawn_config_full(
        root_mode: u32,
        wedge: bool,
        bind: Option<&OsStr>,
        occupied_bind: Option<SocketAddr>,
        endpoint_fixture: EndpointFixture,
        expected_peer_uid: Option<u32>,
    ) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x128-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&root).expect("private state fixture root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("private fixture root before authority writes");
        let dynamic_authority_values = seed_production_authority_stores(&root);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(root_mode))
            .expect("fixture root mode");
        match endpoint_fixture {
            EndpointFixture::Clean => {}
            EndpointFixture::SymlinkRun => {
                std::os::unix::fs::symlink(&root, root.join("run")).expect("planted run symlink");
            }
            EndpointFixture::StaleSocket => {
                let run = root.join("run");
                std::fs::create_dir(&run).expect("stale run directory");
                std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700))
                    .expect("owner-only stale run directory");
                drop(
                    std::os::unix::net::UnixListener::bind(run.join("local-management-v1.sock"))
                        .expect("stale local-management socket"),
                );
            }
        }
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let readiness_source = duplicate_high(readiness.write);
        let liveness_source = duplicate_high(liveness.read);

        let mut command = exchange_command();
        command
            .arg("--supervised")
            .env("FLUX_EXCHANGE_STATE", &root)
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", &root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if wedge {
            command.env("FLUX_EXCHANGE_TEST_WEDGE_AFTER_READY", "1");
        }
        if let Some(bind) = bind {
            command.env("FLUX_EXCHANGE_BIND", bind);
        }
        if let Some(occupied_bind) = occupied_bind {
            command.env(
                "FLUX_EXCHANGE_TEST_OCCUPIED_BIND",
                occupied_bind.to_string(),
            );
        }
        if let Some(expected_peer_uid) = expected_peer_uid {
            command.env(
                "FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_PEER_UID",
                expected_peer_uid.to_string(),
            );
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
            dynamic_authority_values,
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
        let bytes = reader.join().expect("readiness reader");
        assert_no_authority_values(&bytes, &self.dynamic_authority_values);
        bytes
    }

    fn close_liveness(&mut self) {
        if self.liveness_write >= 0 {
            close_fd(self.liveness_write);
            self.liveness_write = -1;
        }
    }

    fn send_liveness_byte(&self) {
        let byte = [0x5a_u8; 1];
        // SAFETY: the one-byte buffer is live and this is the owned liveness write end.
        assert_eq!(
            unsafe { libc::write(self.liveness_write, byte.as_ptr().cast(), 1) },
            1
        );
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
        let output = std::process::Output {
            status,
            stdout,
            stderr,
        };
        assert_no_sentinels(&output.stdout);
        assert_no_sentinels(&output.stderr);
        assert_no_authority_values(&output.stdout, &self.dynamic_authority_values);
        assert_no_authority_values(&output.stderr, &self.dynamic_authority_values);
        output
    }
}

fn assert_new_start_refuses_at_metadata_expiry(target: &str) {
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/exchange-release-v2");
    let fixture: flux_exchange_release::FixtureSet = flux_exchange_release::canonical::parse(
        &flux_exchange_release::read_bounded_file(
            &fixture_root.join("fixture-set.json"),
            256 * 1024,
        )
        .expect("bounded fixture-set read"),
        256 * 1024,
    )
    .expect("canonical fixture-set");
    let case = fixture
        .cases
        .iter()
        .find(|case| case.id == "expiry-equality-stopped")
        .expect("expiry-equality-stopped provider case");
    let policy: flux_exchange_release::RootPolicy = flux_exchange_release::canonical::parse(
        &flux_exchange_release::read_bounded_file(
            &fixture_root.join("root-policy.test.json"),
            64 * 1024,
        )
        .expect("bounded root-policy read"),
        64 * 1024,
    )
    .expect("canonical root policy");
    let attempt = flux_exchange_release::verify_directory_layered(
        &fixture_root.join(&case.input),
        &policy,
        flux_exchange_release::parse_utc(&case.clock).expect("fixture clock"),
        &flux_exchange_release::Protocols::v2(),
        &case.prior_state,
        Some(target),
    );
    assert!(matches!(
        attempt.outcome,
        Err(flux_exchange_release::Error::Time(_))
    ));
    assert_eq!(attempt.state, case.expected_state);
}

fn native_unix_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        platform => panic!("unsupported native release test platform {platform:?}"),
    }
}

fn fxlm_frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(12 + payload.len());
    frame.extend_from_slice(b"FXLM");
    frame.push(1);
    frame.push(direction);
    frame.extend_from_slice(&opcode.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

#[test]
fn supervised_unix_binds_owner_authenticated_fxlm_before_readiness() {
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
    use std::os::unix::net::UnixStream;

    let mut server = SupervisedChild::spawn(0o700);
    let ready: serde_json::Value =
        serde_json::from_slice(&server.readiness()).expect("readiness object");
    let journal = server.state_root.join("coordinator/transactions.sqlite3");
    assert!(
        journal.is_file(),
        "coordinator recovery journal must be bound before readiness"
    );
    let credential_path = server.state_root.join("credentials/store.txt");
    let contention = exchange_host::CredentialStore::bind(&credential_path)
        .expect_err("the serving process retains its one credential-store lease");
    assert!(contention.to_string().contains("lease"), "{contention}");
    let socket = server.state_root.join("run/local-management-v1.sock");
    let run_metadata = std::fs::symlink_metadata(server.state_root.join("run"))
        .expect("run existed when readiness was emitted");
    let socket_metadata =
        std::fs::symlink_metadata(&socket).expect("endpoint existed when readiness was emitted");
    // SAFETY: geteuid has no pointer arguments or preconditions.
    let euid = unsafe { libc::geteuid() };
    assert_eq!(run_metadata.uid(), euid);
    assert_eq!(run_metadata.permissions().mode() & 0o7777, 0o700);
    assert!(socket_metadata.file_type().is_socket());
    assert_eq!(socket_metadata.uid(), euid);
    assert_eq!(socket_metadata.permissions().mode() & 0o7777, 0o600);

    let request = fxlm_frame(1, 0x0007, br#"{"connector":"gitlab","selection":null}"#);
    let mut stream = UnixStream::connect(&socket).expect("same-owner native connection");
    for chunk in request.chunks(5) {
        stream.write_all(chunk).expect("split FXLM stream write");
    }
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("one logical operation EOF");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("closed FXLM response");
    assert_eq!(
        response,
        fxlm_frame(
            2,
            0x7fff,
            br#"{"code":"local_management_unavailable","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":503}"#,
        )
    );

    let address = format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("readiness host"),
        ready["bind"]["port"].as_u64().expect("readiness port")
    );
    let mut http = TcpStream::connect(address).expect("loopback HTTP");
    http.write_all(
        b"GET /api/grants HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer local-owner\r\nConnection: close\r\n\r\n",
    )
    .expect("spoof attempt");
    let mut http_response = String::new();
    http.read_to_string(&mut http_response)
        .expect("HTTP refusal");
    assert!(
        http_response.starts_with("HTTP/1.1 401"),
        "loopback HTTP reproduced local-owner authority: {http_response}"
    );

    let output = server.finish();
    assert!(!output.status.success());
}

#[test]
fn supervised_unix_rejects_an_injected_wrong_peer_before_reading() {
    use std::os::unix::net::UnixStream;

    // SAFETY: geteuid has no pointer arguments or preconditions.
    let wrong_uid = unsafe { libc::geteuid() }.wrapping_add(1);
    let mut server =
        SupervisedChild::spawn_endpoint_fixture(EndpointFixture::Clean, Some(wrong_uid));
    assert!(!server.readiness().is_empty());
    let socket = server.state_root.join("run/local-management-v1.sock");
    let mut stream = UnixStream::connect(socket).expect("connection reaches peer verifier");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("bounded peer refusal read");
    let mut byte = [0_u8; 1];
    assert_eq!(stream.read(&mut byte).expect("peer refusal EOF"), 0);
    let output = server.finish();
    assert!(!output.status.success());
}

#[test]
fn supervised_unix_refuses_planted_endpoint_metadata_without_repair() {
    for fixture in [EndpointFixture::SymlinkRun, EndpointFixture::StaleSocket] {
        let mut server = SupervisedChild::spawn_endpoint_fixture(fixture, None);
        assert!(
            server.readiness().is_empty(),
            "planted endpoint emitted readiness"
        );
        let planted = match fixture {
            EndpointFixture::SymlinkRun => server.state_root.join("run"),
            EndpointFixture::StaleSocket => server.state_root.join("run/local-management-v1.sock"),
            EndpointFixture::Clean => unreachable!("closed fixture list"),
        };
        assert!(
            std::fs::symlink_metadata(&planted).is_ok(),
            "Exchange removed planted metadata at {}",
            planted.display()
        );
        let output = server.finish();
        assert!(!output.status.success());
    }
}

#[test]
fn verified_metadata_expiry_keeps_the_same_healthy_child_until_owner_stop() {
    let mut server = SupervisedChild::spawn(0o700);
    let pid = server.child.id();
    let ready: serde_json::Value =
        serde_json::from_slice(&server.readiness()).expect("readiness object");
    let address: SocketAddr = format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("host"),
        ready["bind"]["port"].as_u64().expect("port")
    )
    .parse()
    .expect("reported address");

    assert_new_start_refuses_at_metadata_expiry(native_unix_target());
    assert_eq!(server.child.id(), pid, "the owned child identity changed");
    assert!(
        server.child.try_wait().expect("child state").is_none(),
        "metadata expiry terminated the already healthy child"
    );
    TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("same healthy child remains reachable after metadata expiry");

    let output = server.finish();
    assert!(!output.status.success());
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_err());
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

#[test]
fn any_liveness_byte_exits_without_waiting_for_eof() {
    let mut server = SupervisedChild::spawn(0o700);
    assert!(!server.readiness().is_empty());
    server.send_liveness_byte();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if server.child.try_wait().expect("child state").is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "liveness byte did not exit Exchange"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = server.finish();
    assert!(!output.status.success());
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

fn assert_no_sentinels(bytes: &[u8]) {
    for (_, sentinel) in SENTINELS {
        assert!(
            !bytes
                .windows(sentinel.len())
                .any(|candidate| candidate == sentinel.as_bytes()),
            "captured a value-shaped sentinel"
        );
    }
}

fn assert_no_authority_values(bytes: &[u8], values: &[String]) {
    for value in values {
        assert!(
            !bytes
                .windows(value.len())
                .any(|candidate| candidate == value.as_bytes()),
            "captured a dynamic authority value"
        );
    }
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
    assert_sigkill_supervisor(true);
}

#[test]
fn sigkill_of_the_real_supervisor_kills_a_responsive_exchange_and_releases_its_port() {
    assert_sigkill_supervisor(false);
}

fn assert_sigkill_supervisor(wedge: bool) {
    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    command
        .args(["--exact", "supervisor_helper_process", "--nocapture"])
        .env("X128_RUN_SUPERVISOR_HELPER", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if wedge {
        command.env("X128_HELPER_WEDGE", "1");
    }
    let mut helper = command.spawn().expect("outer supervisor helper");
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
    // The expected native identity comes from the owned Child before readiness is consumed.
    let expected_pid = server.child.id();
    let expected_start_identity = captured_native_start_identity(expected_pid);
    let readiness = server.readiness();
    assert!(
        !readiness.is_empty(),
        "successful startup emitted no readiness"
    );
    assert!(readiness.len() <= 16 * 1024);
    assert!(!readiness.ends_with(b"\n"));
    assert_no_sentinels(&readiness);
    let ready: serde_json::Value = serde_json::from_slice(&readiness).expect("readiness object");

    let compatibility = exchange_command()
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
    assert_eq!(ready["process"]["pid"], expected_pid);
    let expected = ReadinessExpectation {
        release: ExpectedRelease {
            tag: compatibility["release"]["tag"]
                .as_str()
                .expect("tag")
                .to_owned(),
            version: compatibility["release"]["version"]
                .as_str()
                .expect("version")
                .to_owned(),
            source_commit: compatibility["release"]["source_commit"]
                .as_str()
                .expect("source")
                .to_owned(),
            build_id: compatibility["release"]["build_id"]
                .as_str()
                .expect("build")
                .to_owned(),
            executable_sha256: digest,
        },
        pid: expected_pid,
        platform: native_platform(),
        start_identity: expected_start_identity,
    };
    let committed = verify_readiness(&readiness, &expected)
        .expect("matching the already-open child permits ownership commit");
    assert_eq!(committed.process.pid, expected_pid);

    let host = ready["bind"]["host"].as_str().expect("bind host");
    let port = ready["bind"]["port"].as_u64().expect("bind port") as u16;
    let address: SocketAddr = format!("{host}:{port}").parse().expect("reported address");
    let mut connection = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("reported listener accepts HTTP");
    connection
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("health request");

    let output = server.finish();
    assert_no_sentinels(&output.stdout);
    assert_no_sentinels(&output.stderr);
    let deadline = Instant::now() + Duration::from_secs(2);
    while TcpStream::connect_timeout(&address, Duration::from_millis(20)).is_ok() {
        assert!(
            Instant::now() < deadline,
            "port survived supervisor liveness loss"
        );
    }
}

#[cfg(target_os = "linux")]
fn native_platform() -> NativePlatform {
    NativePlatform::Linux
}

#[cfg(target_os = "linux")]
fn captured_native_start_identity(pid: u32) -> VerifiedStartIdentity {
    let boot_id =
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id").expect("kernel boot identity");
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("open child stat");
    let after_name = &stat[stat.rfind(") ").expect("closed command field") + 2..];
    let ticks = after_name
        .split_ascii_whitespace()
        .nth(19)
        .expect("child start ticks");
    VerifiedStartIdentity::Linux {
        boot_id: boot_id.trim().to_owned(),
        ticks: ticks.to_owned(),
    }
}

#[cfg(target_os = "macos")]
fn native_platform() -> NativePlatform {
    NativePlatform::Macos
}

#[cfg(target_os = "macos")]
fn captured_native_start_identity(pid: u32) -> VerifiedStartIdentity {
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
    VerifiedStartIdentity::Macos {
        microseconds: info.pbi_start_tvusec as u32,
        seconds: info.pbi_start_tvsec.to_string(),
    }
}

#[test]
fn unsafe_store_refusal_emits_no_readiness_or_sentinel() {
    let mut server = SupervisedChild::spawn(0o777);
    let readiness = server.readiness();
    assert!(readiness.is_empty(), "store refusal emitted readiness");
    let output = server.finish();
    assert!(!output.status.success());
    assert_no_sentinels(&output.stdout);
    assert_no_sentinels(&output.stderr);
}

#[test]
fn preselected_or_nonloopback_bind_refuses_before_readiness() {
    for bind in ["127.0.0.1:8080", "127.0.0.2:0", "0.0.0.0:0"] {
        let mut server = SupervisedChild::spawn_config(0o700, false, Some(OsStr::new(bind)), None);
        assert!(server.readiness().is_empty(), "{bind} emitted readiness");
        let output = server.finish();
        assert!(!output.status.success());
    }
}

#[test]
fn non_unicode_supervised_bind_refuses_before_state_or_readiness() {
    use std::os::unix::ffi::OsStringExt;

    let bind = std::ffi::OsString::from_vec(vec![0xff]);
    let root = std::env::temp_dir().join(format!(
        "flux-exchange-x128-nonunicode-bind-{}-{}",
        std::process::id(),
        unique_counter()
    ));
    let readiness = PipeEnds::new();
    let liveness = PipeEnds::new();
    let readiness_source = duplicate_high(readiness.write);
    let liveness_source = duplicate_high(liveness.read);
    let mut command = exchange_command();
    command
        .arg("--supervised")
        .env("FLUX_EXCHANGE_STATE", &root)
        .env("FLUX_EXCHANGE_BIND", bind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    let child = command.spawn().expect("non-Unicode bind refusal process");
    close_fd(readiness.write);
    close_fd(liveness.read);
    close_fd(readiness_source);
    close_fd(liveness_source);
    // SAFETY: ownership of the readiness read descriptor moves to this file exactly once.
    let mut ready = unsafe { std::fs::File::from_raw_fd(readiness.read) };
    let mut readiness_bytes = Vec::new();
    ready
        .read_to_end(&mut readiness_bytes)
        .expect("refusal readiness EOF");
    close_fd(liveness.write);
    let output = child.wait_with_output().expect("non-Unicode bind output");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(readiness_bytes.is_empty());
    assert_no_sentinels(&output.stderr);
    assert!(!root.exists(), "invalid bind opened local state");
}

#[test]
#[cfg(feature = "supervisor-test-bind-refusal")]
fn real_bind_refusal_after_store_validation_emits_no_readiness() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupied fixture listener");
    let occupied_address = occupied.local_addr().expect("occupied fixture address");
    let mut server = SupervisedChild::spawn_config(0o700, false, None, Some(occupied_address));
    assert!(
        server.readiness().is_empty(),
        "a failed real bind emitted readiness"
    );
    let output = server.finish();
    assert!(!output.status.success());
    drop(occupied);
}

#[test]
fn exact_unix_abi_refuses_missing_and_wrong_capabilities() {
    let output = exchange_command()
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
    assert_no_sentinels(&output.stderr);

    let output = exchange_command()
        .args([
            OsStr::new("--supervised"),
            OsStr::new("--readiness-fd"),
            OsStr::new("9"),
        ])
        .output()
        .expect("arbitrary FD option process");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_no_sentinels(&output.stderr);

    for arguments in [
        vec!["--dev", "--supervised"],
        vec!["junk", "--supervised"],
        vec!["--supervisor-readiness-handle", "3"],
        vec!["--supervisor-liveness-handle", "4"],
        vec!["--supervisor-liveness-handle", "4", "--supervised"],
        vec!["--supervised=true"],
        vec!["--supervisor-readiness-handle=3"],
        vec!["--supervisor-liveness-handle=4"],
        vec![
            "--supervisor-liveness-handle=4",
            "--supervisor-readiness-handle=3",
        ],
        vec!["--supervisor-junk"],
    ] {
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x128-mode-refusal-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        let output = exchange_command()
            .args(arguments)
            .env("FLUX_EXCHANGE_STATE", &root)
            .env("FLUX_EXCHANGE_BIND", "127.0.0.1:0")
            .output()
            .expect("closed supervised mode refusal");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_no_sentinels(&output.stderr);
        assert!(!root.exists(), "malformed mode opened local state");
    }
}

#[test]
fn unix_abi_refuses_alias_wrong_kind_direction_and_extra_inherited_fd() {
    fn refusal(mode: &str) -> std::process::Output {
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let extra = PipeEnds::new();
        let null = std::fs::File::open("/dev/null").expect("non-pipe fixture");
        use std::os::fd::AsRawFd;
        let (fd3, fd4) = match mode {
            "alias" => (
                duplicate_high(readiness.write),
                duplicate_high(readiness.read),
            ),
            "fd3-wrong" => (
                duplicate_high(readiness.read),
                duplicate_high(liveness.read),
            ),
            "fd4-wrong" => (
                duplicate_high(readiness.write),
                duplicate_high(liveness.write),
            ),
            "fd3-nonpipe" => (
                duplicate_high(null.as_raw_fd()),
                duplicate_high(liveness.read),
            ),
            "fd4-nonpipe" => (
                duplicate_high(readiness.write),
                duplicate_high(null.as_raw_fd()),
            ),
            "extra" => (
                duplicate_high(readiness.write),
                duplicate_high(liveness.read),
            ),
            _ => unreachable!(),
        };
        let fd5 = (mode == "extra").then(|| duplicate_high(extra.read));
        let mut command = exchange_command();
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
        ("fd3-wrong", "readiness FD 3 has the wrong direction"),
        ("fd4-wrong", "liveness FD 4 has the wrong direction"),
        ("fd3-nonpipe", "readiness FD 3 is not a pipe"),
        ("fd4-nonpipe", "liveness FD 4 is not a pipe"),
        ("extra", "unexpected inherited nonstandard FD 5"),
    ] {
        let output = refusal(mode);
        assert!(!output.status.success(), "{mode}");
        assert!(output.stdout.is_empty(), "{mode} wrote stdout readiness");
        assert_no_sentinels(&output.stderr);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn unix_abi_refuses_each_missing_fd_and_does_not_discover_env_other_fd_or_stdout() {
    fn missing(target: RawFd) -> std::process::Output {
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let readiness_source = duplicate_high(readiness.write);
        let liveness_source = duplicate_high(liveness.read);
        let mut command = exchange_command();
        command
            .arg("--supervised")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: only async-signal-safe descriptor operations run before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(readiness_source, 3) < 0 || libc::dup2(liveness_source, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(target);
                for fd in 5..256 {
                    libc::close(fd);
                }
                Ok(())
            });
        }
        let output = command.output().expect("one missing capability process");
        for fd in [
            readiness.read,
            readiness.write,
            liveness.read,
            liveness.write,
            readiness_source,
            liveness_source,
        ] {
            close_fd(fd);
        }
        output
    }

    for target in [3, 4] {
        let output = missing(target);
        assert!(!output.status.success(), "missing FD {target}");
        assert!(output.stdout.is_empty(), "missing FD {target} used stdout");
        assert_no_sentinels(&output.stderr);
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("required supervisor FD {target} is absent")),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let other = PipeEnds::new();
    let source = duplicate_high(other.write);
    let mut command = exchange_command();
    command
        .arg("--supervised")
        .env("FLUX_EXCHANGE_SUPERVISOR_READINESS_FD", "9")
        .env("FLUX_EXCHANGE_SUPERVISOR_LIVENESS_FD", "10")
        .stdout(Stdio::from(unsafe {
            // SAFETY: this duplicate is independent from the descriptor retained for cleanup.
            std::fs::File::from_raw_fd(libc::dup(source))
        }))
        .stderr(Stdio::piped());
    // SAFETY: the closure moves the would-be readiness capability to an unrecognized FD only.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(source, 9) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::close(3);
            libc::close(4);
            Ok(())
        });
    }
    let output = command.output().expect("non-ABI discovery process");
    for fd in [other.read, other.write, source] {
        close_fd(fd);
    }
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout became readiness");
    assert_no_sentinels(&output.stderr);
}

#[test]
fn compatibility_is_exact_and_never_opens_a_store_or_listener() {
    let root = std::env::temp_dir().join(format!(
        "flux-exchange-x128-compatibility-{}-{}",
        std::process::id(),
        unique_counter()
    ));
    let output = exchange_command()
        .args(["compatibility", "--json"])
        .env("FLUX_EXCHANGE_STATE", &root)
        .env("FLUX_EXCHANGE_BIND", "127.0.0.1:1")
        .output()
        .expect("compatibility process");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_no_sentinels(&output.stdout);
    assert!(!root.exists(), "compatibility opened the configured store");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("exact compatibility object");
    assert_eq!(value["schema"], "exchange.compatibility.v1");
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repository)
        .output()
        .expect("worktree source commit");
    assert!(source.status.success());
    assert_eq!(
        value["release"]["source_commit"],
        String::from_utf8(source.stdout)
            .expect("UTF-8 source commit")
            .trim()
    );
    assert!(
        value["release"]["build_id"]
            .as_str()
            .expect("build id")
            .starts_with("dev-"),
        "development builds carry an explicit worktree-derived identity"
    );

    let wrong = exchange_command()
        .args(["compatibility", "--json", "extra"])
        .env("FLUX_EXCHANGE_STATE", &root)
        .output()
        .expect("invalid compatibility process");
    assert!(!wrong.status.success());
    assert!(wrong.stdout.is_empty());
    assert_no_sentinels(&wrong.stderr);
    assert!(!root.exists());
}
