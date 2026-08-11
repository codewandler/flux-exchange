#![cfg(windows)]

mod supervisor {
    pub mod tests {
        use std::os::windows::io::AsRawHandle as _;

        use flux_exchange::windows_handle::validate_supervisor_handle;
        use windows_sys::Win32::Foundation::{
            CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
        };
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::System::Pipes::CreatePipe;

        fn pipe() -> (HANDLE, HANDLE) {
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };
            let mut read = std::ptr::null_mut();
            let mut write = std::ptr::null_mut();
            // SAFETY: output pointers and security attributes remain live for the call.
            assert_ne!(
                unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
                0
            );
            (read, write)
        }

        fn inheritable(handle: HANDLE, inherited: bool) {
            // SAFETY: only the inheritance bit of this owned fixture handle changes.
            assert_ne!(
                unsafe {
                    SetHandleInformation(
                        handle,
                        HANDLE_FLAG_INHERIT,
                        if inherited { HANDLE_FLAG_INHERIT } else { 0 },
                    )
                },
                0
            );
        }

        #[test]
        fn windows_validator_refuses_noninherited_nonpipe_and_each_wrong_direction() {
            let (read, write) = pipe();
            assert!(validate_supervisor_handle(write, false, "readiness").is_ok());
            inheritable(write, true);
            assert!(validate_supervisor_handle(read, true, "liveness").is_ok());

            inheritable(read, true);
            assert!(validate_supervisor_handle(read, false, "readiness")
                .expect_err("read end is not readiness")
                .contains("wrong direction"));
            inheritable(write, true);
            assert!(validate_supervisor_handle(write, true, "liveness")
                .expect_err("write end is not liveness")
                .contains("wrong direction"));

            inheritable(write, false);
            assert!(validate_supervisor_handle(write, false, "readiness")
                .expect_err("noninherited handle")
                .contains("not inherited"));

            let null = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("NUL")
                .expect("non-pipe Windows fixture");
            let null_handle = null.as_raw_handle().cast();
            inheritable(null_handle, true);
            assert!(validate_supervisor_handle(null_handle, false, "readiness")
                .expect_err("non-pipe handle")
                .contains("not a pipe"));

            // SAFETY: the two pipe handles are owned by this fixture and closed once.
            unsafe {
                CloseHandle(read);
                CloseHandle(write);
            }
        }
    }
}
