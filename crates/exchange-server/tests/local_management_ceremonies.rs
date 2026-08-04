#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "native-root-test-seam"
))]

use std::io::{Read, Write};
use std::os::fd::{FromRawFd as _, RawFd};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use exchange_host::{
    ConnectionLabel, ConnectionRegistry as _, ConnectionRegistryStore, InstanceId, Tenant,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const CLIENT: u8 = 1;
const SERVER: u8 = 2;
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
const SENTINEL: &[u8] = b"x134-ceremony-secret-5e9fb2";

struct Fixture {
    owner: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let owner = std::env::temp_dir().join(format!(
            "flux-exchange-x134-ceremonies-{}-{}",
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

    fn seed_empty_label(&self, connector: &str, label: &str, instance: &str) {
        let store = ConnectionRegistryStore::bind(self.state.join("connections/store.json"))
            .expect("durable connection registry");
        store
            .assign(
                &Tenant::new("local").expect("native owner tenant"),
                connector,
                &ConnectionLabel::new(label).expect("fixture label"),
                &InstanceId::parse(instance).expect("fixture instance"),
            )
            .expect("seed empty held label");
    }

    fn spawn(&self) -> Server {
        Server::spawn(&self.state)
    }

    fn assert_value_free(&self) {
        assert_tree_excludes(&self.state, &self.state.join("credentials"), SENTINEL);
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
        Session {
            stream: UnixStream::connect(&self.socket).expect("owner-authenticated native FXLM"),
        }
    }

    fn plan(&self, connector: &str, selection: Option<&str>) -> Value {
        let mut session = self.session();
        session.send_control(
            PLAN_QUERY,
            &json!({"connector": connector, "selection": selection}),
        );
        let response = session.read();
        assert_eq!(
            response.opcode,
            PLAN_RESPONSE,
            "plan refusal: {}",
            response.text()
        );
        response.json()
    }

    fn finish(mut self) {
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
        let _ = self.child.wait();
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
        assert_excludes(&stdout, SENTINEL);
        assert_excludes(&stderr, SENTINEL);
    }

    fn close_liveness(&mut self) {
        if self.liveness_write >= 0 {
            close_fd(self.liveness_write);
            self.liveness_write = -1;
        }
    }
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
    fn send_control(&mut self, opcode: u16, value: &Value) {
        let payload = serde_json::to_vec(value).expect("canonical control JSON");
        self.stream
            .write_all(&frame(CLIENT, opcode, &payload))
            .expect("control frame write");
    }

    fn send_secret(&mut self, ordinal: u16, value: &[u8]) {
        let mut payload = Vec::with_capacity(2 + value.len());
        payload.extend_from_slice(&ordinal.to_be_bytes());
        payload.extend_from_slice(value);
        self.stream
            .write_all(&frame(CLIENT, SECRET, &payload))
            .expect("secret frame write");
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

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }
}

#[test]
fn settings_only_connect_prepares_empty_batch_and_waits_for_commit() {
    let fixture = Fixture::new();
    let server = fixture.spawn();
    let plan = server.plan("freshdesk", None);
    let begin = connect_begin(&plan, "support", |target| match target {
        "setting.default.endpoint.domain" => Some("acme.freshdesk.com"),
        _ => None,
    });
    let digest = proposal_digest("exchange.local-management.v1.connect-proposal", &begin);
    let mut session = server.session();
    session.send_control(CONNECT_BEGIN, &begin);
    let needed = session.read();
    assert_eq!(
        needed.opcode,
        NEED_SECRETS,
        "connect refusal: {}",
        needed.text()
    );
    let needed = needed.json();
    assert_eq!(needed["proposal_digest"], digest);
    assert_eq!(needed["secrets"], json!([]));
    let transaction_id = needed["transaction_id"]
        .as_str()
        .expect("server-owned transaction id");
    assert_transaction_id(transaction_id);
    session.send_control(
        CONNECT_COMMIT,
        &json!({"proposal_digest": digest, "transaction_id": transaction_id}),
    );
    let receipt = session.read();
    assert_eq!(
        receipt.opcode,
        CONNECT_RECEIPT,
        "connect refusal: {}",
        receipt.text()
    );
    assert_receipt(&receipt.json(), "freshdesk", "support", "connect", false);
    let selected = server.plan("freshdesk", Some("support"));
    assert_head(
        selected["credential_revision"]
            .as_str()
            .expect("initialized head"),
    );
    server.finish();
    fixture.assert_value_free();
}

#[test]
fn acquire_rotate_head_cas_stale_refusal_and_restart_replay_are_live() {
    let fixture = Fixture::new();
    fixture.seed_empty_label("github", "work", "11111111-1111-4111-8111-111111111111");
    let server = fixture.spawn();
    let initial_plan = server.plan("github", Some("work"));
    let initial_head = initial_plan["credential_revision"]
        .as_str()
        .expect("migrated head")
        .to_owned();
    assert_head(&initial_head);

    let acquire = credential_begin(&initial_plan, "work", "acquire", &initial_head);
    let acquire_receipt = complete_credential(&server, &acquire, &[SENTINEL]);
    let acquired_plan = server.plan("github", Some("work"));
    let acquired_head = acquired_plan["credential_revision"]
        .as_str()
        .expect("acquired head")
        .to_owned();
    assert_ne!(
        initial_head, acquired_head,
        "accepted acquire must advance the CAS head"
    );

    let stale = credential_begin(&acquired_plan, "work", "rotate", &initial_head);
    let mut stale_session = server.session();
    stale_session.send_control(CREDENTIAL_BEGIN, &stale);
    assert_error(
        stale_session.read(),
        "stale_credential_revision",
        409,
        "refresh",
    );

    let rotate = credential_begin(&acquired_plan, "work", "rotate", &acquired_head);
    complete_credential(&server, &rotate, &[SENTINEL]);
    let rotated_plan = server.plan("github", Some("work"));
    let rotated_head = rotated_plan["credential_revision"]
        .as_str()
        .expect("rotated head");
    assert_ne!(
        acquired_head, rotated_head,
        "accepted rotate must advance the CAS head"
    );
    server.finish();

    let restarted = fixture.spawn();
    let mut replay = restarted.session();
    replay.send_control(CREDENTIAL_BEGIN, &acquire);
    let replayed = replay.read();
    assert_eq!(
        replayed.opcode,
        CREDENTIAL_RECEIPT,
        "replay refusal: {}",
        replayed.text()
    );
    let replayed = replayed.json();
    assert_eq!(replayed["receipt_id"], acquire_receipt["receipt_id"]);
    assert_receipt(&replayed, "github", "work", "acquire", true);
    assert_eq!(
        restarted.plan("github", Some("work"))["credential_revision"],
        rotated_head,
        "old same-proposal replay must not rewind or re-advance the head"
    );
    restarted.finish();
    fixture.assert_value_free();
}

#[test]
fn secret_order_and_commit_state_are_closed_before_decision() {
    let zero = Fixture::new();
    let zero_server = zero.spawn();
    let zero_plan = zero_server.plan("freshdesk", None);
    let zero_begin = connect_begin(&zero_plan, "zero", |target| match target {
        "setting.default.endpoint.domain" => Some("zero.freshdesk.com"),
        _ => None,
    });
    let mut zero_session = zero_server.session();
    zero_session.send_control(CONNECT_BEGIN, &zero_begin);
    assert_eq!(zero_session.read().opcode, NEED_SECRETS);
    zero_session.send_secret(1, SENTINEL);
    assert_error(zero_session.read(), "unexpected_frame", 409, "never");
    zero_server.finish();
    zero.assert_value_free();

    let early = Fixture::new();
    early.seed_empty_label("github", "early", "22222222-2222-4222-8222-222222222222");
    let early_server = early.spawn();
    let early_plan = early_server.plan("github", Some("early"));
    let early_begin = credential_begin(
        &early_plan,
        "early",
        "acquire",
        early_plan["credential_revision"].as_str().expect("head"),
    );
    let early_digest = proposal_digest(
        "exchange.local-management.v1.credential-proposal",
        &early_begin,
    );
    let mut early_session = early_server.session();
    early_session.send_control(CREDENTIAL_BEGIN, &early_begin);
    let need = early_session.read();
    assert_eq!(
        need.opcode,
        NEED_SECRETS,
        "credential refusal: {}",
        need.text()
    );
    let need = need.json();
    assert!(!need["secrets"].as_array().expect("secret needs").is_empty());
    early_session.send_control(
        CREDENTIAL_COMMIT,
        &json!({
            "proposal_digest": early_digest,
            "transaction_id": need["transaction_id"],
        }),
    );
    assert_error(early_session.read(), "unexpected_frame", 409, "never");
    early_server.finish();
    early.assert_value_free();

    let ordered = Fixture::new();
    ordered.seed_empty_label("zendesk", "ordered", "33333333-3333-4333-8333-333333333333");
    let ordered_server = ordered.spawn();
    let ordered_plan = ordered_server.plan("zendesk", Some("ordered"));
    let ordered_begin = credential_begin(
        &ordered_plan,
        "ordered",
        "acquire",
        ordered_plan["credential_revision"].as_str().expect("head"),
    );
    let mut ordered_session = ordered_server.session();
    ordered_session.send_control(CREDENTIAL_BEGIN, &ordered_begin);
    let need = ordered_session.read();
    assert_eq!(
        need.opcode,
        NEED_SECRETS,
        "credential refusal: {}",
        need.text()
    );
    let need = need.json();
    let needs = need["secrets"].as_array().expect("ordered needs");
    assert!(needs.len() >= 2, "fixture must exercise a nontrivial order");
    ordered_session.send_secret(2, SENTINEL);
    assert_error(ordered_session.read(), "unexpected_frame", 409, "never");
    ordered_server.finish();
    ordered.assert_value_free();
}

fn connect_begin(
    plan: &Value,
    label: &str,
    mut setting: impl FnMut(&str) -> Option<&'static str>,
) -> Value {
    let mut targets = Vec::new();
    let mut settings = Vec::new();
    let mut authorities = Vec::new();
    for field in plan["fields"].as_array().expect("plan fields") {
        if !field["required"].as_bool().expect("required")
            || !field["routable"].as_bool().expect("routable")
        {
            continue;
        }
        let plan_target = &field["target"];
        let target = json!({
            "revision": plan_target["revision"],
            "target": plan_target["id"],
        });
        if !targets.iter().any(|held| held == &target) {
            targets.push(target);
        }
        let id = plan_target["id"].as_str().expect("target id");
        if id != "connection.name" && !field["secret"].as_bool().expect("secret fact") {
            settings.push(json!({
                "target": id,
                "value": setting(id).unwrap_or_else(|| panic!("no fixture value for {id}")),
            }));
        }
        if !field["authority"].is_null() {
            authorities.push(json!({"revision": null, "target": id}));
        }
    }
    json!({
        "authorities": authorities,
        "connector": plan["connector"],
        "label": label,
        "plan_revision": plan["plan_revision"],
        "settings": settings,
        "targets": targets,
    })
}

fn credential_begin(plan: &Value, label: &str, action: &str, head: &str) -> Value {
    let mut targets = Vec::new();
    for field in plan["fields"].as_array().expect("plan fields") {
        if !field["secret"].as_bool().expect("secret fact")
            || !field["routable"].as_bool().expect("routable")
        {
            continue;
        }
        let plan_target = &field["target"];
        let target = json!({
            "revision": plan_target["revision"],
            "target": plan_target["id"],
        });
        if !targets.iter().any(|held| held == &target) {
            targets.push(target);
        }
    }
    assert!(
        !targets.is_empty(),
        "credential fixture needs a credential partition"
    );
    json!({
        "action": action,
        "connector": plan["connector"],
        "credential_revision": head,
        "label": label,
        "plan_revision": plan["plan_revision"],
        "targets": targets,
    })
}

fn complete_credential(server: &Server, begin: &Value, values: &[&[u8]]) -> Value {
    let digest = proposal_digest("exchange.local-management.v1.credential-proposal", begin);
    let mut session = server.session();
    session.send_control(CREDENTIAL_BEGIN, begin);
    let needed = session.read();
    assert_eq!(
        needed.opcode,
        NEED_SECRETS,
        "credential refusal: {}",
        needed.text()
    );
    let needed = needed.json();
    let needs = needed["secrets"].as_array().expect("secret needs");
    assert_eq!(
        needs.len(),
        values.len(),
        "fixture supplied every requested secret"
    );
    assert_eq!(needed["proposal_digest"], digest);
    for (index, value) in values.iter().enumerate() {
        let ordinal = u16::try_from(index + 1).expect("bounded ordinal");
        assert_eq!(needs[index]["ordinal"], ordinal);
        assert_eq!(needs[index]["target"], begin["targets"][index]["target"]);
        session.send_secret(ordinal, value);
    }
    let transaction_id = needed["transaction_id"]
        .as_str()
        .expect("server-owned transaction id");
    assert_transaction_id(transaction_id);
    session.send_control(
        CREDENTIAL_COMMIT,
        &json!({"proposal_digest": digest, "transaction_id": transaction_id}),
    );
    let receipt = session.read();
    assert_eq!(
        receipt.opcode,
        CREDENTIAL_RECEIPT,
        "credential refusal: {}",
        receipt.text()
    );
    let receipt = receipt.json();
    assert_receipt(
        &receipt,
        begin["connector"].as_str().expect("connector"),
        begin["label"].as_str().expect("label"),
        begin["action"].as_str().expect("action"),
        false,
    );
    receipt
}

fn proposal_digest(domain: &str, value: &Value) -> String {
    let canonical = serde_json::to_vec(value).expect("canonical proposal bytes");
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update(canonical);
    lowerhex(&digest.finalize())
}

fn assert_receipt(receipt: &Value, connector: &str, label: &str, operation: &str, replayed: bool) {
    assert_eq!(receipt["schema"], "exchange.connect-receipt.v1");
    assert_eq!(receipt["connector"], connector);
    assert_eq!(receipt["label"], label);
    assert_eq!(receipt["operation"], operation);
    assert_eq!(receipt["replayed"], replayed);
    assert_eq!(
        receipt["commit"],
        json!({"audit": "committed", "resource": "committed"})
    );
    assert_head(receipt["receipt_id"].as_str().expect("receipt id"));
    assert_eq!(receipt.as_object().expect("receipt object").len(), 7);
}

fn assert_error(frame: WireFrame, code: &str, status: u64, retry: &str) {
    assert_eq!(
        frame.opcode,
        ERROR,
        "expected refusal, received {}",
        frame.text()
    );
    assert_eq!(
        frame.json(),
        json!({
            "code": code,
            "commit": "none",
            "retry": retry,
            "schema": "exchange.local-management-error.v1",
            "status": status,
        })
    );
}

fn assert_transaction_id(value: &str) {
    assert_head(value);
    assert_ne!(&value[..16], "0000000000000000");
}

fn assert_head(value: &str) {
    assert_eq!(value.len(), 64);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    assert_ne!(value, "0".repeat(64));
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

fn lowerhex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("hex formatting");
    }
    encoded
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

fn assert_tree_excludes(root: &Path, credential_store: &Path, needle: &[u8]) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path == credential_store {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).expect("fixture metadata");
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).expect("fixture directory") {
                pending.push(entry.expect("fixture entry").path());
            }
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).expect("fixture file");
            assert_excludes(&bytes, needle);
        }
    }
}

fn assert_excludes(haystack: &[u8], needle: &[u8]) {
    assert!(
        !haystack
            .windows(needle.len())
            .any(|candidate| candidate == needle),
        "secret sentinel crossed into a value-free surface"
    );
}
