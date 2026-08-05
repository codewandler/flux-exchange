#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "native-root-test-seam",
    feature = "native-helper-deadline-test-seam"
))]

use std::ffi::OsStr;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const CONNECT_BEGIN: u16 = 0x0001;
const CONNECT_RECEIPT: u16 = 0x0006;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;
const SENTINEL: &[u8] = b"x134-real-tty-secret-bc5d9138";

/// The helper must acquire a terminal independently of its three standard streams. This launches
/// the release binary, not a source-included worker: FD 6/7 are its only Flux capabilities while
/// an otherwise-unreferenced controlling PTY makes `/dev/tty` the only possible secret input.
#[test]
fn supervised_unix_helper_private_input_and_outer_deadline_are_exact() {
    {
        let fixture = Fixture::new();
        let mut server = SupervisedServer::spawn(&fixture.state);
        let plan = server.plan("github", None);
        let begin = connect_begin(&plan, "tty-owned");
        let request = frame(
            CLIENT,
            CONNECT_BEGIN,
            &serde_json::to_vec(&begin).expect("canonical BEGIN"),
        );

        let mut helper = HelperProcess::spawn(&fixture.state, None);
        helper.assert_process_inputs_exclude(SENTINEL);
        helper
            .request
            .as_mut()
            .expect("live helper request")
            .write_all(&request)
            .expect("BEGIN request");
        drop(helper.request.take());
        if let Some(status) = helper.wait_for_private_terminal_read() {
            let response = read_to_eof_before(&helper.response, Duration::from_secs(1));
            panic!(
                "helper exited before private input: {:?}, terminal={}",
                status.code(),
                String::from_utf8_lossy(&response)
            );
        }

        // Canonical-mode terminal input is one line. The synchronization above observes that
        // production disabled terminal echo before this byte exists; writing optimistically would
        // let the kernel echo a secret before even a correct reader had opened `/dev/tty`.
        helper.terminal.write_all(SENTINEL).expect("TTY secret");
        helper.terminal.write_all(b"\n").expect("TTY line ending");

        let response = read_to_eof_before(&helper.response, Duration::from_secs(10));
        let status = wait_before(&mut helper.child, Duration::from_secs(5));
        assert_eq!(status.code(), Some(0), "helper transport did not complete");
        let receipt = decode_server_control(&response, CONNECT_RECEIPT);
        assert_eq!(receipt["schema"], "exchange.connect-receipt.v1");
        assert_eq!(receipt["connector"], "github");
        assert_eq!(receipt["label"], "tty-owned");
        assert_eq!(receipt["operation"], "connect");
        helper.assert_terminal_echo_restored();

        let transcript = drain_terminal(&helper.terminal);
        assert_excludes(&transcript, SENTINEL, "controlling-terminal echo/output");
        server.finish(SENTINEL);

        let credentials =
            exchange_host::CredentialStore::bind(fixture.state.join("credentials/store.txt"))
                .expect("reopen retained credential provider after server exit");
        let reference =
            exchange_host::CredentialRef::new("local", "com.github.api", "default", "token")
                .expect("GitHub credential address");
        let secret = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("credential read runtime")
            .block_on(credentials.secrets().get(&reference))
            .expect("credential committed through the retained provider");
        assert_eq!(secret.expose_secret().as_bytes(), SENTINEL);
        assert_tree_excludes_except_credentials(&fixture.state, SENTINEL);
    }

    // The production binary receives the same unchanged result deadline through its feature-only
    // native test seam. Holding the real controlling TTY past that boundary must cancel the read,
    // restore echo, close the response capability and produce only the fixed transport exit.
    {
        let fixture = Fixture::new();
        let mut server = SupervisedServer::spawn(&fixture.state);
        let plan = server.plan("github", None);
        let begin = connect_begin(&plan, "tty-deadline");
        let request = frame(
            CLIENT,
            CONNECT_BEGIN,
            &serde_json::to_vec(&begin).expect("canonical deadline BEGIN"),
        );
        let mut helper = HelperProcess::spawn(&fixture.state, Some(300));
        helper
            .request
            .as_mut()
            .expect("live deadline request")
            .write_all(&request)
            .expect("deadline BEGIN request");
        drop(helper.request.take());
        if let Some(status) = helper.wait_for_private_terminal_read() {
            let response = read_to_eof_before(&helper.response, Duration::from_secs(1));
            panic!(
                "deadline helper exited before private input: {:?}, terminal={}",
                status.code(),
                String::from_utf8_lossy(&response)
            );
        }

        let response = read_to_eof_before(&helper.response, Duration::from_secs(2));
        let status = wait_before(&mut helper.child, Duration::from_secs(2));
        assert_eq!(
            status.code(),
            Some(1),
            "deadline is a helper transport exit"
        );
        assert!(
            response.is_empty(),
            "expired result pipe must close value-free"
        );
        helper.assert_terminal_echo_restored();
        assert!(drain_terminal(&helper.terminal).is_empty());
        server.finish(SENTINEL);
        assert_tree_excludes_except_credentials(&fixture.state, SENTINEL);
    }
}

