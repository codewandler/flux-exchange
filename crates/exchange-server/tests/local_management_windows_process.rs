#![cfg(windows)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_INSUFFICIENT_BUFFER, HANDLE,
    HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, COORD, ENABLE_ECHO_INPUT, HPCON,
};
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
    GetExitCodeProcess, InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW,
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};

const PIPE_PREFIX: &str = r"\\.\pipe\flux-exchange-local-management-v1-";
const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;
const CONNECT_BEGIN: u16 = 0x0001;
const NEED_SECRETS: u16 = 0x0002;
const SECRET: u16 = 0x0003;
const CONNECT_COMMIT: u16 = 0x0004;
const CONNECT_RECEIPT: u16 = 0x0006;
const SERVICE_ACCOUNT_QUERY: u16 = 0x0021;
const SERVICE_ACCOUNT_RECEIPT: u16 = 0x0022;
const TEST_RESULT_BUDGET_MILLIS: u64 = 2_000;
const PRIVATE_CONSOLE_SENTINEL: &[u8] = b"x137-windows-conin-secret";
const CONNECT_SENTINEL: &[u8] = b"x137-windows-connect-secret";

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct SupervisedServer {
    process: HANDLE,
    readiness: HANDLE,
    liveness: HANDLE,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    state_root: PathBuf,
}

impl SupervisedServer {
    fn spawn() -> Self {
        let state_root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-windows-endpoint-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&state_root).expect("private test state root");

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

        let stdout = NativePipe::new();
        let stderr = NativePipe::new();
        let stdout_writer = stdout.write.as_raw_handle() as HANDLE;
        let stderr_writer = stderr.write.as_raw_handle() as HANDLE;

        let inherited = [readiness_write, liveness_read, stdout_writer, stderr_writer];
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
        // SAFETY: the two-HANDLE array remains live through CreateProcessW and is the complete list.
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
        let mut environment = child_environment(&state_root);
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = std::ptr::null_mut();
        startup.StartupInfo.hStdOutput = stdout_writer;
        startup.StartupInfo.hStdError = stderr_writer;
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
        // SAFETY: CreateProcessW has returned, so the initialized attribute list is no longer used.
        unsafe { DeleteProcThreadAttributeList(attribute_list) };
        assert_ne!(
            created,
            0,
            "CreateProcessW: {}",
            std::io::Error::last_os_error()
        );
        close(readiness_write);
        close(liveness_read);
        drop(stdout.write);
        drop(stderr.write);
        let stdout = std::thread::spawn(move || read_owned_to_end(stdout.read));
        let stderr = std::thread::spawn(move || read_owned_to_end(stderr.read));
        close(process.hThread);
        Self {
            process: process.hProcess,
            readiness: readiness_read,
            liveness: liveness_write,
            stdout: Some(stdout),
            stderr: Some(stderr),
            state_root,
        }
    }

    fn readiness(&mut self) -> Value {
        let raw = std::mem::replace(&mut self.readiness, std::ptr::null_mut());
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let mut read = 0_u32;
            // SAFETY: `raw` is the owned readiness read capability and both outputs are live.
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
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
        close(raw);
        assert!(
            !bytes.is_empty(),
            "supervised process refused before readiness"
        );
        serde_json::from_slice(&bytes).expect("canonical supervisor readiness")
    }

    fn stop(mut self) -> (PathBuf, Vec<u8>, Vec<u8>) {
        close(std::mem::replace(&mut self.liveness, std::ptr::null_mut()));
        // SAFETY: this is the exact still-open child process handle.
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 5_000) },
            WAIT_OBJECT_0,
            "liveness EOF did not stop the supervised process"
        );
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        let stdout = self
            .stdout
            .take()
            .expect("captured server stdout")
            .join()
            .expect("server stdout reader");
        let stderr = self
            .stderr
            .take()
            .expect("captured server stderr")
            .join()
            .expect("server stderr reader");
        (std::mem::take(&mut self.state_root), stdout, stderr)
    }
}

impl Drop for SupervisedServer {
    fn drop(&mut self) {
        close(std::mem::replace(&mut self.liveness, std::ptr::null_mut()));
        close(std::mem::replace(&mut self.readiness, std::ptr::null_mut()));
        if !self.process.is_null() {
            // SAFETY: test cleanup targets only the exact child handle retained by this fixture.
            unsafe {
                TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, 5_000);
            }
            close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        }
        if let Some(stdout) = self.stdout.take() {
            let _ = stdout.join();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
        if !self.state_root.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.state_root);
        }
    }
}

