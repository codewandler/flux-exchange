#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "native-root-test-seam"
))]

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;

struct Fixture {
    owner: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let owner = std::env::temp_dir().join(format!(
            "flux-exchange-x134-supervised-owner-plan-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        std::fs::create_dir(&owner).expect("fixture owner directory");
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture directory");
        let state = owner.join("state");
        std::fs::create_dir(&state).expect("fixture state directory");
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture state");
        Self { owner, state }
    }

    fn socket(&self) -> PathBuf {
        self.state.join("run/local-management-v1.sock")
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
        let mut descriptors = [-1; 2];
        // SAFETY: the live output array receives two newly owned descriptors on success.
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        Self {
            read: descriptors[0],
            write: descriptors[1],
        }
    }
}

struct SupervisedServer {
    child: Child,
    readiness_read: RawFd,
    liveness_write: RawFd,
}

impl SupervisedServer {
    fn spawn(fixture: &Fixture) -> Self {
        let readiness = PipeEnds::new();
        let liveness = PipeEnds::new();
        let readiness_source = duplicate_high(readiness.write);
        let liveness_source = duplicate_high(liveness.read);
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("--supervised")
            .env_clear()
            .env("USER", "x134-owner")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "info")
            .env("FLUX_EXCHANGE_STATE", &fixture.state)
            .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", &fixture.state)
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
        let child = command
            .spawn()
            .expect("genuine supervised Exchange process");
        close_fd(readiness.write);
        close_fd(liveness.read);
        close_fd(readiness_source);
        close_fd(liveness_source);
        let mut server = Self {
            child,
            readiness_read: readiness.read,
            liveness_write: liveness.write,
        };
        let readiness = server.readiness();
        assert!(
            !readiness.is_empty(),
            "supervised process refused before readiness: {}",
            server.diagnostics_if_exited()
        );
        let readiness: Value =
            serde_json::from_slice(&readiness).expect("canonical readiness JSON");
        assert_eq!(readiness["schema"], "exchange.supervisor-ready.v2");
        server
    }

    fn readiness(&mut self) -> Vec<u8> {
        let descriptor = std::mem::replace(&mut self.readiness_read, -1);
        // SAFETY: ownership of the readiness descriptor moves into this guard exactly once.
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        // Readiness hashes the real executable after binding every durable store. Keep this
        // bounded, but leave enough budget for an uncached or I/O-constrained native CI runner.
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            if !poll_readable(descriptor.as_raw_fd(), deadline) {
                panic!(
                    "supervised readiness missed its 30s startup deadline: {}",
                    self.stop_for_diagnostics()
                );
            }
            // SAFETY: descriptor remains live and read writes only inside the output buffer.
            let read = unsafe {
                libc::read(
                    descriptor.as_raw_fd(),
                    chunk.as_mut_ptr().cast(),
                    chunk.len(),
                )
            };
            if read == 0 {
                return bytes;
            }
            assert!(
                read > 0,
                "supervised readiness read: {}",
                std::io::Error::last_os_error()
            );
            bytes.extend_from_slice(&chunk[..read as usize]);
        }
    }

    fn owner_session(&mut self, socket: &Path) -> UnixStream {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match UnixStream::connect(socket) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .expect("bounded native read");
                    stream
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .expect("bounded native write");
                    return stream;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound
                            | std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    if let Some(status) = self.child.try_wait().expect("supervised child state") {
                        panic!(
                            "supervised process exited before the owner endpoint ({status}): {}",
                            self.diagnostics_if_exited()
                        );
                    }
                    assert!(
                        Instant::now() < deadline,
                        "supervised process never bound owner endpoint {}: {error}",
                        socket.display()
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!(
                    "supervised owner endpoint {} refused: {error}",
                    socket.display()
                ),
            }
        }
    }

    fn diagnostics_if_exited(&mut self) -> String {
        if self.child.try_wait().ok().flatten().is_none() {
            return "process is still live".to_owned();
        }
        let mut stdout = Vec::new();
        if let Some(mut stream) = self.child.stdout.take() {
            stream.read_to_end(&mut stdout).expect("supervised stdout");
        }
        let mut stderr = Vec::new();
        if let Some(mut stream) = self.child.stderr.take() {
            stream.read_to_end(&mut stderr).expect("supervised stderr");
        }
        format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        )
    }

    fn stop_for_diagnostics(&mut self) -> String {
        let _ = self.child.kill();
        let status = self.child.wait().ok();
        format!("status={status:?} {}", self.diagnostics_if_exited())
    }

    fn stop(mut self) {
        self.close_liveness();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .child
                .try_wait()
                .expect("supervised child state")
                .is_some()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "liveness EOF did not stop supervised Exchange"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.wait();
    }

    fn close_liveness(&mut self) {
        if self.liveness_write >= 0 {
            close_fd(self.liveness_write);
            self.liveness_write = -1;
        }
    }
}

