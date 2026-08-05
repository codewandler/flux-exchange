#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "native-root-test-seam"
))]

use std::collections::BTreeSet;
use std::io::Read;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

const HELPER_EXIT_FRAME_WRITTEN: i32 = 0;
const FXSA_HEADER_BYTES: usize = 12;
const MAX_FXSA_TOKEN_BYTES: usize = 512;

#[test]
fn supervised_unix_service_account_mint_transfers_one_fxsa_frame_and_no_other_capability() {
    let fixture = Fixture::new();
    let mut server = SupervisedServer::spawn(&fixture.state);
    let endpoint = fixture.state.join("run/local-management-v1.sock");
    let endpoint_metadata = std::fs::symlink_metadata(&endpoint)
        .expect("owner local-management endpoint exists before helper launch");
    assert!(
        endpoint_metadata.file_type().is_socket(),
        "local-management endpoint is a native socket"
    );
    assert_eq!(endpoint_metadata.permissions().mode() & 0o7777, 0o600);

    let expires_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("fixture clock after Unix epoch")
        .as_secs()
        .checked_add(300)
        .expect("bounded expiry");
    let mut helper = MintHelper::spawn(&fixture.state, expires_at);

    let token = read_one_fxsa_frame(&helper.fxsa, Duration::from_secs(5));
    let canary = read_to_eof_before(&helper.canary, Duration::from_secs(5), "canary closure");
    assert!(canary.is_empty(), "unrelated capability received bytes");
    let helper_status = wait_before(&mut helper.child, Duration::from_secs(5), "helper exit");
    assert_eq!(helper_status.code(), Some(HELPER_EXIT_FRAME_WRITTEN));

    let persisted = fixture.state.join("service-accounts/store.json");
    wait_for_path(&persisted, Duration::from_secs(5), "durable MINT receipt");
    let stored_bytes = std::fs::read(&persisted).expect("durable Service Account image");
    assert_closed_committed_mint(&stored_bytes, expires_at);
    assert_absent(&stored_bytes, &token, "durable Service Account image");

    server.finish(&token);
    assert_tree_absent(&fixture.state, &token);
}

struct Fixture {
    root: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-unix-fxsa-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("fixture owner directory");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture directory");
        let state = root.join("state");
        std::fs::create_dir(&state).expect("fixture state root");
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture state root");
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
        // SAFETY: the live output array receives two new descriptors on success.
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        // SAFETY: successful pipe creation transfers each distinct descriptor to this fixture.
        let read = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
        // SAFETY: as above, this is the distinct write endpoint.
        let write = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
        Self { read, write }
    }
}

