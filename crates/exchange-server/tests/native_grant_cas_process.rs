#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "native-root-test-seam"
))]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::{FromRawFd as _, RawFd};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use exchange_host::{Grant, GrantStore, Grants as _, Risk, Selector, Tenant};
use serde_json::{json, Value};

const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const GRANT_PREVIEW: u16 = 0x0010;
const GRANT_CANDIDATE: u16 = 0x0011;
const GRANT_APPLY: u16 = 0x0012;
const GRANT_QUERY: u16 = 0x0013;
const GRANT_RECEIPT: u16 = 0x0014;
const ERROR: u16 = 0x7fff;

const CHILD_MARKER: &str = "FLUX_EXCHANGE_X134_GRANT_APPLY_CHILD";
const CHILD_SOCKET: &str = "FLUX_EXCHANGE_X134_GRANT_SOCKET";
const CHILD_CANDIDATE: &str = "FLUX_EXCHANGE_X134_GRANT_CANDIDATE";
const CHILD_READY: &str = "FLUX_EXCHANGE_X134_GRANT_READY";
const CHILD_RELEASE: &str = "FLUX_EXCHANGE_X134_GRANT_RELEASE";
const CHILD_RESPONSE: &str = "FLUX_EXCHANGE_X134_GRANT_RESPONSE";

struct Fixture {
    owner: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let owner = std::env::temp_dir().join(format!(
            "flux-exchange-x134-native-grant-cas-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&owner).expect("owner fixture directory");
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture directory");
        let state = owner.join("state");
        std::fs::create_dir(&state).expect("state fixture directory");
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only state directory");
        Self { owner, state }
    }

    fn grant_path(&self) -> PathBuf {
        self.state.join("grants/store.json")
    }

    fn spawn(&self) -> Server {
        Server::spawn(&self.state)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.owner);
    }
}

struct PipeEnds {
    read: RawFd,
    write: RawFd,
}

impl PipeEnds {
    fn new() -> Self {
        let mut fds = [-1; 2];
        // SAFETY: the live output array receives two owned descriptors on success.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        Self {
            read: fds[0],
            write: fds[1],
        }
    }
}

struct Server {
    child: Child,
    readiness_read: RawFd,
    liveness_write: RawFd,
    socket: PathBuf,
}

impl Server {
    fn spawn(state: &Path) -> Self {
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let readiness_source = duplicate_high(readiness.write);
        let liveness_source = duplicate_high(liveness.read);
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("--supervised")
            .env("FLUX_EXCHANGE_STATE", state)
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", state)
            .env_remove("FLUX_EXCHANGE_CREDENTIALS")
            .env_remove("FLUX_EXCHANGE_SETTINGS")
            .env_remove("FLUX_EXCHANGE_GRANTS")
            .env_remove("FLUX_EXCHANGE_CONNECTIONS")
            .env_remove("FLUX_EXCHANGE_CHANNELS")
            .env_remove("FLUX_EXCHANGE_WORKFLOWS")
            .env_remove("FLUX_EXCHANGE_AUDIT")
            .env_remove("FLUX_EXCHANGE_SERVICE_ACCOUNTS")
            .env_remove("FLUX_EXCHANGE_APPS")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: this closure uses only descriptor operations before exec.
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
        let child = command.spawn().expect("real supervised Exchange");
        close_fd(readiness.write);
        close_fd(liveness.read);
        close_fd(readiness_source);
        close_fd(liveness_source);
        let mut server = Self {
            child,
            readiness_read: readiness.read,
            liveness_write: liveness.write,
            socket: state.join("run/local-management-v1.sock"),
        };
        let readiness = server.readiness();
        assert!(!readiness.is_empty(), "Exchange refused before readiness");
        await_http_service(&readiness);
        server
    }

    fn readiness(&mut self) -> Vec<u8> {
        let fd = std::mem::replace(&mut self.readiness_read, -1);
        // SAFETY: ownership of the readiness read descriptor moves to this File exactly once.
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("readiness bytes");
        bytes
    }

