//! Unix transfer of the one-shot FXSA writer capability.
//!
//! The helper owns inherited write FD 5, validates it without repairing it, and transfers that
//! exact pipe through one `SCM_RIGHTS` message attached to the first bytes of the already validated
//! FXLM MINT frame. The authenticated server receives exactly one descriptor, immediately marks it
//! close-on-exec, revalidates its anonymous write-pipe identity, and consumes it for one FXSA frame.

use std::io::Write as _;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::net::UnixStream;

use super::{HandoffDecoder, HandoffError, HandoffFrame, HandoffWriter};
use crate::local_management::codec::{Direction, Opcode, StreamDecoder};
use crate::local_management::service_account::{OneShotWriter, WriterRefusal};

const FIXED_WRITER_FD: RawFd = 5;
const MAX_RIGHTS_PER_MESSAGE: usize = 4;

/// The helper's validated inherited FXSA write pipe.
pub(crate) struct HelperWriter {
    descriptor: OwnedFd,
}

impl HelperWriter {
    /// Take ownership of the exact helper ABI descriptor.
    ///
    /// The caller invokes this only after selecting the closed Unix helper grammar. Taking
    /// ownership is intentional: every success or refusal closes FD 5 before the helper exits.
    pub(crate) fn inherited_fd5() -> Result<Self, UnixHandoffError> {
        if descriptor_flags(FIXED_WRITER_FD).is_err() {
            return Err(UnixHandoffError::WriterMissing);
        }
        // SAFETY: the exact helper ABI transfers ownership of inherited FD 5 to this function.
        Self::from_owned(unsafe { OwnedFd::from_raw_fd(FIXED_WRITER_FD) })
    }

    fn from_owned(descriptor: OwnedFd) -> Result<Self, UnixHandoffError> {
        validate_anonymous_write_pipe(descriptor.as_raw_fd())?;
        set_close_on_exec(descriptor.as_raw_fd())?;
        Ok(Self { descriptor })
    }

    /// Authenticate the Exchange peer and transfer this one writer with the exact MINT frame.
    ///
    /// The descriptor is attached once. If the stream accepts only a prefix in that send, the
    /// remaining bytes are written normally; repeating `SCM_RIGHTS` would create a second writer.
    pub(crate) fn transfer_mint(
        self,
        stream: &UnixStream,
        mint_frame: &[u8],
    ) -> Result<(), UnixHandoffError> {
        self.transfer_mint_to_uid(stream, mint_frame, effective_uid())
    }

    fn transfer_mint_to_uid(
        self,
        stream: &UnixStream,
        mint_frame: &[u8],
        expected_uid: u32,
    ) -> Result<(), UnixHandoffError> {
        authenticate_peer(stream, expected_uid)?;
        validate_exact_mint_frame(mint_frame)?;
        let sent = send_one_descriptor(stream, mint_frame, self.descriptor.as_raw_fd())?;
        if sent < mint_frame.len() {
            let mut stream = stream;
            stream
                .write_all(&mint_frame[sent..])
                .map_err(|_| UnixHandoffError::StreamFailure)?;
        }
        Ok(())
    }
}

/// The server's received one-shot writer. Consuming it writes one FXSA frame and closes for EOF.
pub(in crate::local_management) struct ReceivedWriter {
    descriptor: OwnedFd,
}

impl ReceivedWriter {
    pub(in crate::local_management) fn write_frame(
        self,
        frame: &HandoffFrame,
    ) -> Result<(), UnixHandoffError> {
        let mut writer = HandoffWriter::new();
        let mut sink = std::fs::File::from(self.descriptor);
        writer
            .write(&mut sink, frame)
            .map_err(UnixHandoffError::Frame)?;
        debug_assert!(writer.frame_written());
        // `sink` closes here, producing the EOF that completes the FXSA receiver contract.
        Ok(())
    }
}

impl OneShotWriter for ReceivedWriter {
    fn write_once(self: Box<Self>, bytes: &[u8]) -> Result<(), WriterRefusal> {
        let mut decoder = HandoffDecoder::new();
        decoder.push(bytes).map_err(|_| WriterRefusal::Invalid)?;
        let frame = decoder.finish().map_err(|_| WriterRefusal::Invalid)?;
        (*self)
            .write_frame(&frame)
            .map_err(|_| WriterRefusal::Closed)
    }
}

