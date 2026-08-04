#![cfg(windows)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetProcessTimes,
    InitializeProcThreadAttributeList, OpenProcess, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTUPINFOEXW, STARTUPINFOW,
};

use flux_exchange::supervisor::{
    verify_readiness, ExpectedRelease, NativePlatform, ReadinessExpectation, VerifiedStartIdentity,
};

const SENTINELS: [(&str, &str); 6] = [
    ("X128_CREDENTIAL_VALUE", "credential-secret-7c96d9"),
    ("X128_SETTING_VALUE", "setting-value-7c96d9"),
    ("X128_GRANT_BODY", "grant-body-7c96d9"),
    (
        "X128_SERVICE_ACCOUNT_VERIFIER",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ),
    ("X128_SESSION_VALUE", "session-value-7c96d9"),
    ("X128_CONTROL_CREDENTIAL", "control-credential-7c96d9"),
];

fn exchange_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
}

fn assert_no_sentinels(bytes: &[u8]) {
    for (_, sentinel) in SENTINELS {
        assert!(!bytes
            .windows(sentinel.len())
            .any(|candidate| candidate == sentinel.as_bytes()));
    }
}

fn assert_no_authority_values(bytes: &[u8], values: &[String]) {
    for value in values {
        assert!(!bytes
            .windows(value.len())
            .any(|candidate| candidate == value.as_bytes()));
    }
}

fn seed_production_authority_stores(root: &std::path::Path) -> Vec<String> {
    use exchange_host::{ConnectionSettings, Grants};

    let tenant = exchange_host::Tenant::new("dev").expect("sentinel tenant");
    let credential = exchange_host::CredentialStore::bind(root.join("credentials/store.txt"))
        .expect("production credential store");
    let reference =
        exchange_host::CredentialRef::new("dev", "com.zendesk.api", "support", "api_token")
            .expect("sentinel credential address");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("sentinel store runtime")
        .block_on(
            credential
                .secrets()
                .put(&reference, &exchange_host::Secret::new(SENTINELS[0].1)),
        )
        .expect("production credential write");

    let settings = exchange_host::SettingsStore::bind(root.join("settings/store.json"))
        .expect("production settings store");
    let declared = exchange_host::DeclaredSetting::parse("default", "endpoint.subdomain")
        .expect("declared sentinel setting");
    settings
        .set(&tenant, "zendesk", &declared, SENTINELS[1].1)
        .expect("production setting write");
    let grants = exchange_host::GrantStore::bind(root.join("grants/store.json"))
        .expect("production grant store");
    let grant = exchange_host::Grant::for_connector(
        SENTINELS[2].1,
        exchange_host::Selector::at_most(exchange_host::Risk::Low),
    );
    grants
        .set(&tenant, &[grant])
        .expect("production grant write");

    let service_account_path = root.join("service-accounts/store.json");
    let service_accounts =
        flux_exchange::service_account::ServiceAccountStore::open(&service_account_path)
            .expect("production Service Account store");
    let minted = service_accounts
        .mint(
            &exchange_host::Principal::new(
                exchange_host::PrincipalKind::User,
                "sentinel-user",
                tenant,
            ),
            "sentinel-service",
            flux_exchange::service_account::Expiry {
                as_of: 1_800_000_000,
                expires_at: 1_800_000_001,
            },
        )
        .expect("production Service Account mint");
    let token = minted.token.as_str().to_owned();
    drop(service_accounts);
    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(service_account_path).expect("persisted Service Account store"),
    )
    .expect("production Service Account JSON");
    let verifier = stored["agents"]
        .as_object()
        .and_then(|agents| agents.keys().next())
        .expect("persisted Service Account verifier")
        .to_owned();
    vec![token, verifier]
}

struct NativeProcess {
    process: HANDLE,
    pid: u32,
    readiness: HANDLE,
    liveness: HANDLE,
    state_root: PathBuf,
    dynamic_authority_values: Vec<String>,
}

