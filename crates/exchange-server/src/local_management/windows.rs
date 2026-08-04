use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr::{null_mut, NonNull};

use sha2::{Digest as _, Sha256};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use windows_sys::Win32::Foundation::{GetLastError, LocalFree, ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, RevertToSelf, TokenUser, PSECURITY_DESCRIPTOR,
    PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

const PIPE_PREFIX: &str = r"\\.\pipe\flux-exchange-local-management-v1-";
const MAX_FRAME_BYTES: u32 = 65_548;

/// One first-instance, owner-authenticated local-management pipe endpoint.
///
/// The endpoint retains the process `TokenUser` captured at startup. A connected pipe is returned
/// only after named-pipe impersonation proves that the client has that exact SID and the server has
/// reverted to its own token. Callers can therefore read one FXLM operation without constructing a
/// second identity mechanism or trusting a client-supplied spelling of an account.
pub(crate) struct WindowsEndpoint {
    pipe_name: String,
    owner_sid: OwnedSid,
}

impl WindowsEndpoint {
    /// Bind identity to the authenticated account that started this process.
    pub(crate) fn bind() -> Result<Self, WindowsEndpointRefusal> {
        let owner_sid = process_token_sid()?;
        // Windows uses the kernel named-pipe namespace directly. No inherited filesystem path,
        // profile value or caller-controlled component exists here to traverse as a reparse point.
        let pipe_name = pipe_name_for_sid(owner_sid.bytes());
        Ok(Self {
            pipe_name,
            owner_sid,
        })
    }

    /// Accept exactly one connected owner pipe, authenticating before any byte can be read.
    pub(crate) async fn accept_one(&self) -> Result<NamedPipeServer, WindowsEndpointRefusal> {
        let server = self.create_first_instance()?;
        server
            .connect()
            .await
            .map_err(|_| WindowsEndpointRefusal::Connect)?;
        authenticate_owner(server.as_raw_handle() as HANDLE, self.owner_sid.bytes())?;
        Ok(server)
    }

    fn create_first_instance(&self) -> Result<NamedPipeServer, WindowsEndpointRefusal> {
        let descriptor = SecurityDescriptor::owner_and_system(&self.owner_sid)?;
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .map_err(|_| WindowsEndpointRefusal::Security)?,
            lpSecurityDescriptor: descriptor.as_ptr(),
            bInheritHandle: 0,
        };
        let mut options = ServerOptions::new();
        options
            .pipe_mode(PipeMode::Byte)
            .access_inbound(true)
            .access_outbound(true)
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1)
            .in_buffer_size(MAX_FRAME_BYTES)
            .out_buffer_size(MAX_FRAME_BYTES);

        // SAFETY: `attributes` and its descriptor allocation remain live until CreateNamedPipeW
        // returns. Tokio always adds FILE_FLAG_OVERLAPPED; the options additionally select byte
        // mode, FILE_FLAG_FIRST_PIPE_INSTANCE and PIPE_REJECT_REMOTE_CLIENTS.
        unsafe {
            options.create_with_security_attributes_raw(
                std::ffi::OsStr::new(&self.pipe_name),
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
            )
        }
        .map_err(|_| WindowsEndpointRefusal::Bind)
    }

    #[cfg(test)]
    fn pipe_name(&self) -> &str {
        &self.pipe_name
    }
}

/// A fixed, value-free native endpoint refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowsEndpointRefusal {
    OwnerIdentity,
    Security,
    Bind,
    Connect,
    PeerIdentity,
    Revert,
}

impl WindowsEndpointRefusal {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::OwnerIdentity => "owner_identity_unavailable",
            Self::Security => "local_management_security_refused",
            Self::Bind => "local_management_bind_refused",
            Self::Connect => "local_management_connect_refused",
            Self::PeerIdentity => "local_management_peer_refused",
            Self::Revert => "local_management_revert_refused",
        }
    }
}

impl fmt::Display for WindowsEndpointRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

struct OwnedSid {
    storage: Vec<usize>,
    offset: usize,
    length: usize,
}

impl OwnedSid {
    fn bytes(&self) -> &[u8] {
        let base = self.storage.as_ptr().cast::<u8>();
        // SAFETY: construction validates that `[offset, offset + length)` lies within storage.
        unsafe { std::slice::from_raw_parts(base.add(self.offset), self.length) }
    }