/// Authenticate an accepted local-management stream and receive exactly one writer capability.
///
/// `bytes` receives the FXLM bytes carried by the same stream message. It must be nonempty because
/// SCM_RIGHTS is attached to real protocol bytes, never to an invented marker opcode or payload.
pub(in crate::local_management) fn receive_writer(
    stream: &UnixStream,
    expected_uid: u32,
    bytes: &mut [u8],
) -> Result<(ReceivedWriter, usize), UnixHandoffError> {
    let (writer, received) = receive_initial(stream, expected_uid, bytes)?;
    writer
        .map(|writer| (writer, received))
        .ok_or(UnixHandoffError::WriterMissing)
}

/// Receive the first FXLM bytes and at most one separately transferred writer capability.
///
/// Every native operation uses this first read so absence of a descriptor remains a valid input
/// for non-MINT frames. A descriptor on any other opcode stays observable to the dispatcher and is
/// refused there instead of being silently closed and treating the request as descriptor-free.
pub(in crate::local_management) fn receive_initial(
    stream: &UnixStream,
    expected_uid: u32,
    bytes: &mut [u8],
) -> Result<(Option<ReceivedWriter>, usize), UnixHandoffError> {
    receive_initial_fd(stream.as_raw_fd(), expected_uid, bytes)
}

/// Async-listener form of [`receive_initial`] operating on the already-owned socket descriptor.
pub(in crate::local_management) fn receive_initial_fd(
    stream: RawFd,
    expected_uid: u32,
    bytes: &mut [u8],
) -> Result<(Option<ReceivedWriter>, usize), UnixHandoffError> {
    if bytes.is_empty() {
        return Err(UnixHandoffError::MissingProtocolBytes);
    }
    authenticate_peer_fd(stream, expected_uid)?;
    let (mut descriptors, received, flags) = receive_descriptors(stream, bytes)?;
    if flags & libc::MSG_CTRUNC != 0 {
        return Err(UnixHandoffError::ControlTruncated);
    }
    if flags & libc::MSG_TRUNC != 0 {
        return Err(UnixHandoffError::ProtocolBytesTruncated);
    }
    if received == 0 {
        return Err(UnixHandoffError::MissingProtocolBytes);
    }
    if descriptors.len() > 1 {
        return Err(UnixHandoffError::MultipleWriters);
    }
    let Some(descriptor) = descriptors.pop() else {
        return Ok((None, received));
    };
    // Linux receives atomically close-on-exec; macOS receives then immediately sets it here.
    set_close_on_exec(descriptor.as_raw_fd())?;
    validate_anonymous_write_pipe(descriptor.as_raw_fd())?;
    Ok((Some(ReceivedWriter { descriptor }), received))
}

/// Value-free refusal from the Unix capability-transfer boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnixHandoffError {
    WouldBlock,
    WriterMissing,
    MultipleWriters,
    MissingProtocolBytes,
    ProtocolBytesTruncated,
    ControlTruncated,
    MalformedControl,
    WrongCapabilityKind,
    WrongPipeDirection,
    NamedPipeForbidden,
    DescriptorFlags,
    PeerUnverified,
    InvalidMintFrame,
    StreamFailure,
    Frame(HandoffError),
}

fn validate_exact_mint_frame(bytes: &[u8]) -> Result<(), UnixHandoffError> {
    let mut decoder = StreamDecoder::new(Direction::ClientToServer);
    decoder
        .push(bytes)
        .map_err(|_| UnixHandoffError::InvalidMintFrame)?;
    let frame = decoder
        .next_frame()
        .map_err(|_| UnixHandoffError::InvalidMintFrame)?
        .ok_or(UnixHandoffError::InvalidMintFrame)?;
    if frame.direction() != Direction::ClientToServer
        || frame.opcode() != Opcode::ServiceAccountMint
        || frame.control_payload().is_none()
    {
        return Err(UnixHandoffError::InvalidMintFrame);
    }
    if decoder
        .next_frame()
        .map_err(|_| UnixHandoffError::InvalidMintFrame)?
        .is_some()
        || decoder.finish().is_err()
    {
        return Err(UnixHandoffError::InvalidMintFrame);
    }
    Ok(())
}