impl NativeProcess {
    fn spawn(wedge: bool) -> Self {
        let state_root = std::env::temp_dir().join(format!(
            "flux-exchange-x128-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        let dynamic_authority_values = seed_production_authority_stores(&state_root);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let mut readiness_read = std::ptr::null_mut();
        let mut readiness_write = std::ptr::null_mut();
        let mut liveness_read = std::ptr::null_mut();
        let mut liveness_write = std::ptr::null_mut();
        // SAFETY: all output pointers and the security-attributes structure remain live.
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
        let words = attribute_bytes.div_ceil(std::mem::size_of::<usize>());
        let mut attribute_storage = vec![0_usize; words];
        let attribute_list = attribute_storage.as_mut_ptr().cast();
        // SAFETY: aligned storage has the size returned by the sizing call.
        assert_ne!(
            unsafe {
                InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes)
            },
            0
        );
        // SAFETY: the HANDLE array remains live through CreateProcessW and is the complete list.
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
        let mut environment = current_environment(&state_root, wedge);
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attribute_list;
        // Deliberately no STARTF_USESTDHANDLES: Windows requires redirected standard handles to be
        // inherited, which would add them to this complete HANDLE_LIST and violate the exact-two
        // supervisor ABI. The ordinary-startup fixture below exercises real stdout/stderr capture
        // over these same production store values; supervised output remains readiness-only.
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: all pointers reference live, correctly sized Windows structures and mutable
        // nul-terminated UTF-16 buffers. The explicit handle list is exactly the two capabilities.
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
        // SAFETY: the initialized attribute list is no longer needed after CreateProcessW returns.
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
        Self {
            process: process.hProcess,
            pid: process.dwProcessId,
            readiness: readiness_read,
            liveness: liveness_write,
            state_root,
            dynamic_authority_values,
        }
    }

    fn readiness(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let mut read = 0_u32;
            // SAFETY: the buffer/output count are live and readiness is the owned pipe read handle.
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
        assert_no_authority_values(&bytes, &self.dynamic_authority_values);
        bytes
    }

    fn close_liveness(&mut self) {
        if !self.liveness.is_null() {
            close(self.liveness);
            self.liveness = std::ptr::null_mut();
        }
    }

    fn send_liveness_byte(&self) {
        use windows_sys::Win32::Storage::FileSystem::WriteFile;
        let byte = [0x5a_u8; 1];
        let mut written = 0_u32;
        // SAFETY: the buffer/output count are live and this is the owned liveness write handle.
        assert_ne!(
            unsafe {
                WriteFile(
                    self.liveness,
                    byte.as_ptr().cast(),
                    1,
                    &mut written,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(written, 1);
    }

    fn wait_dead(&self) {
        // SAFETY: process is the still-open handle returned by CreateProcessW.
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 5_000) },
            WAIT_OBJECT_0
        );
    }
}

impl Drop for NativeProcess {
    fn drop(&mut self) {
        self.close_liveness();
        if !self.readiness.is_null() {
            close(self.readiness);
        }
        // SAFETY: termination is test cleanup for the exact open child handle.
        unsafe {
            TerminateProcess(self.process, 1);
            WaitForSingleObject(self.process, 5_000);
        }
        close(self.process);
        let _ = std::fs::remove_dir_all(&self.state_root);
    }
}

fn assert_new_start_refuses_at_metadata_expiry() {
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/exchange-release-v1");
    let fixture: flux_exchange_release::FixtureSet = flux_exchange_release::canonical::parse(
        &flux_exchange_release::read_bounded_file(
            &fixture_root.join("fixture-set.json"),
            256 * 1024,
        )
        .expect("bounded fixture-set read"),
        256 * 1024,
    )
    .expect("canonical fixture-set");
    let case = fixture
        .cases
        .iter()
        .find(|case| case.id == "expiry-equality-stopped")
        .expect("expiry-equality-stopped provider case");
    let policy: flux_exchange_release::RootPolicy = flux_exchange_release::canonical::parse(
        &flux_exchange_release::read_bounded_file(
            &fixture_root.join("root-policy.test.json"),
            64 * 1024,
        )
        .expect("bounded root-policy read"),
        64 * 1024,
    )
    .expect("canonical root policy");
    let attempt = flux_exchange_release::verify_directory_layered(
        &fixture_root.join(&case.input),
        &policy,
        flux_exchange_release::parse_utc(&case.clock).expect("fixture clock"),
        &flux_exchange_release::Protocols::v1(),
        &case.prior_state,
        Some("x86_64-pc-windows-msvc"),
    );
    assert!(matches!(
        attempt.outcome,
        Err(flux_exchange_release::Error::Time(_))
    ));
    assert_eq!(attempt.state, case.expected_state);
}

fn readiness_address(bytes: &[u8]) -> SocketAddr {
    let ready: serde_json::Value = serde_json::from_slice(bytes).expect("readiness object");
    format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("host"),
        ready["bind"]["port"].as_u64().expect("port")
    )
    .parse()
    .expect("reported address")
}

