use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{Read as _, Seek as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, Interest};
use tokio::net::{UnixListener, UnixStream};

use super::codec::{Direction, StreamDecoder};
use super::dispatcher::{expired_reply, native_frame_refusal, Transport};
use super::service_account::OneShotWriter;
use super::service_account_handoff::unix_transfer::{receive_initial_fd, UnixHandoffError};
use super::{
    deadline::{finalize_native_connection, write_native_terminal},
    ActiveSession, DeadlineController, Dispatcher, SessionAdvance, SessionBegin,
    TransactionCoordinator,
};
use crate::state::AppState;

#[cfg(any(test, feature = "native-deadline-test-seam"))]
use super::deadline::finalize_native_terminal;

const RUN_DIRECTORY: &str = "run";
const SOCKET_NAME: &str = "local-management-v1.sock";
const LEASE_NAME: &str = "local-management-v1.lease";
const LEASE_SCHEMA: &str = "exchange.local-management-lease.v1";
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
    _lease: EndpointLease,
    expected_peer_uid: u32,
    dispatcher: Dispatcher,
    #[cfg(any(test, feature = "native-deadline-test-seam"))]
    deadline_override: Option<DeadlineController>,
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
        let mut lease = EndpointLease::acquire(&run, &socket, startup_euid)?;

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
            lease.record_socket(bound_device, bound_inode)?;
            Ok(Self {
                listener,
                socket: socket.clone(),
                socket_device: bound_device,
                socket_inode: bound_inode,
                _lease: lease,
                expected_peer_uid: startup_euid,
                dispatcher,
                #[cfg(any(test, feature = "native-deadline-test-seam"))]
                deadline_override: None,
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
        #[cfg(any(test, feature = "native-deadline-test-seam"))]
        let deadline_override = self.deadline_override.clone();
        loop {
            let Ok((stream, _)) = self.listener.accept().await else {
                return;
            };
            let expected_peer_uid = self.expected_peer_uid;
            let dispatcher = self.dispatcher.clone();
            #[cfg(any(test, feature = "native-deadline-test-seam"))]
            let deadline_override = deadline_override.clone();
            tokio::spawn(async move {
                if authenticate_peer(&stream, expected_peer_uid).is_ok() {
                    let owner = LocalOwner::authenticated();
                    let _closed_projection =
                        (owner.tenant, owner.principal, owner.user, owner.operator);
                    let mut stream = stream;
                    #[cfg(any(test, feature = "native-deadline-test-seam"))]
                    let deadline = deadline_override.unwrap_or_else(DeadlineController::start);
                    #[cfg(not(any(test, feature = "native-deadline-test-seam")))]
                    let deadline = DeadlineController::start();
                    let mut initial = [0_u8; 65_548];
                    let received = deadline
                        .race(receive_initial_capability(
                            &stream,
                            expected_peer_uid,
                            &mut initial,
                        ))
                        .await;
                    let (writer, received) = match received {
                        Ok(Ok(received)) => received,
                        Ok(Err(_)) => {
                            finalize_connection(&mut stream, None).await;
                            return;
                        }
                        Err(expired) => {
                            let (reply, _) = expired_reply(expired).into_parts();
                            finalize_connection(&mut stream, Some(&reply)).await;
                            return;
                        }
                    };
                    if let Err(expired) = deadline
                        .race(dispatch_one(
                            &mut stream,
                            dispatcher,
                            &initial[..received],
                            writer,
                            &deadline,
                        ))
                        .await
                    {
                        let (reply, _) = expired_reply(expired).into_parts();
                        finalize_connection(&mut stream, Some(&reply)).await;
                    }
                }
            });
        }
    }

    #[cfg(any(test, feature = "native-deadline-test-seam"))]
    fn path(&self) -> &Path {
        &self.socket
    }
}