struct Fixture {
    root: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-helper-tty-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&root).expect("private fixture owner");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture owner");
        let state = root.join("state");
        std::fs::create_dir(&state).expect("private state root");
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only state root");
        Self { root, state }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Pipe {
    read: OwnedFd,
    write: OwnedFd,
}

impl Pipe {
    fn new() -> Self {
        let mut descriptors = [-1; 2];
        // SAFETY: the live output array receives two owned descriptors on success.
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        // SAFETY: successful pipe creation transfers each distinct descriptor to this fixture.
        let read = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
        // SAFETY: as above, this is the distinct write end.
        let write = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
        Self { read, write }
    }
}

struct SupervisedServer {
    child: Child,
    readiness: Option<OwnedFd>,
    liveness: Option<OwnedFd>,
    socket: PathBuf,
}

impl SupervisedServer {
    fn spawn(state: &Path) -> Self {
        let readiness = Pipe::new();
        let liveness = Pipe::new();
        let readiness_source = duplicate_high(readiness.write.as_raw_fd());
        let liveness_source = duplicate_high(liveness.read.as_raw_fd());
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("--supervised")
            .env("FLUX_EXCHANGE_STATE", state)
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", state)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: the closure uses descriptor-only, async-signal-safe operations before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(readiness_source, 3) < 0 || libc::dup2(liveness_source, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for descriptor in 5..256 {
                    libc::close(descriptor);
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("real supervised Exchange");
        close_raw(readiness_source);
        close_raw(liveness_source);
        drop(readiness.write);
        drop(liveness.read);
        let mut server = Self {
            child,
            readiness: Some(readiness.read),
            liveness: Some(liveness.write),
            socket: state.join("run/local-management-v1.sock"),
        };
        let ready = server.readiness();
        assert!(!ready.is_empty(), "Exchange refused before readiness");
        server
    }

    fn readiness(&mut self) -> Vec<u8> {
        let descriptor = self.readiness.take().expect("one readiness read");
        let mut file = std::fs::File::from(descriptor);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("readiness bytes");
        bytes
    }

    fn plan(&self, connector: &str, selection: Option<&str>) -> Value {
        let mut stream = UnixStream::connect(&self.socket).expect("owner FXLM plan connection");
        let payload = serde_json::to_vec(&json!({
            "connector": connector,
            "selection": selection,
        }))
        .expect("canonical plan query");
        stream
            .write_all(&frame(CLIENT, PLAN_QUERY, &payload))
            .expect("plan query");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("complete PLAN request");
        let plan = decode_server_control(&read_frame(&mut stream), PLAN_RESPONSE);
        let mut terminal = Vec::new();
        stream
            .read_to_end(&mut terminal)
            .expect("PLAN terminal clean EOF");
        assert!(terminal.is_empty(), "PLAN emitted surplus terminal bytes");
        plan
    }

    fn finish(&mut self, sentinel: &[u8]) {
        drop(self.liveness.take());
        let status = wait_before(&mut self.child, Duration::from_secs(5));
        assert_eq!(
            status.code(),
            Some(1),
            "the retained X-128 liveness EOF contract exits exactly 1"
        );
        let mut stdout = Vec::new();
        self.child
            .stdout
            .take()
            .expect("captured server stdout")
            .read_to_end(&mut stdout)
            .expect("server stdout");
        let mut stderr = Vec::new();
        self.child
            .stderr
            .take()
            .expect("captured server stderr")
            .read_to_end(&mut stderr)
            .expect("server stderr");
        assert_excludes(&stdout, sentinel, "server stdout");
        assert_excludes(&stderr, sentinel, "server stderr");
    }
}

