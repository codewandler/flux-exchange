//! Owner-only Windows filesystem primitives for the portable logical store.
//!
//! This is the crate's only unsafe-code island. Every block is a direct Win32 call whose pointers
//! are owned by the adjacent Rust value or by a `LocalFree` guard in this module.

#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::fs::File;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::MetadataExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_INSUFFICIENT_BUFFER, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetLengthSid,
    GetSecurityDescriptorControl, GetTokenInformation, IsValidAcl, IsValidSecurityDescriptor,
    IsValidSid, TokenUser, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION,
    DACL_SECURITY_INFORMATION, INHERITED_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    PSID, SECURITY_ATTRIBUTES, SE_DACL_PRESENT, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FlushFileBuffers, GetFileInformationByHandle, MoveFileExW,
    ReplaceFileW, BY_HANDLE_FILE_INFORMATION, CREATE_NEW, FILE_ALL_ACCESS,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, MOVEFILE_WRITE_THROUGH, OPEN_EXISTING,
    READ_CONTROL,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{unreachable, StoreError};

#[derive(Clone, Copy)]
enum Expected {
    Directory,
    File,
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: Every `LocalAllocation` is constructed only from a Win32 API documented to
        // allocate with LocalAlloc, and the pointer is freed exactly once here.
        unsafe {
            let _ = LocalFree(self.0);
        }
    }
}

struct ProcessSid {
    token_information: Vec<usize>,
    sid_offset: usize,
}

impl ProcessSid {
    fn as_ptr(&self) -> PSID {
        // The offset was computed from TokenUser's SID while this allocation was live. Moving the
        // Vec does not move its allocation, and this pointer never escapes a call using `&self`.
        unsafe { (self.token_information.as_ptr() as *const u8).add(self.sid_offset) as PSID }
    }
}

pub(super) fn ensure_directory(directory: &Path) -> Result<(), StoreError> {
    let mut missing = Vec::new();
    let mut cursor = directory;
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.to_path_buf());
                cursor = cursor
                    .parent()
                    .ok_or_else(|| unreachable(directory, &error))?;
            }
            Err(error) => return Err(unreachable(directory, &error)),
        }
    }

    for path in missing.iter().rev() {
        create_directory(path, directory)?;
    }
    let handle = open_secure_handle(directory, Expected::Directory, false)?;
    drop(handle);
    Ok(())
}

pub(super) fn open_existing(path: &Path) -> Result<Option<File>, StoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreachable(path, &error)),
    }
    let handle = open_secure_handle(path, Expected::File, true)?;
    Ok(Some(File::from(handle)))
}

pub(super) fn create_new(temporary: &Path, store: &Path) -> Result<File, StoreError> {
    let sid = current_process_sid().map_err(|error| unreachable(store, &error))?;
    let descriptor = creation_descriptor(&sid).map_err(|error| unreachable(store, &error))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let wide = wide(temporary).map_err(|error| unreachable(store, &error))?;
    // SAFETY: `wide` and `attributes` remain alive through the call; the descriptor guard owns the
    // LocalAlloc buffer. CREATE_NEW prevents following or truncating a planted destination.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | FILE_ALL_ACCESS | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    let handle = owned_handle(raw).map_err(|error| unreachable(store, &error))?;
    inspect_handle(&handle, store, Expected::File)?;
    Ok(File::from(handle))
}

pub(super) fn validate_destination(path: &Path) -> Result<(), StoreError> {
    let existing = open_existing(path)?;
    drop(existing);
    Ok(())
}

