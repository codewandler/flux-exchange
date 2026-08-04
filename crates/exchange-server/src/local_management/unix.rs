use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{UnixListener, UnixStream};

use super::codec::{Direction, StreamDecoder};
use super::dispatcher::Transport;
use super::{Dispatcher, TransactionCoordinator};
use crate::state::AppState;

const RUN_DIRECTORY: &str = "run";
const SOCKET_NAME: &str = "local-management-v1.sock";
const LOCAL_OWNER_TENANT: &str = "local";
const LOCAL_OWNER_PRINCIPAL: &str = "local-owner";
#[cfg(test)]
const UNAVAILABLE: &[u8] = br#"{"code":"local_management_unavailable","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":503}"#;

/// An owner-authenticated native management listener.
///
/// It is deliberately not an identity provider. The fixed local-owner projection exists only
/// while dispatching an already peer-authenticated stream, so nothing on loopback HTTP can present
/// the same spelling and acquire this authority.
pub(crate) struct LocalManagement {
    listener: UnixListener,
    socket: PathBuf,
    socket_device: u64,
    socket_inode: u64,
    expected_peer_uid: u32,
    dispatcher: Dispatcher,
}

impl LocalManagement {
    /// Bind the native endpoint only for the supervised local composition.
    pub(crate) fn bind_for_mode(
        supervised: bool,
        state: AppState,
        coordinator: Option<Arc<TransactionCoordinator>>,
    ) -> Result<Option<Self>, String> {
        if !supervised {
            return Ok(None);
        }
        let coordinator = coordinator.ok_or_else(|| {
            "the supervised local-management endpoint has no transaction coordinator".to_owned()
        })?;
        let dispatcher = Dispatcher::new(state, coordinator);
        let root = endpoint_root()?;
        let startup_euid = effective_uid();
        #[cfg(feature = "native-root-test-seam")]
        let mut endpoint = Self::bind_at(&root, startup_euid, dispatcher)?;
        #[cfg(not(feature = "native-root-test-seam"))]
        let endpoint = Self::bind_at(&root, startup_euid, dispatcher)?;
        #[cfg(feature = "native-root-test-seam")]
        if let Some(value) = std::env::var_os("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_PEER_UID") {
            let value = value
                .to_str()
                .ok_or_else(|| "the test local-management peer uid is not Unicode".to_owned())?;
            let parsed = value.parse::<u32>().map_err(|_| {
                "the test local-management peer uid is not canonical u32 decimal".to_owned()
            })?;
            if parsed.to_string() != value {
                return Err(
                    "the test local-management peer uid is not canonical u32 decimal".to_owned(),
                );
            }
            endpoint.expected_peer_uid = parsed;
        }
        Ok(Some(endpoint))
    }

