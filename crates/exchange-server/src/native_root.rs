//! Production state-root discovery from the authenticated operating-system account.
//!
//! Inherited environment variables are caller-controlled process inputs. They are useful for a
//! development override, but they cannot identify the account whose native filesystem authority
//! protects production state.

use std::path::PathBuf;

/// Resolve the conventional production state root without creating or repairing any object.
pub(crate) fn authenticated_account_state_root() -> Result<PathBuf, String> {
    platform::authenticated_account_state_root()
}

/// Validate the authenticated Windows profile ancestry and create only Exchange-owned children.
#[cfg(windows)]
pub(crate) fn ensure_authenticated_account_state_root(
    root: &std::path::Path,
) -> Result<PathBuf, String> {
    platform::ensure_authenticated_account_state_root(root)
}

#[cfg(unix)]
mod platform {
    use std::ffi::{CStr, OsString};
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::PathBuf;

    const MAX_ACCOUNT_BUFFER: usize = 1024 * 1024;

    pub(super) fn authenticated_account_state_root() -> Result<PathBuf, String> {
        // SAFETY: `geteuid` has no pointer arguments or preconditions and returns the identity the
        // kernel applies to filesystem access. It is intentionally not inferred from USER/HOME.
        let uid = unsafe { libc::geteuid() };
        let mut size = initial_buffer_size();
        loop {
            let mut buffer = vec![0_u8; size];
            let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
            let mut found = std::ptr::null_mut();
            // SAFETY: all output storage remains live for the call, the byte buffer has the stated
            // length, and `getpwuid_r` writes only within those caller-provided regions.
            let result = unsafe {
                libc::getpwuid_r(
                    uid,
                    entry.as_mut_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut found,
                )
            };
            if result == libc::ERANGE && size < MAX_ACCOUNT_BUFFER {
                size = (size * 2).min(MAX_ACCOUNT_BUFFER);
                continue;
            }
            if result != 0 {
                return Err(format!(
                    "getpwuid_r refused effective uid {uid} with errno {result}"
                ));
            }
            if found.is_null() {
                return Err(format!(
                    "getpwuid_r found no authenticated account for effective uid {uid}"
                ));
            }
            // SAFETY: a successful non-null result initializes `entry`; `pw_dir` points into the
            // still-live caller buffer and is NUL-terminated by the native account API.
            let entry = unsafe { entry.assume_init() };
            if entry.pw_dir.is_null() {
                return Err(format!(
                    "the authenticated account for effective uid {uid} has no home directory"
                ));
            }
            let bytes = unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes();
            if bytes.is_empty() {
                return Err(format!(
                    "the authenticated account for effective uid {uid} has an empty home directory"
                ));
            }
            let home = PathBuf::from(OsString::from_vec(bytes.to_vec()));
            if !home.is_absolute() {
                return Err(format!(
                    "the authenticated account for effective uid {uid} has a non-absolute home directory"
                ));
            }
            #[cfg(target_os = "macos")]
            return Ok(home.join("Library/Application Support/Flux/Exchange"));
            #[cfg(not(target_os = "macos"))]
            return Ok(home.join(".local/state/flux-exchange"));
        }
    }

