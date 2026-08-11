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

use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const CONNECT_BEGIN: u16 = 0x0001;
const NEED_SECRETS: u16 = 0x0002;
const SECRET: u16 = 0x0003;
const CONNECT_COMMIT: u16 = 0x0004;
const CONNECT_QUERY: u16 = 0x0005;
const CONNECT_RECEIPT: u16 = 0x0006;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;
const ERROR: u16 = 0x7fff;
const SENTINEL: &[u8] = b"x134-publication-crash-secret-7c81f0";

#[derive(Clone, Copy)]
struct CrashCase {
    connector: &'static str,
    durable_image: &'static str,
    label: &'static str,
    phase: &'static str,
}

const CASES: [CrashCase; 5] = [
    CrashCase {
        connector: "zendesk",
        durable_image: "settings/store.json",
        label: "after-setting",
        phase: "setting-0",
    },
    CrashCase {
        connector: "gitlab",
        durable_image: "settings/store.json",
        label: "after-authority",
        phase: "authority-0",
    },
    CrashCase {
        connector: "github",
        durable_image: "credential-heads-v1/image.json",
        label: "after-head",
        phase: "head",
    },
    CrashCase {
        connector: "github",
        durable_image: "credential-heads-v1/image.json",
        label: "after-audit",
        phase: "audit",
    },
    CrashCase {
        connector: "github",
        durable_image: "connections/store.json",
        label: "after-label",
        phase: "label",
    },
];