pub(super) fn flush(file: &File, store: &Path) -> Result<(), StoreError> {
    // SAFETY: `file` owns a valid handle for the duration of the call.
    if unsafe { FlushFileBuffers(file.as_raw_handle() as HANDLE) } == 0 {
        return Err(unreachable(store, &std::io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn replace(temporary: &Path, store: &Path) -> Result<(), StoreError> {
    let directory = store
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_directory(directory)?;

    // Revalidate and close the destination inspection handle immediately before replacement.
    let existed = match open_existing(store)? {
        Some(handle) => {
            drop(handle);
            true
        }
        None => false,
    };
    let temporary_wide = wide(temporary).map_err(|error| unreachable(store, &error))?;
    let store_wide = wide(store).map_err(|error| unreachable(store, &error))?;
    let succeeded = if existed {
        // SAFETY: Both nul-terminated path buffers remain live. ReplaceFileW is the atomic Windows
        // replacement primitive; flags are zero because WRITE_THROUGH is documented unsupported.
        unsafe {
            ReplaceFileW(
                store_wide.as_ptr(),
                temporary_wide.as_ptr(),
                null(),
                0,
                null(),
                null(),
            )
        }
    } else {
        // SAFETY: Both path buffers remain live. Same-directory MOVEFILE_WRITE_THROUGH installs a
        // CREATE_NEW temporary without a missing-destination window.
        unsafe {
            MoveFileExW(
                temporary_wide.as_ptr(),
                store_wide.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if succeeded == 0 {
        return Err(unreachable(store, &std::io::Error::last_os_error()));
    }

    let Some(handle) = open_existing(store)? else {
        return Err(StoreError::Unreachable {
            path: store.display().to_string(),
            reason: "the atomically installed store disappeared before it could be revalidated"
                .to_owned(),
        });
    };
    drop(handle);
    Ok(())
}

pub(super) fn sync_directory(_directory: &Path) {
    // ReplaceFileW / MoveFileExW are the Windows durability primitives used above. Windows does not
    // expose the Unix directory-fsync operation through ordinary directory handles.
}

fn create_directory(path: &Path, store: &Path) -> Result<(), StoreError> {
    let sid = current_process_sid().map_err(|error| unreachable(store, &error))?;
    let descriptor = creation_descriptor(&sid).map_err(|error| unreachable(store, &error))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let wide = wide(path).map_err(|error| unreachable(store, &error))?;
    // SAFETY: `wide`, `attributes`, and its descriptor allocation remain live for this call.
    if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
        return Err(unreachable(store, &std::io::Error::last_os_error()));
    }
    let handle = open_secure_handle(path, Expected::Directory, false)?;
    drop(handle);
    Ok(())
}

fn open_secure_handle(
    path: &Path,
    expected: Expected,
    read_data: bool,
) -> Result<OwnedHandle, StoreError> {
    inspect_path(path, expected)?;
    let wide = wide(path).map_err(|error| unreachable(path, &error))?;
    let access = READ_CONTROL | FILE_READ_ATTRIBUTES | if read_data { GENERIC_READ } else { 0 };
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | match expected {
            Expected::Directory => FILE_FLAG_BACKUP_SEMANTICS,
            Expected::File => FILE_ATTRIBUTE_NORMAL,
        };
    // SAFETY: The nul-terminated path buffer lives through the call; OPEN_EXISTING never creates or
    // mutates the object, OPEN_REPARSE_POINT keeps the handle on the inspected object itself.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };
    let handle = owned_handle(raw).map_err(|error| unreachable(path, &error))?;
    inspect_handle(&handle, path, expected)?;
    Ok(handle)
}

fn inspect_path(path: &Path, expected: Expected) -> Result<(), StoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| unreachable(path, &error))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return denied(
            path,
            "it is a reparse point, which this store never follows",
        );
    }
    let right_kind = match expected {
        Expected::Directory => metadata.is_dir(),
        Expected::File => metadata.is_file(),
    };
    if !right_kind {
        return denied(path, expected.kind_reason());
    }
    Ok(())
}

fn inspect_handle(handle: &OwnedHandle, path: &Path, expected: Expected) -> Result<(), StoreError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` stays owned and live; the output structure is local and fully sized.
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle() as HANDLE, &mut information) }
        == 0
    {
        return Err(unreachable(path, &std::io::Error::last_os_error()));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return denied(path, "the opened handle is a reparse point");
    }
    let right_kind = match expected {
        Expected::Directory => information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        Expected::File => information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0,
    };
    if !right_kind {
        return denied(path, expected.kind_reason());
    }
    inspect_security(handle.as_raw_handle() as HANDLE, path)
}

fn inspect_security(handle: HANDLE, path: &Path) -> Result<(), StoreError> {
    let sid = current_process_sid().map_err(|error| unreachable(path, &error))?;
    let mut owner: PSID = null_mut();
    let mut dacl = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: All output pointers refer to local variables. On success descriptor owns the owner
    // and DACL pointers until LocalFree, and neither pointer escapes this function.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(unreachable(
            path,
            &std::io::Error::from_raw_os_error(status as i32),
        ));
    }
    let _descriptor = LocalAllocation(descriptor);
    // SAFETY: `descriptor` is the live LocalAlloc pointer returned by GetSecurityInfo.
    if unsafe { IsValidSecurityDescriptor(descriptor) } == 0 {
        return denied(path, "its security descriptor is malformed");
    }
    if owner.is_null() || dacl.is_null() {
        return denied(
            path,
            "its security descriptor has no owner or has a null DACL",
        );
    }
    // SAFETY: Owner/DACL pointers are fields within the validated live descriptor.
    if unsafe { IsValidSid(owner) } == 0 || unsafe { IsValidAcl(dacl) } == 0 {
        return denied(path, "its owner SID or DACL is malformed");
    }
    // SAFETY: Both SIDs are live for this comparison (`owner` under descriptor, process SID under
    // `sid`), and neither pointer escapes.
    if unsafe { EqualSid(owner, sid.as_ptr()) } == 0 {
        return denied(
            path,
            "its owner SID is not the current process identity SID",
        );
    }

    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: `descriptor` remains live under `_descriptor`; output pointers are local.
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(unreachable(path, &std::io::Error::last_os_error()));
    }
    if control & SE_DACL_PRESENT == 0 || control & SE_DACL_PROTECTED == 0 {
        return denied(
            path,
            "its DACL is absent, null, or inherits from an ancestor",
        );
    }

    let mut information = ACL_SIZE_INFORMATION::default();
    // SAFETY: `dacl` lives under `_descriptor`; the output buffer has the exact API structure size.
    if unsafe {
        GetAclInformation(
            dacl,
            &mut information as *mut ACL_SIZE_INFORMATION as *mut c_void,
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(unreachable(path, &std::io::Error::last_os_error()));
    }
    if information.AceCount != 1 {
        return denied(
            path,
            format!(
                "its protected DACL contains {} entries rather than one explicit owner allow entry",
                information.AceCount
            ),
        );
    }

    let mut ace = null_mut();
    // SAFETY: `dacl` is live and GetAclInformation established that index zero exists.
    if unsafe { GetAce(dacl, 0, &mut ace) } == 0 {
        return Err(unreachable(path, &std::io::Error::last_os_error()));
    }
    // SAFETY: GetAce returned a pointer to an ACE inside the validated live DACL. Read only the
    // common fixed-size header until its type and declared size have been checked.
    let header = unsafe { &*(ace as *const ACE_HEADER) };
    if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE {
        return denied(path, "its sole DACL entry is not an ACCESS_ALLOWED_ACE");
    }
    if header.AceFlags as u32 & INHERITED_ACE != 0 {
        return denied(
            path,
            "its owner allow entry is inherited rather than explicit",
        );
    }
    let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
    if (header.AceSize as usize) < sid_offset + size_of::<u32>() {
        return denied(path, "its owner allow entry is too short to carry a SID");
    }
    // SAFETY: The declared ACE size now covers ACCESS_ALLOWED_ACE through SidStart.
    let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
    if allowed.Mask != FILE_ALL_ACCESS {
        return denied(
            path,
            "its owner allow entry does not grant exactly file full control",
        );
    }
    let ace_sid = &allowed.SidStart as *const u32 as PSID;
    // SAFETY: `ace_sid` is within the live ACE; validate it before querying its length.
    if unsafe { IsValidSid(ace_sid) } == 0 {
        return denied(path, "its owner allow entry carries a malformed SID");
    }
    // SAFETY: IsValidSid accepted this SID pointer.
    let sid_len = unsafe { GetLengthSid(ace_sid) } as usize;
    if sid_offset
        .checked_add(sid_len)
        .is_none_or(|end| end > header.AceSize as usize)
    {
        return denied(path, "its owner allow entry's SID exceeds the ACE bounds");
    }
    // SAFETY: `ace_sid` points into the live ACE and `sid` owns the process SID buffer.
    if unsafe { EqualSid(ace_sid, sid.as_ptr()) } == 0 {
        return denied(
            path,
            "its allow entry grants a SID other than the current process identity",
        );
    }
    Ok(())
}

fn current_process_sid() -> std::io::Result<ProcessSid> {
    let mut token: HANDLE = null_mut();
    // SAFETY: The output pointer is local; GetCurrentProcess returns a pseudo-handle that must not
    // be closed, while the returned token is closed by OwnedHandle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let token = owned_handle(token)?;

    let mut length = 0u32;
    // SAFETY: Null/zero is the documented sizing call; `length` is a live output.
    let first = unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenUser,
            null_mut(),
            0,
            &mut length,
        )
    };
    if first != 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(std::io::Error::last_os_error());
    }
    let byte_len = length as usize;
    let words = byte_len.div_ceil(size_of::<usize>());
    let mut bytes = vec![0usize; words];
    // SAFETY: `bytes` is writable for exactly `length` bytes and the output length is local.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenUser,
            bytes.as_mut_ptr() as *mut c_void,
            length,
            &mut length,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if byte_len < size_of::<TOKEN_USER>() || length as usize > byte_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "TokenUser returned an undersized or overlong buffer",
        ));
    }
    // SAFETY: A successful TokenUser query begins with TOKEN_USER, whose SID points into `bytes`.
    let base = bytes.as_ptr() as *const u8;
    let sid = unsafe { (*(base as *const TOKEN_USER)).User.Sid as *const u8 };
    let Some(sid_offset) = (sid as usize).checked_sub(base as usize) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "TokenUser returned a SID outside its buffer",
        ));
    };
    if sid_offset >= byte_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "TokenUser returned a SID outside its buffer",
        ));
    }
    // SAFETY: The pointer lies in the still-live, suitably aligned TokenUser buffer.
    if unsafe { IsValidSid(sid as PSID) } == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "TokenUser returned a malformed SID",
        ));
    }
    // SAFETY: IsValidSid accepted the pointer.
    let sid_len = unsafe { GetLengthSid(sid as PSID) } as usize;
    if sid_offset
        .checked_add(sid_len)
        .is_none_or(|end| end > byte_len)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "TokenUser returned a SID outside its buffer",
        ));
    }
    Ok(ProcessSid {
        token_information: bytes,
        sid_offset,
    })
}

fn creation_descriptor(sid: &ProcessSid) -> std::io::Result<LocalAllocation> {
    let sid_text = sid_string(sid)?;
    let sddl = wide(std::ffi::OsStr::new(&format!(
        "O:{sid_text}D:P(A;;FA;;;{sid_text})"
    )))?;
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: The SDDL buffer and output pointer are live. Success returns a LocalAlloc descriptor.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(LocalAllocation(descriptor))
}

fn sid_string(sid: &ProcessSid) -> std::io::Result<String> {
    let mut string = null_mut();
    // SAFETY: `sid` owns the backing TokenUser buffer; success returns a LocalAlloc UTF-16 string.
    if unsafe { ConvertSidToStringSidW(sid.as_ptr(), &mut string) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let allocation = LocalAllocation(string as *mut c_void);
    let mut length = 0usize;
    // SAFETY: ConvertSidToStringSidW returns a nul-terminated UTF-16 allocation.
    while unsafe { *string.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: The preceding scan found the terminator within the API-owned string allocation.
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(string, length) })
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "SID was not UTF-16"))?;
    drop(allocation);
    Ok(text)
}

fn owned_handle(raw: HANDLE) -> std::io::Result<OwnedHandle> {
    if raw == INVALID_HANDLE_VALUE || raw.is_null() {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: A successful Win32 handle-returning call transfers one owned handle to us.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

fn denied(path: &Path, reason: impl Into<String>) -> Result<(), StoreError> {
    Err(StoreError::Denied {
        path: path.display().to_string(),
        reason: reason.into(),
    })
}

fn wide(path: impl AsRef<std::ffi::OsStr>) -> std::io::Result<Vec<u16>> {
    let mut encoded: Vec<u16> = path.as_ref().encode_wide().collect();
    if encoded.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a filesystem path contains an interior NUL",
        ));
    }
    encoded.push(0);
    Ok(encoded)
}

impl Expected {
    fn kind_reason(self) -> &'static str {
        match self {
            Self::Directory => "it is not a directory",
            Self::File => "it is not a regular file",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, SetSecurityInfo,
    };
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, PROTECTED_DACL_SECURITY_INFORMATION,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows_sys::Win32::Storage::FileSystem::{WRITE_DAC, WRITE_OWNER};

    const SENTINEL: &[u8] = b"SENTINEL-NOT-A-REAL-SECRET-windows-acl";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "connector-secrets-windows-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }

        fn store(&self) -> PathBuf {
            self.0.join("state").join("credentials")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn planted_store(label: &str) -> (Scratch, PathBuf) {
        let scratch = Scratch::new(label);
        let path = scratch.store();
        super::super::write_atomic(&path, SENTINEL).expect("create protected store");
        (scratch, path)
    }

    /// Repeat the same parent-then-file inspection every production file-store binder performs.
    fn rebind(path: &Path) -> Result<(), StoreError> {
        if let Some(directory) = path.parent() {
            super::super::ensure_directory(directory)?;
        }
        let file = open_existing(path)?.ok_or_else(|| StoreError::Unreachable {
            path: path.display().to_string(),
            reason: "the state file disappeared".to_owned(),
        })?;
        drop(file);
        Ok(())
    }

    fn control_handle(path: &Path, directory: bool) -> OwnedHandle {
        let wide = wide(path).expect("path");
        let flags = FILE_FLAG_OPEN_REPARSE_POINT
            | if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                FILE_ATTRIBUTE_NORMAL
            };
        // SAFETY: path buffer lives through the call; returned handle is uniquely owned below.
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_ALL_ACCESS | READ_CONTROL | WRITE_DAC | WRITE_OWNER,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                flags,
                null_mut(),
            )
        };
        owned_handle(raw).expect("open retained control handle")
    }

    fn descriptor_sddl(handle: &OwnedHandle) -> String {
        let mut descriptor = null_mut();
        // SAFETY: output pointers are local and descriptor is LocalFree-owned on success.
        let status = unsafe {
            GetSecurityInfo(
                handle.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(
            status,
            0,
            "GetSecurityInfo: {}",
            std::io::Error::from_raw_os_error(status as i32)
        );
        let descriptor_guard = LocalAllocation(descriptor);
        let mut rendered = null_mut();
        // SAFETY: descriptor remains live and output pointers are local.
        assert_ne!(
            unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    SDDL_REVISION_1,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    &mut rendered,
                    null_mut(),
                )
            },
            0
        );
        let rendered_guard = LocalAllocation(rendered as *mut c_void);
        let mut length = 0usize;
        // SAFETY: the API returned a nul-terminated LocalAlloc UTF-16 string.
        while unsafe { *rendered.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: the scan found the terminator in the live allocation.
        let result = String::from_utf16(unsafe { std::slice::from_raw_parts(rendered, length) })
            .expect("descriptor SDDL is UTF-16");
        drop(rendered_guard);
        drop(descriptor_guard);
        result
    }

    fn apply_sddl(handle: &OwnedHandle, sddl: &str, security_information: u32) {
        let encoded = wide(std::ffi::OsStr::new(sddl)).expect("SDDL");
        let mut descriptor = null_mut();
        // SAFETY: input/output buffers are live; success returns LocalAlloc descriptor.
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    encoded.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            0,
            "parse SDDL: {}",
            std::io::Error::last_os_error()
        );
        let guard = LocalAllocation(descriptor);
        let mut owner = null_mut();
        let mut owner_defaulted = 0;
        let mut dacl = null_mut();
        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        // SAFETY: descriptor is valid/live and outputs are local.
        assert_ne!(
            unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) },
            0
        );
        // SAFETY: descriptor is valid/live and outputs are local.
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(
                    descriptor,
                    &mut dacl_present,
                    &mut dacl,
                    &mut dacl_defaulted,
                )
            },
            0
        );
        // SAFETY: owner/DACL pointers stay live under guard; retained handle owns requested rights.
        let status = unsafe {
            SetSecurityInfo(
                handle.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                security_information,
                owner,
                null_mut(),
                dacl,
                null_mut(),
            )
        };
        assert_eq!(
            status,
            0,
            "SetSecurityInfo: {}",
            std::io::Error::from_raw_os_error(status as i32)
        );
        drop(guard);
    }

    #[test]
    fn creation_is_owned_and_protected_for_only_the_process_sid() {
        let (_scratch, path) = planted_store("protected-creation");
        let handle = control_handle(&path, false);
        inspect_security(handle.as_raw_handle() as HANDLE, &path)
            .expect("exact protected descriptor");
        let sddl = descriptor_sddl(&handle);
        assert!(sddl.contains("D:P"), "unprotected descriptor: {sddl}");
        let directory = path.parent().expect("state directory");
        let directory_handle = control_handle(directory, true);
        inspect_security(directory_handle.as_raw_handle() as HANDLE, directory)
            .expect("exact protected directory descriptor");
    }

    #[test]
    fn unsafe_descriptors_refuse_without_repair_or_leak() {
        let sid = sid_string(&current_process_sid().expect("process SID")).expect("SID text");
        for (label, planted, flags) in [
            (
                "foreign-allow",
                format!("O:{sid}D:P(A;;FA;;;{sid})(A;;FR;;;WD)"),
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
            ),
            (
                "foreign-owner",
                format!("O:BAD:P(A;;FA;;;{sid})"),
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
            ),
            (
                "unprotected",
                format!("O:{sid}D:(A;;FA;;;{sid})"),
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | UNPROTECTED_DACL_SECURITY_INFORMATION,
            ),
            (
                "unreadable",
                format!("O:{sid}D:P(D;;RC;;;OW)(A;;FA;;;{sid})"),
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
            ),
        ] {
            for directory in [false, true] {
                let (_scratch, path) = planted_store(label);
                let bytes = std::fs::read(&path).expect("prior bytes");
                let target = if directory {
                    path.parent().expect("directory")
                } else {
                    path.as_path()
                };
                let control = control_handle(target, directory);
                let original = descriptor_sddl(&control);
                apply_sddl(&control, &planted, flags);
                let planted_equivalent = descriptor_sddl(&control);

                let error = rebind(&path).expect_err("unsafe descriptor must refuse");
                let message = error.to_string();
                if label == "unreadable" {
                    assert!(matches!(error, StoreError::Unreachable { .. }), "{error:?}");
                    assert!(message.to_ascii_lowercase().contains("denied"), "{message}");
                } else {
                    assert!(matches!(error, StoreError::Denied { .. }), "{error:?}");
                }
                assert!(message.contains(&target.display().to_string()), "{message}");
                assert!(!message.contains("SENTINEL"), "{message}");
                assert_eq!(
                    descriptor_sddl(&control),
                    planted_equivalent,
                    "refusal repaired {label} (directory={directory})"
                );

                apply_sddl(
                    &control,
                    &original,
                    OWNER_SECURITY_INFORMATION
                        | DACL_SECURITY_INFORMATION
                        | PROTECTED_DACL_SECURITY_INFORMATION,
                );
                assert_eq!(std::fs::read(&path).expect("bytes unchanged"), bytes);
            }
        }
    }

    #[test]
    fn genuinely_inherited_allow_entries_are_refused_without_repair_or_reading() {
        let sid = sid_string(&current_process_sid().expect("process SID")).expect("SID text");
        for directory in [false, true] {
            let scratch = Scratch::new("inherited-allow");
            std::fs::create_dir_all(&scratch.0).expect("create inheritance parent");
            let parent = control_handle(&scratch.0, true);
            apply_sddl(
                &parent,
                &format!("O:{sid}D:P(A;OICI;FA;;;{sid})"),
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
            );

            let target = scratch.0.join(if directory {
                "inherited-directory"
            } else {
                "inherited-credentials"
            });
            let prior_bytes = if directory {
                std::fs::create_dir(&target).expect("create inherited directory");
                None
            } else {
                let bytes = SENTINEL.to_vec();
                std::fs::write(&target, &bytes).expect("create inherited store");
                Some(bytes)
            };
            let control = control_handle(&target, directory);
            let inherited = descriptor_sddl(&control);
            assert!(
                inherited.contains("ID"),
                "the native fixture did not inherit an ACE: {inherited}"
            );

            let error = if directory {
                open_secure_handle(&target, Expected::Directory, false)
                    .expect_err("inherited directory must refuse")
            } else {
                open_existing(&target).expect_err("inherited file must refuse")
            };
            let message = error.to_string();
            assert!(matches!(error, StoreError::Denied { .. }), "{error:?}");
            assert!(message.contains(&target.display().to_string()), "{message}");
            assert!(!message.contains("SENTINEL"), "{message}");
            assert_eq!(
                descriptor_sddl(&control),
                inherited,
                "refusal repaired the DACL"
            );
            if let Some(bytes) = prior_bytes {
                assert_eq!(std::fs::read(&target).expect("bytes unchanged"), bytes);
            }
        }
    }

    #[test]
    fn mutations_revalidate_file_and_directory_descriptors_before_writing() {
        let sid = sid_string(&current_process_sid().expect("process SID")).expect("SID text");
        let widened = format!("O:{sid}D:P(A;;FA;;;{sid})(A;;FR;;;WD)");
        for directory in [false, true] {
            let scratch = Scratch::new("post-open-mutation");
            let path = scratch.store();
            super::super::write_atomic(&path, SENTINEL).expect("plant");
            let bytes = std::fs::read(&path).expect("bytes");
            let target = if directory {
                path.parent().expect("directory")
            } else {
                path.as_path()
            };
            let control = control_handle(target, directory);
            let original = descriptor_sddl(&control);
            apply_sddl(
                &control,
                &widened,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
            );
            let planted = descriptor_sddl(&control);
            let error = super::super::write_atomic(&path, b"replacement").expect_err("must refuse");
            assert!(matches!(error, StoreError::Denied { .. }), "{error:?}");
            let message = error.to_string();
            assert!(message.contains(&target.display().to_string()), "{message}");
            assert!(!message.contains("SENTINEL"));
            assert_eq!(std::fs::read(&path).expect("bytes remain"), bytes);
            assert_eq!(descriptor_sddl(&control), planted);
            let temporaries = std::fs::read_dir(path.parent().expect("parent"))
                .expect("read parent")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .count();
            assert_eq!(temporaries, 0, "temporary was written");
            apply_sddl(
                &control,
                &original,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
            );
        }
    }

    #[test]
    fn reparse_points_and_wrong_kinds_are_refused_unchanged() {
        let (scratch, real) = planted_store("reparse");
        let link = real.parent().expect("parent").join("linked-credentials");
        std::os::windows::fs::symlink_file(&real, &link).expect("create file symlink");
        let linked_target = std::fs::read_link(&link).expect("link target");
        let before = std::fs::symlink_metadata(&link)
            .expect("link metadata")
            .file_attributes();
        let link_control = control_handle(&link, false);
        let link_descriptor = descriptor_sddl(&link_control);
        let error = rebind(&link).expect_err("reparse point must refuse");
        let message = error.to_string();
        assert!(message.contains(&link.display().to_string()), "{message}");
        assert!(!message.contains("SENTINEL"));
        assert_eq!(
            std::fs::read_link(&link).expect("link unchanged"),
            linked_target
        );
        assert_eq!(
            std::fs::symlink_metadata(&link)
                .expect("link remains")
                .file_attributes(),
            before
        );
        assert_eq!(descriptor_sddl(&link_control), link_descriptor);

        let wrong = scratch.0.join("state").join("directory-at-file");
        std::fs::create_dir(&wrong).expect("plant directory");
        let before = std::fs::metadata(&wrong)
            .expect("metadata")
            .file_attributes();
        let wrong_control = control_handle(&wrong, true);
        let wrong_descriptor = descriptor_sddl(&wrong_control);
        let error = rebind(&wrong).expect_err("wrong kind must refuse");
        let message = error.to_string();
        assert!(message.contains(&wrong.display().to_string()), "{message}");
        assert!(!message.contains("SENTINEL"));
        assert_eq!(
            std::fs::metadata(&wrong)
                .expect("unchanged")
                .file_attributes(),
            before
        );
        assert_eq!(descriptor_sddl(&wrong_control), wrong_descriptor);
    }
}
