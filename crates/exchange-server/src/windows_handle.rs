//! Closed validation for inherited Windows supervisor pipe capabilities.

/// Validate one inherited supervisor pipe HANDLE and clear its inheritance bit.
pub fn validate_supervisor_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    read: bool,
    name: &str,
) -> Result<(), String> {
    use windows_sys::Wdk::Foundation::{NtQueryObject, ObjectBasicInformation};
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, SetHandleInformation, HANDLE_FLAG_INHERIT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileType, FILE_READ_DATA, FILE_TYPE_PIPE, FILE_WRITE_DATA,
    };
    use windows_sys::Win32::System::WindowsProgramming::PUBLIC_OBJECT_BASIC_INFORMATION;

    let mut inherited = 0_u32;
    // SAFETY: output storage is valid and the numeric capability is only observed.
    if unsafe { GetHandleInformation(handle, &mut inherited) } == 0
        || inherited & HANDLE_FLAG_INHERIT == 0
    {
        return Err(format!(
            "supervisor {name} HANDLE is absent or not inherited"
        ));
    }
    // SAFETY: the call only identifies the validated inherited handle.
    if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
        return Err(format!("supervisor {name} HANDLE is not a pipe"));
    }
    let mut information = PUBLIC_OBJECT_BASIC_INFORMATION::default();
    let expected_size = std::mem::size_of::<PUBLIC_OBJECT_BASIC_INFORMATION>() as u32;
    let mut returned_size = 0_u32;
    // A zero-byte `ReadFile` on an empty synchronous pipe can block on Windows. Querying the
    // kernel-granted access mask proves direction without consuming or waiting on either stream.
    // SAFETY: the typed public output structure is live for the complete native query.
    let status = unsafe {
        NtQueryObject(
            handle,
            ObjectBasicInformation,
            (&mut information as *mut PUBLIC_OBJECT_BASIC_INFORMATION).cast(),
            expected_size,
            &mut returned_size,
        )
    };
    if status < 0 || returned_size != expected_size {
        return Err(format!(
            "cannot inspect supervisor {name} HANDLE access without reading it"
        ));
    }
    let (required, forbidden) = if read {
        (FILE_READ_DATA, FILE_WRITE_DATA)
    } else {
        (FILE_WRITE_DATA, FILE_READ_DATA)
    };
    if information.GrantedAccess & required == 0 || information.GrantedAccess & forbidden != 0 {
        return Err(format!("supervisor {name} HANDLE has the wrong direction"));
    }
    // SAFETY: clearing inheritance on the discovered capability cannot widen authority.
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(format!(
            "cannot protect supervisor {name} HANDLE from child processes"
        ));
    }
    Ok(())
}