struct SupervisedServer {
    child: Child,
    liveness: Option<OwnedFd>,
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
            .env_clear()
            .env("USER", "x134-owner")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "info")
            .env("FLUX_EXCHANGE_STATE", state)
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", state)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: the closure uses only descriptor operations before exec and retains exactly the
        // supervisor's readiness/liveness ABI beyond the three standard streams.
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
        let mut child = command.spawn().expect("real supervised Exchange process");
        close_raw(readiness_source);
        close_raw(liveness_source);
        drop(readiness.write);
        drop(liveness.read);
        let readiness = std::fs::File::from(readiness.read);
        let ready = read_supervised_readiness(&mut child, &readiness);
        assert!(
            !ready.is_empty(),
            "Exchange refused before readiness: {}",
            child_diagnostics(&mut child)
        );
        serde_json::from_slice::<Value>(&ready).expect("canonical supervised readiness JSON");
        Self {
            child,
            liveness: Some(liveness.write),
        }
    }

    fn finish(&mut self, token: &[u8]) {
        drop(self.liveness.take());
        let status = wait_before(&mut self.child, Duration::from_secs(5), "server exit");
        let stdout = read_child_output(self.child.stdout.take(), "server stdout");
        let stderr = read_child_output(self.child.stderr.take(), "server stderr");
        assert_absent(&stdout, token, "server stdout");
        assert_absent(&stderr, token, "server stderr");
        assert_eq!(
            status.code(),
            Some(1),
            "native liveness EOF must terminate with the supervised failure status; stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    }
}

impl Drop for SupervisedServer {
    fn drop(&mut self) {
        drop(self.liveness.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct MintHelper {
    child: Child,
    fxsa: std::fs::File,
    canary: std::fs::File,
}

impl MintHelper {
    fn spawn(state: &Path, expires_at: u64) -> Self {
        let fxsa = Pipe::new();
        let canary = Pipe::new();
        let writer_source = duplicate_high(fxsa.write.as_raw_fd());
        let canary_source = duplicate_high(canary.write.as_raw_fd());
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("local")
            .arg("service-account-mint")
            .arg("--id")
            .arg("native-worker")
            .arg("--expires-at")
            .arg(expires_at.to_string())
            .arg("--writer-fd")
            .arg("5")
            .env_clear()
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: the helper receives only write FD5. FD8 is planted pre-exec with CLOEXEC as the
        // unrelated-capability canary; the kernel must remove it while entering the real binary.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(writer_source, 5) < 0 || libc::dup2(canary_source, 8) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let flags = libc::fcntl(8, libc::F_GETFD);
                if flags < 0 || libc::fcntl(8, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for descriptor in 3..256 {
                    if descriptor != 5 && descriptor != 8 {
                        libc::close(descriptor);
                    }
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .expect("released Unix Service Account helper");
        close_raw(writer_source);
        close_raw(canary_source);
        drop(fxsa.write);
        drop(canary.write);
        Self {
            child,
            fxsa: std::fs::File::from(fxsa.read),
            canary: std::fs::File::from(canary.read),
        }
    }
}

impl Drop for MintHelper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_one_fxsa_frame(file: &std::fs::File, budget: Duration) -> Vec<u8> {
    let deadline = Instant::now() + budget;
    let mut header = [0_u8; FXSA_HEADER_BYTES];
    read_exact_before(file, &mut header, deadline, "FXSA header");
    assert_eq!(&header[..4], b"FXSA");
    assert_eq!(header[4], 1, "FXSA version");
    assert_eq!(header[5], 1, "FXSA server-to-client direction");
    assert_eq!(&header[6..8], &[0, 0], "FXSA reserved flags");
    let length = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    assert!(
        (1..=MAX_FXSA_TOKEN_BYTES).contains(&length),
        "FXSA payload length is inside the closed bound"
    );
    let mut token = vec![0_u8; length];
    read_exact_before(file, &mut token, deadline, "FXSA payload");
    let mut surplus = [0_u8; 1];
    assert_eq!(
        read_before(file, &mut surplus, deadline, "FXSA writer closure"),
        0,
        "FXSA stream carried surplus bytes or remained open"
    );
    token
}

fn assert_closed_committed_mint(bytes: &[u8], expires_at: u64) {
    let stored: Value = serde_json::from_slice(bytes).expect("Service Account store JSON");
    let root = stored
        .as_object()
        .expect("closed Service Account store object");
    assert_eq!(
        root.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["agents", "mint_receipts", "version"])
    );
    assert_eq!(root["version"], 2);
    assert_eq!(root["agents"].as_object().expect("one verifier").len(), 1);
    let receipts = root["mint_receipts"]
        .as_object()
        .expect("one durable mint receipt");
    assert_eq!(receipts.len(), 1);
    let (receipt_id, receipt) = receipts.iter().next().expect("terminal mint receipt");
    assert!(
        receipt_id.len() == 64
            && receipt_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && receipt_id.bytes().any(|byte| byte != b'0'),
        "receipt identity is nonzero 64-lowerhex"
    );
    let receipt = receipt.as_object().expect("closed receipt object");
    assert_eq!(
        receipt.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["expires_at", "id", "proposal_identity", "state", "tenant",])
    );
    assert_eq!(receipt["tenant"], "local");
    assert_eq!(receipt["id"], "native-worker");
    assert_eq!(receipt["expires_at"], expires_at);
    assert_eq!(receipt["state"], "committed");
}

fn read_child_output(output: Option<impl Read>, label: &str) -> Vec<u8> {
    let mut output = output.unwrap_or_else(|| panic!("captured {label}"));
    let mut bytes = Vec::new();
    output
        .read_to_end(&mut bytes)
        .unwrap_or_else(|_| panic!("read {label}"));
    bytes
}

fn read_supervised_readiness(child: &mut Child, file: &std::fs::File) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let status = child.wait().ok();
            panic!(
                "supervised readiness missed its 30s startup deadline: status={status:?} {}",
                child_diagnostics(child)
            );
        }
        let timeout = deadline
            .duration_since(now)
            .as_millis()
            .min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        // SAFETY: poll receives one live readiness descriptor and a bounded timeout.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready == 0 {
            continue;
        }
        assert!(ready > 0, "supervised readiness poll refused");
        // SAFETY: read writes at most the live chunk length.
        let received =
            unsafe { libc::read(file.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
        if received == 0 {
            return bytes;
        }
        assert!(received > 0, "supervised readiness read refused");
        bytes.extend_from_slice(&chunk[..received as usize]);
    }
}

fn child_diagnostics(child: &mut Child) -> String {
    if child.try_wait().ok().flatten().is_none() {
        return "process is still live".to_owned();
    }
    let stdout = read_child_output(child.stdout.take(), "server stdout");
    let stderr = read_child_output(child.stderr.take(), "server stderr");
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    )
}

fn read_to_eof_before(file: &std::fs::File, budget: Duration, label: &str) -> Vec<u8> {
    let deadline = Instant::now() + budget;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let received = read_before(file, &mut chunk, deadline, label);
        if received == 0 {
            return bytes;
        }
        bytes.extend_from_slice(&chunk[..received]);
    }
}

