use std::io::{Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const STORE_SETTINGS: [&str; 8] = [
    "FLUX_EXCHANGE_CREDENTIALS",
    "FLUX_EXCHANGE_SETTINGS",
    "FLUX_EXCHANGE_GRANTS",
    "FLUX_EXCHANGE_CONNECTIONS",
    "FLUX_EXCHANGE_CHANNELS",
    "FLUX_EXCHANGE_WORKFLOWS",
    "FLUX_EXCHANGE_AUDIT",
    "FLUX_EXCHANGE_SERVICE_ACCOUNTS",
];

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    next_log: usize,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("fixture clock after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-sentinel-{}-{nonce}",
            std::process::id()
        ));
        exchange_host::ensure_private_state_directory(&root)
            .expect("owner-only sentinel fixture root");
        let state = root.join("state");
        exchange_host::ensure_private_state_directory(&state)
            .expect("owner-only sentinel state root");
        Self {
            root,
            state,
            next_log: 0,
        }
    }

    fn spawn(&mut self) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve loopback fixture port");
        let address = listener.local_addr().expect("reserved loopback address");
        drop(listener);

        let log_path = self.root.join(format!("server-{}.log", self.next_log));
        self.next_log += 1;
        let log = std::fs::File::create(&log_path).expect("server diagnostic capture");
        let stderr = log.try_clone().expect("shared diagnostic capture");
        let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
        command
            .arg("--dev")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "info")
            .env("USER", "x134-sentinel-owner")
            .env("FLUX_EXCHANGE_BIND", address.to_string())
            .env("FLUX_EXCHANGE_STATE", &self.state)
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr));
        for setting in STORE_SETTINGS {
            command.env_remove(setting);
        }
        for identity in [
            "FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS",
            "FLUX_EXCHANGE_APPS",
            "FLUX_EXCHANGE_CONSOLE",
            "FLUX_EXCHANGE_CONSOLE_ORIGIN",
            "FLUX_EXCHANGE_DEV_IDENTITY",
            "FLUX_EXCHANGE_LEGACY_WRITER_CHILD",
            "FLUX_EXCHANGE_LEGACY_WRITER_READY",
            "FLUX_EXCHANGE_LEGACY_WRITER_RELEASE",
            "FLUX_EXCHANGE_LEGACY_WRITER_STORE",
            "FLUX_EXCHANGE_LOCAL_USERS",
            "FLUX_EXCHANGE_OPERATOR_SUBJECTS",
            "FLUX_EXCHANGE_TENANT",
            "FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_PEER_UID",
            "FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT",
            "FLUX_EXCHANGE_TEST_OCCUPIED_BIND",
            "FLUX_EXCHANGE_TEST_WEDGE_AFTER_READY",
            "FLUX_EXCHANGE_OIDC_ISSUER",
            "FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT",
            "FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT",
            "FLUX_EXCHANGE_OIDC_JWKS_URI",
            "FLUX_EXCHANGE_OIDC_CLIENT_ID",
            "FLUX_EXCHANGE_OIDC_CLIENT_SECRET",
            "FLUX_EXCHANGE_OIDC_REDIRECT_URI",
            "FLUX_EXCHANGE_OIDC_TENANT",
            "FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN",
        ] {
            command.env_remove(identity);
        }
        let child = command.spawn().expect("real development Exchange process");
        let mut server = Server {
            child,
            address,
            log_path,
        };
        server.wait_until_listening();
        server
    }

    fn assert_all_outputs_exclude(&self, forms: &[Vec<u8>]) {
        let mut pending = vec![self.root.clone()];
        while let Some(path) = pending.pop() {
            let metadata = std::fs::symlink_metadata(&path).expect("sentinel fixture metadata");
            if metadata.is_dir() {
                for entry in std::fs::read_dir(&path).expect("sentinel fixture directory") {
                    pending.push(entry.expect("sentinel fixture entry").path());
                }
            } else if metadata.is_file() {
                let bytes = std::fs::read(&path).expect("sentinel fixture output");
                assert_forms_absent(&bytes, forms, &path);
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Server {
    child: Child,
    address: SocketAddr,
    log_path: PathBuf,
}

impl Server {
    fn wait_until_listening(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if TcpStream::connect_timeout(&self.address, Duration::from_millis(100)).is_ok() {
                return;
            }
            assert!(
                self.child.try_wait().expect("server child state").is_none(),
                "development Exchange exited before listening"
            );
            assert!(
                Instant::now() < deadline,
                "development Exchange did not bind its loopback listener"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn owner_cookie(&self) -> String {
        let response = request(self.address, "POST", "/api/signin", &[], b"");
        assert!(
            matches!(status(&response), 200 | 303),
            "development sign-in did not admit the local owner"
        );
        let headers = response_headers(&response);
        headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("set-cookie")
                    .then(|| value.trim_start())
            })
            .and_then(|value| value.split(';').next())
            .map(str::to_owned)
            .expect("development sign-in session cookie")
    }

    fn refuse_secret_json(&self, cookie: &str, bytes: &[u8], forms: &[Vec<u8>]) -> Vec<u8> {
        let percent = std::str::from_utf8(&forms[2]).expect("ASCII percent sentinel");
        let base64 = std::str::from_utf8(&forms[3]).expect("ASCII base64 sentinel");
        let path = format!("/api/connections/intercom/plan?attempt={percent}");
        let response = request(
            self.address,
            "POST",
            &path,
            &[
                ("Content-Type", "application/json"),
                ("Cookie", cookie),
                ("X-X134-Adversary", base64),
            ],
            bytes,
        );
        assert_eq!(
            status(&response),
            415,
            "secret-bearing connection JSON did not fail before decoding"
        );
        assert_eq!(
            response_body(&response),
            br#"{"code":"secret_json_forbidden"}"#,
            "secret-bearing connection JSON did not return the closed refusal"
        );
        response
    }

    fn abort_secret_json(&self, cookie: &str, bytes: &[u8]) {
        let mut stream = connect(self.address);
        let header = format!(
            "POST /api/connections/intercom/plan HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nCookie: {cookie}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.address,
            bytes.len() + 4096
        );
        stream
            .write_all(header.as_bytes())
            .expect("aborted request header");
        stream.write_all(bytes).expect("aborted request bytes");
        stream.flush().expect("flush aborted request bytes");
        let _ = stream.shutdown(Shutdown::Both);
    }

    fn crash(mut self) -> PathBuf {
        self.child.kill().expect("abrupt Exchange termination");
        self.child.wait().expect("reap crashed Exchange");
        self.log_path.clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn transformed_secret_sentinels_never_enter_refusal_abort_crash_or_restart_outputs() {
    let mut fixture = Fixture::new();
    let raw = format!(
        "X134-sensitive-{}-quote\"-slash\\-newline\n-end",
        std::process::id()
    )
    .into_bytes();
    let forms = transformed_forms(&raw);
    for (index, form) in forms.iter().enumerate() {
        assert!(!form.is_empty(), "sentinel representation {index} is empty");
        assert!(
            forms[..index].iter().all(|prior| prior != form),
            "sentinel representation {index} duplicates an earlier encoding"
        );
    }

    let first = fixture.spawn();
    let cookie = first.owner_cookie();
    for form in &forms {
        let response = first.refuse_secret_json(&cookie, form, &forms);
        assert_forms_absent(&response, &forms, Path::new("HTTP refusal"));
    }

    first.abort_secret_json(&cookie, &raw);
    std::thread::sleep(Duration::from_millis(50));

    let mut in_flight = connect(first.address);
    let crash_query = std::str::from_utf8(&forms[2]).expect("ASCII crash query sentinel");
    let crash_header = std::str::from_utf8(&forms[3]).expect("ASCII crash header sentinel");
    let header = format!(
        "POST /api/connections/intercom/plan?attempt={crash_query} HTTP/1.1\r\nHost: {}\r\nCookie: {cookie}\r\nX-X134-Crash: {crash_header}",
        first.address
    );
    in_flight
        .write_all(header.as_bytes())
        .expect("incomplete crash request headers");
    in_flight.flush().expect("flush crash request bytes");
    let first_log = first.crash();
    drop(in_flight);
    assert_forms_absent(
        &std::fs::read(first_log).expect("first process diagnostics"),
        &forms,
        Path::new("first process diagnostics"),
    );

    let restarted = fixture.spawn();
    let restarted_cookie = restarted.owner_cookie();
    let response = restarted.refuse_secret_json(&restarted_cookie, &forms[2], &forms);
    assert_forms_absent(&response, &forms, Path::new("restart refusal"));
    let restarted_log = restarted.crash();
    assert_forms_absent(
        &std::fs::read(restarted_log).expect("restarted process diagnostics"),
        &forms,
        Path::new("restarted process diagnostics"),
    );

    fixture.assert_all_outputs_exclude(&forms);
}

fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut stream = connect(address);
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("HTTP request headers");
    stream.write_all(body).expect("HTTP request body");
    stream.flush().expect("flush HTTP request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("complete HTTP response");
    response
}

fn connect(address: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("connect to real Exchange process");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bounded response read");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("bounded request write");
    stream
}

fn status(response: &[u8]) -> u16 {
    let line = response
        .split(|byte| *byte == b'\n')
        .next()
        .expect("HTTP status line");
    let line = std::str::from_utf8(line).expect("ASCII HTTP status line");
    line.split_ascii_whitespace()
        .nth(1)
        .expect("HTTP status code")
        .parse()
        .expect("numeric HTTP status code")
}

fn response_headers(response: &[u8]) -> &str {
    let boundary = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("HTTP header terminator");
    std::str::from_utf8(&response[..boundary]).expect("ASCII HTTP headers")
}

fn response_body(response: &[u8]) -> &[u8] {
    let boundary = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("HTTP header terminator");
    &response[boundary + 4..]
}

fn transformed_forms(raw: &[u8]) -> Vec<Vec<u8>> {
    let text = std::str::from_utf8(raw).expect("fixture sentinel is UTF-8");
    let json = serde_json::to_string(text).expect("JSON sentinel encoding");
    vec![
        raw.to_vec(),
        json.as_bytes()[1..json.len() - 1].to_vec(),
        percent_encode(raw),
        base64(raw),
    ]
}

fn percent_encode(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = Vec::with_capacity(bytes.len() * 3);
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte);
        } else {
            encoded.push(b'%');
            encoded.push(HEX[usize::from(byte >> 4)]);
            encoded.push(HEX[usize::from(byte & 0x0f)]);
        }
    }
    encoded
}

fn base64(bytes: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(ALPHABET[usize::from(first >> 2)]);
        encoded.push(ALPHABET[usize::from(((first & 0x03) << 4) | (second >> 4))]);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[usize::from(((second & 0x0f) << 2) | (third >> 6))]
        } else {
            b'='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[usize::from(third & 0x3f)]
        } else {
            b'='
        });
    }
    encoded
}

fn assert_forms_absent(bytes: &[u8], forms: &[Vec<u8>], surface: &Path) {
    for (index, form) in forms.iter().enumerate() {
        assert!(
            !bytes
                .windows(form.len())
                .any(|candidate| candidate == form.as_slice()),
            "secret representation {index} entered value-free surface {}",
            surface.display()
        );
    }
}
