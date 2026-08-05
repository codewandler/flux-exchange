#![cfg(windows)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use exchange_host::{Grant, GrantStore, Grants as _, Risk, Selector, Tenant};
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
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTUPINFOEXW, STARTUPINFOW,
};

const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const GRANT_PREVIEW: u16 = 0x0010;
const GRANT_CANDIDATE: u16 = 0x0011;
const GRANT_APPLY: u16 = 0x0012;
const GRANT_QUERY: u16 = 0x0013;
const GRANT_RECEIPT: u16 = 0x0014;
const ERROR: u16 = 0x7fff;

const CHILD_MARKER: &str = "FLUX_EXCHANGE_X134_WINDOWS_GRANT_APPLY_CHILD";
const CHILD_PIPE: &str = "FLUX_EXCHANGE_X134_WINDOWS_GRANT_PIPE";
const CHILD_CANDIDATE: &str = "FLUX_EXCHANGE_X134_WINDOWS_GRANT_CANDIDATE";
const CHILD_READY: &str = "FLUX_EXCHANGE_X134_WINDOWS_GRANT_READY";
const CHILD_RELEASE: &str = "FLUX_EXCHANGE_X134_WINDOWS_GRANT_RELEASE";
const CHILD_RESPONSE: &str = "FLUX_EXCHANGE_X134_WINDOWS_GRANT_RESPONSE";

struct Fixture {
    owner: PathBuf,
    state: PathBuf,
    pipe: String,
}

