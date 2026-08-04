#![cfg(all(
    any(target_os = "linux", target_os = "macos", windows),
    feature = "native-root-test-seam"
))]

//! Native process evidence for the C-515 credential-store lifetime lease.
//!
//! The server must retain the one registry-resolved 0.20 `FileStore` through coordinator recovery
//! and readiness. A separate process therefore cannot reopen the same path while the server is
//! alive, while an abrupt server exit must release the provider-owned kernel lease without a
//! cleanup, replacement or repair step.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use exchange_host::{
    CredentialScope, CredentialStore, CredentialStoreError, PreparedSecretStore, SecretBatch,
    SecretProposalDigest, SecretTransactionGeneration, SecretTransactionId, SecretTransactionState,
    StoreError,
};

const HELPER_MODE: &str = "FLUX_EXCHANGE_C515_LEASE_HELPER";
const HELPER_STORE: &str = "FLUX_EXCHANGE_C515_LEASE_STORE";
const REFUSE: &str = "refuse";
const REOPEN: &str = "reopen";
const STORE_OVERRIDES: [&str; 8] = [
    "FLUX_EXCHANGE_CREDENTIALS",
    "FLUX_EXCHANGE_SETTINGS",
    "FLUX_EXCHANGE_GRANTS",
    "FLUX_EXCHANGE_CONNECTIONS",
    "FLUX_EXCHANGE_CHANNELS",
    "FLUX_EXCHANGE_WORKFLOWS",
    "FLUX_EXCHANGE_AUDIT",
    "FLUX_EXCHANGE_SERVICE_ACCOUNTS",
];

fn transaction_id() -> SecretTransactionId {
    let generation = SecretTransactionGeneration::from_protocol_bytes(0x134_u64.to_be_bytes())
        .expect("the fixture generation is nonzero");
    SecretTransactionId::new(generation, [0x51; 24])
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("fixture runtime")
        .block_on(future)
}

fn seed_committed_provider_state(path: &Path) {
    let store = CredentialStore::bind(path).expect("fresh registry 0.20 credential store");
    let provider = store.prepared_secrets();
    let id = transaction_id();
    let digest = SecretProposalDigest::from_protocol_bytes([0x73; 32]);
    let batch = SecretBatch::new(
        CredentialScope::new("local", "com.example.api").expect("fixture credential scope"),
    );
    assert_eq!(
        block_on(provider.state(id)),
        Ok(SecretTransactionState::Absent)
    );
    assert_eq!(
        block_on(provider.prepare(id, digest, &batch)),
        Ok(SecretTransactionState::Prepared)
    );
    assert_eq!(
        block_on(provider.commit(id)),
        Ok(SecretTransactionState::Committed)
    );
    drop(provider);
    drop(store);
}

fn assert_provider_lease_conflict(error: CredentialStoreError) {
    let rendered = error.to_string();
    assert!(
        matches!(
            error,
            CredentialStoreError::Unusable {
                source: StoreError::Conflict { .. },
                ..
            }
        ),
        "the second process must receive the provider's exact lifetime-lease conflict: {rendered}"
    );
}

#[test]
fn c515_lease_opener_process() {
    let Some(mode) = std::env::var_os(HELPER_MODE) else {
        return;
    };
    let path = PathBuf::from(std::env::var_os(HELPER_STORE).expect("helper store path"));
    match mode.to_str().expect("ASCII helper mode") {
        REFUSE => {
            let error = CredentialStore::bind(&path)
                .expect_err("a second process must not open the live server's store");
            assert_provider_lease_conflict(error);
        }
        REOPEN => {
            let store = CredentialStore::bind(&path)
                .expect("abrupt server exit must release the provider-owned lease");
            let provider: std::sync::Arc<dyn PreparedSecretStore> = store.prepared_secrets();
            assert_eq!(
                block_on(provider.state(transaction_id())),
                Ok(SecretTransactionState::Committed),
                "reopening must preserve the C-515 terminal state; it must not replace the store"
            );
        }
        unexpected => panic!("unexpected helper mode {unexpected}"),
    }
}