#[test]
fn supervised_windows_service_account_helper_delivers_exact_fxsa_and_closes_fxha_adversaries() {
    let mut server = SupervisedServer::spawn();
    let readiness = server.readiness();
    assert_eq!(readiness["schema"], "exchange.supervisor-ready.v2");
    let address: SocketAddr = format!(
        "{}:{}",
        readiness["bind"]["host"].as_str().expect("readiness host"),
        readiness["bind"]["port"].as_u64().expect("readiness port")
    )
    .parse()
    .expect("readiness address");

    let query = frame(
        CLIENT,
        PLAN_QUERY,
        br#"{"connector":"jira","selection":null}"#,
    );
    let mut owner_pipe = open_owner_pipe();
    for chunk in query.chunks(5) {
        owner_pipe.write_all(chunk).expect("split PLAN write");
    }
    let mut response = Vec::new();
    owner_pipe
        .read_to_end(&mut response)
        .expect("one PLAN response plus EOF");
    drop(owner_pipe);
    let (opcode, payload) = decode_server_frame(&response);
    assert_eq!(opcode, PLAN_RESPONSE, "owner pipe must serve PLAN_RESPONSE");
    let plan: Value = serde_json::from_slice(payload).expect("PLAN JSON");
    assert_eq!(
        serde_json::to_vec(&plan).expect("canonical PLAN"),
        payload,
        "native PLAN payload must be byte-canonical"
    );
    assert_eq!(response, frame(SERVER, PLAN_RESPONSE, payload));
    assert_eq!(plan["version"], "exchange.connection-plan.v2");
    assert_eq!(plan["connector"], "jira");
    assert_eq!(plan["selection"], Value::Null);
    assert_eq!(plan["credential_revision"], Value::Null);
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
    assert_nonzero_lowerhex(plan["plan_revision"].as_str().expect("plan revision"));
    let fields = plan["fields"].as_array().expect("plan fields");
    assert!(!fields.is_empty());
    for field in fields {
        assert_eq!(
            field
                .as_object()
                .expect("closed field object")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "aliases",
                "also_binds",
                "authority",
                "binds",
                "choices",
                "help",
                "identity",
                "input",
                "label",
                "name",
                "provenance",
                "reason",
                "required",
                "routable",
                "secret",
                "service",
                "set",
                "target",
            ])
        );
    }

    windows_private_console_input_survives_null_stdio_and_restores_mode(&server);

    // Re-query after the first durable connection: its publication intentionally changes the plan
    // revision, so a cached BEGIN would be a stale-plan test rather than ActiveSession evidence.
    let github_plan = plan_query("github", None);
    let begin = connect_begin(&github_plan, "windows-active-session");
    let proposal = serde_json::to_vec(&begin).expect("canonical CONNECT proposal");
    let proposal_digest =
        proposal_digest("exchange.local-management.v1.connect-proposal", &proposal);
    let mut ceremony = open_owner_pipe();
    ceremony
        .write_all(&frame(CLIENT, CONNECT_BEGIN, &proposal))
        .expect("CONNECT BEGIN");
    let needed = read_frame(&mut ceremony);
    assert_eq!(needed.0, NEED_SECRETS, "CONNECT must enter ActiveSession");
    let needed_json: Value = serde_json::from_slice(&needed.1).expect("NEED_SECRETS JSON");
    assert_eq!(needed_json["proposal_digest"], proposal_digest);
    let needs = needed_json["secrets"].as_array().expect("secret needs");
    assert_eq!(needs.len(), 1, "GitHub fixture has one secret prompt");
    let ordinal = u16::try_from(needs[0]["ordinal"].as_u64().expect("secret ordinal"))
        .expect("bounded ordinal");
    let mut secret = Vec::with_capacity(2 + CONNECT_SENTINEL.len());
    secret.extend_from_slice(&ordinal.to_be_bytes());
    secret.extend_from_slice(CONNECT_SENTINEL);
    ceremony
        .write_all(&frame(CLIENT, SECRET, &secret))
        .expect("CONNECT secret frame");
    let commit = serde_json::to_vec(&serde_json::json!({
        "proposal_digest": proposal_digest,
        "transaction_id": needed_json["transaction_id"],
    }))
    .expect("canonical CONNECT COMMIT");
    ceremony
        .write_all(&frame(CLIENT, CONNECT_COMMIT, &commit))
        .expect("CONNECT COMMIT");
    let receipt = read_frame(&mut ceremony);
    assert_eq!(receipt.0, CONNECT_RECEIPT, "multi-frame CONNECT receipt");
    let receipt: Value = serde_json::from_slice(&receipt.1).expect("CONNECT receipt JSON");
    assert_eq!(receipt["label"], "windows-active-session");
    assert_eq!(receipt["replayed"], false);
    let mut eof = [0_u8; 1];
    assert_eq!(ceremony.read(&mut eof).expect("CONNECT terminal EOF"), 0);

    let expires_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("fixture clock")
        .as_secs()
        .checked_add(300)
        .expect("bounded expiry");
    let helper = MintHelper::spawn(&server.state_root, expires_at);
    let token = helper.finish();
    let stored_path = server.state_root.join("service-accounts/store.json");
    let stored = std::fs::read(&stored_path).expect("durable Service Account authority");
    let receipt_id = committed_mint_receipt(&stored, expires_at);
    assert_forms_absent(&stored, &transformed_forms(&token), "Service Account store");
    let queried = request_one(
        SERVICE_ACCOUNT_QUERY,
        &serde_json::to_vec(&serde_json::json!({"receipt_id": receipt_id}))
            .expect("canonical receipt query"),
    );
    assert_eq!(queried.0, SERVICE_ACCOUNT_RECEIPT);
    let queried: Value = serde_json::from_slice(&queried.1).expect("queried receipt JSON");
    assert_eq!(queried["receipt_id"], receipt_id);
    assert_eq!(queried["replayed"], true);

    let before_adversaries = std::fs::read(&stored_path).expect("pre-adversary durable image");
    let valid = NativePipe::new();
    let valid_source = valid.write.as_raw_handle() as usize as u64;
    let mint = frame(
        CLIENT,
        0x0020,
        format!(r#"{{"expires_at":"{expires_at}","id":"adversary"}}"#).as_bytes(),
    );
    let mut attachment = fxha(valid_source);
    for index in 0..8 {
        let original = attachment[index];
        attachment[index] ^= 0xff;
        assert_attachment_refusal(attachment_exchange(&attachment, &mint), "writer_invalid");
        attachment[index] = original;
    }
    assert_attachment_refusal(attachment_exchange(&fxha(0), &mint), "writer_invalid");
    assert_attachment_refusal(
        attachment_exchange(&fxha(u64::MAX), &mint),
        "writer_invalid",
    );

    let wrong_direction = NativePipe::new();
    let wrong_direction_value = wrong_direction.read.as_raw_handle() as usize as u64;
    assert_attachment_refusal(
        attachment_exchange(&fxha(wrong_direction_value), &mint),
        "writer_invalid",
    );
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    assert!(!event.is_null());
    let event = unsafe { OwnedHandle::from_raw_handle(event.cast()) };
    assert_attachment_refusal(
        attachment_exchange(&fxha(event.as_raw_handle() as usize as u64), &mint),
        "writer_invalid",
    );

    let mut alias_pipe = open_owner_pipe();
    let alias = alias_pipe.as_raw_handle() as usize as u64;
    let mut alias_request = fxha(alias).to_vec();
    alias_request.extend_from_slice(&mint);
    alias_pipe
        .write_all(&alias_request)
        .expect("aliased FXHA request");
    assert_attachment_refusal(read_frame(&mut alias_pipe), "writer_invalid");

    let mut surplus = mint.clone();
    surplus.push(0);
    assert_attachment_refusal(
        attachment_exchange(&fxha(valid_source), &surplus),
        "invalid_frame",
    );
    assert_attachment_refusal(
        attachment_exchange(&fxha(valid_source), b"FXHA"),
        "unexpected_frame",
    );
    assert_attachment_refusal(
        attachment_exchange(
            &fxha(valid_source),
            &frame(
                CLIENT,
                PLAN_QUERY,
                br#"{"connector":"github","selection":null}"#,
            ),
        ),
        "unexpected_frame",
    );

    let mut truncated = open_owner_pipe();
    truncated
        .write_all(&fxha(valid_source)[..9])
        .expect("truncated FXHA");
    drop(truncated);
    assert_eq!(
        plan_query("github", None)["version"],
        "exchange.connection-plan.v2",
        "a truncated attachment must drop fully before endpoint rearm"
    );
    let after_adversaries = std::fs::read(&stored_path).expect("post-adversary durable image");
    assert_eq!(after_adversaries, before_adversaries);
    for source in [valid_source, wrong_direction_value, alias] {
        let spelling = source.to_string();
        assert!(
            !after_adversaries
                .windows(spelling.len())
                .any(|candidate| candidate == spelling.as_bytes()),
            "numeric FXHA source HANDLE entered persistence"
        );
    }

    let mut raw_tcp = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("loopback TCP listener");
    raw_tcp
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("raw TCP read deadline");
    raw_tcp.write_all(&query).expect("raw FXLM attack");
    raw_tcp
        .shutdown(std::net::Shutdown::Write)
        .expect("finish raw FXLM attack");
    let mut raw_reply = Vec::new();
    match raw_tcp.read_to_end(&mut raw_reply) {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) => {}
        Err(error) => panic!("raw TCP attack read: {error}"),
    }
    assert_ne!(raw_reply, frame(SERVER, PLAN_RESPONSE, payload));
    assert!(!raw_reply
        .windows(b"exchange.connection-plan.v2".len())
        .any(|candidate| candidate == b"exchange.connection-plan.v2"));

    let mut http = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("loopback HTTP listener");
    http.write_all(
        b"GET /api/grants HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer local-owner\r\nConnection: close\r\n\r\n",
    )
    .expect("forged local-owner request");
    let mut http_reply = String::new();
    http.read_to_string(&mut http_reply)
        .expect("HTTP authentication refusal");
    assert!(
        http_reply.starts_with("HTTP/1.1 401"),
        "loopback HTTP reproduced native local-owner authority: {http_reply}"
    );

    let (state_root, stdout, stderr) = server.stop();
    let private_forms = transformed_forms(PRIVATE_CONSOLE_SENTINEL);
    assert_forms_absent(&stdout, &private_forms, "server stdout");
    assert_forms_absent(&stderr, &private_forms, "server stderr");
    assert_tree_excludes_except_credentials(&state_root, &private_forms);
    let registry =
        exchange_host::ConnectionRegistryStore::bind(state_root.join("connections/store.json"))
            .expect("reopen value-free connection registry");
    let tenant = exchange_host::Tenant::new("local").expect("local tenant");
    let label = exchange_host::ConnectionLabel::new("windows-private-console")
        .expect("private-console label");
    let instance = exchange_host::ConnectionRegistry::resolve(&registry, &tenant, "github", &label)
        .expect("resolve private-console label")
        .expect("private-console instance");
    let credentials =
        exchange_host::CredentialStore::bind(state_root.join("credentials/store.txt"))
            .expect("reopen retained credential provider after server exit");
    let reference = exchange_host::CredentialRef::for_instance(
        "local",
        "com.github.api",
        instance.as_str(),
        "default",
        "token",
    )
    .expect("private-console GitHub credential address");
    let private_secret = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("credential read runtime")
        .block_on(credentials.secrets().get(&reference))
        .expect("private-console secret committed through retained provider");
    assert_eq!(
        private_secret.expose_secret().as_bytes(),
        PRIVATE_CONSOLE_SENTINEL
    );
    drop(credentials);
    drop(registry);
    let _ = std::fs::remove_dir_all(state_root);
}