fn send_one_descriptor(
    stream: &UnixStream,
    bytes: &[u8],
    descriptor: RawFd,
) -> Result<usize, UnixHandoffError> {
    if bytes.is_empty() {
        return Err(UnixHandoffError::MissingProtocolBytes);
    }
    let control_bytes = cmsg_space(std::mem::size_of::<RawFd>());
    let mut control = aligned_control(control_bytes);
    let mut iovec = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: zero is a valid initial msghdr; every pointer below references live storage for the
    // complete sendmsg call.
    let mut message = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
    message.msg_iov = &mut iovec;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control_bytes;
    // SAFETY: the initialized control buffer is large and aligned for one cmsghdr plus one fd.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null() {
            return Err(UnixHandoffError::MalformedControl);
        }
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as _) as _;
        std::ptr::copy_nonoverlapping(
            (&descriptor as *const RawFd).cast::<u8>(),
            libc::CMSG_DATA(header),
            std::mem::size_of::<RawFd>(),
        );
    }
    // SAFETY: msghdr and all referenced buffers remain live; sendmsg only reads them.
    let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &message, libc::MSG_NOSIGNAL) };
    if sent <= 0 {
        Err(UnixHandoffError::StreamFailure)
    } else {
        Ok(sent as usize)
    }
}

fn receive_descriptors(
    stream: RawFd,
    bytes: &mut [u8],
) -> Result<(Vec<OwnedFd>, usize, i32), UnixHandoffError> {
    let control_bytes = cmsg_space(MAX_RIGHTS_PER_MESSAGE * std::mem::size_of::<RawFd>());
    let mut control = aligned_control(control_bytes);
    let mut iovec = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: zero is a valid initial msghdr; output pointers below reference live writable storage.
    let mut message = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
    message.msg_iov = &mut iovec;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control_bytes;
    #[cfg(target_os = "linux")]
    let receive_flags = libc::MSG_CMSG_CLOEXEC;
    #[cfg(target_os = "macos")]
    let receive_flags = 0;
    // SAFETY: msghdr and all referenced output buffers remain live for recvmsg.
    let received = unsafe { libc::recvmsg(stream, &mut message, receive_flags) };
    if received < 0 {
        return Err(
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                UnixHandoffError::WouldBlock
            } else {
                UnixHandoffError::StreamFailure
            },
        );
    }

    let mut descriptors = Vec::new();
    // SAFETY: libc's CMSG iterators operate within msg_control/msg_controllen populated by recvmsg.
    let mut header = unsafe { libc::CMSG_FIRSTHDR(&message) };
    while !header.is_null() {
        // SAFETY: CMSG_FIRSTHDR/NXTHDR returned this header within the live control buffer.
        let value = unsafe { &*header };
        let header_bytes = cmsg_len(0);
        let value_len = value.cmsg_len as usize;
        if value_len < header_bytes
            || value.cmsg_level != libc::SOL_SOCKET
            || value.cmsg_type != libc::SCM_RIGHTS
        {
            return Err(UnixHandoffError::MalformedControl);
        }
        let data_bytes = value_len - header_bytes;
        if data_bytes == 0 || !data_bytes.is_multiple_of(std::mem::size_of::<RawFd>()) {
            return Err(UnixHandoffError::MalformedControl);
        }
        let count = data_bytes / std::mem::size_of::<RawFd>();
        // SAFETY: the validated SCM_RIGHTS payload contains `count` native file descriptors.
        let raw =
            unsafe { std::slice::from_raw_parts(libc::CMSG_DATA(header).cast::<RawFd>(), count) };
        for descriptor in raw {
            if *descriptor < 0 {
                return Err(UnixHandoffError::MalformedControl);
            }
            // SAFETY: every descriptor in SCM_RIGHTS is newly owned by this process exactly once.
            descriptors.push(unsafe { OwnedFd::from_raw_fd(*descriptor) });
        }
        // SAFETY: advances only within the validated msghdr control region.
        header = unsafe { libc::CMSG_NXTHDR(&message, header) };
    }
    Ok((descriptors, received as usize, message.msg_flags))
}

fn validate_anonymous_write_pipe(descriptor: RawFd) -> Result<(), UnixHandoffError> {
    // SAFETY: the output structure is live and fstat only writes it.
    let mut metadata = unsafe { MaybeUninit::<libc::stat>::zeroed().assume_init() };
    if unsafe { libc::fstat(descriptor, &mut metadata) } != 0
        || metadata.st_mode & libc::S_IFMT != libc::S_IFIFO
    {
        return Err(UnixHandoffError::WrongCapabilityKind);
    }
    // SAFETY: F_GETFL reads descriptor status flags without modifying the descriptor.
    let status = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if status < 0 {
        return Err(UnixHandoffError::DescriptorFlags);
    }
    if status & libc::O_ACCMODE != libc::O_WRONLY {
        return Err(UnixHandoffError::WrongPipeDirection);
    }
    validate_anonymous_pipe(descriptor)
}