fn assert_port_released(address: SocketAddr) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while TcpStream::connect_timeout(&address, Duration::from_millis(20)).is_ok() {
        assert!(
            std::time::Instant::now() < deadline,
            "supervised Exchange process died without releasing its port"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn verified_metadata_expiry_keeps_the_same_healthy_child_until_owner_stop() {
    let mut server = NativeProcess::spawn(false);
    let pid = server.pid;
    let bytes = server.readiness();
    let address = readiness_address(&bytes);

    assert_new_start_refuses_at_metadata_expiry();
    assert_eq!(server.pid, pid, "the owned child identity changed");
    // SAFETY: process is the still-open handle returned by CreateProcessW.
    assert_ne!(
        unsafe { WaitForSingleObject(server.process, 0) },
        WAIT_OBJECT_0,
        "metadata expiry terminated the already healthy child"
    );
    TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("same healthy child remains reachable after metadata expiry");

    server.close_liveness();
    server.wait_dead();
    assert_port_released(address);
}

#[test]
fn real_windows_handle_list_readiness_identity_and_native_liveness() {
    let mut server = NativeProcess::spawn(false);
    let expected_filetime = creation_filetime(server.process);
    let bytes = server.readiness();
    assert!(!bytes.is_empty());
    assert_no_sentinels(&bytes);
    assert!(bytes.len() <= 16 * 1024);
    let ready: serde_json::Value = serde_json::from_slice(&bytes).expect("readiness object");
    assert_eq!(ready["process"]["pid"], server.pid);
    assert_eq!(
        ready["process"]["start_identity"]["kind"],
        "windows-process-creation"
    );
    assert_eq!(
        ready["process"]["start_identity"]["filetime"],
        expected_filetime.to_string()
    );
    let compatibility = exchange_command()
        .args(["compatibility", "--json"])
        .output()
        .expect("compatibility process");
    let compatibility: serde_json::Value =
        serde_json::from_slice(&compatibility.stdout).expect("compatibility object");
    assert_eq!(ready["protocols"], compatibility["protocols"]);
    let executable = std::fs::read(env!("CARGO_BIN_EXE_flux-exchange")).expect("executable bytes");
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(&executable)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expected = ReadinessExpectation {
        release: ExpectedRelease {
            tag: compatibility["release"]["tag"]
                .as_str()
                .expect("tag")
                .to_owned(),
            version: compatibility["release"]["version"]
                .as_str()
                .expect("version")
                .to_owned(),
            source_commit: compatibility["release"]["source_commit"]
                .as_str()
                .expect("source")
                .to_owned(),
            build_id: compatibility["release"]["build_id"]
                .as_str()
                .expect("build")
                .to_owned(),
            executable_sha256: digest,
        },
        pid: server.pid,
        platform: NativePlatform::Windows,
        start_identity: VerifiedStartIdentity::Windows {
            filetime: expected_filetime.to_string(),
        },
    };
    verify_readiness(&bytes, &expected).expect("open child identity permits ownership commit");
    let address: SocketAddr = format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("host"),
        ready["bind"]["port"].as_u64().expect("port")
    )
    .parse()
    .expect("reported address");
    TcpStream::connect_timeout(&address, Duration::from_secs(2)).expect("reported listener");
    server.close_liveness();
    server.wait_dead();
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_err());
}

#[test]
fn windows_tokio_wedge_still_dies_through_native_liveness() {
    let mut server = NativeProcess::spawn(true);
    let bytes = server.readiness();
    assert!(!bytes.is_empty());
    let address = readiness_address(&bytes);
    server.close_liveness();
    server.wait_dead();
    assert_port_released(address);
}

#[test]
fn windows_liveness_byte_exits_without_waiting_for_eof() {
    let mut server = NativeProcess::spawn(false);
    let bytes = server.readiness();
    assert!(!bytes.is_empty());
    server.send_liveness_byte();
    server.wait_dead();
}

#[test]
fn windows_supervisor_helper_process() {
    if std::env::var_os("X128_RUN_WINDOWS_SUPERVISOR_HELPER").is_none() {
        return;
    }
    let wedge = std::env::var_os("X128_WINDOWS_HELPER_WEDGE").is_some();
    let mut server = NativeProcess::spawn(wedge);
    let readiness = server.readiness();
    println!(
        "X128_READY\t{}\t{}\t{}",
        server.pid,
        server.state_root.display(),
        String::from_utf8(readiness).expect("UTF-8 readiness")
    );
    std::io::stdout().flush().expect("helper readiness flush");
    loop {
        std::thread::park();
    }
}