fn run_opener(path: &Path, mode: &str) {
    let output = Command::new(std::env::current_exe().expect("integration test executable"))
        .arg("--exact")
        .arg("c515_lease_opener_process")
        .arg("--nocapture")
        .env(HELPER_MODE, mode)
        .env(HELPER_STORE, path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("separate credential opener process");
    assert!(
        output.status.success(),
        "{mode} opener failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn real_server_retains_the_c515_lease_through_recovery_and_readiness() {
    let root = private_root();
    let store_path = root.join("credentials/store.txt");
    seed_committed_provider_state(&store_path);

    let mut server = NativeServer::spawn(&root);
    let readiness = server.readiness();
    let ready: serde_json::Value =
        serde_json::from_slice(&readiness).expect("canonical supervisor readiness");
    assert_eq!(ready["schema"], "exchange.supervisor-ready.v2");
    assert!(
        root.join("coordinator/transactions.sqlite3").is_file(),
        "the production coordinator must bind and recover before readiness"
    );

    run_opener(&store_path, REFUSE);
    server.abrupt_exit();
    run_opener(&store_path, REOPEN);

    drop(server);
    std::fs::remove_dir_all(&root).expect("fixture cleanup after every process released the store");
}

fn private_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "flux-exchange-c515-process-lease-{}-{}",
        std::process::id(),
        unique_counter()
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::create_dir(&root).expect("private fixture root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture root");
    }
    root
}

fn unique_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(unix)]
mod platform {
    use std::io::Read as _;
    use std::os::fd::{FromRawFd as _, RawFd};
    use std::os::unix::process::CommandExt as _;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    struct PipeEnds {
        read: RawFd,
        write: RawFd,
    }