impl Fixture {
    fn new() -> Self {
        let owner = std::env::temp_dir().join(format!(
            "flux-exchange-x134-native-windows-grant-cas-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&owner).expect("owner fixture directory");
        let state = owner.join("state");
        std::fs::create_dir(&state).expect("state fixture directory");
        Self {
            owner,
            state,
            pipe: owner_pipe_name(),
        }
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

struct Server {
    process: HANDLE,
    readiness: HANDLE,
    liveness: HANDLE,
}

impl Server {
    fn spawn(state: &Path) -> Self {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let mut readiness_read = std::ptr::null_mut();
        let mut readiness_write = std::ptr::null_mut();
        let mut liveness_read = std::ptr::null_mut();
        let mut liveness_write = std::ptr::null_mut();
        // SAFETY: all output pointers and the security attributes remain live.
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
        // SAFETY: aligned storage has the size returned by the sizing call.
        assert_ne!(
            unsafe {
                InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes)
            },
            0
        );
        // SAFETY: the complete HANDLE list remains live through CreateProcessW.
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
        let mut environment = current_environment(state);
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attribute_list;
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: pointers reference live Windows structures and mutable NUL-terminated buffers;
        // the explicit list is exactly the readiness and liveness capabilities.
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
        // SAFETY: CreateProcessW has returned and no longer reads the attribute list.
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
        };
        let readiness = server.readiness();
        assert!(!readiness.is_empty(), "Exchange refused before readiness");
        await_http_service(&readiness);
        server
    }

    fn readiness(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let mut read = 0_u32;
            // SAFETY: the output buffer/count and owned readiness handle remain live.
            let success = unsafe {
                ReadFile(
                    self.readiness,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if success == 0 || read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
        close(std::mem::replace(&mut self.readiness, std::ptr::null_mut()));
        bytes
    }

    fn stop(mut self) {
        self.close_liveness();
        // SAFETY: process is the live child handle returned by CreateProcessW.
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 5_000) },
            WAIT_OBJECT_0
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
        if !self.readiness.is_null() {
            close(std::mem::replace(&mut self.readiness, std::ptr::null_mut()));
        }
        if !self.process.is_null() {
            // SAFETY: termination is cleanup for the exact child handle this fixture owns.
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
                        "owner local-management pipe was not reachable: {error}"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("owner local-management pipe refused: {error}"),
            }
        }
    }

    fn request(&mut self, opcode: u16, payload: &[u8]) -> WireFrame {
        self.stream
            .write_all(&frame(CLIENT, opcode, payload))
            .expect("complete client frame");
        self.stream.flush().expect("flush named-pipe frame");
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
    fn spawn(root: &Path, pipe: &str, candidate: &[u8], name: &str, release: &Path) -> Self {
        let candidate_path = root.join(format!("{name}.candidate.json"));
        let ready = root.join(format!("{name}.ready"));
        let response = root.join(format!("{name}.response"));
        std::fs::write(&candidate_path, candidate).expect("candidate handoff");
        let child = Command::new(std::env::current_exe().expect("current integration test"))
            .arg("--exact")
            .arg("concurrent_native_process_apply_is_one_cas_and_restart_stable_on_windows")
            .arg("--nocapture")
            .env(CHILD_MARKER, "1")
            .env(CHILD_PIPE, pipe)
            .env(CHILD_CANDIDATE, &candidate_path)
            .env(CHILD_READY, &ready)
            .env(CHILD_RELEASE, release)
            .env(CHILD_RESPONSE, &response)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("native Windows APPLY client process");
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
            "native Windows APPLY client failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        WireFrame::decode_result(&std::fs::read(&self.response).expect("APPLY response bytes"))
    }
}

#[test]
fn concurrent_native_process_apply_is_one_cas_and_restart_stable_on_windows() {
    if std::env::var_os(CHILD_MARKER).is_some() {
        run_apply_windows_client_child();
        return;
    }
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
    let low = preview(&fixture.pipe, "low");
    let high = preview(&fixture.pipe, "high");
    assert_eq!(low.json()["revision"], high.json()["revision"]);
    assert_ne!(
        low.json()["proposal_digest"],
        high.json()["proposal_digest"],
        "the race must carry distinct proposals at one revision"
    );

    let release = fixture.owner.join("apply.release");
    let mut clients = vec![
        ApplyClient::spawn(&fixture.owner, &fixture.pipe, &low.payload, "low", &release),
        ApplyClient::spawn(
            &fixture.owner,
            &fixture.pipe,
            &high.payload,
            "high",
            &release,
        ),
    ];
    wait_until_ready(&mut clients);
    std::fs::write(&release, b"both native clients released").expect("release APPLY race");
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
    assert_eq!(accepted.len(), 1, "exactly one proposal wins the CAS");
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
    server.stop();

    let restarted = fixture.spawn();
    let queried = Session::connect(&fixture.pipe)
        .request_json(GRANT_QUERY, &json!({"receipt_id": &receipt_id}));
    assert_eq!(queried.opcode, GRANT_RECEIPT);
    let queried = queried.json();
    assert_eq!(queried["receipt_id"], receipt_id);
    assert_eq!(queried["revision"], revision);
    assert_eq!(queried["replayed"], true);

    let replayed = Session::connect(&fixture.pipe).request(GRANT_APPLY, &winning_candidate.payload);
    assert_eq!(replayed.opcode, GRANT_RECEIPT);
    let replayed = replayed.json();
    assert_eq!(replayed["receipt_id"], receipt_id);
    assert_eq!(replayed["revision"], revision);
    assert_eq!(replayed["replayed"], true);
    assert_eq!(
        preview(
            &fixture.pipe,
            winning_candidate.json()["candidate"]["selector"]["max_risk"]
                .as_str()
                .expect("winning risk"),
        )
        .json()["revision"],
        revision
    );
    restarted.stop();

    let reopened = GrantStore::bind(fixture.grant_path()).expect("restart-stable grant store");
    let held = reopened.held(&tenant);
    assert_eq!(held.len(), 3, "the CAS changes exactly one connector row");
    assert_eq!(
        &held[..unrelated.len()],
        unrelated.as_slice(),
        "unrelated duplicate grants retain order, multiplicity and selectors"
    );
    let github = held
        .iter()
        .find(|grant| grant.connector == "github")
        .expect("winning github grant");
    let expected_risk = if winner == 0 { Risk::Low } else { Risk::High };
    assert_eq!(github.selector.max_risk, Some(expected_risk));
}

fn run_apply_windows_client_child() {
    let pipe = std::env::var(CHILD_PIPE).expect("child pipe name");
    let candidate = std::fs::read(std::env::var_os(CHILD_CANDIDATE).expect("candidate path"))
        .expect("candidate bytes");
    let ready = PathBuf::from(std::env::var_os(CHILD_READY).expect("ready path"));
    let release = PathBuf::from(std::env::var_os(CHILD_RELEASE).expect("release path"));
    let response = PathBuf::from(std::env::var_os(CHILD_RESPONSE).expect("response path"));
    std::fs::write(&ready, b"ready to connect").expect("child ready barrier");
    wait_for_path(&release, Duration::from_secs(5));
    let result = Session::connect(&pipe).request(GRANT_APPLY, &candidate);
    std::fs::write(response, result.encode_result()).expect("child response handoff");
}

fn preview(pipe: &str, max_risk: &str) -> WireFrame {
    let response = Session::connect(pipe).request_json(
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
                panic!("native APPLY client exited before release: {status}");
            }
        }
        assert!(Instant::now() < deadline, "APPLY clients missed barrier");
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

fn await_http_service(readiness: &[u8]) {
    let ready: Value = serde_json::from_slice(readiness).expect("supervised readiness JSON");
    let host: IpAddr = ready["bind"]["host"]
        .as_str()
        .expect("readiness host")
        .parse()
        .expect("numeric readiness host");
    let port = ready["bind"]["port"].as_u64().expect("readiness port") as u16;
    let mut stream = TcpStream::connect(SocketAddr::new(host, port)).expect("HTTP listener");
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
    assert_eq!(&response, b"HTTP/1.1 200");
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

fn current_environment(state: &Path) -> Vec<u16> {
    let mut values = std::env::vars_os().collect::<BTreeMap<OsString, OsString>>();
    for name in [
        "FLUX_EXCHANGE_CREDENTIALS",
        "FLUX_EXCHANGE_SETTINGS",
        "FLUX_EXCHANGE_GRANTS",
        "FLUX_EXCHANGE_CONNECTIONS",
        "FLUX_EXCHANGE_CHANNELS",
        "FLUX_EXCHANGE_WORKFLOWS",
        "FLUX_EXCHANGE_AUDIT",
        "FLUX_EXCHANGE_SERVICE_ACCOUNTS",
        "FLUX_EXCHANGE_APPS",
    ] {
        values.remove(OsStr::new(name));
    }
    values.insert("FLUX_EXCHANGE_STATE".into(), state.as_os_str().to_owned());
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
        // SAFETY: fixture ownership closes each native handle once.
        unsafe { CloseHandle(handle) };
    }
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