fn windows_private_console_input_survives_null_stdio_and_restores_mode(server: &SupervisedServer) {
    // The released vendor helper has no standard streams and opens the real attached `CONIN$`
    // only after its complete BEGIN capability reaches EOF. A feature-only probe attached to the
    // same supported pseudoconsole observes the production echo transition; it carries no secret
    // and is not a helper protocol or a production route.
    let console = PseudoConsole::new(&server.state_root);
    let original_mode = console.mode();
    assert_ne!(
        original_mode & ENABLE_ECHO_INPUT,
        0,
        "the test console must begin with echo enabled"
    );
    let private_plan = plan_query("github", None);
    let private_begin = connect_begin(&private_plan, "windows-private-console");
    let private_request = frame(
        CLIENT,
        CONNECT_BEGIN,
        &serde_json::to_vec(&private_begin).expect("canonical private-console BEGIN"),
    );
    assert_forms_absent(
        &private_request,
        &transformed_forms(PRIVATE_CONSOLE_SENTINEL),
        "helper BEGIN capability",
    );
    let mut private_helper =
        VendorHelper::spawn(&server.state_root, &private_request, None, console.handle());
    private_helper.wait_for_private_console_read(&console);
    console.write_line(PRIVATE_CONSOLE_SENTINEL);
    let private_response = private_helper.finish(0);
    let private_receipt = decode_server_control(&private_response, CONNECT_RECEIPT);
    assert_eq!(private_receipt["connector"], "github");
    assert_eq!(private_receipt["label"], "windows-private-console");
    assert_eq!(
        console.mode(),
        original_mode,
        "helper restored `CONIN$` mode"
    );
    assert_forms_absent(
        &private_response,
        &transformed_forms(PRIVATE_CONSOLE_SENTINEL),
        "helper terminal response",
    );

    // The same production process seam shortens only the otherwise fixed 335-second result cap.
    // Holding the real console read beyond that cap must cancel ReadConsoleW, restore the exact
    // original mode, close the result capability without bytes and return the fixed transport exit.
    let deadline_plan = plan_query("github", None);
    let deadline_begin = connect_begin(&deadline_plan, "windows-private-console-deadline");
    let deadline_request = frame(
        CLIENT,
        CONNECT_BEGIN,
        &serde_json::to_vec(&deadline_begin).expect("canonical deadline BEGIN"),
    );
    let mut deadline_helper = VendorHelper::spawn(
        &server.state_root,
        &deadline_request,
        Some(TEST_RESULT_BUDGET_MILLIS),
        console.handle(),
    );
    deadline_helper.wait_for_private_console_read(&console);
    let deadline_response = deadline_helper.finish(1);
    assert!(
        deadline_response.is_empty(),
        "expired helper result capability closes value-free"
    );
    assert_eq!(
        console.mode(),
        original_mode,
        "deadline cancellation restored `CONIN$` mode"
    );
    let console_transcript = console.finish();
    assert_forms_absent(
        &console_transcript,
        &transformed_forms(PRIVATE_CONSOLE_SENTINEL),
        "pseudoconsole transcript",
    );
}