async fn finalize_connection(stream: &mut UnixStream, response: Option<&[u8]>) {
    finalize_native_connection(stream, response).await;
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

struct EndpointLease {
    file: File,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl EndpointLease {
    fn acquire(run: &Path, socket: &Path, owner: u32) -> Result<Self, String> {
        let path = run.join(LEASE_NAME);
        let (file, created) = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => (file, true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                    .open(&path)
                    .map_err(|error| {
                        format!(
                            "refusing local-management lease `{}` without following it: {error}",
                            path.display()
                        )
                    })?;
                (file, false)
            }
            Err(error) => {
                return Err(format!(
                    "cannot create local-management lease `{}` at owner mode 0600: {error}",
                    path.display()
                ));
            }
        };
        let metadata = file.metadata().map_err(|error| {
            format!(
                "cannot inspect local-management lease `{}`: {error}",
                path.display()
            )
        })?;
        let named = std::fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "cannot inspect local-management lease name `{}`: {error}",
                path.display()
            )
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != owner
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
            || metadata.dev() != named.dev()
            || metadata.ino() != named.ino()
        {
            if created {
                let _ = std::fs::remove_file(&path);
            }
            return Err(format!(
                "refusing local-management lease `{}`: expected one owner regular file for uid {owner} at mode 0600",
                path.display()
            ));
        }
        // SAFETY: flock operates on this owned regular-file descriptor and holds until it closes.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(format!(
                "refusing local-management lease `{}`: another server still owns the endpoint",
                path.display()
            ));
        }
        let mut lease = Self {
            file,
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        let existing = std::fs::symlink_metadata(socket);
        if created {
            return match existing {
                Ok(metadata) => {
                    lease.remove_exact();
                    Err(format!(
                    "refusing local-management endpoint `{}`: an existing {} has no Exchange lease and is planted there; Exchange did not remove or replace it",
                    socket.display(),
                    object_kind(&metadata)
                    ))
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(lease),
                Err(error) => {
                    lease.remove_exact();
                    Err(format!(
                        "cannot inspect local-management endpoint `{}` without following it: {error}",
                        socket.display()
                    ))
                }
            };
        }

        let mut record = String::new();
        lease
            .file
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|error| {
                format!(
                    "cannot read local-management lease `{}`: {error}",
                    lease.path.display()
                )
            })?;
        lease.file.read_to_string(&mut record).map_err(|error| {
            format!(
                "cannot read local-management lease `{}`: {error}",
                lease.path.display()
            )
        })?;
        match existing {
            Err(error) if error.kind() == io::ErrorKind::NotFound && record.is_empty() => Ok(lease),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                lease.file.set_len(0).map_err(|error| error.to_string())?;
                Ok(lease)
            }
            Err(error) => Err(format!(
                "cannot inspect local-management endpoint `{}` without following it: {error}",
                socket.display()
            )),
            Ok(metadata) => {
                let mut fields = record.trim_end_matches('\n').split(' ');
                let schema = fields.next();
                let device = fields.next().and_then(|value| value.parse::<u64>().ok());
                let inode = fields.next().and_then(|value| value.parse::<u64>().ok());
                if schema != Some(LEASE_SCHEMA)
                    || device != Some(metadata.dev())
                    || inode != Some(metadata.ino())
                    || fields.next().is_some()
                    || !metadata.file_type().is_socket()
                    || metadata.uid() != owner
                    || metadata.mode() & 0o7777 != 0o600
                {
                    return Err(format!(
                        "refusing local-management endpoint `{}`: its stale lease does not identify this exact owner socket",
                        socket.display()
                    ));
                }
                remove_exact_socket(socket, metadata.dev(), metadata.ino());
                if std::fs::symlink_metadata(socket).is_ok() {
                    return Err(format!(
                        "refusing local-management endpoint `{}`: the exact stale owner socket could not be removed",
                        socket.display()
                    ));
                }
                lease.file.set_len(0).map_err(|error| {
                    format!(
                        "cannot reset local-management lease `{}`: {error}",
                        lease.path.display()
                    )
                })?;
                Ok(lease)
            }
        }
    }

    fn record_socket(&mut self, device: u64, inode: u64) -> Result<(), String> {
        self.file
            .seek(std::io::SeekFrom::Start(0))
            .and_then(|_| self.file.set_len(0))
            .and_then(|_| writeln!(self.file, "{LEASE_SCHEMA} {device} {inode}"))
            .and_then(|_| self.file.sync_all())
            .map_err(|error| {
                format!(
                    "cannot publish local-management lease `{}`: {error}",
                    self.path.display()
                )
            })
    }

    fn remove_exact(&self) {
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path) {
            if metadata.is_file() && metadata.dev() == self.device && metadata.ino() == self.inode {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

impl Drop for EndpointLease {
    fn drop(&mut self) {
        self.remove_exact();
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

async fn receive_initial_capability(
    stream: &UnixStream,
    expected_peer_uid: u32,
    bytes: &mut [u8],
) -> io::Result<(Option<Box<dyn OneShotWriter>>, usize)> {
    stream
        .async_io(Interest::READABLE, || {
            receive_initial_fd(stream.as_raw_fd(), expected_peer_uid, bytes)
                .map(|(writer, received)| {
                    (
                        writer.map(|writer| Box::new(writer) as Box<dyn OneShotWriter>),
                        received,
                    )
                })
                .map_err(|refusal| match refusal {
                    UnixHandoffError::WouldBlock => io::ErrorKind::WouldBlock.into(),
                    _ => io::Error::other("local-management writer capability refused"),
                })
        })
        .await
}

async fn dispatch_one(
    stream: &mut UnixStream,
    dispatcher: Dispatcher,
    initial: &[u8],
    mut writer: Option<Box<dyn OneShotWriter>>,
    deadline: &DeadlineController,
) -> io::Result<()> {
    let mut decoder = StreamDecoder::new(Direction::ClientToServer);
    let mut bytes = [0_u8; 4096];
    let mut first = Some(initial);
    let mut active: Option<Box<ActiveSession>> = None;
    loop {
        let received = if let Some(initial) = first.take() {
            if let Err(error) = decoder.push(initial) {
                let response = native_frame_refusal(error);
                write_native_terminal(stream, &response, deadline).await;
                return Ok(());
            }
            initial.len()
        } else {
            let received = stream.read(&mut bytes).await?;
            if received != 0 {
                if let Err(error) = decoder.push(&bytes[..received]) {
                    if let Some(session) = active.as_mut() {
                        session.abort().await;
                    }
                    let response = native_frame_refusal(error);
                    write_native_terminal(stream, &response, deadline).await;
                    return Ok(());
                }
            }
            received
        };
        if received == 0 {
            if deadline.may_abort() {
                if let Some(session) = active.as_mut() {
                    session.abort().await;
                }
            }
            if let Err(error) = decoder.finish() {
                let response = native_frame_refusal(error);
                write_native_terminal(stream, &response, deadline).await;
            }
            return Ok(());
        }
        while let Some(request) = match decoder.next_frame() {
            Ok(frame) => frame,
            Err(error) => {
                if deadline.may_abort() {
                    if let Some(session) = active.as_mut() {
                        session.abort().await;
                    }
                }
                let response = native_frame_refusal(error);
                write_native_terminal(stream, &response, deadline).await;
                return Ok(());
            }
        } {
            if let Some(session) = active.as_mut() {
                match session.accept_frame(request).await {
                    SessionAdvance::Awaiting => {}
                    SessionAdvance::Terminal(reply) => {
                        let (response, _) = reply.into_parts();
                        write_native_terminal(stream, &response, deadline).await;
                        return Ok(());
                    }
                }
            } else {
                let tenant = exchange_host::Tenant::new(LOCAL_OWNER_TENANT)
                    .expect("the fixed native owner tenant is valid");
                match dispatcher
                    .begin_frame_with_writer(
                        Transport::Native,
                        &tenant,
                        request,
                        writer.take(),
                        deadline,
                    )
                    .await
                {
                    SessionBegin::Terminal(reply) => {
                        let (response, _) = reply.into_parts();
                        write_native_terminal(stream, &response, deadline).await;
                        return Ok(());
                    }
                    SessionBegin::Active { response, session } => {
                        deadline
                            .race_response(stream.write_all(&response))
                            .await
                            .map_err(|()| io::Error::from(io::ErrorKind::TimedOut))??;
                        active = Some(session);
                    }
                }
            }
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

#[cfg(feature = "native-deadline-test-seam")]
pub(crate) fn run_deadline_process_fixture() -> Result<(), String> {
    tests::run_deadline_process_fixture()
}

#[cfg(any(test, feature = "native-deadline-test-seam"))]
mod tests {
    #[cfg(test)]
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::PermissionsExt as _;
    #[cfg(test)]
    use std::os::unix::fs::{symlink, MetadataExt as _};
    #[cfg(test)]
    use std::os::unix::net::UnixStream as StdUnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};

    use exchange_host::{
        GrantReceiptId, GrantSelector, GrantStore, GrantTransactions as _, Tenant,
    };

    use super::*;
    use crate::audit::AuditJournal;
    use crate::local_management::codec::{Frame, Opcode};
    use crate::local_management::{Expired, ReceiptIdentity, Unresolved};

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

    fn grant_dispatcher(root: &Path, seed: bool) -> (Dispatcher, GrantReceiptId) {
        let store = exchange_host::CredentialStore::bind(root.join("test-credentials/store"))
            .expect("one test credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("test-coordinator/transactions.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("test coordinator"),
        );
        let audit = Arc::new(
            AuditJournal::bind(root.join("test-audit/events.sqlite3")).expect("test audit"),
        );
        let grants =
            Arc::new(GrantStore::bind(root.join("test-grants.json")).expect("grant store"));
        let receipt = GrantReceiptId::from_protocol_bytes([0x61; 32]).expect("receipt");
        if seed {
            let tenant = Tenant::new(LOCAL_OWNER_TENANT).expect("owner tenant");
            let selector: GrantSelector = serde_json::from_slice(
                br#"{"effects_within":null,"idempotency":null,"max_risk":"low"}"#,
            )
            .expect("selector");
            let candidate = grants
                .preview(&tenant, "github", selector)
                .expect("grant preview");
            grants
                .apply(
                    &tenant,
                    &candidate.candidate,
                    candidate.revision,
                    candidate.proposal_digest,
                    receipt,
                )
                .expect("durable grant decision");
        }
        let state = AppState::without_identity()
            .with_transaction_coordinator(coordinator.clone())
            .with_audit(audit)
            .with_grant_transactions(grants);
        (Dispatcher::new(state, coordinator), receipt)
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

    async fn unix_deadline_fixture() {
        let root = private_root("absolute-deadline");
        let endpoint = LocalManagement::bind_at(&root, effective_uid(), test_dispatcher(&root))
            .expect("owner endpoint");
        let path = endpoint.path().to_owned();
        let server = tokio::spawn(endpoint.serve());
        let stream = UnixStream::connect(path).await.expect("owner connection");
        let (mut reader, mut writer) = stream.into_split();

        tokio::io::AsyncWriteExt::write_all(&mut writer, b"FXLM")
            .await
            .expect("partial first header");
        let response = tokio::spawn(async move {
            let mut bytes = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut bytes)
                .await
                .expect("deadline response EOF");
            bytes
        });
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(299)).await;
        tokio::task::yield_now().await;
        assert!(!response.is_finished());

        // More syntactically incomplete bytes do not replace the authentication-time anchor.
        tokio::io::AsyncWriteExt::write_all(&mut writer, &[1, 0, 0, 1])
            .await
            .expect("more partial header bytes");
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        let response = response.await.expect("response task");
        assert_eq!(response, crate::local_management::deadline_frame());

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(root);

        let post_root = private_root("post-decision-deadline");
        let (dispatcher, receipt) = grant_dispatcher(&post_root, true);
        let deadline = DeadlineController::start();
        let mut endpoint = LocalManagement::bind_at(&post_root, effective_uid(), dispatcher)
            .expect("post-decision owner endpoint");
        endpoint.deadline_override = Some(deadline.clone());
        let path = endpoint.path().to_owned();
        let server = tokio::spawn(endpoint.serve());
        let stream = UnixStream::connect(path).await.expect("owner connection");
        let (mut reader, mut writer) = stream.into_split();
        tokio::task::yield_now().await;
        let receipt_identity = ReceiptIdentity::from_protocol_bytes(receipt.protocol_bytes())
            .expect("receipt identity");
        deadline
            .decided(receipt_identity, Unresolved::Store)
            .expect("durable decision");
        let response = tokio::spawn(async move {
            let mut bytes = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut bytes)
                .await
                .expect("post-decision response EOF");
            bytes
        });
        tokio::time::advance(std::time::Duration::from_secs(29)).await;
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"FX")
            .await
            .expect("post-decision partial traffic");
        tokio::task::yield_now().await;
        assert!(
            !response.is_finished(),
            "partial traffic cannot expire or reset the post-decision clock at 29 seconds"
        );
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        let response = response.await.expect("post-decision response task");
        let (expected, close_code) =
            crate::local_management::expired_reply(Expired::PostDecision {
                receipt: receipt_identity,
                unresolved: Unresolved::Store,
            })
            .into_parts();
        assert_eq!(close_code, 1000);
        assert_eq!(
            response, expected,
            "native post-decision stream ends in EOF"
        );
        server.abort();
        let _ = server.await;

        let replay_root = private_root("post-decision-replay-endpoint");
        let (dispatcher, reopened_receipt) = grant_dispatcher(&post_root, false);
        assert_eq!(reopened_receipt, receipt);
        let endpoint = LocalManagement::bind_at(&replay_root, effective_uid(), dispatcher)
            .expect("restarted owner endpoint");
        let path = endpoint.path().to_owned();
        let server = tokio::spawn(endpoint.serve());
        let mut stream = UnixStream::connect(path).await.expect("replay connection");
        let query = format!(r#"{{"receipt_id":"{receipt}"}}"#);
        let query = Frame::control(
            Direction::ClientToServer,
            Opcode::GrantQuery,
            query.into_bytes(),
        )
        .expect("grant QUERY")
        .encode();
        tokio::io::AsyncWriteExt::write_all(&mut stream, &query)
            .await
            .expect("grant QUERY write");
        tokio::io::AsyncWriteExt::shutdown(&mut stream)
            .await
            .expect("grant QUERY EOF");
        let mut replayed = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut replayed)
            .await
            .expect("grant replay response EOF");
        let replayed_text = String::from_utf8_lossy(&replayed);
        assert!(replayed_text.contains(&receipt.to_string()));
        assert!(replayed_text.contains("\"replayed\":true"));
        server.abort();
        let _ = server.await;

        // Exercise the finalizer through a real native socket rather than only the generic duplex
        // writer test. The peer deliberately leaves the send buffer full until the short frame
        // attempt has elapsed; the server must still half-close and the peer must observe EOF.
        let (mut terminal_writer, mut terminal_reader) =
            UnixStream::pair().expect("terminal backpressure socket pair");
        let oversized = vec![0x5a_u8; 2 * 1024 * 1024];
        let oversized_len = oversized.len();
        let terminal = tokio::spawn(async move {
            finalize_native_terminal(&mut terminal_writer, Some(&oversized)).await;
        });
        tokio::task::yield_now().await;
        assert!(
            !terminal.is_finished(),
            "a two-megabyte frame must backpressure an unread Unix socket"
        );
        tokio::time::advance(std::time::Duration::from_millis(250)).await;
        tokio::task::yield_now().await;
        let mut partial = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut terminal_reader, &mut partial)
            .await
            .expect("backpressured Unix terminal EOF");
        terminal.await.expect("bounded Unix terminal finalizer");
        assert!(
            partial.len() < oversized_len,
            "the blocked Unix frame attempt must be abandoned before EOF"
        );

        // The cleanup primitive also remains bounded when the authenticated peer never stops
        // writing. Nothing reads this request side: SHUT_RD must atomically discard queued bytes,
        // reject the flood and preserve the canonical terminal response plus clean EOF.
        let (mut flood_server, flood_client) =
            UnixStream::pair().expect("terminal flood socket pair");
        let (mut flood_reader, mut flood_writer) = flood_client.into_split();
        let flood_bytes = [0x41_u8; 4096];
        tokio::io::AsyncWriteExt::write_all(&mut flood_writer, &flood_bytes)
            .await
            .expect("queue adversarial bytes before read-half shutdown");
        let flood = tokio::spawn(async move {
            let mut writes = 1_usize;
            loop {
                match tokio::io::AsyncWriteExt::write_all(&mut flood_writer, &flood_bytes).await {
                    Ok(()) => {
                        writes += 1;
                        tokio::task::yield_now().await;
                    }
                    Err(_) => return writes,
                }
            }
        });
        tokio::task::yield_now().await;
        let deadline_response = crate::local_management::deadline_frame();
        let expected_response = deadline_response.clone();
        let terminal = tokio::spawn(async move {
            finalize_native_connection(&mut flood_server, Some(&deadline_response)).await;
        });
        let mut received = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut flood_reader, &mut received)
            .await
            .expect("flooded peer terminal EOF");
        assert_eq!(received, expected_response);
        assert!(
            !terminal.is_finished(),
            "the connection remains retained while its inbound flood is bounded"
        );
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        terminal.await.expect("bounded flooded finalizer");
        assert!(
            flood.await.expect("flood task") > 0,
            "the peer queued adversarial bytes before bounded read-half shutdown"
        );

        let _ = std::fs::remove_dir_all(replay_root);
        let _ = std::fs::remove_dir_all(post_root);
    }

    #[cfg(test)]
    #[tokio::test(start_paused = true)]
    async fn authenticated_native_idle_and_partial_traffic_expire_on_one_absolute_clock() {
        unix_deadline_fixture().await;
    }

    #[cfg(feature = "native-deadline-test-seam")]
    pub(super) fn run_deadline_process_fixture() -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(unix_deadline_fixture());
        Ok(())
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