    fn as_ptr(&self) -> PSID {
        self.bytes().as_ptr().cast_mut().cast()
    }
}

fn process_token_sid() -> Result<OwnedSid, WindowsEndpointRefusal> {
    let mut raw: HANDLE = null_mut();
    // SAFETY: the pseudo-handle remains process-owned; success initializes one owned token handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(WindowsEndpointRefusal::OwnerIdentity);
    }
    let token = owned_handle(raw).ok_or(WindowsEndpointRefusal::OwnerIdentity)?;
    token_sid(
        token.as_raw_handle() as HANDLE,
        WindowsEndpointRefusal::OwnerIdentity,
    )
}

fn thread_token_sid() -> Result<OwnedSid, WindowsEndpointRefusal> {
    let mut raw: HANDLE = null_mut();
    // `OpenAsSelf = FALSE` asks for the impersonated client's thread token, never the process token.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut raw) } == 0 {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    let token = owned_handle(raw).ok_or(WindowsEndpointRefusal::PeerIdentity)?;
    token_sid(
        token.as_raw_handle() as HANDLE,
        WindowsEndpointRefusal::PeerIdentity,
    )
}

fn token_sid(
    token: HANDLE,
    refusal: WindowsEndpointRefusal,
) -> Result<OwnedSid, WindowsEndpointRefusal> {
    let mut length = 0_u32;
    // SAFETY: the null-buffer sizing call writes only the required byte count.
    let sized = unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut length) };
    if sized != 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(refusal);
    }
    let capacity = length as usize;
    if capacity < size_of::<TOKEN_USER>() {
        return Err(refusal);
    }
    let mut storage = vec![0_usize; capacity.div_ceil(size_of::<usize>())];
    // SAFETY: the aligned allocation is at least `capacity` bytes and remains live below.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
        || length as usize > capacity
    {
        return Err(refusal);
    }
    let base = storage.as_ptr().cast::<u8>();
    // SAFETY: the buffer holds a TOKEN_USER after successful GetTokenInformation.
    let sid = unsafe { (*(base.cast::<TOKEN_USER>())).User.Sid.cast::<u8>() };
    let offset = (sid as usize)
        .checked_sub(base as usize)
        .filter(|offset| *offset < capacity)
        .ok_or(refusal)?;
    if unsafe { IsValidSid(sid.cast()) } == 0 {
        return Err(refusal);
    }
    let sid_length = unsafe { GetLengthSid(sid.cast()) } as usize;
    if offset
        .checked_add(sid_length)
        .is_none_or(|end| end > capacity)
    {
        return Err(refusal);
    }
    Ok(OwnedSid {
        storage,
        offset,
        length: sid_length,
    })
}

fn pipe_name_for_sid(sid: &[u8]) -> String {
    let digest = Sha256::digest(sid);
    let mut suffix = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write as _;
        let _ = write!(suffix, "{byte:02x}");
    }
    format!("{PIPE_PREFIX}{suffix}")
}

fn authenticate_owner(pipe: HANDLE, expected_sid: &[u8]) -> Result<(), WindowsEndpointRefusal> {
    authenticated_flow(
        expected_sid,
        || {
            if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
                Err(WindowsEndpointRefusal::PeerIdentity)
            } else {
                Ok(())
            }
        },
        || thread_token_sid().map(|sid| sid.bytes().to_vec()),
        || unsafe { RevertToSelf() } != 0,
    )
}

fn authenticated_flow<Begin, Query, Revert>(
    expected_sid: &[u8],
    begin: Begin,
    query: Query,
    revert: Revert,
) -> Result<(), WindowsEndpointRefusal>
where
    Begin: FnOnce() -> Result<(), WindowsEndpointRefusal>,
    Query: FnOnce() -> Result<Vec<u8>, WindowsEndpointRefusal>,
    Revert: FnMut() -> bool,
{
    begin()?;
    let mut guard = RevertGuard::new(revert);
    let peer = query();
    if !guard.finish() {
        return Err(WindowsEndpointRefusal::Revert);
    }
    let peer = peer?;
    if peer != expected_sid {
        return Err(WindowsEndpointRefusal::PeerIdentity);
    }
    Ok(())
}

struct RevertGuard<Revert: FnMut() -> bool> {
    revert: Option<Revert>,
}