    fn initial_buffer_size() -> usize {
        // SAFETY: `sysconf` has no pointer arguments. A negative/implausible advisory is ignored;
        // the retry loop is authoritative and bounded against untrusted account-database growth.
        let advised = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
        usize::try_from(advised)
            .ok()
            .filter(|size| *size >= 1024 && *size <= MAX_ACCOUNT_BUFFER)
            .unwrap_or(4096)
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::ffi::OsString;
    use std::mem::{offset_of, size_of};
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use std::path::{Component, Path, PathBuf};
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        GetLastError, LocalFree, ERROR_INSUFFICIENT_BUFFER, GENERIC_ALL, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetLengthSid, GetTokenInformation,
        IsValidAcl, IsValidSecurityDescriptor, IsValidSid, TokenUser, ACCESS_ALLOWED_ACE,
        ACE_HEADER, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PRESENT, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE,
        FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, OPEN_EXISTING, READ_CONTROL,
        WRITE_DAC, WRITE_OWNER,
    };
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_Profile, SHGetKnownFolderPath, KF_FLAG_DONT_VERIFY,
    };

    pub(super) fn authenticated_account_state_root() -> Result<PathBuf, String> {
        Ok(authenticated_account_paths()?.2)
    }

    pub(super) fn ensure_authenticated_account_state_root(root: &Path) -> Result<PathBuf, String> {
        let (profile, local_app_data, expected) = authenticated_account_paths()?;
        if root != expected {
            return Err(format!(
                "the selected production root `{}` differs from the authenticated account root `{}`",
                root.display(),
                expected.display()
            ));
        }
        ensure_conventional_root(&profile, &local_app_data, &expected)
    }

    fn authenticated_account_paths() -> Result<(PathBuf, PathBuf, PathBuf), String> {
        let profile = known_folder(&FOLDERID_Profile, "FOLDERID_Profile")?;
        let local_app_data = known_folder(&FOLDERID_LocalAppData, "FOLDERID_LocalAppData")?;
        if !local_app_data.starts_with(&profile) {
            return Err(format!(
                "the authenticated account LocalAppData `{}` is outside its profile boundary `{}`",
                local_app_data.display(),
                profile.display()
            ));
        }
        let root = local_app_data.join("Flux/Exchange");
        Ok((profile, local_app_data, root))
    }

    fn known_folder(folder: &windows_sys::core::GUID, name: &str) -> Result<PathBuf, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: the API allocates a NUL-terminated path for the current process token when passed
        // a null token. `raw` is released with the documented COM allocator on every success path.
        let result = unsafe {
            SHGetKnownFolderPath(
                folder,
                KF_FLAG_DONT_VERIFY as u32,
                std::ptr::null_mut(),
                &mut raw,
            )
        };
        if result != 0 {
            return Err(format!(
                "SHGetKnownFolderPath({name}) refused the authenticated account with HRESULT {result:#x}"
            ));
        }
        if raw.is_null() {
            return Err(format!("SHGetKnownFolderPath({name}) returned a null path"));
        }
        // SAFETY: success returned a NUL-terminated UTF-16 allocation owned by the COM allocator.
        let length = unsafe { (0..).take_while(|offset| *raw.add(*offset) != 0).count() };
        let value = OsString::from_wide(unsafe { std::slice::from_raw_parts(raw, length) });
        unsafe { CoTaskMemFree(raw.cast()) };
        let base = PathBuf::from(value);
        if !base.is_absolute() {
            return Err(format!(
                "the authenticated account {name} path is not absolute"
            ));
        }
        Ok(base)
    }

    fn ensure_conventional_root(
        profile: &Path,
        local_app_data: &Path,
        root: &Path,
    ) -> Result<PathBuf, String> {
        validate_shape(profile, local_app_data, root)?;
        let sid = current_process_sid()?;
        let mut reached_profile = false;
        for prefix in existing_prefixes(local_app_data)? {
            inspect_directory(&prefix)?;
            if prefix == profile {
                reached_profile = true;
            }
            inspect_ancestor_security(&prefix, &sid, reached_profile)?;
        }
        if !reached_profile {
            return Err(format!(
                "the authenticated account profile boundary `{}` was not traversed",
                profile.display()
            ));
        }

        // Native account paths must already exist. Exchange may create only its two named children;
        // the shared filesystem primitive gives both an exact current-SID protected DACL.
        let flux = local_app_data.join("Flux");
        exchange_host::ensure_private_state_directory(&flux)
            .map_err(|error| format!("cannot create or inspect `{}`: {error}", flux.display()))?;
        inspect_directory(&flux)?;
        exchange_host::ensure_private_state_directory(root)
            .map_err(|error| format!("cannot create or inspect `{}`: {error}", root.display()))?;
        inspect_directory(root)?;
        Ok(root.to_path_buf())
    }

    fn validate_shape(profile: &Path, local_app_data: &Path, root: &Path) -> Result<(), String> {
        for (name, path) in [
            ("profile", profile),
            ("LocalAppData", local_app_data),
            ("Exchange root", root),
        ] {
            if !path.is_absolute() {
                return Err(format!("the authenticated account {name} is not absolute"));
            }
            if path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(format!(
                    "the authenticated account {name} contains parent-directory traversal"
                ));
            }
        }
        if !local_app_data.starts_with(profile) {
            return Err(
                "LocalAppData is outside the authenticated account profile boundary".into(),
            );
        }
        if root != local_app_data.join("Flux/Exchange") {
            return Err(
                "the production root is not the exact LocalAppData/Flux/Exchange path".into(),
            );
        }
        Ok(())
    }

    fn existing_prefixes(path: &Path) -> Result<Vec<PathBuf>, String> {
        let mut prefixes = Vec::new();
        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component.as_os_str());
            if prefix.is_absolute() {
                prefixes.push(prefix.clone());
            }
        }
        for prefix in &prefixes {
            if !prefix.exists() {
                return Err(format!(
                    "authenticated account ancestry `{}` does not exist; Exchange did not create it",
                    prefix.display()
                ));
            }
        }
        Ok(prefixes)
    }

    fn inspect_directory(path: &Path) -> Result<OwnedHandle, String> {
        let wide = wide(path)?;
        // SAFETY: the path buffer remains live; OPEN_REPARSE_POINT opens the named component itself,
        // and this read-only handle is closed by OwnedHandle.
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        let handle = owned_handle(raw)
            .map_err(|error| format!("cannot inspect `{}`: {error}", path.display()))?;
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: handle and output storage remain live for the call.
        if unsafe { GetFileInformationByHandle(handle.as_raw_handle() as HANDLE, &mut information) }
            == 0
        {
            return Err(format!(
                "cannot inspect `{}`: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!(
                "`{}` is a reparse point; Exchange did not follow or replace it",
                path.display()
            ));
        }
        if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(format!("`{}` is not a directory", path.display()));
        }
        Ok(handle)
    }

    fn inspect_ancestor_security(
        path: &Path,
        sid: &ProcessSid,
        require_account_owner: bool,
    ) -> Result<(), String> {
        let handle = inspect_directory(path)?;
        let mut owner: PSID = null_mut();
        let mut dacl = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: outputs are local. On success descriptor owns owner and DACL until LocalFree.
        let status = unsafe {
            GetSecurityInfo(
                handle.as_raw_handle() as HANDLE,
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
            return Err(format!(
                "cannot inspect `{}` security: {}",
                path.display(),
                std::io::Error::from_raw_os_error(status as i32)
            ));
        }
        if descriptor.is_null() {
            return Err(format!(
                "`{}` has no inspectable security descriptor",
                path.display()
            ));
        }
        let _descriptor = LocalAllocation(descriptor.cast());
        // SAFETY: all pointers lie in the live descriptor returned above.
        if unsafe { IsValidSecurityDescriptor(descriptor) } == 0
            || owner.is_null()
            || dacl.is_null()
            || unsafe { IsValidSid(owner) } == 0
            || unsafe { IsValidAcl(dacl) } == 0
        {
            return Err(format!("`{}` has unsafe security metadata", path.display()));
        }
        // SAFETY: both validated SIDs remain live for this comparison.
        if require_account_owner && unsafe { EqualSid(owner, sid.as_ptr()) } == 0 {
            return Err(format!(
                "`{}` is owned by a foreign SID rather than the authenticated process account; Exchange did not repair it",
                path.display()
            ));
        }
        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: descriptor and output pointers remain live.
        if unsafe {
            windows_sys::Win32::Security::GetSecurityDescriptorControl(
                descriptor,
                &mut control,
                &mut revision,
            )
        } == 0
            || control & SE_DACL_PRESENT == 0
        {
            return Err(format!("`{}` has no inspectable DACL", path.display()));
        }
        reject_untrusted_writers(path, dacl, sid)
    }

    fn reject_untrusted_writers(
        path: &Path,
        dacl: *mut windows_sys::Win32::Security::ACL,
        sid: &ProcessSid,
    ) -> Result<(), String> {
        let mut information = ACL_SIZE_INFORMATION::default();
        // SAFETY: DACL is valid/live; output storage is exact.
        if unsafe {
            GetAclInformation(
                dacl,
                (&mut information as *mut ACL_SIZE_INFORMATION).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(format!(
                "cannot inspect `{}` DACL: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        for index in 0..information.AceCount {
            let mut ace = null_mut();
            // SAFETY: index is bounded by GetAclInformation's ACE count.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(format!(
                    "cannot inspect `{}` DACL entry {index}: {}",
                    path.display(),
                    std::io::Error::last_os_error()
                ));
            }
            // SAFETY: GetAce returned at least the common ACE header in the valid DACL.
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            if header.AceType as u32 == ACCESS_DENIED_ACE_TYPE {
                continue;
            }
            if (header.AceSize as usize) < size_of::<ACE_HEADER>() + size_of::<u32>() {
                return Err(format!("`{}` has a truncated DACL entry", path.display()));
            }
            // Every access ACE stores its mask directly after ACE_HEADER. Unknown allow-like ACEs
            // with write authority refuse because Exchange cannot prove whom they empower.
            let mask = unsafe {
                std::ptr::read_unaligned(
                    (ace as *const u8).add(size_of::<ACE_HEADER>()) as *const u32
                )
            };
            if mask & WRITE_MASK == 0 {
                continue;
            }
            if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE {
                return Err(format!(
                    "`{}` has an unsupported writable DACL entry; Exchange did not repair it",
                    path.display()
                ));
            }
            let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
            if (header.AceSize as usize) < sid_offset + size_of::<u32>() {
                return Err(format!("`{}` has a truncated DACL SID", path.display()));
            }
            let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let ace_sid = &allowed.SidStart as *const u32 as PSID;
            if unsafe { IsValidSid(ace_sid) } == 0 {
                return Err(format!("`{}` has a malformed DACL SID", path.display()));
            }
            let sid_length = unsafe { GetLengthSid(ace_sid) } as usize;
            if sid_offset
                .checked_add(sid_length)
                .is_none_or(|end| end > header.AceSize as usize)
            {
                return Err(format!("`{}` has an overlong DACL SID", path.display()));
            }
            if !trusted_writer(ace_sid, sid)? {
                return Err(format!(
                    "`{}` grants write authority to an untrusted SID; Exchange did not repair it",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    const WRITE_MASK: u32 = GENERIC_ALL
        | GENERIC_WRITE
        | DELETE
        | WRITE_DAC
        | WRITE_OWNER
        | FILE_WRITE_DATA
        | FILE_APPEND_DATA
        | FILE_WRITE_EA
        | FILE_WRITE_ATTRIBUTES
        | FILE_DELETE_CHILD;

    fn trusted_writer(candidate: PSID, process: &ProcessSid) -> Result<bool, String> {
        // SAFETY: both SIDs were validated and remain live.
        if unsafe { EqualSid(candidate, process.as_ptr()) } != 0 {
            return Ok(true);
        }
        let text = sid_string(candidate)?;
        Ok(matches!(
            text.as_str(),
            "S-1-5-18" | "S-1-5-32-544" | "S-1-3-0" | "S-1-3-4"
        ))
    }

    struct ProcessSid {
        bytes: Vec<usize>,
        sid_offset: usize,
    }

    impl ProcessSid {
        fn as_ptr(&self) -> PSID {
            unsafe { (self.bytes.as_ptr() as *const u8).add(self.sid_offset) as PSID }
        }
    }

    fn current_process_sid() -> Result<ProcessSid, String> {
        let mut raw: HANDLE = null_mut();
        // SAFETY: output is local; token becomes OwnedHandle, process pseudo-handle is not closed.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(format!(
                "OpenProcessToken refused: {}",
                std::io::Error::last_os_error()
            ));
        }
        let token = owned_handle(raw).map_err(|error| error.to_string())?;
        let mut length = 0_u32;
        let sized = unsafe {
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenUser,
                null_mut(),
                0,
                &mut length,
            )
        };
        if sized != 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            return Err(format!(
                "TokenUser sizing refused: {}",
                std::io::Error::last_os_error()
            ));
        }
        let capacity = length as usize;
        let mut bytes = vec![0_usize; capacity.div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(format!(
                "TokenUser refused: {}",
                std::io::Error::last_os_error()
            ));
        }
        if capacity < size_of::<TOKEN_USER>() || length as usize > capacity {
            return Err("TokenUser returned an invalid buffer length".into());
        }
        let base = bytes.as_ptr() as *const u8;
        let sid = unsafe { (*(base as *const TOKEN_USER)).User.Sid as *const u8 };
        let sid_offset = (sid as usize)
            .checked_sub(base as usize)
            .filter(|offset| *offset < capacity)
            .ok_or_else(|| "TokenUser returned a SID outside its buffer".to_owned())?;
        if unsafe { IsValidSid(sid as PSID) } == 0 {
            return Err("TokenUser returned a malformed SID".into());
        }
        let sid_length = unsafe { GetLengthSid(sid as PSID) } as usize;
        if sid_offset
            .checked_add(sid_length)
            .is_none_or(|end| end > capacity)
        {
            return Err("TokenUser returned a SID outside its buffer".into());
        }
        Ok(ProcessSid { bytes, sid_offset })
    }

    fn sid_string(sid: PSID) -> Result<String, String> {
        let mut raw = null_mut();
        // SAFETY: validated SID remains live; success returns a LocalAlloc string.
        if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 {
            return Err(format!(
                "cannot render SID: {}",
                std::io::Error::last_os_error()
            ));
        }
        if raw.is_null() {
            return Err("cannot render SID: the native API returned a null string".into());
        }
        let allocation = LocalAllocation(raw.cast());
        let length = unsafe { (0..).take_while(|offset| *raw.add(*offset) != 0).count() };
        let rendered = String::from_utf16(unsafe { std::slice::from_raw_parts(raw, length) })
            .map_err(|_| "SID text is not UTF-16".to_owned())?;
        drop(allocation);
        Ok(rendered)
    }

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(self.0);
            }
        }
    }

    fn owned_handle(raw: HANDLE) -> std::io::Result<OwnedHandle> {
        if raw == INVALID_HANDLE_VALUE || raw.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
        }
    }

    fn wide(path: &Path) -> Result<Vec<u16>, String> {
        let mut encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if encoded.contains(&0) {
            return Err(format!("`{}` contains an interior NUL", path.display()));
        }
        encoded.push(0);
        Ok(encoded)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicU64, Ordering};
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SetSecurityInfo, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{
            GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
            PROTECTED_DACL_SECURITY_INFORMATION,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

        struct Scratch(PathBuf);

        impl Scratch {
            fn new(label: &str) -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "flux-exchange-x134-windows-root-{label}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                let _ = std::fs::remove_dir_all(&path);
                exchange_host::ensure_private_state_directory(&path)
                    .expect("owner-only fixture boundary");
                Self(path)
            }

            fn paths(&self) -> (PathBuf, PathBuf, PathBuf) {
                let profile = self.0.join("profile");
                let local = profile.join("AppData/Local");
                exchange_host::ensure_private_state_directory(&local)
                    .expect("owner-only profile fixture");
                let root = local.join("Flux/Exchange");
                (profile, local, root)
            }
        }

        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn creates_only_flux_and_exchange_below_a_valid_profile_boundary() {
            let scratch = Scratch::new("create");
            let (profile, local, root) = scratch.paths();

            assert_eq!(
                ensure_conventional_root(&profile, &local, &root).expect("safe native root"),
                root
            );
            exchange_host::ensure_private_state_directory(local.join("Flux"))
                .expect("Flux has exact owner-only DACL");
            exchange_host::ensure_private_state_directory(&root)
                .expect("Exchange has exact owner-only DACL");
        }

        #[test]
        fn refuses_a_reparse_component_without_following_or_repairing_it() {
            let scratch = Scratch::new("reparse");
            let profile = scratch.0.join("profile");
            exchange_host::ensure_private_state_directory(&profile).expect("profile boundary");
            let redirected = scratch.0.join("redirected");
            exchange_host::ensure_private_state_directory(redirected.join("Local"))
                .expect("redirect destination");
            let link = profile.join("AppData");
            std::os::windows::fs::symlink_dir(&redirected, &link).expect("directory reparse point");
            let before = std::fs::read_link(&link).expect("link target");
            let local = link.join("Local");
            let root = local.join("Flux/Exchange");

            let refusal = ensure_conventional_root(&profile, &local, &root)
                .expect_err("reparse ancestry must refuse");
            assert!(refusal.contains("reparse point"), "{refusal}");
            assert!(refusal.contains(&link.display().to_string()), "{refusal}");
            assert_eq!(std::fs::read_link(&link).expect("link unchanged"), before);
            assert!(
                !redirected.join("Local/Flux").exists(),
                "Exchange followed and populated the planted reparse point"
            );
        }

        #[test]
        fn refuses_foreign_profile_owner_and_untrusted_writer_without_repair() {
            let sid =
                sid_string(current_process_sid().expect("process SID").as_ptr()).expect("SID text");
            for (label, planted) in [
                ("foreign-owner", format!("O:BAD:P(A;;FA;;;{sid})")),
                (
                    "untrusted-writer",
                    format!("O:{sid}D:P(A;;FA;;;{sid})(A;;FW;;;WD)"),
                ),
            ] {
                let scratch = Scratch::new(label);
                let (profile, local, root) = scratch.paths();
                let handle = control_handle(&profile);
                let original = descriptor_sddl(&handle);
                apply_sddl(&handle, &planted);
                let planted_equivalent = descriptor_sddl(&handle);

                let refusal = ensure_conventional_root(&profile, &local, &root)
                    .expect_err("unsafe profile metadata must refuse");
                assert!(
                    refusal.contains(&profile.display().to_string()),
                    "{refusal}"
                );
                assert!(
                    refusal.contains(if label == "foreign-owner" {
                        "foreign SID"
                    } else {
                        "untrusted SID"
                    }),
                    "{refusal}"
                );
                assert_eq!(
                    descriptor_sddl(&handle),
                    planted_equivalent,
                    "refusal repaired {label}"
                );
                assert!(!local.join("Flux").exists());

                apply_sddl(&handle, &original);
            }
        }

        fn control_handle(path: &Path) -> OwnedHandle {
            let encoded = wide(path).expect("path");
            let raw = unsafe {
                CreateFileW(
                    encoded.as_ptr(),
                    FILE_ALL_ACCESS | READ_CONTROL | WRITE_DAC | WRITE_OWNER,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    null_mut(),
                )
            };
            owned_handle(raw).expect("retained control handle")
        }

        fn descriptor_sddl(handle: &OwnedHandle) -> String {
            let mut descriptor = null_mut();
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
            assert_eq!(status, 0, "GetSecurityInfo status {status}");
            let descriptor_guard = LocalAllocation(descriptor);
            let mut rendered = null_mut();
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
            let rendered_guard = LocalAllocation(rendered.cast());
            let length = unsafe {
                (0..)
                    .take_while(|offset| *rendered.add(*offset) != 0)
                    .count()
            };
            let text = String::from_utf16(unsafe { std::slice::from_raw_parts(rendered, length) })
                .expect("descriptor SDDL");
            drop(rendered_guard);
            drop(descriptor_guard);
            text
        }

        fn apply_sddl(handle: &OwnedHandle, sddl: &str) {
            let mut encoded = OsString::from(sddl).encode_wide().collect::<Vec<_>>();
            encoded.push(0);
            let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        encoded.as_ptr(),
                        SDDL_REVISION_1,
                        &mut descriptor,
                        null_mut(),
                    )
                },
                0
            );
            let guard = LocalAllocation(descriptor);
            let mut owner = null_mut();
            let mut owner_defaulted = 0;
            let mut dacl = null_mut();
            let mut dacl_present = 0;
            let mut dacl_defaulted = 0;
            assert_ne!(
                unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) },
                0
            );
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
            let status = unsafe {
                SetSecurityInfo(
                    handle.as_raw_handle() as HANDLE,
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION
                        | DACL_SECURITY_INFORMATION
                        | PROTECTED_DACL_SECURITY_INFORMATION,
                    owner,
                    null_mut(),
                    dacl,
                    null_mut(),
                )
            };
            assert_eq!(status, 0, "SetSecurityInfo status {status}");
            drop(guard);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use std::path::PathBuf;

    pub(super) fn authenticated_account_state_root() -> Result<PathBuf, String> {
        Err("this platform has no authenticated-account state-root resolver".to_owned())
    }
}
