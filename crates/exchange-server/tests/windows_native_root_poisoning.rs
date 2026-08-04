#![cfg(all(windows, feature = "native-root-test-seam"))]

use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::Foundation::{LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ALL_ACCESS, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    WRITE_OWNER,
};

const PROFILE_SEAM: &str = "FLUX_EXCHANGE_TEST_WINDOWS_PROFILE";
const LOCAL_APP_DATA_SEAM: &str = "FLUX_EXCHANGE_TEST_WINDOWS_LOCAL_APP_DATA";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-x134-windows-root-process-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        exchange_host::ensure_private_state_directory(&path)
            .expect("owner-only process fixture boundary");
        Self(path)
    }

    fn profile(&self) -> PathBuf {
        self.0.join("profile")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: every instance owns one Win32 LocalAlloc result.
        unsafe {
            let _ = LocalFree(self.0);
        }
    }
}

#[test]
fn windows_supervised_startup_refuses_reparse_point_owner_root_ancestor_without_repair() {
    let scratch = Scratch::new("reparse");
    let profile = scratch.profile();
    exchange_host::ensure_private_state_directory(&profile).expect("private profile boundary");
    let redirected = scratch.0.join("redirected");
    let redirected_local = redirected.join("Local");
    exchange_host::ensure_private_state_directory(&redirected_local)
        .expect("private redirect destination");
    let link = profile.join("AppData");
    std::os::windows::fs::symlink_dir(&redirected, &link).expect("directory reparse point");
    let before = std::fs::read_link(&link).expect("reparse target before startup");
    let local_app_data = link.join("Local");

    let output = run_supervised_root_startup(&profile, &local_app_data);
    let refusal = diagnostics(&output);

    assert!(
        !output.status.success(),
        "poisoned startup succeeded:\n{refusal}"
    );
    assert!(output.stdout.is_empty(), "startup wrote stdout:\n{refusal}");
    assert!(
        refusal.contains("reparse point") && refusal.contains(&link.display().to_string()),
        "startup did not precisely refuse the reparse ancestor:\n{refusal}"
    );
    assert_eq!(
        std::fs::read_link(&link).expect("reparse target after refusal"),
        before,
        "startup repaired or replaced the reparse point"
    );
    assert!(
        !redirected_local.join("Flux").exists(),
        "startup followed the reparse ancestor and created below its destination"
    );
}

#[test]
fn windows_supervised_startup_refuses_untrusted_writable_owner_root_ancestor_without_repair() {
    let scratch = Scratch::new("untrusted-writer");
    let profile = scratch.profile();
    let local_app_data = profile.join("AppData/Local");
    exchange_host::ensure_private_state_directory(&local_app_data)
        .expect("private LocalAppData fixture");
    let handle = control_handle(&profile);
    let original = descriptor_sddl(&handle);
    let sid = current_owner_sid(&handle);
    apply_sddl(&handle, &format!("O:{sid}D:P(A;;FA;;;{sid})(A;;FW;;;WD)"));
    let poisoned = descriptor_sddl(&handle);

    let output = run_supervised_root_startup(&profile, &local_app_data);
    let refusal = diagnostics(&output);

    assert!(
        !output.status.success(),
        "poisoned startup succeeded:\n{refusal}"
    );
    assert!(output.stdout.is_empty(), "startup wrote stdout:\n{refusal}");
    assert!(
        refusal.contains("untrusted SID") && refusal.contains(&profile.display().to_string()),
        "startup did not precisely refuse the writable DACL ancestor:\n{refusal}"
    );
    assert_eq!(
        descriptor_sddl(&handle),
        poisoned,
        "startup repaired the unsafe ancestor DACL"
    );
    assert!(
        !local_app_data.join("Flux").exists(),
        "startup created Exchange children below unsafe DACL ancestry"
    );

    apply_sddl(&handle, &original);
}

fn run_supervised_root_startup(profile: &Path, local_app_data: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .arg("--supervised")
        .env_clear()
        .env(PROFILE_SEAM, profile)
        .env(LOCAL_APP_DATA_SEAM, local_app_data)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "warn")
        .output()
        .expect("real supervised Exchange process")
}

fn diagnostics(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn control_handle(path: &Path) -> OwnedHandle {
    let encoded = wide(path);
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
    assert!(raw != INVALID_HANDLE_VALUE && !raw.is_null());
    // SAFETY: CreateFileW returned one live owned handle.
    unsafe { OwnedHandle::from_raw_handle(raw) }
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
    let _descriptor = LocalAllocation(descriptor);
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
    let _rendered = LocalAllocation(rendered.cast());
    let length = unsafe {
        (0..)
            .take_while(|offset| *rendered.add(*offset) != 0)
            .count()
    };
    String::from_utf16(unsafe { std::slice::from_raw_parts(rendered, length) })
        .expect("descriptor SDDL")
}

fn current_owner_sid(handle: &OwnedHandle) -> String {
    let sddl = descriptor_sddl(handle);
    sddl.strip_prefix("O:")
        .and_then(|value| value.split_once("D:").map(|(owner, _)| owner))
        .filter(|owner| !owner.is_empty())
        .expect("descriptor carries an owner SID")
        .to_owned()
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
    let _descriptor = LocalAllocation(descriptor);
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
}

fn wide(path: &Path) -> Vec<u16> {
    let mut encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    assert!(
        !encoded.contains(&0),
        "fixture path contains an interior NUL"
    );
    encoded.push(0);
    encoded
}