fn plan_query(connector: &str, selection: Option<&str>) -> Value {
    let response = request_one(
        PLAN_QUERY,
        &serde_json::to_vec(&serde_json::json!({
            "connector": connector,
            "selection": selection,
        }))
        .expect("canonical PLAN query"),
    );
    assert_eq!(response.0, PLAN_RESPONSE);
    serde_json::from_slice(&response.1).expect("PLAN response JSON")
}

fn request_one(opcode: u16, payload: &[u8]) -> (u16, Vec<u8>) {
    let mut pipe = open_owner_pipe();
    pipe.write_all(&frame(CLIENT, opcode, payload))
        .expect("native request");
    let response = read_frame(&mut pipe);
    let mut eof = [0_u8; 1];
    assert_eq!(pipe.read(&mut eof).expect("native terminal EOF"), 0);
    response
}

fn read_frame(pipe: &mut std::fs::File) -> (u16, Vec<u8>) {
    let mut header = [0_u8; 12];
    pipe.read_exact(&mut header).expect("complete FXLM header");
    assert_eq!(&header[..4], b"FXLM");
    assert_eq!(header[4], 1);
    assert_eq!(header[5], SERVER);
    let length = u32::from_be_bytes(header[8..12].try_into().expect("frame length")) as usize;
    assert!(length <= 65_536);
    let mut payload = vec![0_u8; length];
    pipe.read_exact(&mut payload)
        .expect("complete FXLM payload");
    (u16::from_be_bytes([header[6], header[7]]), payload)
}

fn connect_begin(plan: &Value, label: &str) -> Value {
    let mut targets = Vec::new();
    let mut settings = Vec::new();
    let mut authorities = Vec::new();
    for field in plan["fields"].as_array().expect("plan fields") {
        if !field["required"].as_bool().expect("required")
            || !field["routable"].as_bool().expect("routable")
        {
            continue;
        }
        let target = &field["target"];
        let projected = serde_json::json!({
            "revision": target["revision"],
            "target": target["id"],
        });
        if !targets.iter().any(|held| held == &projected) {
            targets.push(projected);
        }
        if field["secret"] == false && target["id"] != "connection.name" {
            settings.push(serde_json::json!({
                "target": target["id"],
                "value": "fixture-value",
            }));
            if field["authority"].is_object() {
                authorities.push(serde_json::json!({
                    "revision": null,
                    "target": target["id"],
                }));
            }
        }
    }
    serde_json::json!({
        "authorities": authorities,
        "connector": plan["connector"],
        "label": label,
        "plan_revision": plan["plan_revision"],
        "settings": settings,
        "targets": targets,
    })
}