impl Drop for SupervisedServer {
    fn drop(&mut self) {
        drop(self.liveness.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct HelperProcess {
    child: Child,
    request: Option<std::fs::File>,
    response: std::fs::File,
    terminal: std::fs::File,
    terminal_control: OwnedFd,
    state_root: PathBuf,
}

impl HelperProcess {
    fn spawn(state_root: &Path, result_budget_millis: Option<u64>) -> Self {
        let request = Pipe::new();
        let response = Pipe::new();
        let request_source = duplicate_high(request.read.as_raw_fd());
        let response_source = duplicate_high(response.write.as_raw_fd());
        let (terminal, terminal_slave) = pseudo_terminal();
        let terminal_source = duplicate_high(terminal_slave.as_raw_fd());

        let arguments = [OsStr::new("local"), OsStr::new("vendor-secret")];
        assert!(arguments.iter().all(|argument| !argument
            .as_encoded_bytes()
            .windows(SENTINEL.len())
            .any(|bytes| bytes == SENTINEL)));
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .args(arguments)
            .env_clear()
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", state_root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(milliseconds) = result_budget_millis {
            command.env(
                "FLUX_EXCHANGE_TEST_HELPER_RESULT_MILLIS",
                milliseconds.to_string(),
            );
        }
        // SAFETY: the closure establishes an otherwise descriptor-free session and uses only
        // async-signal-safe descriptor/session operations before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(terminal_source, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::dup2(request_source, 6) < 0 || libc::dup2(response_source, 7) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for descriptor in 3..256 {
                    if descriptor != 6 && descriptor != 7 {
                        libc::close(descriptor);
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("real vendor helper");
        close_raw(request_source);
        close_raw(response_source);
        close_raw(terminal_source);
        drop(request.read);
        drop(response.write);
        Self {
            child,
            request: Some(std::fs::File::from(request.write)),
            response: std::fs::File::from(response.read),
            terminal: std::fs::File::from(terminal),
            terminal_control: terminal_slave,
            state_root: state_root.to_path_buf(),
        }
    }

    fn wait_for_private_terminal_read(&mut self) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().expect("helper state") {
                return Some(status);
            }
            // SAFETY: tcgetattr only writes the live termios output for this PTY slave.
            let mut attributes = unsafe { std::mem::zeroed::<libc::termios>() };
            assert_eq!(
                unsafe { libc::tcgetattr(self.terminal_control.as_raw_fd(), &mut attributes) },
                0,
                "PTY terminal attributes"
            );
            if attributes.c_lflag & libc::ECHO == 0 {
                return None;
            }
            assert!(
                Instant::now() < deadline,
                "helper never entered a no-echo /dev/tty read"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn assert_terminal_echo_restored(&self) {
        let mut attributes = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(self.terminal_control.as_raw_fd(), &mut attributes) },
            0,
            "PTY terminal attributes after helper exit"
        );
        assert_ne!(
            attributes.c_lflag & libc::ECHO,
            0,
            "helper did not restore private-terminal echo"
        );
    }

    fn assert_process_inputs_exclude(&self, sentinel: &[u8]) {
        assert_excludes(
            self.state_root.as_os_str().as_encoded_bytes(),
            sentinel,
            "helper environment root",
        );
        #[cfg(target_os = "linux")]
        {
            let process = PathBuf::from(format!("/proc/{}", self.child.id()));
            let command_line = std::fs::read(process.join("cmdline")).expect("live helper argv");
            let environment = std::fs::read(process.join("environ")).expect("live helper env");
            assert_excludes(&command_line, sentinel, "helper argv");
            assert_excludes(&environment, sentinel, "helper environment");
            for standard in 0..=2 {
                let target = std::fs::read_link(process.join(format!("fd/{standard}")))
                    .expect("live helper standard stream");
                assert_eq!(target, Path::new("/dev/null"));
            }
        }
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
        let begin_target = json!({
            "revision": target["revision"],
            "target": target["id"],
        });
        if !targets.iter().any(|held| held == &begin_target) {
            targets.push(begin_target);
        }
        let target_id = target["id"].as_str().expect("target id");
        assert!(
            target_id == "connection.name" || field["secret"].as_bool() == Some(true),
            "github process fixture unexpectedly requires a non-secret setting: {target_id}"
        );
    }
    assert_eq!(
        targets
            .iter()
            .filter(|target| target["target"] != "connection.name")
            .count(),
        1,
        "process fixture requires exactly one TTY secret"
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

fn pseudo_terminal() -> (OwnedFd, OwnedFd) {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: the two output pointers are live and null optional termios/winsize use defaults.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // SAFETY: successful openpty transfers two distinct live descriptors.
    unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) }
}

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0_u8; 12];
    stream
        .read_exact(&mut header)
        .expect("complete FXLM header");
    assert_eq!(&header[..4], b"FXLM");
    let length = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    assert!(length <= 65_536, "bounded control payload");
    let mut bytes = header.to_vec();
    bytes.resize(12 + length, 0);
    stream
        .read_exact(&mut bytes[12..])
        .expect("complete FXLM payload");
    bytes
}

