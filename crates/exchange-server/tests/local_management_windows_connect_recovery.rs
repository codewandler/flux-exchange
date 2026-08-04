#![cfg(all(windows, feature = "native-root-test-seam"))]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read as _, Write as _};
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTUPINFOEXW, STARTUPINFOW,
};

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
const SENTINEL: &[u8] = b"x134-windows-connect-crash-secret-5e7d91";

struct Fixture {
    owner: PathBuf,
    state: PathBuf,
    pipe: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let owner = std::env::temp_dir().join(format!(
            "flux-exchange-x134-windows-connect-{name}-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&owner).expect("private fixture owner");
        let state = owner.join("state");
        std::fs::create_dir(&state).expect("private fixture state");
        Self {
            owner,
            state,
            pipe: owner_pipe_name(),
        }
    }

    fn spawn(&self, crash_after: Option<&str>) -> Server {
        Server::spawn(&self.state, crash_after)
    }

    fn image(&self, relative: &str) -> Vec<u8> {
        std::fs::read(self.state.join(relative)).unwrap_or_else(|error| {
            panic!("durable image {relative} was absent after injected crash: {error}")
        })
    }

    fn assert_value_free(&self) {
        assert_tree_excludes_except_credentials(&self.state, SENTINEL);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.owner);
    }
}

struct Server {
    process: HANDLE,
    readiness: HANDLE,
    liveness: HANDLE,
    readiness_bytes: Vec<u8>,
}