    fn session(&self) -> Session {
        Session::connect(&self.socket)
    }

    fn finish(mut self) {
        // A restart is a graceful operator stop, not supervisor-liveness loss. SIGINT exercises
        // the production shutdown future so the native endpoint object is dropped and unlinks only
        // the exact socket inode it created before the replacement process binds.
        // SAFETY: the child id names the live process owned by this fixture.
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGINT) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.child.try_wait().expect("child state").is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "SIGINT did not gracefully stop Exchange"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        self.close_liveness();
        let status = self.child.wait().expect("server exit status");
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
        assert!(
            status.success(),
            "graceful supervised shutdown failed: stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    }

    fn close_liveness(&mut self) {
        if self.liveness_write >= 0 {
            close_fd(self.liveness_write);
            self.liveness_write = -1;
        }
    }
}

fn await_http_service(readiness: &[u8]) {
    let ready: Value = serde_json::from_slice(readiness).expect("supervised readiness JSON");
    let host = ready["bind"]["host"].as_str().expect("readiness host");
    let port = ready["bind"]["port"].as_u64().expect("readiness port") as u16;
    let mut stream = TcpStream::connect((host, port)).expect("bound supervised HTTP listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("HTTP barrier timeout");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("HTTP barrier request");
    let mut response = [0_u8; 12];
    stream
        .read_exact(&mut response)
        .expect("HTTP barrier response");
    assert_eq!(
        &response,
        b"HTTP/1.1 200",
        "server did not enter the live route loop: {}",
        String::from_utf8_lossy(&response)
    );
}

impl Drop for Server {
    fn drop(&mut self) {
        self.close_liveness();
        if self.readiness_read >= 0 {
            close_fd(self.readiness_read);
            self.readiness_read = -1;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Session {
    stream: UnixStream,
}

impl Session {
    fn connect(socket: &Path) -> Self {
        Self {
            stream: UnixStream::connect(socket)
                .expect("owner-authenticated native local-management connection"),
        }
    }

    fn request(&mut self, opcode: u16, payload: &[u8]) -> WireFrame {
        self.stream
            .write_all(&frame(CLIENT, opcode, payload))
            .expect("complete client frame");
        self.read()
    }

    fn request_json(&mut self, opcode: u16, value: &Value) -> WireFrame {
        self.request(
            opcode,
            &serde_json::to_vec(value).expect("canonical control JSON"),
        )
    }

    fn read(&mut self) -> WireFrame {
        let mut header = [0_u8; 12];
        self.stream
            .read_exact(&mut header)
            .expect("complete response header");
        assert_eq!(&header[..4], b"FXLM");
        assert_eq!(header[4], 1);
        assert_eq!(header[5], SERVER);
        let opcode = u16::from_be_bytes([header[6], header[7]]);
        let length = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
        assert!(length <= 65_536, "server control frame exceeded bound");
        let mut payload = vec![0_u8; length];
        self.stream
            .read_exact(&mut payload)
            .expect("complete response payload");
        WireFrame { opcode, payload }
    }
}

struct WireFrame {
    opcode: u16,
    payload: Vec<u8>,
}

impl WireFrame {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.payload).expect("canonical server control JSON")
    }

    fn encode_result(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(2 + self.payload.len());
        result.extend_from_slice(&self.opcode.to_be_bytes());
        result.extend_from_slice(&self.payload);
        result
    }

    fn decode_result(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 2, "child result has an opcode");
        Self {
            opcode: u16::from_be_bytes([bytes[0], bytes[1]]),
            payload: bytes[2..].to_vec(),
        }
    }
}

struct ApplyClient {
    child: Child,
    ready: PathBuf,
    response: PathBuf,
}

impl ApplyClient {
    fn spawn(root: &Path, socket: &Path, candidate: &[u8], name: &str, release: &Path) -> Self {
        let candidate_path = root.join(format!("{name}.candidate.json"));
        let ready = root.join(format!("{name}.ready"));
        let response = root.join(format!("{name}.response"));
        std::fs::write(&candidate_path, candidate).expect("candidate handoff");
        let child = Command::new(std::env::current_exe().expect("current integration test"))
            .arg("--exact")
            .arg("concurrent_apply_client_child")
            .arg("--nocapture")
            .env(CHILD_MARKER, "1")
            .env(CHILD_SOCKET, socket)
            .env(CHILD_CANDIDATE, &candidate_path)
            .env(CHILD_READY, &ready)
            .env(CHILD_RELEASE, release)
            .env(CHILD_RESPONSE, &response)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("native APPLY client process");
        Self {
            child,
            ready,
            response,
        }
    }

    fn finish(self) -> WireFrame {
        let output = self.child.wait_with_output().expect("APPLY client status");
        assert!(
            output.status.success(),
            "native APPLY client failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        WireFrame::decode_result(&std::fs::read(&self.response).expect("APPLY response bytes"))
    }
}

#[test]
fn concurrent_native_process_apply_is_one_cas_and_restart_stable() {
    let fixture = Fixture::new();
    let tenant = Tenant::new("local").expect("native owner tenant");
    let unrelated = vec![
        Grant::for_connector(
            "slack",
            Selector::at_most(Risk::Medium).deny("channels.delete"),
        ),
        Grant::for_connector(
            "slack",
            Selector::at_most(Risk::High).allow("channels.create"),
        ),
    ];
    let seeded = GrantStore::bind(fixture.grant_path()).expect("production grant store");
    seeded
        .set(&tenant, &unrelated)
        .expect("seed unrelated authority");
    drop(seeded);

    let server = fixture.spawn();
    let low = preview(&server, "low");
    let high = preview(&server, "high");
    assert_eq!(low.json()["revision"], high.json()["revision"]);
    assert_ne!(
        low.json()["proposal_digest"],
        high.json()["proposal_digest"],
        "the race must carry two distinct proposals at one revision"
    );

    let release = fixture.owner.join("apply.release");
    let mut clients = vec![
        ApplyClient::spawn(
            &fixture.owner,
            &server.socket,
            &low.payload,
            "low",
            &release,
        ),
        ApplyClient::spawn(
            &fixture.owner,
            &server.socket,
            &high.payload,
            "high",
            &release,
        ),
    ];
    wait_until_ready(&mut clients);
    std::fs::write(&release, b"both native clients connected").expect("release APPLY race");
    let responses = clients
        .drain(..)
        .map(ApplyClient::finish)
        .collect::<Vec<_>>();

    let accepted = responses
        .iter()
        .enumerate()
        .filter(|(_, response)| response.opcode == GRANT_RECEIPT)
        .collect::<Vec<_>>();
    let stale = responses
        .iter()
        .filter(|response| response.opcode == ERROR)
        .collect::<Vec<_>>();
    assert_eq!(
        accepted.len(),
        1,
        "exactly one proposal wins the whole-set CAS"
    );
    assert_eq!(stale.len(), 1, "the competing proposal must refuse stale");
    assert_eq!(
        stale[0].json(),
        json!({
            "code": "grant_stale",
            "commit": "none",
            "retry": "refresh",
            "schema": "exchange.local-management-error.v1",
            "status": 409,
        })
    );
    let (winner, accepted) = accepted[0];
    let receipt = accepted.json();
    assert_eq!(receipt["schema"], "exchange.grant-apply-receipt.v1");
    assert_eq!(receipt["replayed"], false);
    assert_eq!(
        receipt["commit"],
        json!({"audit": "committed", "resource": "committed"})
    );
    let receipt_id = receipt["receipt_id"]
        .as_str()
        .expect("durable receipt id")
        .to_owned();
    let revision = receipt["revision"]
        .as_str()
        .expect("post-commit revision")
        .to_owned();
    let winning_candidate = if winner == 0 { &low } else { &high };
    server.finish();

    let restarted = fixture.spawn();
    let queried = restarted
        .session()
        .request_json(GRANT_QUERY, &json!({"receipt_id": &receipt_id}));
    assert_eq!(queried.opcode, GRANT_RECEIPT);
    let queried = queried.json();
    assert_eq!(queried["receipt_id"], receipt_id);
    assert_eq!(queried["revision"], revision);
    assert_eq!(queried["replayed"], true);

    let replayed = restarted
        .session()
        .request(GRANT_APPLY, &winning_candidate.payload);
    assert_eq!(replayed.opcode, GRANT_RECEIPT);
    let replayed = replayed.json();
    assert_eq!(replayed["receipt_id"], receipt_id);
    assert_eq!(replayed["revision"], revision);
    assert_eq!(replayed["replayed"], true);

    let current = preview(
        &restarted,
        winning_candidate.json()["candidate"]["selector"]["max_risk"]
            .as_str()
            .expect("winning risk"),
    );
    assert_eq!(current.json()["revision"], revision);
    restarted.finish();

    let reopened = GrantStore::bind(fixture.grant_path()).expect("restart-stable grant store");
    let held = reopened.held(&tenant);
    assert_eq!(held.len(), 3, "the CAS changes exactly one connector row");
    assert_eq!(
        &held[..unrelated.len()],
        unrelated.as_slice(),
        "unrelated duplicate grants retain position, multiplicity and complete selectors"
    );
    let github = held
        .iter()
        .find(|grant| grant.connector == "github")
        .expect("winning github grant");
    let expected_risk = if winner == 0 { Risk::Low } else { Risk::High };
    assert_eq!(github.selector.max_risk, Some(expected_risk));
}

// This process-only test is selected by the parent test executable with `--exact`. Keeping the
// client in another process proves the native transport and server-side CAS rather than sharing a
// GrantStore object or lock with the test coordinator.
#[test]
fn concurrent_apply_client_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    let socket = PathBuf::from(std::env::var_os(CHILD_SOCKET).expect("child socket"));
    let candidate = std::fs::read(std::env::var_os(CHILD_CANDIDATE).expect("child candidate path"))
        .expect("child candidate bytes");
    let ready = PathBuf::from(std::env::var_os(CHILD_READY).expect("child ready path"));
    let release = PathBuf::from(std::env::var_os(CHILD_RELEASE).expect("child release path"));
    let response = PathBuf::from(std::env::var_os(CHILD_RESPONSE).expect("child response path"));
    let mut session = Session::connect(&socket);
    std::fs::write(&ready, b"connected").expect("child ready barrier");
    wait_for_path(&release, Duration::from_secs(5));
    let result = session.request(GRANT_APPLY, &candidate);
    std::fs::write(response, result.encode_result()).expect("child response handoff");
}

fn preview(server: &Server, max_risk: &str) -> WireFrame {
    let response = server.session().request_json(
        GRANT_PREVIEW,
        &json!({
            "connector": "github",
            "selector": {
                "effects_within": null,
                "idempotency": null,
                "max_risk": max_risk,
            },
        }),
    );
    assert_eq!(
        response.opcode,
        GRANT_CANDIDATE,
        "preview refusal: {}",
        String::from_utf8_lossy(&response.payload)
    );
    response
}

fn wait_until_ready(clients: &mut [ApplyClient]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if clients.iter().all(|client| client.ready.exists()) {
            return;
        }
        for client in clients.iter_mut() {
            if let Some(status) = client.child.try_wait().expect("APPLY client state") {
                panic!("native APPLY client exited before the release barrier: {status}");
            }
        }
        assert!(
            Instant::now() < deadline,
            "native APPLY clients did not reach the connected barrier"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(Instant::now() < deadline, "timed out at process barrier");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12 + payload.len());
    bytes.extend_from_slice(b"FXLM");
    bytes.push(1);
    bytes.push(direction);
    bytes.extend_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn duplicate_high(fd: RawFd) -> RawFd {
    // SAFETY: F_DUPFD_CLOEXEC creates a distinct owned descriptor or returns -1.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 32) };
    assert!(duplicate >= 32, "duplicate inherited capability");
    duplicate
}

fn close_fd(fd: RawFd) {
    // SAFETY: fixture ownership closes each descriptor at most once.
    unsafe {
        libc::close(fd);
    }
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