fn proposal_digest(domain: &str, proposal: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update(proposal);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fxha(source: u64) -> [u8; 16] {
    let mut attachment = *b"FXHA\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    attachment[8..].copy_from_slice(&source.to_be_bytes());
    attachment
}

fn attachment_exchange(attachment: &[u8], following: &[u8]) -> (u16, Vec<u8>) {
    let mut pipe = open_owner_pipe();
    pipe.write_all(attachment).expect("FXHA attachment");
    pipe.write_all(following).expect("FXHA following bytes");
    read_frame(&mut pipe)
}

fn assert_attachment_refusal(response: (u16, Vec<u8>), code: &str) {
    assert_eq!(response.0, 0x7fff);
    let body: Value = serde_json::from_slice(&response.1).expect("attachment refusal JSON");
    assert_eq!(body["code"], code);
    assert_eq!(body["commit"], "none");
    assert_eq!(body["retry"], "never");
}

struct PseudoConsole {
    handle: HPCON,
    input: Option<OwnedHandle>,
    output: Option<JoinHandle<Vec<u8>>>,
    state_root: PathBuf,
}

impl PseudoConsole {
    fn new(state_root: &Path) -> Self {
        let input = NativePipe::new();
        let output = NativePipe::new();
        clear_inherit(input.write.as_raw_handle() as HANDLE);
        clear_inherit(output.write.as_raw_handle() as HANDLE);
        let mut handle = 0_isize;
        let created = unsafe {
            CreatePseudoConsole(
                COORD { X: 80, Y: 25 },
                input.read.as_raw_handle() as HANDLE,
                output.write.as_raw_handle() as HANDLE,
                0,
                &mut handle,
            )
        };
        assert!(
            created >= 0,
            "CreatePseudoConsole failed with HRESULT {created:#x}"
        );
        drop(input.read);
        drop(output.write);
        let output = std::thread::spawn(move || read_owned_to_end(output.read));
        Self {
            handle,
            input: Some(input.write),
            output: Some(output),
            state_root: state_root.to_path_buf(),
        }
    }

    fn handle(&self) -> HPCON {
        self.handle
    }

    fn mode(&self) -> u32 {
        let report = NativePipe::new();
        let writer = report.write.as_raw_handle() as HANDLE;
        let command = format!(
            "\"{}\" native-console-mode-test-seam {}",
            env!("CARGO_BIN_EXE_flux-exchange"),
            writer as usize
        );
        let child = spawn_attached(self.handle, &[writer], &command, &self.state_root, None);
        drop(report.write);
        assert_ne!(unsafe { ResumeThread(child.hThread) }, u32::MAX);
        close(child.hThread);
        let mut bytes = [0_u8; 4];
        read_exact_handle(&report.read, &mut bytes);
        let mut eof = [0_u8; 1];
        assert_eq!(read_handle(&report.read, &mut eof), 0, "mode probe EOF");
        assert_eq!(
            unsafe { WaitForSingleObject(child.hProcess, 5_000) },
            WAIT_OBJECT_0,
            "console-mode probe exit deadline"
        );
        assert_process_exit(child.hProcess, 0, "console-mode probe");
        close(child.hProcess);
        u32::from_be_bytes(bytes)
    }

    fn write_line(&self, secret: &[u8]) {
        let input = self.input.as_ref().expect("live pseudoconsole input");
        write_handle_all(input, secret);
        write_handle_all(input, b"\r");
    }

    fn finish(mut self) -> Vec<u8> {
        drop(self.input.take());
        if self.handle != 0 {
            unsafe { ClosePseudoConsole(self.handle) };
            self.handle = 0;
        }
        self.output
            .take()
            .expect("pseudoconsole output reader")
            .join()
            .expect("pseudoconsole output task")
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        drop(self.input.take());
        if self.handle != 0 {
            unsafe { ClosePseudoConsole(self.handle) };
            self.handle = 0;
        }
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
    }
}

struct VendorHelper {
    process: HANDLE,
    response: Option<OwnedHandle>,
}

impl VendorHelper {
    fn spawn(
        state_root: &Path,
        request: &[u8],
        result_budget_millis: Option<u64>,
        console: HPCON,
    ) -> Self {
        let request_pipe = NativePipe::new();
        let response_pipe = NativePipe::new();
        let canary = NativePipe::new();
        set_inherit(request_pipe.read.as_raw_handle() as HANDLE);
        clear_inherit(request_pipe.write.as_raw_handle() as HANDLE);
        let request_reader = request_pipe.read.as_raw_handle() as HANDLE;
        let response_writer = response_pipe.write.as_raw_handle() as HANDLE;
        let command = format!(
            "\"{}\" local vendor-secret --request-handle {} --response-handle {}",
            env!("CARGO_BIN_EXE_flux-exchange"),
            request_reader as usize,
            response_writer as usize
        );
        let child = spawn_attached(
            console,
            &[request_reader, response_writer],
            &command,
            state_root,
            result_budget_millis,
        );
        drop(request_pipe.read);
        drop(response_pipe.write);
        drop(canary.write);
        assert_pipe_closed_without_bytes(&canary.read);
        assert_ne!(unsafe { ResumeThread(child.hThread) }, u32::MAX);
        close(child.hThread);
        write_handle_all(&request_pipe.write, request);
        drop(request_pipe.write);
        Self {
            process: child.hProcess,
            response: Some(response_pipe.read),
        }
    }

    fn wait_for_private_console_read(&mut self, console: &PseudoConsole) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if unsafe { WaitForSingleObject(self.process, 0) } == WAIT_OBJECT_0 {
                let mut code = u32::MAX;
                assert_ne!(unsafe { GetExitCodeProcess(self.process, &mut code) }, 0);
                panic!("vendor helper exited before its private `CONIN$` read: {code}");
            }
            if console.mode() & ENABLE_ECHO_INPUT == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "vendor helper never entered its no-echo `CONIN$` read"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn finish(mut self, expected_exit: u32) -> Vec<u8> {
        let response = read_owned_to_end(self.response.take().expect("helper response capability"));
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 10_000) },
            WAIT_OBJECT_0,
            "released Windows vendor helper exit deadline"
        );
        assert_process_exit(
            self.process,
            expected_exit,
            "released Windows vendor helper",
        );
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        response
    }
}

