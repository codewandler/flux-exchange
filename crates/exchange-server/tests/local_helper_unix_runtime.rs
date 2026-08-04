#![cfg(unix)]
#![allow(dead_code)]

#[path = "../src/local_helper.rs"]
mod local_helper;
#[path = "../src/local_helper_plan.rs"]
mod local_helper_plan;
#[path = "../src/local_helper_unix.rs"]
mod local_helper_unix;
#[path = "support/local_helper_unix_runtime_shim.rs"]
mod local_management;
#[path = "../src/native_root.rs"]
mod native_root;

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use local_helper::{
    parse_local_helper, HelperDeadlineSchedule, HelperExit, HelperPlatform, LocalHelperInvocation,
    MintWriterCapability,
};
use local_helper_unix::{MintTransfer, PinnedEndpoint, VendorCeremony, VendorRequest};

const WORKER_ENV: &str = "FLUX_TEST_X134_UNIX_HELPER_WORKER";
const ROOT_ENV: &str = "FLUX_TEST_X134_UNIX_HELPER_ROOT";
const MODE_ENV: &str = "FLUX_TEST_X134_UNIX_HELPER_MODE";

#[test]
fn exact_vendor_fd_set_forwards_one_split_frame_and_is_silent() {
    let fixture = EndpointFixture::new();
    let request = client_frame(0x0001, br#"{"authorities":[],"connector":"demo","label":"main","plan_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","settings":[],"targets":[]}"#);
    let expected = server_frame(0x0006, br#"{"commit":{"audit":"committed","resource":"committed"},"connector":"demo","label":"main","operation":"connect","receipt_id":"1111111111111111111111111111111111111111111111111111111111111111","replayed":false,"schema":"exchange.connect-receipt.v1"}"#);

    let mut child = HelperChild::spawn(&fixture.root, "vendor", CapabilityLayout::Closed);
    for byte in &request {
        child
            .request
            .write_all(&[*byte])
            .expect("split request byte");
        std::thread::sleep(Duration::from_micros(200));
    }
    drop(child.request);
    fixture.accept_one();

    let mut response = Vec::new();
    child
        .response
        .read_to_end(&mut response)
        .expect("response EOF");
    let output = child.process.wait_with_output().expect("helper exits");
    assert_eq!(
        output.status.code(),
        Some(HelperExit::TerminalFrameWritten.code().into())
    );
    assert_eq!(response, expected);
    assert!(output.stdout.is_empty(), "helper stdout must remain empty");
    assert!(output.stderr.is_empty(), "helper stderr must remain empty");
}

#[test]
fn planted_fd5_or_fd8_refuses_before_reading_or_endpoint_connection() {
    for layout in [CapabilityLayout::Fd5Planted, CapabilityLayout::Fd8Planted] {
        let fixture = EndpointFixture::new();
        let mut child = HelperChild::spawn(&fixture.root, "vendor", layout);
        drop(child.request);
        let mut response = Vec::new();
        child
            .response
            .read_to_end(&mut response)
            .expect("response EOF");
        let output = child.process.wait_with_output().expect("helper exits");
        assert_eq!(
            output.status.code(),
            Some(HelperExit::CapabilityOrTransportFailure.code().into())
        );
        assert!(response.is_empty());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        fixture.assert_no_connection();
    }
}

#[test]
fn swapped_pipe_directions_refuse_as_capability_failure() {
    for layout in [CapabilityLayout::Swapped, CapabilityLayout::SamePipe] {
        let fixture = EndpointFixture::new();
        let mut child = HelperChild::spawn(&fixture.root, "vendor", layout);
        drop(child.request);
        let mut response = Vec::new();
        child
            .response
            .read_to_end(&mut response)
            .expect("response EOF");
        let output = child.process.wait_with_output().expect("helper exits");
        assert_eq!(
            output.status.code(),
            Some(HelperExit::CapabilityOrTransportFailure.code().into())
        );
        assert!(response.is_empty());
        fixture.assert_no_connection();
    }
}

#[test]
fn coalesced_second_frame_is_surplus_and_never_reaches_the_ceremony() {
    let fixture = EndpointFixture::new();
    let first = client_frame(0x0001, b"{}");
    let second = client_frame(0x0030, b"{}");
    let mut child = HelperChild::spawn(&fixture.root, "vendor", CapabilityLayout::Closed);
    child.request.write_all(&first).expect("first frame");
    child.request.write_all(&second).expect("coalesced frame");
    drop(child.request);

    let mut response = Vec::new();
    child
        .response
        .read_to_end(&mut response)
        .expect("response EOF");
    let output = child.process.wait_with_output().expect("helper exits");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        response,
        server_frame(
            0x7fff,
            br#"{"code":"surplus_data","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#,
        )
    );
    fixture.assert_no_connection();
}

#[test]
fn complete_request_without_eof_reaches_the_absolute_read_deadline_value_free() {
    let fixture = EndpointFixture::new();
    let mut child = HelperChild::spawn(&fixture.root, "deadline", CapabilityLayout::Closed);
    child
        .request
        .write_all(&client_frame(0x0001, b"{}"))
        .expect("complete frame without EOF");

    let mut response = Vec::new();
    child
        .response
        .read_to_end(&mut response)
        .expect("deadline response EOF");
    let output = child.process.wait_with_output().expect("helper exits");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        response,
        server_frame(
            0x7fff,
            br#"{"code":"deadline_exceeded","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":408}"#,
        )
    );
    fixture.assert_no_connection();
}