fn decode_server_control(frame: &[u8], opcode: u16) -> Value {
    assert!(frame.len() >= 12, "truncated server frame: {frame:?}");
    assert_eq!(&frame[..4], b"FXLM");
    assert_eq!(frame[4], 1);
    assert_eq!(frame[5], SERVER);
    let length = u32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
    assert_eq!(frame.len(), 12 + length);
    let actual = u16::from_be_bytes([frame[6], frame[7]]);
    assert_eq!(
        actual,
        opcode,
        "unexpected server opcode with payload {}",
        String::from_utf8_lossy(&frame[12..])
    );
    serde_json::from_slice(&frame[12..]).expect("canonical server control JSON")
}

fn frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12 + payload.len());
    bytes.extend_from_slice(b"FXLM");
    bytes.extend_from_slice(&[1, direction]);
    bytes.extend_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn read_to_eof_before(file: &std::fs::File, budget: Duration) -> Vec<u8> {
    let deadline = Instant::now() + budget;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        assert!(
            poll_readable(file.as_raw_fd(), deadline),
            "response deadline"
        );
        // SAFETY: the live file descriptor writes only within the output buffer.
        let read = unsafe { libc::read(file.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
        if read == 0 {
            return bytes;
        }
        assert!(
            read > 0,
            "response read: {}",
            std::io::Error::last_os_error()
        );
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
}

fn poll_readable(descriptor: RawFd, deadline: Instant) -> bool {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let remaining = deadline.duration_since(now);
        let milliseconds = remaining.as_millis().min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd: descriptor,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        // SAFETY: poll references one live descriptor record for a bounded duration.
        let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
        if result > 0 {
            return true;
        }
        if result == 0 {
            return false;
        }
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::Interrupted,
            "response poll"
        );
    }
}

fn wait_before(child: &mut Child, budget: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(status) = child.try_wait().expect("child state") {
            return status;
        }
        assert!(Instant::now() < deadline, "child exit deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn drain_terminal(file: &std::fs::File) -> Vec<u8> {
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = unsafe { libc::read(file.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
        if read > 0 {
            bytes.extend_from_slice(&chunk[..read as usize]);
            continue;
        }
        if read == 0 {
            return bytes;
        }
        let error = std::io::Error::last_os_error();
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock)
            || error.raw_os_error() == Some(libc::EIO)
        {
            return bytes;
        }
        panic!("terminal transcript read: {error}");
    }
}

fn assert_tree_excludes_except_credentials(root: &Path, needle: &[u8]) {
    let credentials = root.join("credentials");
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path == credentials {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).expect("value-free metadata");
        if metadata.is_dir() {
            pending.extend(
                std::fs::read_dir(path)
                    .expect("value-free directory")
                    .map(|entry| entry.expect("value-free entry").path()),
            );
        } else if metadata.is_file() {
            let bytes = std::fs::read(path).expect("value-free file");
            assert_excludes(&bytes, needle, "persisted value-free state");
        }
    }
}

fn assert_excludes(haystack: &[u8], needle: &[u8], surface: &str) {
    assert!(
        !haystack
            .windows(needle.len())
            .any(|candidate| candidate == needle),
        "secret sentinel crossed into {surface}"
    );
}

fn duplicate_high(descriptor: RawFd) -> RawFd {
    // SAFETY: F_DUPFD_CLOEXEC creates a distinct owned descriptor or returns -1.
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 32) };
    assert!(duplicate >= 32, "duplicate inherited capability");
    duplicate
}

fn close_raw(descriptor: RawFd) {
    // SAFETY: each fixture-owned raw duplicate closes exactly once.
    unsafe {
        libc::close(descriptor);
    }
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