fn read_exact_before(file: &std::fs::File, mut output: &mut [u8], deadline: Instant, label: &str) {
    while !output.is_empty() {
        let received = read_before(file, output, deadline, label);
        assert!(received > 0, "{label} closed before completion");
        output = &mut output[received..];
    }
}

fn read_before(file: &std::fs::File, output: &mut [u8], deadline: Instant, label: &str) -> usize {
    loop {
        let now = Instant::now();
        assert!(now < deadline, "{label} exceeded its bounded wait");
        let timeout = deadline
            .duration_since(now)
            .as_millis()
            .min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        // SAFETY: poll receives one live descriptor record and a bounded timeout.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready == 0 {
            panic!("{label} exceeded its bounded wait");
        }
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            panic!("{label} poll refused");
        }
        // SAFETY: read writes at most the live output slice length.
        let received =
            unsafe { libc::read(file.as_raw_fd(), output.as_mut_ptr().cast(), output.len()) };
        if received >= 0 {
            return received as usize;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            panic!("{label} read refused");
        }
    }
}

fn wait_before(child: &mut Child, budget: Duration, label: &str) -> std::process::ExitStatus {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(status) = child.try_wait().expect("child state") {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "{label} exceeded its bounded wait"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_path(path: &Path, budget: Duration, label: &str) {
    let deadline = Instant::now() + budget;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{label} exceeded its bounded wait"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn duplicate_high(descriptor: RawFd) -> RawFd {
    // SAFETY: F_DUPFD_CLOEXEC duplicates the live descriptor at or above the chosen floor.
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 32) };
    assert!(duplicate >= 32, "high descriptor duplicate");
    duplicate
}

fn close_raw(descriptor: RawFd) {
    // SAFETY: each raw duplicate is closed exactly once by its parent after spawn.
    assert_eq!(unsafe { libc::close(descriptor) }, 0);
}

fn assert_absent(haystack: &[u8], needle: &[u8], label: &str) {
    assert!(
        !haystack
            .windows(needle.len())
            .any(|candidate| candidate == needle),
        "secret bytes reached {label}"
    );
}

fn assert_tree_absent(root: &Path, needle: &[u8]) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).expect("persisted metadata");
        if metadata.is_dir() {
            pending.extend(
                std::fs::read_dir(path)
                    .expect("persisted directory")
                    .map(|entry| entry.expect("persisted entry").path()),
            );
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).expect("persisted file");
            assert_absent(&bytes, needle, "persisted output");
        }
    }
}