#[cfg(target_os = "linux")]
fn validate_anonymous_pipe(descriptor: RawFd) -> Result<(), UnixHandoffError> {
    let target = std::fs::read_link(format!("/proc/self/fd/{descriptor}"))
        .map_err(|_| UnixHandoffError::WrongCapabilityKind)?;
    let bytes = target.into_os_string().into_vec();
    if bytes.starts_with(b"pipe:[") && bytes.ends_with(b"]") {
        Ok(())
    } else {
        Err(UnixHandoffError::NamedPipeForbidden)
    }
}

#[cfg(target_os = "macos")]
fn validate_anonymous_pipe(descriptor: RawFd) -> Result<(), UnixHandoffError> {
    let mut path = [0_u8; libc::PATH_MAX as usize];
    // F_GETPATH succeeds for a named FIFO and refuses an anonymous pipe, which has no pathname.
    let result = unsafe { libc::fcntl(descriptor, libc::F_GETPATH, path.as_mut_ptr()) };
    if result == -1 {
        Ok(())
    } else {
        Err(UnixHandoffError::NamedPipeForbidden)
    }
}

fn set_close_on_exec(descriptor: RawFd) -> Result<(), UnixHandoffError> {
    let flags = descriptor_flags(descriptor)?;
    // SAFETY: the descriptor is owned/live and only its descriptor flags are changed.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        Err(UnixHandoffError::DescriptorFlags)
    } else {
        Ok(())
    }
}

fn descriptor_flags(descriptor: RawFd) -> Result<i32, UnixHandoffError> {
    // SAFETY: F_GETFD has no pointer argument and does not mutate the descriptor.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags == -1 {
        Err(UnixHandoffError::DescriptorFlags)
    } else {
        Ok(flags)
    }
}

fn authenticate_peer(stream: &UnixStream, expected_uid: u32) -> Result<(), UnixHandoffError> {
    authenticate_peer_fd(stream.as_raw_fd(), expected_uid)
}