impl Drop for SupervisedServer {
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

#[test]
fn supervised_owner_endpoint_serves_one_canonical_v2_plan() {
    let fixture = Fixture::new();
    let mut server = SupervisedServer::spawn(&fixture);
    let socket = fixture.socket();
    let mut owner = server.owner_session(&socket);

    let metadata = std::fs::symlink_metadata(&socket).expect("bound owner endpoint metadata");
    assert!(metadata.file_type().is_socket());
    // SAFETY: geteuid has no pointer arguments or preconditions.
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);

    let request = frame(
        CLIENT,
        PLAN_QUERY,
        br#"{"connector":"jira","selection":null}"#,
    );
    for chunk in request.chunks(5) {
        owner
            .write_all(chunk)
            .expect("split canonical PLAN request");
    }
    owner
        .shutdown(std::net::Shutdown::Write)
        .expect("finish one PLAN request");

    let mut header = [0_u8; 12];
    owner
        .read_exact(&mut header)
        .expect("bounded complete PLAN response header");
    assert_eq!(&header[..4], b"FXLM");
    assert_eq!(header[4], 1);
    assert_eq!(header[5], SERVER);
    let opcode = u16::from_be_bytes([header[6], header[7]]);
    let length = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    assert!(length <= 65_536, "bounded PLAN response");
    let mut payload = vec![0_u8; length];
    owner
        .read_exact(&mut payload)
        .expect("bounded complete PLAN response payload");
    assert_eq!(
        opcode,
        PLAN_RESPONSE,
        "the genuine owner endpoint returned a refusal: {}",
        String::from_utf8_lossy(&payload)
    );
    let mut trailing = [0_u8; 1];
    assert_eq!(
        owner.read(&mut trailing).expect("bounded response EOF"),
        0,
        "owner endpoint emitted more than one response frame"
    );

    let plan: Value = serde_json::from_slice(&payload).expect("canonical PLAN JSON");
    assert_eq!(
        serde_json::to_vec(&plan).expect("re-encode canonical PLAN"),
        payload,
        "owner endpoint PLAN payload is not byte-canonical"
    );
    let mut complete = header.to_vec();
    complete.extend_from_slice(&payload);
    assert_eq!(complete, frame(SERVER, PLAN_RESPONSE, &payload));
    assert_eq!(plan["version"], "exchange.connection-plan.v2");
    assert_eq!(plan["connector"], "jira");
    assert_eq!(plan["selection"], Value::Null);
    assert_eq!(plan["credential_revision"], Value::Null);
    assert_nonzero_lowerhex(plan["plan_revision"].as_str().expect("plan revision"));
    assert!(!plan["fields"].as_array().expect("plan fields").is_empty());
    assert_eq!(
        plan.as_object()
            .expect("closed plan object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "connector",
            "credential_revision",
            "fields",
            "labels",
            "plan_revision",
            "selection",
            "state",
            "vendor",
            "version",
        ])
    );

    server.stop();
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

fn duplicate_high(descriptor: RawFd) -> RawFd {
    // SAFETY: F_DUPFD_CLOEXEC creates one distinct owned descriptor or returns -1.
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 32) };
    assert!(duplicate >= 32, "duplicate inherited capability");
    duplicate
}

fn close_fd(descriptor: RawFd) {
    // SAFETY: fixture ownership closes each descriptor at most once.
    unsafe {
        libc::close(descriptor);
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
            "readiness poll"
        );
    }
}

fn assert_nonzero_lowerhex(value: &str) {
    assert_eq!(value.len(), 64);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert!(value.bytes().any(|byte| byte != b'0'));
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