impl<Revert: FnMut() -> bool> RevertGuard<Revert> {
    fn new(revert: Revert) -> Self {
        Self {
            revert: Some(revert),
        }
    }

    fn finish(&mut self) -> bool {
        self.revert.take().is_some_and(|mut revert| revert())
    }
}

impl<Revert: FnMut() -> bool> Drop for RevertGuard<Revert> {
    fn drop(&mut self) {
        if let Some(mut revert) = self.revert.take() {
            let _ = revert();
        }
    }
}

struct SecurityDescriptor(NonNull<c_void>);

impl SecurityDescriptor {
    fn owner_and_system(owner: &OwnedSid) -> Result<Self, WindowsEndpointRefusal> {
        let owner = sid_string(owner.as_ptr())?;
        // `P` marks the DACL protected. These are the only two ACEs: LocalSystem and the startup
        // owner. There is deliberately no Administrators, BUILTIN Users, Everyone or AppContainer
        // grant and the handle is non-inheritable.
        let sddl = format!("O:{owner}D:P(A;;GA;;;SY)(A;;GA;;;{owner})");
        let mut encoded: Vec<u16> = std::ffi::OsStr::new(&sddl).encode_wide().collect();
        encoded.push(0);
        let mut raw: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: the UTF-16 input is NUL-terminated; success returns a LocalAlloc descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_ptr(),
                SDDL_REVISION_1,
                &mut raw,
                null_mut(),
            )
        } == 0
        {
            return Err(WindowsEndpointRefusal::Security);
        }
        NonNull::new(raw.cast())
            .map(Self)
            .ok_or(WindowsEndpointRefusal::Security)
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: this is the exact LocalAlloc pointer returned by the SDDL converter.
        unsafe {
            let _ = LocalFree(self.0.as_ptr());
        }
    }
}

fn sid_string(sid: PSID) -> Result<String, WindowsEndpointRefusal> {
    let mut raw = null_mut();
    // SAFETY: callers pass a validated live SID; success returns a LocalAlloc UTF-16 string.
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 || raw.is_null() {
        return Err(WindowsEndpointRefusal::Security);
    }
    let length = unsafe { (0..).take_while(|offset| *raw.add(*offset) != 0).count() };
    let rendered = String::from_utf16(unsafe { std::slice::from_raw_parts(raw, length) })
        .map_err(|_| WindowsEndpointRefusal::Security);
    unsafe {
        let _ = LocalFree(raw.cast());
    }
    rendered
}