impl Server {
    fn spawn(state: &Path, crash_after: Option<&str>) -> Self {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let mut readiness_read = std::ptr::null_mut();
        let mut readiness_write = std::ptr::null_mut();
        let mut liveness_read = std::ptr::null_mut();
        let mut liveness_write = std::ptr::null_mut();
        // SAFETY: every output pointer and the security-attributes object remains live.
        assert_ne!(
            unsafe { CreatePipe(&mut readiness_read, &mut readiness_write, &attributes, 0) },
            0
        );
        assert_ne!(
            unsafe { CreatePipe(&mut liveness_read, &mut liveness_write, &attributes, 0) },
            0
        );
        clear_inherit(readiness_read);
        clear_inherit(liveness_write);

        let inherited = [readiness_write, liveness_read];
        let mut attribute_bytes = 0_usize;
        // SAFETY: the documented sizing call writes only the required byte count.
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attribute_bytes);
        }
        let mut attribute_storage =
            vec![0_usize; attribute_bytes.div_ceil(std::mem::size_of::<usize>())];
        let attribute_list = attribute_storage.as_mut_ptr().cast();
        // SAFETY: aligned storage has the exact size returned by the sizing call.
        assert_ne!(
            unsafe {
                InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes)
            },
            0
        );
        // SAFETY: this complete two-HANDLE capability list remains live through CreateProcessW.
        assert_ne!(
            unsafe {
                UpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_ptr().cast(),
                    std::mem::size_of_val(&inherited),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            },
            0
        );

        let executable = PathBuf::from(env!("CARGO_BIN_EXE_flux-exchange"));
        let application = wide(executable.as_os_str());
        let mut command_line = wide(OsStr::new(&format!(
            "\"{}\" --supervised --supervisor-readiness-handle {} --supervisor-liveness-handle {}",
            executable.display(),
            readiness_write as usize,
            liveness_read as usize
        )));
        let mut environment = child_environment(state, crash_after);
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attribute_list;
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: all pointers reference live Windows structures or NUL-terminated mutable buffers;
        // the explicit inheritance list contains exactly readiness-write and liveness-read.
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                environment.as_mut_ptr().cast(),
                std::ptr::null(),
                (&startup as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
                &mut process,
            )
        };
        // SAFETY: CreateProcessW returned, so the initialized attribute list is no longer read.
        unsafe { DeleteProcThreadAttributeList(attribute_list) };
        assert_ne!(
            created,
            0,
            "CreateProcessW: {}",
            std::io::Error::last_os_error()
        );
        close(readiness_write);
        close(liveness_read);
        close(process.hThread);
        let mut server = Self {
            process: process.hProcess,
            readiness: readiness_read,
            liveness: liveness_write,
            readiness_bytes: Vec::new(),
        };
        server.read_readiness();
        server
    }

    fn read_readiness(&mut self) {
        let raw = std::mem::replace(&mut self.readiness, std::ptr::null_mut());
        let mut buffer = [0_u8; 4096];
        loop {
            let mut read = 0_u32;
            // SAFETY: raw is the owned readiness read capability and both outputs remain live.
            let success = unsafe {
                ReadFile(
                    raw,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if success == 0 || read == 0 {
                break;
            }
            self.readiness_bytes
                .extend_from_slice(&buffer[..read as usize]);
        }
        close(raw);
        assert!(
            !self.readiness_bytes.is_empty(),
            "supervised Exchange refused before readiness"
        );
        assert_excludes(&self.readiness_bytes, SENTINEL, "supervisor readiness");
        let readiness: Value =
            serde_json::from_slice(&self.readiness_bytes).expect("canonical readiness JSON");
        assert_eq!(readiness["schema"], "exchange.supervisor-ready.v2");
    }

    fn terminate_before_decision(mut self) {
        // The NEED_SECRETS response is the observable barrier proving the allocation exists. This
        // termination then bypasses session cleanup, just as an abrupt native supervisor death does.
        assert_ne!(unsafe { TerminateProcess(self.process, 91) }, 0);
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 5_000) },
            WAIT_OBJECT_0,
            "pre-decision process did not terminate"
        );
        assert_eq!(exit_code(self.process), 91, "wrong abrupt-death boundary");
        self.close_liveness();
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
    }

    fn wait_for_injected_crash(mut self, phase: &str) {
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 10_000) },
            WAIT_OBJECT_0,
            "server did not crash after durable publication phase {phase}"
        );
        assert_eq!(
            exit_code(self.process),
            86,
            "server exited somewhere other than publication phase {phase}"
        );
        self.close_liveness();
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
    }

    fn stop(mut self) {
        self.close_liveness();
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 5_000) },
            WAIT_OBJECT_0,
            "liveness EOF did not stop Exchange"
        );
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
    }

    fn close_liveness(&mut self) {
        close(std::mem::replace(&mut self.liveness, std::ptr::null_mut()));
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.close_liveness();
        close(std::mem::replace(&mut self.readiness, std::ptr::null_mut()));
        if !self.process.is_null() {
            // SAFETY: test cleanup targets only this fixture's retained child process handle.
            unsafe {
                TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, 5_000);
            }
            close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        }
    }
}

struct Session {
    stream: std::fs::File,
}