    fn bind_at(root: &Path, startup_euid: u32, dispatcher: Dispatcher) -> Result<Self, String> {
        inspect_private_directory(root, startup_euid, 0o700, "native Exchange root")?;
        let run = root.join(RUN_DIRECTORY);
        create_private_run_directory(&run)?;
        inspect_private_directory(&run, startup_euid, 0o700, "local-management run directory")?;

        let socket = run.join(SOCKET_NAME);
        match std::fs::symlink_metadata(&socket) {
            Ok(metadata) => {
                return Err(format!(
                    "refusing local-management endpoint `{}`: an existing {} is planted there; Exchange did not remove or replace it",
                    socket.display(),
                    object_kind(&metadata)
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect local-management endpoint `{}` without following it: {error}",
                    socket.display()
                ));
            }
        }

        let listener = std::os::unix::net::UnixListener::bind(&socket).map_err(|error| {
            format!(
                "cannot bind local-management endpoint `{}` without replacing an existing object: {error}",
                socket.display()
            )
        })?;
        let bound_metadata = std::fs::symlink_metadata(&socket).map_err(|error| {
            format!(
                "cannot inspect freshly bound local-management endpoint `{}`: {error}",
                socket.display()
            )
        })?;
        if !bound_metadata.file_type().is_socket() || bound_metadata.uid() != startup_euid {
            return Err(format!(
                "refusing freshly bound local-management endpoint `{}`: the pathname no longer identifies the owner socket Exchange created",
                socket.display()
            ));
        }
        let bound_device = bound_metadata.dev();
        let bound_inode = bound_metadata.ino();
        let result = (|| {
            // No untrusted account can enter the already-verified 0700 parent while the freshly
            // created socket is narrowed. This completes creation; existing metadata is never
            // chmodded as a repair.
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    format!(
                        "cannot set freshly created local-management endpoint `{}` to owner mode 0600: {error}",
                        socket.display()
                    )
                },
            )?;
            let metadata = std::fs::symlink_metadata(&socket).map_err(|error| {
                format!(
                    "cannot inspect freshly bound local-management endpoint `{}`: {error}",
                    socket.display()
                )
            })?;
            if !metadata.file_type().is_socket()
                || metadata.uid() != startup_euid
                || metadata.mode() & 0o7777 != 0o600
                || metadata.dev() != bound_device
                || metadata.ino() != bound_inode
            {
                return Err(format!(
                    "refusing local-management endpoint `{}`: expected an owner socket for uid {startup_euid} at mode 0600, found {} owned by uid {} at mode {:04o}",
                    socket.display(),
                    object_kind(&metadata),
                    metadata.uid(),
                    metadata.mode() & 0o7777
                ));
            }
            listener.set_nonblocking(true).map_err(|error| {
                format!(
                    "cannot make local-management endpoint `{}` asynchronous: {error}",
                    socket.display()
                )
            })?;
            let listener = UnixListener::from_std(listener).map_err(|error| {
                format!(
                    "cannot adopt local-management endpoint `{}` into the async runtime: {error}",
                    socket.display()
                )
            })?;
            Ok(Self {
                listener,
                socket: socket.clone(),
                socket_device: bound_device,
                socket_inode: bound_inode,
                expected_peer_uid: startup_euid,
                dispatcher,
            })
        })();
        if result.is_err() {
            // This path did not predate the bind. Remove only the object this attempt just created;
            // planted/stale metadata encountered before bind is never changed.
            remove_exact_socket(&socket, bound_device, bound_inode);
        }
        result
    }

    /// Accept owner streams until shutdown. Peer authentication precedes the first byte read.
    pub(crate) async fn serve(self) {
        loop {
            let Ok((stream, _)) = self.listener.accept().await else {
                return;
            };
            let expected_peer_uid = self.expected_peer_uid;
            let dispatcher = self.dispatcher.clone();
            tokio::spawn(async move {
                if authenticate_peer(&stream, expected_peer_uid).is_ok() {
                    let owner = LocalOwner::authenticated();
                    let _closed_projection =
                        (owner.tenant, owner.principal, owner.user, owner.operator);
                    let _ = dispatch_one(stream, dispatcher).await;
                }
            });
        }
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.socket
    }
}

impl Drop for LocalManagement {
    fn drop(&mut self) {
        // Never unlink a replacement planted after startup. Cleanup is limited to the exact socket
        // inode this process created, making an ordinary graceful restart possible while crashes
        // remain visible as stale metadata that the next start refuses.
        if let Ok(metadata) = std::fs::symlink_metadata(&self.socket) {
            if metadata.file_type().is_socket()
                && metadata.dev() == self.socket_device
                && metadata.ino() == self.socket_inode
            {
                remove_exact_socket(&self.socket, self.socket_device, self.socket_inode);
            }
        }
    }
}