fn owned_handle(raw: HANDLE) -> Option<OwnedHandle> {
    if raw.is_null() {
        None
    } else {
        // SAFETY: successful token APIs return a new handle owned by the caller.
        Some(unsafe { OwnedHandle::from_raw_handle(raw.cast()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    use tokio::net::windows::named_pipe::ClientOptions;
    use windows_sys::Win32::Foundation::{
        GetLastError, LocalFree, ERROR_NO_TOKEN, ERROR_SUCCESS, HANDLE,
    };
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows_sys::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_DACL_PRESENT, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

    #[test]
    fn sid_bytes_select_exact_lowerhex_pipe_name() {
        assert_eq!(
            pipe_name_for_sid(&[0, 1, 2]),
            r"\\.\pipe\flux-exchange-local-management-v1-ae4b3280e56e2faf83f414a6e3dabe9d"
        );
    }

    #[test]
    fn peer_mismatch_refuses_after_exactly_one_revert() {
        let reverts = Cell::new(0_u8);
        let refusal = authenticated_flow(
            b"startup-owner",
            || Ok(()),
            || Ok(b"different-owner".to_vec()),
            || {
                reverts.set(reverts.get() + 1);
                true
            },
        )
        .expect_err("another account must refuse");
        assert_eq!(refusal, WindowsEndpointRefusal::PeerIdentity);
        assert_eq!(reverts.get(), 1);
    }

    #[test]
    fn token_query_failure_still_reverts_before_refusal() {
        let reverts = Cell::new(0_u8);
        let refusal = authenticated_flow(
            b"startup-owner",
            || Ok(()),
            || Err(WindowsEndpointRefusal::PeerIdentity),
            || {
                reverts.set(reverts.get() + 1);
                true
            },
        )
        .expect_err("token query failure must refuse");
        assert_eq!(refusal, WindowsEndpointRefusal::PeerIdentity);
        assert_eq!(reverts.get(), 1);
    }

    #[test]
    fn unwinding_token_query_still_reverts_exactly_once() {
        let reverts = Cell::new(0_u8);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = authenticated_flow(
                b"startup-owner",
                || Ok(()),
                || -> Result<Vec<u8>, WindowsEndpointRefusal> { panic!("planted token panic") },
                || {
                    reverts.set(reverts.get() + 1);
                    true
                },
            );
        }));
        assert!(unwind.is_err());
        assert_eq!(reverts.get(), 1);
    }

    #[test]
    fn revert_failure_is_a_distinct_value_free_refusal() {
        let refusal = authenticated_flow(
            b"startup-owner",
            || Ok(()),
            || Ok(b"startup-owner".to_vec()),
            || false,
        )
        .expect_err("failed RevertToSelf must refuse");
        assert_eq!(refusal, WindowsEndpointRefusal::Revert);
        assert_eq!(refusal.to_string(), "local_management_revert_refused");
    }

    #[tokio::test]
    async fn native_pipe_has_only_owner_and_system_in_a_protected_dacl() {
        let endpoint = WindowsEndpoint::bind().expect("startup TokenUser");
        assert!(endpoint.pipe_name().starts_with(PIPE_PREFIX));
        assert_eq!(endpoint.pipe_name().len(), PIPE_PREFIX.len() + 32);
        let owner_text = sid_string(endpoint.owner_sid.as_ptr()).expect("owner SID text");
        let pipe = endpoint.create_first_instance().expect("owner pipe");
        assert_eq!(
            endpoint.create_first_instance().err(),
            Some(WindowsEndpointRefusal::Bind),
            "the exact first-instance name cannot be preempted"
        );
        let info = pipe.info().expect("named-pipe metadata");
        assert_eq!(info.mode, PipeMode::Byte);
        assert_eq!(info.max_instances, 1);

        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let status = unsafe {
            GetSecurityInfo(
                pipe.as_raw_handle() as HANDLE,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let descriptor_allocation = NonNull::new(descriptor.cast()).expect("descriptor");
        assert_eq!(sid_string(owner).expect("pipe owner"), owner_text);

        let mut control = 0_u16;
        let mut revision = 0_u32;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        assert_eq!(
            control & (SE_DACL_PRESENT | SE_DACL_PROTECTED),
            SE_DACL_PRESENT | SE_DACL_PROTECTED
        );

        let mut size = ACL_SIZE_INFORMATION::default();
        assert_ne!(
            unsafe {
                GetAclInformation(
                    dacl,
                    (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                    u32::try_from(size_of::<ACL_SIZE_INFORMATION>()).expect("ACL size"),
                    AclSizeInformation,
                )
            },
            0
        );
        assert_eq!(size.AceCount, 2);
        let mut trustees = Vec::new();
        for index in 0..size.AceCount {
            let mut raw_ace = null_mut();
            assert_ne!(unsafe { GetAce(dacl, index, &mut raw_ace) }, 0);
            let ace = unsafe { &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>()) };
            assert_eq!(u32::from(ace.Header.AceType), ACCESS_ALLOWED_ACE_TYPE);
            trustees.push(
                sid_string((&ace.SidStart as *const u32).cast_mut().cast())
                    .expect("allowed trustee"),
            );
        }
        trustees.sort();
        let mut expected = vec!["S-1-5-18".to_owned(), owner_text];
        expected.sort();
        assert_eq!(trustees, expected);

        unsafe {
            let _ = LocalFree(descriptor_allocation.as_ptr());
        }

        let client = ClientOptions::new()
            .open(endpoint.pipe_name())
            .expect("same-account local client");
        pipe.connect().await.expect("connected owner pipe");
        authenticate_owner(pipe.as_raw_handle() as HANDLE, endpoint.owner_sid.bytes())
            .expect("same startup owner");
        let mut unexpected_token: HANDLE = null_mut();
        assert_eq!(
            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut unexpected_token,) },
            0,
            "the server thread remained impersonated after peer authentication"
        );
        assert_eq!(unsafe { GetLastError() }, ERROR_NO_TOKEN);
        drop(client);
        drop(pipe);
    }
}