impl Session {
    fn connect(pipe: &str) -> Self {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(pipe)
            {
                Ok(stream) => return Self { stream },
                Err(error)
                    if matches!(
                        error.raw_os_error().map(|code| code as u32),
                        Some(ERROR_FILE_NOT_FOUND) | Some(ERROR_PIPE_BUSY)
                    ) =>
                {
                    assert!(
                        Instant::now() < deadline,
                        "owner pipe was not reachable after readiness: {error}"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("owner pipe refused: {error}"),
            }
        }
    }

    fn send_control(&mut self, opcode: u16, value: &Value) {
        let payload = serde_json::to_vec(value).expect("canonical control JSON");
        self.send(opcode, &payload);
    }

    fn send_secret(&mut self, ordinal: u16, value: &[u8]) {
        let mut payload = Vec::with_capacity(2 + value.len());
        payload.extend_from_slice(&ordinal.to_be_bytes());
        payload.extend_from_slice(value);
        self.send(SECRET, &payload);
    }

    fn send(&mut self, opcode: u16, payload: &[u8]) {
        self.stream
            .write_all(&frame(CLIENT, opcode, payload))
            .expect("complete client FXLM frame");
        self.stream.flush().expect("flush named-pipe frame");
    }

    fn request_control(&mut self, opcode: u16, value: &Value) -> WireFrame {
        self.send_control(opcode, value);
        self.read()
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
        assert_excludes(&payload, SENTINEL, "server control response");
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
fn windows_connect_crashes_recover_before_readiness_and_replay_one_receipt() {
    predecision_crash_aborts_without_a_visible_label();
    postdecision_crash_rolls_forward_once_and_replays_one_receipt();
}

fn predecision_crash_aborts_without_a_visible_label() {
    let fixture = Fixture::new("predecision");
    let server = fixture.spawn(None);
    let plan = plan(&fixture.pipe, "github", None);
    let begin = connect_begin(&plan, "predecision-crash");
    let mut active = Session::connect(&fixture.pipe);
    let needed = active.request_control(CONNECT_BEGIN, &begin);
    assert_eq!(
        needed.opcode,
        NEED_SECRETS,
        "connect refusal: {}",
        needed.text()
    );
    assert!(
        !needed.json()["secrets"]
            .as_array()
            .expect("secret needs")
            .is_empty(),
        "pre-decision fixture needs a secret-bearing allocation"
    );
    server.terminate_before_decision();
    drop(active);

    let restarted = fixture.spawn(None);
    let missing = plan_response(&fixture.pipe, "github", Some("predecision-crash"));
    assert_eq!(missing.opcode, ERROR, "aborted label became visible");
    assert_eq!(missing.json()["code"], "unknown_label");

    // Reusing the same proposal must allocate again instead of observing the recovered row as Busy.
    let retried = Session::connect(&fixture.pipe).request_control(CONNECT_BEGIN, &begin);
    assert_eq!(
        retried.opcode,
        NEED_SECRETS,
        "startup did not abort the pre-decision allocation: {}",
        retried.text()
    );
    restarted.terminate_before_decision();
    fixture.assert_value_free();
}

fn postdecision_crash_rolls_forward_once_and_replays_one_receipt() {
    let fixture = Fixture::new("postdecision");
    let crashed = fixture.spawn(Some("head"));
    let initial = plan(&fixture.pipe, "github", None);
    let begin = connect_begin(&initial, "postdecision-crash");
    commit_until_crash(&fixture.pipe, &begin);
    crashed.wait_for_injected_crash("head");

    let head_at_crash = fixture.image("credential-heads-v1/image.json");
    assert_excludes(&head_at_crash, SENTINEL, "credential-head durable image");
    assert!(
        tree_contains(&fixture.state.join("credentials"), SENTINEL),
        "the committed provider never retained the secret used by the real ceremony"
    );
    fixture.assert_value_free();

    // Readiness is the recovery barrier: the selected label must be complete before any route or
    // owner-pipe session can observe this restarted process.
    let recovered = fixture.spawn(None);
    let selected = plan(&fixture.pipe, "github", Some("postdecision-crash"));
    assert_eq!(selected["selection"], "postdecision-crash");
    assert_head(
        selected["credential_revision"]
            .as_str()
            .expect("recovered durable credential head"),
    );
    assert_eq!(
        fixture.image("credential-heads-v1/image.json"),
        head_at_crash,
        "startup recovery advanced the durable head more than once"
    );

    let replayed = replay_receipt(&fixture.pipe, &begin);
    let receipt_id = replayed["receipt_id"]
        .as_str()
        .expect("durable receipt id")
        .to_owned();
    let queried = query_receipt(&fixture.pipe, &receipt_id);
    assert_eq!(queried, replayed, "QUERY and same-proposal replay diverged");
    recovered.stop();

    let stable = fixture.spawn(None);
    assert_eq!(
        query_receipt(&fixture.pipe, &receipt_id)["receipt_id"],
        receipt_id
    );
    assert_eq!(
        replay_receipt(&fixture.pipe, &begin)["receipt_id"],
        receipt_id,
        "second restart created a second receipt"
    );
    assert_eq!(
        fixture.image("credential-heads-v1/image.json"),
        head_at_crash,
        "second restart created another semantic head revision"
    );
    stable.stop();
    fixture.assert_value_free();
}

fn plan(pipe: &str, connector: &str, selection: Option<&str>) -> Value {
    let response = plan_response(pipe, connector, selection);
    assert_eq!(
        response.opcode,
        PLAN_RESPONSE,
        "plan refusal: {}",
        response.text()
    );
    response.json()
}

fn plan_response(pipe: &str, connector: &str, selection: Option<&str>) -> WireFrame {
    Session::connect(pipe).request_control(
        PLAN_QUERY,
        &json!({"connector": connector, "selection": selection}),
    )
}

fn commit_until_crash(pipe: &str, begin: &Value) {
    let digest = proposal_digest("exchange.local-management.v1.connect-proposal", begin);
    let mut session = Session::connect(pipe);
    let needed = session.request_control(CONNECT_BEGIN, begin);
    assert_eq!(
        needed.opcode,
        NEED_SECRETS,
        "connect refusal: {}",
        needed.text()
    );
    let needed = needed.json();
    assert_eq!(needed["proposal_digest"], digest);
    let needs = needed["secrets"].as_array().expect("secret needs");
    assert_eq!(needs.len(), 1, "GitHub fixture requires one secret");
    for (index, need) in needs.iter().enumerate() {
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
}

fn replay_receipt(pipe: &str, begin: &Value) -> Value {
    let receipt = Session::connect(pipe).request_control(CONNECT_BEGIN, begin);
    assert_eq!(
        receipt.opcode,
        CONNECT_RECEIPT,
        "same-proposal replay prompted or refused: {}",
        receipt.text()
    );
    let receipt = receipt.json();
    assert_receipt(&receipt, true);
    receipt
}

fn query_receipt(pipe: &str, receipt_id: &str) -> Value {
    let receipt =
        Session::connect(pipe).request_control(CONNECT_QUERY, &json!({"receipt_id": receipt_id}));
    assert_eq!(
        receipt.opcode,
        CONNECT_RECEIPT,
        "receipt QUERY refused: {}",
        receipt.text()
    );
    let receipt = receipt.json();
    assert_receipt(&receipt, true);
    receipt
}

fn connect_begin(plan: &Value, label: &str) -> Value {
    let mut targets = Vec::new();
    for field in plan["fields"].as_array().expect("plan fields") {
        if !field["required"].as_bool().expect("required")
            || !field["routable"].as_bool().expect("routable")
        {
            continue;
        }
        let target = &field["target"];
        if !targets.iter().any(|held| held == target) {
            targets.push(target.clone());
        }
        let target_id = target["id"].as_str().expect("target id");
        assert!(
            target_id == "connection.name" || field["secret"].as_bool() == Some(true),
            "GitHub fixture unexpectedly requires a non-secret setting: {target_id}"
        );
    }
    assert_eq!(
        targets
            .iter()
            .filter(|target| target["id"] != "connection.name")
            .count(),
        1,
        "GitHub fixture requires exactly one secret"
    );
    json!({
        "authorities": [],
        "connector": plan["connector"],
        "label": label,
        "plan_revision": plan["plan_revision"],
        "settings": [],
        "targets": targets,
    })
}

fn proposal_digest(domain: &str, value: &Value) -> String {
    let canonical = serde_json::to_vec(value).expect("canonical proposal bytes");
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update(canonical);
    lowerhex(&digest.finalize())
}

fn assert_receipt(receipt: &Value, replayed: bool) {
    assert_eq!(receipt["schema"], "exchange.connect-receipt.v1");
    assert_eq!(receipt["connector"], "github");
    assert_eq!(receipt["label"], "postdecision-crash");
    assert_eq!(receipt["operation"], "connect");
    assert_eq!(receipt["replayed"], replayed);
    assert_eq!(
        receipt["commit"],
        json!({"audit": "committed", "resource": "committed"})
    );
    assert_head(receipt["receipt_id"].as_str().expect("receipt id"));
    assert_eq!(receipt.as_object().expect("receipt object").len(), 7);
}

fn assert_head(value: &str) {
    assert_eq!(value.len(), 64);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert!(value.bytes().any(|byte| byte != b'0'));
}

fn owner_pipe_name() -> String {
    let sid = process_token_sid();
    let digest = Sha256::digest(&sid);
    let suffix = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(r"\\.\pipe\flux-exchange-local-management-v1-{suffix}")
}

fn process_token_sid() -> Vec<u8> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: success initializes one token handle owned by this function.
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        0
    );
    let mut length = 0_u32;
    // SAFETY: the sizing call writes only the required byte count.
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
    }
    let mut storage = vec![0_usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: aligned storage has at least the byte count returned by the sizing call.
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                storage.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        },
        0
    );
    let base = storage.as_ptr().cast::<u8>();
    // SAFETY: successful GetTokenInformation populated one TOKEN_USER.
    let sid = unsafe { (*(base.cast::<TOKEN_USER>())).User.Sid.cast::<u8>() };
    let sid_length = unsafe { GetLengthSid(sid.cast()) } as usize;
    // SAFETY: GetLengthSid reports the complete valid SID inside live token storage.
    let bytes = unsafe { std::slice::from_raw_parts(sid, sid_length) }.to_vec();
    close(token);
    bytes
}