#[test]
fn terminate_process_of_supervisor_kills_wedged_exchange_and_releases_port() {
    assert_terminate_supervisor(true);
}

#[test]
fn terminate_process_of_supervisor_kills_responsive_exchange_and_releases_port() {
    assert_terminate_supervisor(false);
}

fn assert_terminate_supervisor(wedge: bool) {
    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    command
        .args([
            "--exact",
            "windows_supervisor_helper_process",
            "--nocapture",
        ])
        .env("X128_RUN_WINDOWS_SUPERVISOR_HELPER", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if wedge {
        command.env("X128_WINDOWS_HELPER_WEDGE", "1");
    }
    let mut helper = command.spawn().expect("Windows supervisor helper");
    let mut reader = std::io::BufReader::new(helper.stdout.take().expect("helper stdout"));
    let mut helper_stderr = helper.stderr.take().expect("helper stderr");
    let line = loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).expect("helper output"), 0);
        if let Some((_, ready)) = line.split_once("X128_READY\t") {
            break ready.to_owned();
        }
    };
    let mut fields = line.trim_end().splitn(3, '\t');
    let exchange_pid = fields
        .next()
        .expect("pid")
        .parse::<u32>()
        .expect("numeric pid");
    let state_root = PathBuf::from(fields.next().expect("state root"));
    let ready: serde_json::Value =
        serde_json::from_str(fields.next().expect("readiness")).expect("readiness object");
    let address: SocketAddr = format!(
        "{}:{}",
        ready["bind"]["host"].as_str().expect("host"),
        ready["bind"]["port"].as_u64().expect("port")
    )
    .parse()
    .expect("reported address");

    // SAFETY: the raw handle belongs to the exact still-open helper Child object.
    assert_ne!(
        unsafe { TerminateProcess(helper.as_raw_handle().cast(), 9) },
        0
    );
    assert!(!helper.wait().expect("helper status").success());
    let mut remaining_stdout = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut remaining_stdout).expect("captured helper stdout");
    let mut captured_stderr = Vec::new();
    std::io::Read::read_to_end(&mut helper_stderr, &mut captured_stderr)
        .expect("captured helper stderr");
    assert_no_sentinels(&remaining_stdout);
    assert_no_sentinels(&captured_stderr);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        // SYNCHRONIZE is sufficient to observe termination without opening any PID-named authority.
        // SAFETY: OpenProcess only observes the exact PID that the verified helper returned.
        let process = unsafe { OpenProcess(0x0010_0000, 0, exchange_pid) };
        let process_gone = if process.is_null() {
            true
        } else {
            // SAFETY: `process` is a live synchronization handle returned immediately above.
            let gone = unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0;
            close(process);
            gone
        };
        if process_gone && TcpStream::connect_timeout(&address, Duration::from_millis(20)).is_err()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Exchange survived TerminateProcess of its supervisor"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(state_root);
}

#[test]
fn malformed_windows_handle_flags_refuse_without_stdout_readiness() {
    for arguments in [
        vec!["--supervised"],
        vec!["--dev", "--supervised"],
        vec!["junk", "--supervised"],
        vec!["--supervisor-readiness-handle"],
        vec!["--supervisor-liveness-handle"],
        vec!["--supervised=true"],
        vec!["--supervisor-readiness-handle=1"],
        vec!["--supervisor-junk"],
        vec![
            "--supervised",
            "--supervisor-readiness-handle",
            "0",
            "--supervisor-liveness-handle",
            "1",
        ],
        vec![
            "--supervised",
            "--supervisor-readiness-handle",
            "01",
            "--supervisor-liveness-handle",
            "2",
        ],
        vec![
            "--supervised",
            "--supervisor-readiness-handle",
            "+1",
            "--supervisor-liveness-handle",
            "2",
        ],
        vec![
            "--supervised",
            "--supervisor-readiness-handle",
            "-1",
            "--supervisor-liveness-handle",
            "2",
        ],
        vec![
            "--supervised",
            "--supervisor-readiness-handle",
            "18446744073709551616",
            "--supervisor-liveness-handle",
            "2",
        ],
        vec![
            "--supervised",
            "--supervisor-readiness-handle",
            "1",
            "--supervisor-liveness-handle",
            "1",
        ],
        vec![
            "--supervised",
            "--supervisor-liveness-handle",
            "2",
            "--supervisor-readiness-handle",
            "1",
        ],
        vec![
            "--supervisor-readiness-handle",
            "1",
            "--supervisor-liveness-handle",
            "2",
            "--supervised",
        ],
    ] {
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x128-windows-mode-refusal-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        let output = exchange_command()
            .args(arguments)
            .env("FLUX_EXCHANGE_STATE", &root)
            .output()
            .expect("malformed ABI process");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_no_sentinels(&output.stderr);
        assert!(!root.exists(), "malformed mode opened local state");
    }
}