impl Drop for VendorHelper {
    fn drop(&mut self) {
        if !self.process.is_null() {
            unsafe {
                TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, 5_000);
            }
            close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        }
    }
}

fn spawn_attached(
    console: HPCON,
    inherited: &[HANDLE],
    command: &str,
    state_root: &Path,
    result_budget_millis: Option<u64>,
) -> PROCESS_INFORMATION {
    let mut attribute_bytes = 0_usize;
    unsafe {
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut attribute_bytes);
    }
    let mut attribute_storage =
        vec![0_usize; attribute_bytes.div_ceil(std::mem::size_of::<usize>())];
    let attribute_list = attribute_storage.as_mut_ptr().cast();
    assert_ne!(
        unsafe { InitializeProcThreadAttributeList(attribute_list, 2, 0, &mut attribute_bytes) },
        0
    );
    assert_ne!(
        unsafe {
            UpdateProcThreadAttribute(
                attribute_list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr().cast(),
                std::mem::size_of_val(inherited),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    assert_ne!(
        unsafe {
            UpdateProcThreadAttribute(
                attribute_list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as usize as *const std::ffi::c_void,
                std::mem::size_of::<HPCON>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );

    let executable = PathBuf::from(env!("CARGO_BIN_EXE_flux-exchange"));
    let application = wide(executable.as_os_str());
    let mut command_line = wide(OsStr::new(command));
    let mut environment = child_environment_with_budget(state_root, result_budget_millis);
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = std::ptr::null_mut();
    startup.StartupInfo.hStdOutput = std::ptr::null_mut();
    startup.StartupInfo.hStdError = std::ptr::null_mut();
    startup.lpAttributeList = attribute_list;
    let mut child = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            environment.as_mut_ptr().cast(),
            std::ptr::null(),
            (&startup as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
            &mut child,
        )
    };
    unsafe { DeleteProcThreadAttributeList(attribute_list) };
    assert_ne!(
        created,
        0,
        "create pseudoconsole-attached production process: {}",
        std::io::Error::last_os_error()
    );
    child
}

struct MintHelper {
    process: HANDLE,
    fxsa: OwnedHandle,
}

impl MintHelper {
    fn spawn(state_root: &Path, expires_at: u64) -> Self {
        let fxsa = NativePipe::new();
        let canary = NativePipe::new();
        let writer = fxsa.write.as_raw_handle() as HANDLE;
        let inherited = [writer];
        let mut attribute_bytes = 0_usize;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attribute_bytes);
        }
        let mut attribute_storage =
            vec![0_usize; attribute_bytes.div_ceil(std::mem::size_of::<usize>())];
        let attribute_list = attribute_storage.as_mut_ptr().cast();
        assert_ne!(
            unsafe {
                InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes)
            },
            0
        );
        assert_ne!(
            unsafe {
                UpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_ptr().cast(),
                    std::mem::size_of_val(&inherited),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );

        let executable = PathBuf::from(env!("CARGO_BIN_EXE_flux-exchange"));
        let application = wide(executable.as_os_str());
        let mut command_line = wide(OsStr::new(&format!(
            "\"{}\" local service-account-mint --id native-worker --expires-at {} --writer-handle {}",
            executable.display(),
            expires_at,
            writer as usize
        )));
        let mut environment = child_environment(state_root);
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = std::ptr::null_mut();
        startup.StartupInfo.hStdOutput = std::ptr::null_mut();
        startup.StartupInfo.hStdError = std::ptr::null_mut();
        startup.lpAttributeList = attribute_list;
        let mut child = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_NO_WINDOW
                    | CREATE_SUSPENDED
                    | CREATE_UNICODE_ENVIRONMENT,
                environment.as_mut_ptr().cast(),
                std::ptr::null(),
                (&startup as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
                &mut child,
            )
        };
        unsafe { DeleteProcThreadAttributeList(attribute_list) };
        assert_ne!(
            created,
            0,
            "create released Windows Service Account helper: {}",
            std::io::Error::last_os_error()
        );

        drop(fxsa.write);
        drop(canary.write);
        assert_pipe_closed_without_bytes(&canary.read);
        assert_ne!(unsafe { ResumeThread(child.hThread) }, u32::MAX);
        close(child.hThread);
        Self {
            process: child.hProcess,
            fxsa: fxsa.read,
        }
    }

    fn finish(mut self) -> Vec<u8> {
        let token = read_one_fxsa(&self.fxsa);
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 10_000) },
            WAIT_OBJECT_0,
            "released Windows helper exit deadline"
        );
        let mut code = u32::MAX;
        assert_ne!(unsafe { GetExitCodeProcess(self.process, &mut code) }, 0);
        assert_eq!(code, 0, "one FXSA plus receipt is helper exit zero");
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        token
    }
}