#[test]
fn oversized_declared_request_refuses_without_reading_its_body() {
    let fixture = EndpointFixture::new();
    let mut child = HelperChild::spawn(&fixture.root, "vendor", CapabilityLayout::Closed);
    let mut header = b"FXLM".to_vec();
    header.extend_from_slice(&[1, 1]);
    header.extend_from_slice(&0x0001_u16.to_be_bytes());
    header.extend_from_slice(&65_537_u32.to_be_bytes());
    child.request.write_all(&header).expect("oversized header");

    let mut response = Vec::new();
    child
        .response
        .read_to_end(&mut response)
        .expect("response EOF");
    let output = child.process.wait_with_output().expect("helper exits");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        response,
        server_frame(
            0x7fff,
            br#"{"code":"frame_too_large","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":413}"#,
        )
    );
    fixture.assert_no_connection();
}

#[test]
fn terminal_response_write_and_eof_share_the_absolute_result_deadline() {
    let (read, write) = pipe();
    let flags = unsafe { libc::fcntl(write.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_ne!(
        unsafe { libc::fcntl(write.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        -1
    );
    let fill = [0x5a_u8; 4096];
    loop {
        let written = unsafe { libc::write(write.as_raw_fd(), fill.as_ptr().cast(), fill.len()) };
        if written > 0 {
            continue;
        }
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::WouldBlock
        );
        break;
    }
    let started = std::time::Instant::now();
    let exit = local_helper_unix::finish_response_before_for_test(
        write,
        server_frame(0x7fff, br#"{"code":"deadline_exceeded"}"#),
        started + Duration::from_millis(25),
    );
    assert!(
        exit == HelperExit::CapabilityOrTransportFailure,
        "a blocked response cannot be reported as frame+EOF"
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut drained = Vec::new();
    File::from(read)
        .read_to_end(&mut drained)
        .expect("writer closure produces EOF after queued bytes");
    assert!(!drained.is_empty());
}

#[test]
fn service_account_mode_builds_exact_mint_frame_for_the_fd5_transfer_seam() {
    let fixture = EndpointFixture::new();
    let invocation = parse_local_helper(
        HelperPlatform::Unix,
        &[
            "local",
            "service-account-mint",
            "--id",
            "worker_1",
            "--expires-at",
            "2147483647",
            "--writer-fd",
            "5",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>(),
    )
    .expect("closed mint argv");
    let LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::UnixFd5,
    } = invocation
    else {
        panic!("expected Unix mint invocation");
    };
    let expected = client_frame(0x0020, br#"{"expires_at":"2147483647","id":"worker_1"}"#);
    let mut observed = Vec::new();
    let exit = local_helper_unix::run_mint_at_for_test(
        &fixture.root,
        &id,
        expires_at,
        CaptureMint(&mut observed),
        Duration::from_secs(1),
    );
    fixture.accept_one();
    assert_eq!(exit.code(), 0);
    assert_eq!(observed, expected);
}

#[test]
fn unix_helper_worker() {
    let Some(mode) = std::env::var_os(WORKER_ENV) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("worker root"));
    let timeout = if std::env::var_os(MODE_ENV).as_deref() == Some("deadline".as_ref()) {
        Duration::from_millis(40)
    } else {
        Duration::from_secs(1)
    };
    let mut ceremony = ConnectAndReturn {
        response: server_frame(0x0006, br#"{"commit":{"audit":"committed","resource":"committed"},"connector":"demo","label":"main","operation":"connect","receipt_id":"1111111111111111111111111111111111111111111111111111111111111111","replayed":false,"schema":"exchange.connect-receipt.v1"}"#),
    };
    let exit = local_helper_unix::run_vendor_at_for_test(&root, &mut ceremony, timeout, timeout);
    drop(mode);
    std::process::exit(exit.code().into());
}

struct ConnectAndReturn {
    response: Vec<u8>,
}

impl VendorCeremony for ConnectAndReturn {
    type Error = ();

    fn execute(
        &mut self,
        endpoint: &PinnedEndpoint,
        _request: &VendorRequest,
        deadlines: HelperDeadlineSchedule,
    ) -> Result<Vec<u8>, Self::Error> {
        let stream = endpoint
            .connect_before(deadlines.setup_by())
            .map_err(|_| ())?;
        drop(stream);
        Ok(self.response.clone())
    }
}

struct CaptureMint<'a>(&'a mut Vec<u8>);

impl MintTransfer for CaptureMint<'_> {
    type Error = ();

    fn transfer(
        self,
        stream: &std::os::unix::net::UnixStream,
        mint_frame: &[u8],
    ) -> Result<(), Self::Error> {
        self.0.extend_from_slice(mint_frame);
        drop(stream.try_clone().map_err(|_| ())?);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum CapabilityLayout {
    Closed,
    Fd5Planted,
    Fd8Planted,
    Swapped,
    SamePipe,
}

struct HelperChild {
    process: std::process::Child,
    request: File,
    response: File,
}

impl HelperChild {
    fn spawn(root: &Path, mode: &str, layout: CapabilityLayout) -> Self {
        let (request_read, request_write) = pipe();
        let (response_read, response_write) = pipe();
        let planted = pipe();
        let request_source = duplicate_high(request_read.as_raw_fd());
        let request_write_source = duplicate_high(request_write.as_raw_fd());
        let response_source = duplicate_high(response_write.as_raw_fd());
        let planted_source = duplicate_high(planted.0.as_raw_fd());

        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command
            .arg("--quiet")
            .arg("--exact")
            .arg("unix_helper_worker")
            .env(WORKER_ENV, "1")
            .env(ROOT_ENV, root)
            .env(MODE_ENV, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                let (six, seven) = match layout {
                    CapabilityLayout::Swapped => (response_source, request_source),
                    CapabilityLayout::SamePipe => (request_source, request_write_source),
                    _ => (request_source, response_source),
                };
                if libc::dup2(six, 6) == -1 || libc::dup2(seven, 7) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if matches!(layout, CapabilityLayout::Fd5Planted)
                    && libc::dup2(planted_source, 5) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                if matches!(layout, CapabilityLayout::Fd8Planted)
                    && libc::dup2(planted_source, 8) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                for descriptor in 3..256 {
                    let keep = descriptor == 6
                        || descriptor == 7
                        || (descriptor == 5 && matches!(layout, CapabilityLayout::Fd5Planted))
                        || (descriptor == 8 && matches!(layout, CapabilityLayout::Fd8Planted));
                    if !keep {
                        libc::close(descriptor);
                    }
                }
                Ok(())
            });
        }
        let process = command.spawn().expect("spawn helper fixture");
        drop(request_read);
        drop(response_write);
        drop(planted);
        unsafe {
            libc::close(request_source);
            libc::close(request_write_source);
            libc::close(response_source);
            libc::close(planted_source);
        }
        Self {
            process,
            request: File::from(request_write),
            response: File::from(response_read),
        }
    }
}

struct EndpointFixture {
    root: PathBuf,
    listener: UnixListener,
}

impl EndpointFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-helper-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("root mode");
        let run = root.join("run");
        std::fs::create_dir(&run).expect("run");
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).expect("run mode");
        let socket = run.join("local-management-v1.sock");
        let listener = UnixListener::bind(&socket).expect("socket");
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .expect("socket mode");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        Self { root, listener }
    }

    fn accept_one(&self) {
        let start = std::time::Instant::now();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    drop(stream);
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        start.elapsed() < Duration::from_secs(2),
                        "helper did not connect"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    }

    fn assert_no_connection(&self) {
        match self.listener.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Ok(_) => panic!("refused request reached the endpoint"),
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

impl Drop for EndpointFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.root.join("run/local-management-v1.sock"));
        let _ = std::fs::remove_dir(self.root.join("run"));
        let _ = std::fs::remove_dir(&self.root);
    }
}

fn pipe() -> (OwnedFd, OwnedFd) {
    let mut descriptors = [-1; 2];
    assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
    for descriptor in descriptors {
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        assert_ne!(flags, -1);
        assert_ne!(
            unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) },
            -1
        );
    }
    unsafe {
        (
            OwnedFd::from_raw_fd(descriptors[0]),
            OwnedFd::from_raw_fd(descriptors[1]),
        )
    }
}

fn duplicate_high(descriptor: RawFd) -> RawFd {
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 64) };
    assert!(duplicate >= 64, "duplicate descriptor");
    duplicate
}

fn client_frame(opcode: u16, payload: &[u8]) -> Vec<u8> {
    frame(1, opcode, payload)
}

fn server_frame(opcode: u16, payload: &[u8]) -> Vec<u8> {
    frame(2, opcode, payload)
}

fn frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"FXLM".to_vec();
    bytes.push(1);
    bytes.push(direction);
    bytes.extend_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}