struct Fixture {
    owner: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(case: CrashCase) -> Self {
        let owner = std::env::temp_dir().join(format!(
            "flux-exchange-x134-publication-{}-{}-{}",
            case.phase,
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

    fn spawn(&self, crash_after: Option<&str>) -> Server {
        Server::spawn(&self.state, crash_after, None)
    }

    fn spawn_with_failure(&self, fail_after: &str) -> Server {
        Server::spawn(&self.state, None, Some(fail_after))
    }

    fn image(&self, relative: &str) -> Vec<u8> {
        std::fs::read(self.state.join(relative)).unwrap_or_else(|error| {
            panic!("durable step image {relative} was absent after injected crash: {error}")
        })
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
    fn spawn(state: &Path, crash_after: Option<&str>, fail_after: Option<&str>) -> Self {
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
            .env_remove("FLUX_EXCHANGE_CHANNELS")
            .env_remove("FLUX_EXCHANGE_CONNECTIONS")
            .env_remove("FLUX_EXCHANGE_WORKFLOWS")
            .env_remove("FLUX_EXCHANGE_APPS")
            .env_remove("FLUX_EXCHANGE_SERVICE_ACCOUNTS")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(phase) = crash_after {
            command.env("FLUX_EXCHANGE_TEST_PUBLICATION_CRASH_AFTER", phase);
        } else {
            command.env_remove("FLUX_EXCHANGE_TEST_PUBLICATION_CRASH_AFTER");
        }
        if let Some(phase) = fail_after {
            command.env("FLUX_EXCHANGE_TEST_PUBLICATION_FAIL_AFTER", phase);
        } else {
            command.env_remove("FLUX_EXCHANGE_TEST_PUBLICATION_FAIL_AFTER");
        }
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
        if readiness.is_empty() {
            let status = server.child.wait().expect("startup refusal status");
            let mut stdout = String::new();
            server
                .child
                .stdout
                .take()
                .expect("captured startup stdout")
                .read_to_string(&mut stdout)
                .expect("startup stdout");
            let mut stderr = String::new();
            server
                .child
                .stderr
                .take()
                .expect("captured startup stderr")
                .read_to_string(&mut stderr)
                .expect("startup stderr");
            panic!(
                "Exchange refused before readiness ({status}): stdout={stdout:?} stderr={stderr:?}"
            );
        }
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

    fn wait_for_injected_crash(&mut self, phase: &str, mut committing: Session) {
        committing
            .stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("crash-observation timeout");
        let mut header = [0_u8; 12];
        match committing.stream.read_exact(&mut header) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            Err(error) => {
                panic!("server neither exited nor returned a terminal frame after {phase}: {error}")
            }
            Ok(()) => {
                let length =
                    u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
                let mut payload = vec![0_u8; length];
                committing
                    .stream
                    .read_exact(&mut payload)
                    .expect("terminal payload after missing crash");
                panic!(
                    "server returned opcode {:#06x} instead of crashing after {phase}: {}",
                    u16::from_be_bytes([header[6], header[7]]),
                    String::from_utf8_lossy(&payload)
                );
            }
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("child state") {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "server did not crash after durable publication phase {phase}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            !status.success(),
            "publication phase {phase} exited as an ordinary shutdown"
        );
        self.close_liveness();
        self.assert_diagnostics_value_free();
    }

    fn terminate_before_decision(mut self) {
        self.child
            .kill()
            .expect("abrupt pre-decision server termination");
        let status = self.child.wait().expect("reap pre-decision server");
        assert!(!status.success(), "pre-decision termination was graceful");
        self.close_liveness();
        self.assert_diagnostics_value_free();
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
        self.assert_diagnostics_value_free();
    }

    fn assert_diagnostics_value_free(&mut self) {
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
fn unix_connect_crashes_recover_before_readiness_and_replay_one_receipt() {
    predecision_crash_aborts_without_a_visible_label();
    every_postdecision_projection_crash_recovers_one_indivisible_publication();
    connection_readers_and_mutations_are_both_guarded_by_pending_publication_state();
    a_live_unresolved_publication_gates_plan_and_mutation_until_same_proposal_replay();
}

fn predecision_crash_aborts_without_a_visible_label() {
    let case = CrashCase {
        connector: "github",
        durable_image: "coordinator/transactions.sqlite3",
        label: "predecision-crash",
        phase: "predecision",
    };
    let fixture = Fixture::new(case);
    let server = fixture.spawn(None);
    let plan = server.plan(case.connector, None);
    let begin = connect_begin(&plan, case.label);
    let mut active = server.session();
    active.send_control(CONNECT_BEGIN, &begin);
    let needed = active.read();
    assert_eq!(needed.opcode, NEED_SECRETS, "pre-decision allocation");
    assert!(
        !needed.json()["secrets"]
            .as_array()
            .expect("secret needs")
            .is_empty(),
        "the abrupt process boundary must follow a real provider allocation"
    );
    server.terminate_before_decision();
    drop(active);

    let restarted = fixture.spawn(None);
    let mut selected = restarted.session();
    selected.send_control(
        PLAN_QUERY,
        &json!({"connector": case.connector, "selection": case.label}),
    );
    let missing = selected.read();
    assert_eq!(missing.opcode, ERROR, "aborted label became visible");
    assert_eq!(missing.json()["code"], "unknown_label");

    let mut retry = restarted.session();
    retry.send_control(CONNECT_BEGIN, &begin);
    let retried = retry.read();
    assert_eq!(
        retried.opcode,
        NEED_SECRETS,
        "same proposal did not allocate after pre-decision recovery: {}",
        retried.text()
    );
    restarted.terminate_before_decision();
    drop(retry);

    let stable = fixture.spawn(None);
    stable.finish();
    assert_eq!(transaction_row_count(&fixture), 0);
    fixture.assert_value_free();
}

fn every_postdecision_projection_crash_recovers_one_indivisible_publication() {
    for case in CASES {
        eprintln!("publication crash case {}", case.phase);
        let fixture = Fixture::new(case);
        let mut crashed = fixture.spawn(Some(case.phase));
        let plan = crashed.plan(case.connector, None);
        let begin = connect_begin(&plan, case.label);
        if case.phase == "setting-0" || case.phase == "authority-0" {
            assert!(
                begin["settings"]
                    .as_array()
                    .is_some_and(|settings| !settings.is_empty()),
                "the injected metadata boundary must be present in the exact proposal: {plan}"
            );
        }
        if case.phase == "authority-0" {
            assert_eq!(
                begin["authorities"].as_array().map(Vec::len),
                Some(1),
                "the authority crash case must carry one exact CAS projection"
            );
        }
        // Keep the transport alive until the injected exit. Closing immediately after COMMIT
        // creates a scheduler race in which EOF can abort the active ceremony before the server
        // consumes the decision frame, which would make this crash boundary test vacuous.
        let committing = start_commit(&crashed, &begin);
        crashed.wait_for_injected_crash(case.phase, committing);

        let durable_at_crash = fixture.image(case.durable_image);
        assert_excludes(&durable_at_crash, SENTINEL);

        // Startup recovery is itself the visibility barrier: readiness is delivered only after
        // the provider decision and every value-free projection have converged.
        let recovered = fixture.spawn(None);
        let selected = recovered.plan(case.connector, Some(case.label));
        assert_complete_selected_plan(&selected, case.label);
        let first_receipt = replay_receipt(&recovered, &begin);
        assert_eq!(
            query_receipt(
                &recovered,
                first_receipt["receipt_id"].as_str().expect("receipt id")
            ),
            first_receipt,
            "QUERY and same-proposal replay diverged after {}",
            case.phase
        );
        assert_changed_proposal_conflicts(&recovered, &begin);
        let recovered_image = fixture.image(case.durable_image);
        if case.durable_image == "settings/store.json" {
            assert_settings_replay_only_completes_missing_projection(
                &durable_at_crash,
                &recovered_image,
                case.phase,
            );
        } else {
            assert_eq!(
                recovered_image, durable_at_crash,
                "recovery after {} must not create a second semantic revision",
                case.phase
            );
        }
        let selected_bytes = serde_json::to_vec(&selected).expect("canonical selected plan");
        recovered.finish();
        assert_eq!(
            transaction_row_count(&fixture),
            1,
            "recovery after {} created another transaction row",
            case.phase
        );

        let stable = fixture.spawn(None);
        assert_eq!(
            serde_json::to_vec(&stable.plan(case.connector, Some(case.label)))
                .expect("canonical stable plan"),
            selected_bytes,
            "a second restart after {} exposed another public revision",
            case.phase
        );
        assert_eq!(
            replay_receipt(&stable, &begin)["receipt_id"],
            first_receipt["receipt_id"],
            "same-proposal replay after {} must retain the first receipt",
            case.phase
        );
        stable.finish();
        assert_eq!(
            transaction_row_count(&fixture),
            1,
            "stable replay after {} allocated another transaction",
            case.phase
        );
        fixture.assert_value_free();
    }
}

fn connection_readers_and_mutations_are_both_guarded_by_pending_publication_state() {
    let connection = include_str!("../src/local_management/connection.rs");
    let transaction = include_str!("../src/local_management/transaction.rs");
    let plan = include_str!("../src/routes/connections/plan.rs");

    assert!(
        transaction.contains("pub fn publication_pending_for("),
        "the durable coordinator must expose a value-free unresolved-publication predicate"
    );
    assert!(
        connection.matches(".publication_pending_for(").count() >= 2,
        "connect and credential mutations must both refuse around an unresolved publication"
    );
    assert!(
        plan.contains("connection_publication_pending("),
        "the live connection-plan reader must not project settings/head/label partial state"
    );
}

fn a_live_unresolved_publication_gates_plan_and_mutation_until_same_proposal_replay() {
    let case = CrashCase {
        connector: "github",
        durable_image: "credential-heads-v1/image.json",
        label: "live-gate",
        phase: "head",
    };
    let fixture = Fixture::new(case);
    let server = fixture.spawn_with_failure(case.phase);
    let initial = server.plan(case.connector, None);
    let begin = connect_begin(&initial, case.label);
    let refusal = commit_and_read(&server, &begin);
    assert_eq!(refusal.opcode, ERROR);
    assert_eq!(refusal.json()["commit"], "query_receipt");

    let mut plan = server.session();
    plan.send_control(
        PLAN_QUERY,
        &json!({"connector": case.connector, "selection": null}),
    );
    let refusal = plan.read();
    assert_eq!(refusal.opcode, ERROR);
    assert_eq!(refusal.json()["code"], "connect_busy");

    let mut changed = begin.clone();
    changed["label"] = json!("different-proposal");
    let mut mutation = server.session();
    mutation.send_control(CONNECT_BEGIN, &changed);
    let refusal = mutation.read();
    assert_eq!(refusal.opcode, ERROR);
    assert_eq!(refusal.json()["code"], "connect_busy");

    let receipt = replay_receipt(&server, &begin);
    assert_eq!(receipt["commit"]["resource"], "committed");
    assert_complete_selected_plan(&server.plan(case.connector, Some(case.label)), case.label);
    server.finish();
    fixture.assert_value_free();
}

fn commit_and_read(server: &Server, begin: &Value) -> WireFrame {
    start_commit(server, begin).read()
}

fn start_commit(server: &Server, begin: &Value) -> Session {
    let digest = proposal_digest("exchange.local-management.v1.connect-proposal", begin);
    let mut session = server.session();
    session.send_control(CONNECT_BEGIN, begin);
    let needed = session.read();
    assert_eq!(
        needed.opcode,
        NEED_SECRETS,
        "connect refusal: {}",
        needed.text()
    );
    let needed = needed.json();
    assert_eq!(needed["proposal_digest"], digest);
    for (index, need) in needed["secrets"]
        .as_array()
        .expect("secret needs")
        .iter()
        .enumerate()
    {
        let ordinal = u16::try_from(index + 1).expect("bounded ordinal");
        assert_eq!(need["ordinal"], ordinal);
        session.send_secret(ordinal, SENTINEL);
    }
    session.send_control(
        CONNECT_COMMIT,
        &json!({
            "proposal_digest": digest,
            "transaction_id": needed["transaction_id"],
        }),
    );
    session
}

fn replay_receipt(server: &Server, begin: &Value) -> Value {
    let mut replay = server.session();
    replay.send_control(CONNECT_BEGIN, begin);
    let receipt = replay.read();
    assert_eq!(
        receipt.opcode,
        CONNECT_RECEIPT,
        "replay prompted or refused: {}",
        receipt.text()
    );
    let receipt = receipt.json();
    assert_eq!(receipt["replayed"], true);
    assert_eq!(
        receipt["commit"],
        json!({"audit": "committed", "resource": "committed"})
    );
    receipt
}

fn query_receipt(server: &Server, receipt_id: &str) -> Value {
    let mut query = server.session();
    query.send_control(CONNECT_QUERY, &json!({"receipt_id": receipt_id}));
    let receipt = query.read();
    assert_eq!(
        receipt.opcode,
        CONNECT_RECEIPT,
        "QUERY refused: {}",
        receipt.text()
    );
    let receipt = receipt.json();
    assert_eq!(receipt["replayed"], true);
    receipt
}

fn assert_changed_proposal_conflicts(server: &Server, begin: &Value) {
    let mut changed = begin.clone();
    changed["plan_revision"] = json!("1".repeat(64));
    let mut request = server.session();
    request.send_control(CONNECT_BEGIN, &changed);
    let refusal = request.read();
    assert_eq!(refusal.opcode, ERROR, "changed proposal was admitted");
    assert_eq!(refusal.json()["code"], "proposal_conflict");
}

fn transaction_row_count(fixture: &Fixture) -> i64 {
    let connection =
        rusqlite::Connection::open(fixture.state.join("coordinator/transactions.sqlite3"))
            .expect("open value-free transaction journal");
    connection
        .query_row("SELECT COUNT(*) FROM transactions", [], |row| row.get(0))
        .expect("count value-free transaction rows")
}

fn assert_complete_selected_plan(plan: &Value, label: &str) {
    assert_eq!(plan["selection"], label);
    assert_head(
        plan["credential_revision"]
            .as_str()
            .expect("selected plan has a durable credential head"),
    );
    for field in plan["fields"].as_array().expect("plan fields") {
        if field["required"].as_bool().expect("required")
            && field["routable"].as_bool().expect("routable")
            && !field["secret"].as_bool().expect("secret")
            && field["target"]["id"] != "connection.name"
        {
            if field["authority"].is_null() {
                assert_eq!(
                    field["set"], true,
                    "readiness exposed a partially published required setting"
                );
            } else {
                assert_eq!(field["authority"]["state"], "proposed");
                assert!(
                    field["authority"]["revision"]
                        .as_str()
                        .is_some_and(|revision| revision != "0"),
                    "readiness exposed a missing authority proposal revision"
                );
            }
        }
    }
}

fn assert_settings_replay_only_completes_missing_projection(
    at_crash: &[u8],
    recovered: &[u8],
    phase: &str,
) {
    let at_crash: Value =
        serde_json::from_slice(at_crash).expect("settings image at crash is canonical JSON");
    let recovered: Value =
        serde_json::from_slice(recovered).expect("recovered settings image is canonical JSON");
    assert_eq!(
        recovered["next_origin_revision"], at_crash["next_origin_revision"],
        "recovery after {phase} allocated a second authority revision"
    );
    assert_json_subset(
        &at_crash["values"],
        &recovered["values"],
        "settings values already durable at the crash boundary",
    );
}

fn assert_json_subset(expected: &Value, actual: &Value, path: &str) {
    match expected {
        Value::Object(expected) => {
            let actual = actual
                .as_object()
                .unwrap_or_else(|| panic!("{path} stopped being an object"));
            for (key, expected) in expected {
                let actual = actual
                    .get(key)
                    .unwrap_or_else(|| panic!("{path}.{key} disappeared during recovery"));
                assert_json_subset(expected, actual, &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path} changed during recovery replay"),
    }
}

fn connect_begin(plan: &Value, label: &str) -> Value {
    let mut targets = Vec::new();
    let mut settings = Vec::new();
    let mut authorities = Vec::new();
    for field in plan["fields"].as_array().expect("plan fields") {
        // Select every routable optional as well as every required target so the crash matrix
        // necessarily crosses settings, authority, head, audit, and label publication boundaries.
        if !field["routable"].as_bool().expect("routable") {
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
            settings.push(json!({"target": id, "value": setting_value(id)}));
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

fn setting_value(target: &str) -> &'static str {
    match target {
        "setting.default.endpoint.domain" => "acme.freshdesk.com",
        "setting.default.endpoint.origin" => "https://gitlab.example",
        target if target.ends_with(".endpoint.subdomain") => "acme",
        target if target.contains(".username.") => "operator@example.com",
        other => panic!("publication crash fixture has no value for {other}"),
    }
}

fn proposal_digest(domain: &str, value: &Value) -> String {
    let canonical = serde_json::to_vec(value).expect("canonical proposal bytes");
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update(canonical);
    lowerhex(&digest.finalize())
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
            assert_excludes(&std::fs::read(&path).expect("fixture file"), needle);
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