impl Drop for MintHelper {
    fn drop(&mut self) {
        if !self.process.is_null() {
            unsafe {
                TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, 5_000);
            }
            close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        }
    }
}

struct NativePipe {
    read: OwnedHandle,
    write: OwnedHandle,
}

impl NativePipe {
    fn new() -> Self {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let mut read = std::ptr::null_mut();
        let mut write = std::ptr::null_mut();
        assert_ne!(
            unsafe { CreatePipe(&mut read, &mut write, &attributes, 4_096) },
            0
        );
        clear_inherit(read);
        unsafe {
            Self {
                read: OwnedHandle::from_raw_handle(read.cast()),
                write: OwnedHandle::from_raw_handle(write.cast()),
            }
        }
    }
}

fn assert_pipe_closed_without_bytes(read: &OwnedHandle) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let mut available = 0_u32;
        let result = unsafe {
            PeekNamedPipe(
                read.as_raw_handle() as HANDLE,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if result == 0 && unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
            return;
        }
        assert_eq!(available, 0, "unrelated canary received bytes");
        assert!(
            Instant::now() < deadline,
            "suspended helper inherited an unlisted canary capability"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn read_one_fxsa(read: &OwnedHandle) -> Vec<u8> {
    let mut header = [0_u8; 12];
    read_exact_handle(read, &mut header);
    assert_eq!(&header[..4], b"FXSA");
    assert_eq!(&header[4..8], &[1, 1, 0, 0]);
    let length = u32::from_be_bytes(header[8..12].try_into().expect("FXSA length")) as usize;
    assert!((1..=512).contains(&length));
    let mut token = vec![0_u8; length];
    read_exact_handle(read, &mut token);
    let mut surplus = [0_u8; 1];
    assert_eq!(
        read_handle(read, &mut surplus),
        0,
        "FXSA writer closes at EOF"
    );
    token
}

fn read_exact_handle(read: &OwnedHandle, mut output: &mut [u8]) {
    while !output.is_empty() {
        let received = read_handle(read, output);
        assert!(received != 0, "native pipe closed before complete frame");
        output = &mut output[received..];
    }
}

fn read_handle(read: &OwnedHandle, output: &mut [u8]) -> usize {
    let mut received = 0_u32;
    let result = unsafe {
        ReadFile(
            read.as_raw_handle() as HANDLE,
            output.as_mut_ptr(),
            output.len() as u32,
            &mut received,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        assert_eq!(unsafe { GetLastError() }, ERROR_BROKEN_PIPE);
        0
    } else {
        received as usize
    }
}

fn write_handle_all(write: &OwnedHandle, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let mut written = 0_u32;
        let success = unsafe {
            WriteFile(
                write.as_raw_handle() as HANDLE,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(success, 0, "native capability write");
        assert_ne!(written, 0, "native capability write made no progress");
        bytes = &bytes[written as usize..];
    }
}

fn read_owned_to_end(handle: OwnedHandle) -> Vec<u8> {
    let mut bytes = Vec::new();
    std::fs::File::from(handle)
        .read_to_end(&mut bytes)
        .expect("native capability read to EOF");
    bytes
}

fn assert_process_exit(process: HANDLE, expected: u32, surface: &str) {
    let mut code = u32::MAX;
    assert_ne!(unsafe { GetExitCodeProcess(process, &mut code) }, 0);
    assert_eq!(code, expected, "{surface} exit code");
}

fn committed_mint_receipt(stored: &[u8], expires_at: u64) -> String {
    let stored: Value = serde_json::from_slice(stored).expect("Service Account JSON");
    let receipts = stored["mint_receipts"]
        .as_object()
        .expect("durable receipt map");
    assert_eq!(receipts.len(), 1);
    let (receipt_id, receipt) = receipts.iter().next().expect("one mint receipt");
    assert_nonzero_lowerhex(receipt_id);
    assert_eq!(receipt["tenant"], "local");
    assert_eq!(receipt["id"], "native-worker");
    assert_eq!(receipt["expires_at"], expires_at);
    assert_eq!(receipt["state"], "committed");
    receipt_id.clone()
}

fn transformed_forms(raw: &[u8]) -> Vec<Vec<u8>> {
    let text = std::str::from_utf8(raw).expect("token UTF-8");
    let json = serde_json::to_string(text).expect("JSON token encoding");
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
            encoded.extend_from_slice(&[
                b'%',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 0x0f)],
            ]);
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
        encoded.push(ALPHABET[usize::from(((first & 3) << 4) | (second >> 4))]);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[usize::from(((second & 15) << 2) | (third >> 6))]
        } else {
            b'='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[usize::from(third & 63)]
        } else {
            b'='
        });
    }
    encoded
}