fn authenticate_peer_fd(stream: RawFd, expected_uid: u32) -> Result<(), UnixHandoffError> {
    if peer_uid(stream)? == expected_uid {
        Ok(())
    } else {
        Err(UnixHandoffError::PeerUnverified)
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: RawFd) -> Result<u32, UnixHandoffError> {
    let mut credential = MaybeUninit::<libc::ucred>::zeroed();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the stream is live and the output region has the exact ucred size.
    let result = unsafe {
        libc::getsockopt(
            stream,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credential.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(UnixHandoffError::PeerUnverified);
    }
    // SAFETY: getsockopt succeeded with the complete output size.
    Ok(unsafe { credential.assume_init() }.uid)
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: RawFd) -> Result<u32, UnixHandoffError> {
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: the live stream and both output pointers satisfy getpeereid.
    if unsafe { libc::getpeereid(stream, &mut uid, &mut gid) } != 0 {
        Err(UnixHandoffError::PeerUnverified)
    } else {
        Ok(uid)
    }
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no pointer arguments or preconditions.
    unsafe { libc::geteuid() }
}

fn aligned_control(bytes: usize) -> Vec<MaybeUninit<libc::cmsghdr>> {
    let unit = std::mem::size_of::<libc::cmsghdr>();
    vec![MaybeUninit::zeroed(); bytes.div_ceil(unit)]
}

fn cmsg_space(bytes: usize) -> usize {
    // SAFETY: CMSG_SPACE performs bounded integer alignment for this tiny descriptor count.
    unsafe { libc::CMSG_SPACE(bytes as _) as usize }
}

fn cmsg_len(bytes: usize) -> usize {
    // SAFETY: CMSG_LEN performs bounded integer alignment for this tiny descriptor count.
    unsafe { libc::CMSG_LEN(bytes as _) as usize }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::io::Read as _;
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::net::UnixStream;

    use super::*;
    use crate::local_management::codec::{Direction, Frame, Opcode};

    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut descriptors = [-1; 2];
        // SAFETY: the live array receives two owned descriptors on success.
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        // SAFETY: successful pipe returned two distinct newly owned descriptors.
        unsafe {
            (
                OwnedFd::from_raw_fd(descriptors[0]),
                OwnedFd::from_raw_fd(descriptors[1]),
            )
        }
    }

    fn mint_frame() -> Vec<u8> {
        Frame::control(
            Direction::ClientToServer,
            Opcode::ServiceAccountMint,
            br#"{"expires_at":"1800000000","id":"worker"}"#.to_vec(),
        )
        .expect("MINT frame")
        .encode()
    }

    #[test]
    fn helper_accepts_only_an_anonymous_write_pipe_and_sets_cloexec() {
        let (read, write) = pipe();
        let helper = HelperWriter::from_owned(write).expect("anonymous write pipe");
        assert_ne!(
            descriptor_flags(helper.descriptor.as_raw_fd()).expect("descriptor flags")
                & libc::FD_CLOEXEC,
            0
        );
        assert_eq!(
            HelperWriter::from_owned(read).err(),
            Some(UnixHandoffError::WrongPipeDirection)
        );

        let (left, _right) = UnixStream::pair().expect("socket pair");
        // SAFETY: dup creates a distinct descriptor owned by this test.
        let socket = unsafe { libc::dup(left.as_raw_fd()) };
        assert!(socket >= 0);
        assert_eq!(
            HelperWriter::from_owned(unsafe { OwnedFd::from_raw_fd(socket) }).err(),
            Some(UnixHandoffError::WrongCapabilityKind)
        );

        let fifo = std::env::temp_dir().join(format!(
            "flux-exchange-x134-fxsa-fifo-{}",
            std::process::id()
        ));
        let native = CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path");
        // SAFETY: the path is NUL-terminated and names a test-owned absent path.
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        // Keep one reader open so opening the write-only FIFO cannot block.
        let reader = unsafe { libc::open(native.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        let writer = unsafe {
            libc::open(
                native.as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        assert!(reader >= 0 && writer >= 0);
        assert_eq!(
            HelperWriter::from_owned(unsafe { OwnedFd::from_raw_fd(writer) }).err(),
            Some(UnixHandoffError::NamedPipeForbidden)
        );
        // SAFETY: the read descriptor remains test-owned and is closed exactly once.
        unsafe { libc::close(reader) };
        std::fs::remove_file(fifo).expect("remove planted named FIFO fixture");
    }

    #[test]
    fn authenticated_transfer_carries_one_writer_and_one_exact_fxsa_frame() {
        let (read, write) = pipe();
        let helper = HelperWriter::from_owned(write).expect("helper write pipe");
        let (sender, receiver) = UnixStream::pair().expect("local-management stream");
        let mint = mint_frame();
        helper
            .transfer_mint(&sender, &mint)
            .expect("one SCM_RIGHTS transfer");

        let mut protocol = vec![0_u8; 65_548];
        let (writer, received) = receive_writer(&receiver, effective_uid(), &mut protocol)
            .expect("authenticated writer receive");
        assert_eq!(&protocol[..received], mint);
        assert_ne!(
            descriptor_flags(writer.descriptor.as_raw_fd()).expect("received flags")
                & libc::FD_CLOEXEC,
            0
        );
        let frame = HandoffFrame::new(vec![0x00, 0xff, 0x42]).expect("opaque FXSA frame");
        writer.write_frame(&frame).expect("one-shot FXSA write");

        let mut received_frame = Vec::new();
        std::fs::File::from(read)
            .read_to_end(&mut received_frame)
            .expect("FXSA frame plus EOF");
        let mut decoder = super::super::HandoffDecoder::new();
        for byte in received_frame {
            decoder.push(&[byte]).expect("split receiver input");
        }
        assert_eq!(
            decoder.finish().expect("sole FXSA frame").token(),
            &[0x00, 0xff, 0x42]
        );
    }

    #[test]
    fn helper_rejects_wrong_peer_and_non_mint_or_multiple_frames_before_transfer() {
        let cases = [
            Frame::control(
                Direction::ClientToServer,
                Opcode::ServiceAccountQuery,
                b"{}".to_vec(),
            )
            .expect("query")
            .encode(),
            mint_frame()[..10].to_vec(),
            {
                let mut two = mint_frame();
                two.extend_from_slice(&mint_frame());
                two
            },
        ];
        for bytes in cases {
            let (_read, write) = pipe();
            let helper = HelperWriter::from_owned(write).expect("write pipe");
            let (sender, receiver) = UnixStream::pair().expect("stream");
            assert_eq!(
                helper.transfer_mint(&sender, &bytes),
                Err(UnixHandoffError::InvalidMintFrame)
            );
            receiver
                .set_nonblocking(true)
                .expect("nonblocking untouched receiver");
            let mut byte = [0_u8; 1];
            match (&receiver).read(&mut byte) {
                Ok(0) => {}
                Err(refusal) if refusal.kind() == std::io::ErrorKind::WouldBlock => {}
                outcome => panic!("invalid MINT transferred protocol bytes: {outcome:?}"),
            }
        }

        let (_read, write) = pipe();
        let helper = HelperWriter::from_owned(write).expect("write pipe");
        let (sender, receiver) = UnixStream::pair().expect("stream");
        assert_eq!(
            helper.transfer_mint_to_uid(&sender, &mint_frame(), effective_uid().wrapping_add(1)),
            Err(UnixHandoffError::PeerUnverified)
        );
        drop(receiver);
    }

    #[test]
    fn receiver_rejects_missing_multiple_truncated_and_wrong_direction_capabilities() {
        let (sender, receiver) = UnixStream::pair().expect("stream");
        (&sender).write_all(b"F").expect("protocol byte");
        let mut byte = [0_u8; 1];
        assert!(matches!(
            receive_writer(&receiver, effective_uid(), &mut byte),
            Err(UnixHandoffError::WriterMissing)
        ));

        let (sender, receiver) = UnixStream::pair().expect("stream");
        let (read, write) = pipe();
        send_descriptors_for_test(&sender, b"F", &[read.as_raw_fd(), write.as_raw_fd()]);
        assert!(matches!(
            receive_writer(&receiver, effective_uid(), &mut byte),
            Err(UnixHandoffError::MultipleWriters)
        ));

        let (sender, receiver) = UnixStream::pair().expect("stream");
        let pipes = (0..5).map(|_| pipe()).collect::<Vec<_>>();
        let descriptors = pipes
            .iter()
            .map(|(_, write)| write.as_raw_fd())
            .collect::<Vec<_>>();
        send_descriptors_for_test(&sender, b"F", &descriptors);
        assert!(matches!(
            receive_writer(&receiver, effective_uid(), &mut byte),
            Err(UnixHandoffError::ControlTruncated)
        ));

        let (sender, receiver) = UnixStream::pair().expect("stream");
        let (read, _write) = pipe();
        send_descriptors_for_test(&sender, b"F", &[read.as_raw_fd()]);
        assert!(matches!(
            receive_writer(&receiver, effective_uid(), &mut byte),
            Err(UnixHandoffError::WrongPipeDirection)
        ));
    }

    #[test]
    fn receiver_authenticates_before_consuming_protocol_or_capability() {
        let (sender, receiver) = UnixStream::pair().expect("stream");
        let (_read, write) = pipe();
        send_descriptors_for_test(&sender, b"F", &[write.as_raw_fd()]);
        let mut byte = [0_u8; 1];
        assert!(matches!(
            receive_writer(&receiver, effective_uid().wrapping_add(1), &mut byte),
            Err(UnixHandoffError::PeerUnverified)
        ));
        assert_eq!(
            receive_writer(&receiver, effective_uid(), &mut byte)
                .expect("same owner still receives after injected refusal")
                .1,
            1
        );
    }

    fn send_descriptors_for_test(stream: &UnixStream, bytes: &[u8], descriptors: &[RawFd]) {
        let control_bytes = cmsg_space(std::mem::size_of_val(descriptors));
        let mut control = aligned_control(control_bytes);
        let mut iovec = libc::iovec {
            iov_base: bytes.as_ptr().cast_mut().cast(),
            iov_len: bytes.len(),
        };
        // SAFETY: zero is a valid msghdr and all installed pointers remain live for sendmsg.
        let mut message = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
        message.msg_iov = &mut iovec;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = control_bytes;
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            assert!(!header.is_null());
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(descriptors) as _) as _;
            std::ptr::copy_nonoverlapping(
                descriptors.as_ptr().cast::<u8>(),
                libc::CMSG_DATA(header),
                std::mem::size_of_val(descriptors),
            );
            assert_eq!(
                libc::sendmsg(stream.as_raw_fd(), &message, libc::MSG_NOSIGNAL),
                bytes.len() as isize
            );
        }
    }
}