    impl PipeEnds {
        fn new() -> Self {
            let mut fds = [-1; 2];
            // SAFETY: the output array is valid and receives two owned descriptors on success.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            Self {
                read: fds[0],
                write: fds[1],
            }
        }
    }

    pub(super) struct NativeServer {
        child: Child,
        readiness: RawFd,
        liveness: RawFd,
        dead: bool,
    }

    impl NativeServer {
        pub(super) fn spawn(root: &Path) -> Self {
            let readiness = PipeEnds::new();
            let liveness = PipeEnds::new();
            let readiness_source = duplicate_high(readiness.write);
            let liveness_source = duplicate_high(liveness.read);
            let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
            command
                .arg("--supervised")
                .env("FLUX_EXCHANGE_STATE", root)
                .env("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT", root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            for setting in super::STORE_OVERRIDES {
                command.env_remove(setting);
            }
            // SAFETY: this closure uses only async-signal-safe descriptor operations before exec.
            unsafe {
                command.pre_exec(move || {
                    if libc::dup2(readiness_source, 3) < 0 || libc::dup2(liveness_source, 4) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    for fd in 5..256 {
                        libc::close(fd);
                    }
                    Ok(())
                });
            }
            let child = command.spawn().expect("real supervised Exchange process");
            close_fd(readiness.write);
            close_fd(liveness.read);
            close_fd(readiness_source);
            close_fd(liveness_source);
            Self {
                child,
                readiness: readiness.read,
                liveness: liveness.write,
                dead: false,
            }
        }

        pub(super) fn readiness(&mut self) -> Vec<u8> {
            let fd = std::mem::replace(&mut self.readiness, -1);
            // SAFETY: ownership of this pipe descriptor moves into the File exactly once.
            let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).expect("supervisor readiness");
            assert!(!bytes.is_empty(), "server exited before readiness");
            bytes
        }

        pub(super) fn abrupt_exit(&mut self) {
            self.child.kill().expect("abrupt server termination");
            self.child.wait().expect("reap abruptly terminated server");
            self.dead = true;
            close_fd(std::mem::replace(&mut self.liveness, -1));
        }
    }

    impl Drop for NativeServer {
        fn drop(&mut self) {
            if !self.dead {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
            close_fd(std::mem::replace(&mut self.readiness, -1));
            close_fd(std::mem::replace(&mut self.liveness, -1));
        }
    }

    fn duplicate_high(fd: RawFd) -> RawFd {
        // SAFETY: `fd` is live and F_DUPFD_CLOEXEC returns a separately owned descriptor.
        let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 32) };
        assert!(duplicated >= 32, "high descriptor duplication failed");
        duplicated
    }

    fn close_fd(fd: RawFd) {
        if fd >= 0 {
            // SAFETY: each test-owned descriptor is closed exactly once.
            unsafe { libc::close(fd) };
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
        TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW,
        CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTUPINFOEXW, STARTUPINFOW,
    };

    pub(super) struct NativeServer {
        process: HANDLE,
        readiness: HANDLE,
        liveness: HANDLE,
        dead: bool,
    }

    impl NativeServer {
        pub(super) fn spawn(root: &Path) -> Self {
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };
            let mut readiness_read = std::ptr::null_mut();
            let mut readiness_write = std::ptr::null_mut();
            let mut liveness_read = std::ptr::null_mut();
            let mut liveness_write = std::ptr::null_mut();
            // SAFETY: output pointers and security attributes remain valid for both calls.
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
            // SAFETY: this sizing call writes only the required byte count.
            unsafe {
                InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attribute_bytes);
            }
            let words = attribute_bytes.div_ceil(std::mem::size_of::<usize>());
            let mut attribute_storage = vec![0_usize; words];
            let attribute_list = attribute_storage.as_mut_ptr().cast();
            // SAFETY: aligned storage has the exact size reported by the sizing call.
            assert_ne!(
                unsafe {
                    InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes)
                },
                0
            );
            // SAFETY: the two-handle array remains live through CreateProcessW.
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
            let mut environment = current_environment(root);
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = attribute_list;
            let mut process = PROCESS_INFORMATION::default();
            // SAFETY: all pointers reference live native structures and nul-terminated buffers;
            // the explicit inherited list is exactly readiness-write plus liveness-read.
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
            // SAFETY: CreateProcessW no longer reads the initialized attribute list.
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
                dead: false,
            }
        }

        pub(super) fn readiness(&mut self) -> Vec<u8> {
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let mut read = 0_u32;
                // SAFETY: buffer/count are live and readiness is this process's pipe handle.
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
            assert!(!bytes.is_empty(), "server exited before readiness");
            bytes
        }

        pub(super) fn abrupt_exit(&mut self) {
            // SAFETY: this is the exact still-open child handle returned by CreateProcessW.
            assert_ne!(unsafe { TerminateProcess(self.process, 137) }, 0);
            assert_eq!(
                unsafe { WaitForSingleObject(self.process, 5_000) },
                WAIT_OBJECT_0
            );
            self.dead = true;
            close(std::mem::replace(&mut self.liveness, std::ptr::null_mut()));
        }
    }

    impl Drop for NativeServer {
        fn drop(&mut self) {
            if !self.dead {
                // SAFETY: cleanup acts only on the exact still-open child handle.
                unsafe {
                    TerminateProcess(self.process, 1);
                    WaitForSingleObject(self.process, 5_000);
                }
            }
            close(std::mem::replace(&mut self.readiness, std::ptr::null_mut()));
            close(std::mem::replace(&mut self.liveness, std::ptr::null_mut()));
            close(std::mem::replace(&mut self.process, std::ptr::null_mut()));
        }
    }

    fn current_environment(state_root: &Path) -> Vec<u16> {
        let mut values = std::env::vars_os().collect::<BTreeMap<OsString, OsString>>();
        for setting in super::STORE_OVERRIDES {
            values.remove(OsStr::new(setting));
        }
        values.insert(
            "FLUX_EXCHANGE_STATE".into(),
            state_root.as_os_str().to_owned(),
        );
        values.insert(
            "FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT".into(),
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
        // SAFETY: only the inheritance flag of this live, owned handle changes.
        assert_ne!(
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) },
            0
        );
    }

    fn close(handle: HANDLE) {
        if !handle.is_null() {
            // SAFETY: test ownership ensures each native handle is closed exactly once.
            unsafe { CloseHandle(handle) };
        }
    }
}

use platform::NativeServer;
