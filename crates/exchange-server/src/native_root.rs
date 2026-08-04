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
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;
    use std::path::PathBuf;

    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DONT_VERIFY,
    };

    pub(super) fn authenticated_account_state_root() -> Result<PathBuf, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: the API allocates a NUL-terminated path for the current process token when passed
        // a null token. `raw` is released with the documented COM allocator on every success path.
        let result = unsafe {
            SHGetKnownFolderPath(
                &FOLDERID_LocalAppData,
                KF_FLAG_DONT_VERIFY as u32,
                std::ptr::null_mut(),
                &mut raw,
            )
        };
        if result != 0 {
            return Err(format!(
                "SHGetKnownFolderPath(FOLDERID_LocalAppData) refused the authenticated account with HRESULT {result:#x}"
            ));
        }
        if raw.is_null() {
            return Err(
                "SHGetKnownFolderPath(FOLDERID_LocalAppData) returned a null path".to_owned(),
            );
        }
        // SAFETY: success returned a NUL-terminated UTF-16 allocation owned by the COM allocator.
        let length = unsafe { (0..).take_while(|offset| *raw.add(*offset) != 0).count() };
        let value = OsString::from_wide(unsafe { std::slice::from_raw_parts(raw, length) });
        unsafe { CoTaskMemFree(raw.cast()) };
        let base = PathBuf::from(value);
        if !base.is_absolute() {
            return Err("the authenticated account LocalAppData path is not absolute".to_owned());
        }
        Ok(base.join("Flux/Exchange"))
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use std::path::PathBuf;

    pub(super) fn authenticated_account_state_root() -> Result<PathBuf, String> {
        Err("this platform has no authenticated-account state-root resolver".to_owned())
    }
}