fn child_environment(state: &Path, crash_after: Option<&str>) -> Vec<u16> {
    let mut values = std::env::vars_os().collect::<BTreeMap<OsString, OsString>>();
    values.retain(|name, _| {
        !name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("FLUX_EXCHANGE_")
    });
    values.insert("FLUX_EXCHANGE_STATE".into(), state.as_os_str().to_owned());
    if let Some(phase) = crash_after {
        values.insert(
            "FLUX_EXCHANGE_TEST_PUBLICATION_CRASH_AFTER".into(),
            phase.into(),
        );
    }
    let mut block = Vec::new();
    for (name, value) in values {
        block.extend(name.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
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

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn clear_inherit(handle: HANDLE) {
    use windows_sys::Win32::Foundation::SetHandleInformation;
    // SAFETY: only the inheritance bit on this owned pipe handle changes.
    assert_ne!(
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) },
        0
    );
}

fn close(handle: HANDLE) {
    if !handle.is_null() {
        // SAFETY: fixture ownership ensures every native handle is closed at most once.
        unsafe { CloseHandle(handle) };
    }
}

fn exit_code(process: HANDLE) -> u32 {
    let mut code = 0_u32;
    // SAFETY: process is a retained process handle that has just reached a signaled state.
    assert_ne!(unsafe { GetExitCodeProcess(process, &mut code) }, 0);
    code
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn tree_contains(root: &Path, needle: &[u8]) -> bool {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .expect("fixture directory")
                    .map(|entry| entry.expect("fixture entry").path()),
            );
        } else if metadata.is_file()
            && std::fs::read(&path)
                .expect("fixture file")
                .windows(needle.len())
                .any(|candidate| candidate == needle)
        {
            return true;
        }
    }
    false
}

fn assert_tree_excludes_except_credentials(root: &Path, needle: &[u8]) {
    let credential_store = root.join("credentials");
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path == credential_store {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).expect("fixture metadata");
        if metadata.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .expect("fixture directory")
                    .map(|entry| entry.expect("fixture entry").path()),
            );
        } else if metadata.is_file() {
            assert_excludes(
                &std::fs::read(&path).expect("fixture file"),
                needle,
                path.display(),
            );
        }
    }
}

fn assert_excludes(haystack: &[u8], needle: &[u8], surface: impl std::fmt::Display) {
    assert!(
        !haystack
            .windows(needle.len())
            .any(|candidate| candidate == needle),
        "secret sentinel crossed value-free surface {surface}"
    );
}

fn lowerhex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("hex formatting");
    }
    encoded
}