fn remove_exact_socket(path: &Path, device: u64, inode: u64) {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_socket() && metadata.dev() == device && metadata.ino() == inode {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct LocalOwner {
    tenant: &'static str,
    principal: &'static str,
    user: bool,
    operator: bool,
}

impl LocalOwner {
    const fn authenticated() -> Self {
        Self {
            tenant: LOCAL_OWNER_TENANT,
            principal: LOCAL_OWNER_PRINCIPAL,
            user: true,
            operator: true,
        }
    }
}

async fn dispatch_one(mut stream: UnixStream, dispatcher: Dispatcher) -> io::Result<()> {
    let mut decoder = StreamDecoder::new(Direction::ClientToServer);
    let mut bytes = [0_u8; 4096];
    loop {
        let received = stream.read(&mut bytes).await?;
        if received == 0 {
            let _ = decoder.finish();
            return Ok(());
        }
        if decoder.push(&bytes[..received]).is_err() {
            return Ok(());
        }
        match decoder.next_frame() {
            Ok(Some(request)) => {
                let response = dispatcher
                    .dispatch_frame(
                        Transport::Native,
                        &exchange_host::Tenant::new(LOCAL_OWNER_TENANT)
                            .expect("the fixed native owner tenant is valid"),
                        request,
                    )
                    .await
                    .encode();
                stream.write_all(&response).await?;
                stream.shutdown().await?;
                return Ok(());
            }
            Ok(None) => {}
            Err(_) => return Ok(()),
        }
    }
}

fn authenticate_peer(stream: &UnixStream, startup_euid: u32) -> Result<(), String> {
    let peer = peer_uid(stream)?;
    authenticate_uid(peer, startup_euid)
}

fn authenticate_uid(peer: u32, startup_euid: u32) -> Result<(), String> {
    if peer == startup_euid {
        Ok(())
    } else {
        Err(format!(
            "local-management peer uid {peer} does not match startup effective uid {startup_euid}"
        ))
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Result<u32, String> {
    let mut credential = std::mem::MaybeUninit::<libc::ucred>::zeroed();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the stream descriptor is live and the output region is sized exactly for ucred.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credential.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(format!(
            "SO_PEERCRED refused the accepted local-management stream: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: getsockopt succeeded with the complete expected output size.
    Ok(unsafe { credential.assume_init() }.uid)
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Result<u32, String> {
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: the stream descriptor is live and both output pointers reference initialized values.
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if result != 0 {
        return Err(format!(
            "getpeereid refused the accepted local-management stream: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(uid)
}

fn endpoint_root() -> Result<PathBuf, String> {
    #[cfg(feature = "native-root-test-seam")]
    if let Some(root) = std::env::var_os("FLUX_EXCHANGE_TEST_LOCAL_MANAGEMENT_ROOT") {
        return Ok(PathBuf::from(root));
    }
    crate::native_root::authenticated_account_state_root()
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no pointer arguments or preconditions.
    unsafe { libc::geteuid() }
}

fn create_private_run_directory(path: &Path) -> Result<(), String> {
    let native = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        format!(
            "refusing local-management run directory `{}`: path contains a NUL byte",
            path.display()
        )
    })?;
    // SAFETY: the NUL-terminated path remains live for this call. mkdir creates exactly the final
    // component and never follows a planted final symlink.
    let result = unsafe { libc::mkdir(native.as_ptr(), 0o700) };
    if result == 0 || io::Error::last_os_error().kind() == io::ErrorKind::AlreadyExists {
        Ok(())
    } else {
        Err(format!(
            "cannot create local-management run directory `{}` at owner mode 0700: {}",
            path.display(),
            io::Error::last_os_error()
        ))
    }
}

fn inspect_private_directory(
    path: &Path,
    startup_euid: u32,
    required_mode: u32,
    description: &str,
) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!(
            "cannot inspect {description} `{}` without following it: {error}",
            path.display()
        )
    })?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != startup_euid
        || metadata.mode() & 0o7777 != required_mode
    {
        return Err(format!(
            "refusing {description} `{}`: expected an owner directory for uid {startup_euid} at mode {required_mode:04o}, found {} owned by uid {} at mode {:04o}; Exchange did not chmod, chown, follow or replace it",
            path.display(),
            object_kind(&metadata),
            metadata.uid(),
            metadata.mode() & 0o7777
        ));
    }
    Ok(())
}

fn object_kind(metadata: &std::fs::Metadata) -> &'static str {
    let kind = metadata.file_type();
    if kind.is_symlink() {
        "symlink"
    } else if kind.is_socket() {
        "socket"
    } else if kind.is_dir() {
        "directory"
    } else if kind.is_file() {
        "file"
    } else {
        "special object"
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::{symlink, MetadataExt as _, PermissionsExt as _};
    use std::os::unix::net::UnixStream as StdUnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::local_management::codec::{Frame, Opcode};

    fn private_root(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-x134-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("private fixture root");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only fixture root");
        path
    }

    fn test_dispatcher(root: &Path) -> Dispatcher {
        let store = exchange_host::CredentialStore::bind(root.join("test-credentials/store"))
            .expect("one test credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("test-coordinator/transactions.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("test coordinator"),
        );
        Dispatcher::new(AppState::without_identity(), coordinator)
    }

    #[tokio::test]
    async fn endpoint_is_owner_only_and_same_owner_receives_closed_fxlm_refusal() {
        let root = private_root("same-owner");
        let endpoint = LocalManagement::bind_at(&root, effective_uid(), test_dispatcher(&root))
            .expect("owner endpoint");
        let path = endpoint.path().to_owned();
        let run = root.join(RUN_DIRECTORY);
        let run_metadata = std::fs::symlink_metadata(&run).expect("run metadata");
        let socket_metadata = std::fs::symlink_metadata(&path).expect("socket metadata");
        assert_eq!(run_metadata.uid(), effective_uid());
        assert_eq!(run_metadata.mode() & 0o7777, 0o700);
        assert_eq!(socket_metadata.uid(), effective_uid());
        assert_eq!(socket_metadata.mode() & 0o7777, 0o600);

        let server = tokio::spawn(endpoint.serve());
        let response = tokio::task::spawn_blocking(move || {
            let mut stream = StdUnixStream::connect(path).expect("same-owner connect");
            let request = Frame::control(
                Direction::ClientToServer,
                Opcode::PlanQuery,
                br#"{"connector":"gitlab","selection":null}"#.to_vec(),
            )
            .expect("plan query frame")
            .encode();
            for chunk in request.chunks(3) {
                stream.write_all(chunk).expect("split native write");
            }
            stream
                .shutdown(std::net::Shutdown::Write)
                .expect("request EOF");
            let mut response = Vec::new();
            stream.read_to_end(&mut response).expect("closed response");
            response
        })
        .await
        .expect("client task");
        server.abort();

        let expected = Frame::control(
            Direction::ServerToClient,
            Opcode::Error,
            UNAVAILABLE.to_vec(),
        )
        .expect("fixed response")
        .encode();
        assert_eq!(response, expected);
        assert!(!String::from_utf8_lossy(&response).contains(LOCAL_OWNER_PRINCIPAL));
        assert!(!String::from_utf8_lossy(&response).contains("\"tenant\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn injected_wrong_peer_is_closed_before_any_fxlm_byte_is_read() {
        let root = private_root("wrong-peer");
        let mut endpoint = LocalManagement::bind_at(&root, effective_uid(), test_dispatcher(&root))
            .expect("owner endpoint binds");
        endpoint.expected_peer_uid = effective_uid().wrapping_add(1);
        let path = endpoint.path().to_owned();
        let server = tokio::spawn(endpoint.serve());
        let received = tokio::task::spawn_blocking(move || {
            let mut stream = StdUnixStream::connect(path).expect("connect before peer refusal");
            let mut byte = [0_u8; 1];
            stream.read(&mut byte).expect("peer refusal EOF")
        })
        .await
        .expect("client task");
        server.abort();
        assert_eq!(received, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn wrong_peer_uid_is_never_accepted_as_filesystem_authority() {
        assert!(authenticate_uid(effective_uid(), effective_uid()).is_ok());
        assert!(authenticate_uid(effective_uid().wrapping_add(1), effective_uid()).is_err());
    }

    #[test]
    fn dev_and_ordinary_modes_do_not_resolve_or_bind_an_endpoint() {
        assert!(
            LocalManagement::bind_for_mode(false, AppState::without_identity(), None)
                .expect("non-supervised mode")
                .is_none()
        );
    }

    #[test]
    fn planted_symlink_and_stale_socket_refuse_without_removal() {
        let symlink_root = private_root("planted-symlink");
        let symlink_run = symlink_root.join(RUN_DIRECTORY);
        symlink(&symlink_root, &symlink_run).expect("planted run symlink");
        let refusal = match LocalManagement::bind_at(
            &symlink_root,
            effective_uid(),
            test_dispatcher(&symlink_root),
        ) {
            Ok(_) => panic!("run symlink must refuse"),
            Err(refusal) => refusal,
        };
        assert!(refusal.contains("symlink"), "{refusal}");
        assert!(symlink_run.is_symlink());

        let stale_root = private_root("stale-socket");
        let stale_run = stale_root.join(RUN_DIRECTORY);
        std::fs::create_dir(&stale_run).expect("stale run directory");
        std::fs::set_permissions(&stale_run, std::fs::Permissions::from_mode(0o700))
            .expect("private stale run directory");
        let stale_path = stale_run.join(SOCKET_NAME);
        let stale = std::os::unix::net::UnixListener::bind(&stale_path).expect("stale socket");
        let refusal = match LocalManagement::bind_at(
            &stale_root,
            effective_uid(),
            test_dispatcher(&stale_root),
        ) {
            Ok(_) => panic!("stale socket must refuse"),
            Err(refusal) => refusal,
        };
        assert!(refusal.contains("existing socket"), "{refusal}");
        assert!(stale_path.exists());
        drop(stale);

        let _ = std::fs::remove_dir_all(symlink_root);
        let _ = std::fs::remove_dir_all(stale_root);
    }

    #[test]
    fn local_owner_projection_is_closed_and_operator_only_inside_dispatch() {
        let owner = LocalOwner::authenticated();
        assert_eq!(owner.tenant, "local");
        assert_eq!(owner.principal, "local-owner");
        assert!(owner.user);
        assert!(owner.operator);
    }
}
