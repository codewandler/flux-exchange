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

struct NativeProcess {
    process: HANDLE,
    pid: u32,
    readiness: HANDLE,
    liveness: HANDLE,
    state_root: PathBuf,
}

impl NativeProcess {
    fn spawn(wedge: bool) -> Self {
        let state_root = std::env::temp_dir().join(format!(
            "flux-exchange-x128-{}-{}",
            std::process::id(),
            unique_counter()
        ));
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
        bytes
    }

    fn close_liveness(&mut self) {
        if !self.liveness.is_null() {
            close(self.liveness);
            self.liveness = std::ptr::null_mut();
        }
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

#[test]
fn real_windows_handle_list_readiness_identity_and_native_liveness() {
    let mut server = NativeProcess::spawn(false);
    let bytes = server.readiness();
    assert!(!bytes.is_empty());
    assert!(bytes.len() <= 16 * 1024);
    let ready: serde_json::Value = serde_json::from_slice(&bytes).expect("readiness object");
    assert_eq!(ready["process"]["pid"], server.pid);
    assert_eq!(
        ready["process"]["start_identity"]["kind"],
        "windows-process-creation"
    );
    assert_eq!(
        ready["process"]["start_identity"]["filetime"],
        creation_filetime(server.process).to_string()
    );
    let compatibility = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .args(["compatibility", "--json"])
        .output()
        .expect("compatibility process");
    let compatibility: serde_json::Value =
        serde_json::from_slice(&compatibility.stdout).expect("compatibility object");
    assert_eq!(ready["protocols"], compatibility["protocols"]);
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
    server.close_liveness();
    server.wait_dead();
}

#[test]
fn windows_supervisor_helper_process() {
    if std::env::var_os("X128_RUN_WINDOWS_SUPERVISOR_HELPER").is_none() {
        return;
    }
    let mut server = NativeProcess::spawn(true);
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
    let mut helper = Command::new(std::env::current_exe().expect("integration test executable"))
        .args([
            "--exact",
            "windows_supervisor_helper_process",
            "--nocapture",
        ])
        .env("X128_RUN_WINDOWS_SUPERVISOR_HELPER", "1")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Windows supervisor helper");
    let mut reader = std::io::BufReader::new(helper.stdout.take().expect("helper stdout"));
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
            "1",
            "--supervisor-liveness-handle",
            "1",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
            .args(arguments)
            .output()
            .expect("malformed ABI process");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
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
