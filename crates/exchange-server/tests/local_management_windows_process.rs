#![cfg(windows)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_INSUFFICIENT_BUFFER, HANDLE, HANDLE_FLAG_INHERIT,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
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

const PIPE_PREFIX: &str = r"\\.\pipe\flux-exchange-local-management-v1-";
const CLIENT: u8 = 1;
const SERVER: u8 = 2;
const PLAN_QUERY: u16 = 0x0007;
const PLAN_RESPONSE: u16 = 0x0008;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct SupervisedServer {
    process: HANDLE,
    readiness: HANDLE,
    liveness: HANDLE,
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
        close(process.hThread);
        Self {
            process: process.hProcess,
            readiness: readiness_read,
            liveness: liveness_write,
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

    fn stop(mut self) {
        close(std::mem::replace(&mut self.liveness, std::ptr::null_mut()));
        // SAFETY: this is the exact still-open child process handle.
        assert_eq!(
            unsafe { WaitForSingleObject(self.process, 5_000) },
            WAIT_OBJECT_0,
            "liveness EOF did not stop the supervised process"
        );
        close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        let _ = std::fs::remove_dir_all(&self.state_root);
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
        let _ = std::fs::remove_dir_all(&self.state_root);
    }
}

#[test]
fn supervised_owner_pipe_serves_exact_plan_while_loopback_tcp_cannot_bootstrap() {
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

    server.stop();
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

fn close(handle: HANDLE) {
    if !handle.is_null() {
        // SAFETY: fixture ownership ensures every native handle is closed at most once.
        unsafe { CloseHandle(handle) };
    }
}