fn assert_forms_absent(bytes: &[u8], forms: &[Vec<u8>], surface: &str) {
    for (index, form) in forms.iter().enumerate() {
        assert!(
            !bytes
                .windows(form.len())
                .any(|candidate| candidate == form.as_slice()),
            "secret representation {index} entered {surface}"
        );
    }
}

fn assert_tree_excludes_except_credentials(root: &Path, forms: &[Vec<u8>]) {
    fn walk(path: &Path, root: &Path, forms: &[Vec<u8>]) {
        for entry in std::fs::read_dir(path).expect("read durable state directory") {
            let entry = entry.expect("durable state entry");
            let path = entry.path();
            if path
                .strip_prefix(root)
                .expect("state entry below root")
                .components()
                .next()
                .is_some_and(|component| component.as_os_str() == "credentials")
            {
                continue;
            }
            let kind = entry.file_type().expect("durable state entry type");
            if kind.is_dir() {
                walk(&path, root, forms);
            } else if kind.is_file() {
                let bytes = std::fs::read(&path).expect("read durable state file");
                assert_forms_absent(&bytes, forms, &path.display().to_string());
            }
        }
    }
    walk(root, root, forms);
}

fn open_owner_pipe() -> std::fs::File {
    let path = owner_pipe_name();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(pipe) => return pipe,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "owner pipe was not bound before readiness: {error}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn owner_pipe_name() -> String {
    let mut raw: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo process handle stays process-owned; success initializes one token handle.
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) },
        0
    );
    // SAFETY: OpenProcessToken returned one newly owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
    let mut length = 0_u32;
    let sized = unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut length,
        )
    };
    assert_eq!(sized, 0);
    assert_eq!(unsafe { GetLastError() }, ERROR_INSUFFICIENT_BUFFER);
    let capacity = length as usize;
    let mut storage = vec![0_usize; capacity.div_ceil(std::mem::size_of::<usize>())];
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenUser,
                storage.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        },
        0
    );
    assert!(capacity >= std::mem::size_of::<TOKEN_USER>());
    assert!(length as usize <= capacity);
    let base = storage.as_ptr().cast::<u8>();
    // SAFETY: the successful query initialized TOKEN_USER at this aligned buffer start.
    let sid = unsafe { (*(base.cast::<TOKEN_USER>())).User.Sid.cast::<u8>() };
    let offset = (sid as usize)
        .checked_sub(base as usize)
        .filter(|offset| *offset < capacity)
        .expect("TokenUser SID inside query buffer");
    assert_ne!(unsafe { IsValidSid(sid.cast()) }, 0);
    let sid_length = unsafe { GetLengthSid(sid.cast()) } as usize;
    assert!(offset
        .checked_add(sid_length)
        .is_some_and(|end| end <= capacity));
    // SAFETY: the validated SID byte range lies wholly inside the live query allocation.
    let sid = unsafe { std::slice::from_raw_parts(sid, sid_length) };
    let digest = Sha256::digest(sid);
    let suffix = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{PIPE_PREFIX}{suffix}")
}

fn decode_server_frame(bytes: &[u8]) -> (u16, &[u8]) {
    assert!(bytes.len() >= 12, "complete FXLM header");
    assert_eq!(&bytes[..4], b"FXLM");
    assert_eq!(bytes[4], 1);
    assert_eq!(bytes[5], SERVER);
    let length = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    assert_eq!(bytes.len(), 12 + length, "exactly one FXLM response");
    (u16::from_be_bytes([bytes[6], bytes[7]]), &bytes[12..])
}

fn decode_server_control(bytes: &[u8], expected_opcode: u16) -> Value {
    let (opcode, payload) = decode_server_frame(bytes);
    assert_eq!(opcode, expected_opcode);
    serde_json::from_slice(payload).expect("canonical server control JSON")
}

fn frame(direction: u8, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(12 + payload.len());
    frame.extend_from_slice(b"FXLM");
    frame.extend_from_slice(&[1, direction]);
    frame.extend_from_slice(&opcode.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn assert_nonzero_lowerhex(value: &str) {
    assert_eq!(value.len(), 64);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert!(value.bytes().any(|byte| byte != b'0'));
}

fn child_environment(state_root: &Path) -> Vec<u16> {
    child_environment_with_budget(state_root, None)
}

fn child_environment_with_budget(state_root: &Path, result_budget_millis: Option<u64>) -> Vec<u16> {
    let mut values = std::env::vars_os().collect::<BTreeMap<OsString, OsString>>();
    values.retain(|name, _| {
        !name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("FLUX_EXCHANGE_")
    });
    values.insert(
        "FLUX_EXCHANGE_STATE".into(),
        state_root.as_os_str().to_owned(),
    );
    if let Some(milliseconds) = result_budget_millis {
        values.insert(
            "FLUX_EXCHANGE_TEST_HELPER_RESULT_MILLIS".into(),
            milliseconds.to_string().into(),
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

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn clear_inherit(handle: HANDLE) {
    use windows_sys::Win32::Foundation::SetHandleInformation;
    // SAFETY: only the inheritance flag of this exact owned pipe handle changes.
    assert_ne!(
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) },
        0
    );
}

fn set_inherit(handle: HANDLE) {
    use windows_sys::Win32::Foundation::SetHandleInformation;
    assert_ne!(
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
        0
    );
}

fn close(handle: HANDLE) {
    if !handle.is_null() {
        // SAFETY: fixture ownership ensures every native handle is closed at most once.
        unsafe { CloseHandle(handle) };
    }
}