#[test]
fn environment_stdout_and_handles_outside_the_explicit_list_are_not_capabilities() {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut readiness_read = std::ptr::null_mut();
    let mut readiness_write = std::ptr::null_mut();
    let mut liveness_read = std::ptr::null_mut();
    let mut liveness_write = std::ptr::null_mut();
    // Plant real pipe handles, then make them ineligible for inheritance before starting the child.
    // Numeric argv and similarly named environment inputs cannot turn an unlisted handle, stdout or
    // the environment into the supervised ABI.
    assert_ne!(
        unsafe { CreatePipe(&mut readiness_read, &mut readiness_write, &attributes, 0) },
        0
    );
    assert_ne!(
        unsafe { CreatePipe(&mut liveness_read, &mut liveness_write, &attributes, 0) },
        0
    );
    clear_inherit(readiness_read);
    clear_inherit(readiness_write);
    clear_inherit(liveness_read);
    clear_inherit(liveness_write);
    let output = exchange_command()
        .args([
            "--supervised",
            "--supervisor-readiness-handle",
            &(readiness_write as usize).to_string(),
            "--supervisor-liveness-handle",
            &(liveness_read as usize).to_string(),
        ])
        .env(
            "FLUX_EXCHANGE_SUPERVISOR_READINESS_HANDLE",
            (readiness_write as usize).to_string(),
        )
        .env(
            "FLUX_EXCHANGE_SUPERVISOR_LIVENESS_HANDLE",
            (liveness_read as usize).to_string(),
        )
        .output()
        .expect("unlisted Windows capability process");
    for handle in [
        readiness_read,
        readiness_write,
        liveness_read,
        liveness_write,
    ] {
        close(handle);
    }
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout became readiness");
    assert_no_sentinels(&output.stderr);
}

#[test]
fn ordinary_startup_over_the_same_authority_stores_captures_value_free_logs() {
    let root = std::env::temp_dir().join(format!(
        "flux-exchange-x128-windows-log-capture-{}-{}",
        std::process::id(),
        unique_counter()
    ));
    let dynamic_authority_values = seed_production_authority_stores(&root);
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupied log fixture");
    let occupied_address = occupied.local_addr().expect("occupied log address");
    let output = exchange_command()
        .arg("--dev")
        .env("USER", "sentinel-user")
        .env("FLUX_EXCHANGE_STATE", &root)
        .env("FLUX_EXCHANGE_BIND", occupied_address.to_string())
        .output()
        .expect("ordinary startup log capture");
    assert!(!output.status.success());
    assert_no_sentinels(&output.stdout);
    assert_no_sentinels(&output.stderr);
    assert_no_authority_values(&output.stdout, &dynamic_authority_values);
    assert_no_authority_values(&output.stderr, &dynamic_authority_values);
    drop(occupied);
    let _ = std::fs::remove_dir_all(root);
}

fn current_environment(state_root: &PathBuf, wedge: bool) -> Vec<u16> {
    let mut values = std::env::vars_os().collect::<BTreeMap<OsString, OsString>>();
    values.insert(
        "FLUX_EXCHANGE_STATE".into(),
        state_root.as_os_str().to_owned(),
    );
    if wedge {
        values.insert("FLUX_EXCHANGE_TEST_WEDGE_AFTER_READY".into(), "1".into());
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

fn creation_filetime(process: HANDLE) -> u64 {
    use windows_sys::Win32::Foundation::FILETIME;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the exact open child handle and all output pointers are valid.
    assert_ne!(
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) },
        0
    );
    (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)
}

fn close(handle: HANDLE) {
    if !handle.is_null() {
        // SAFETY: test ownership ensures each native handle is closed once.
        unsafe { CloseHandle(handle) };
    }
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
