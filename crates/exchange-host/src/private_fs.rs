//! Platform-native owner-only filesystem operations for Exchange's non-credential state.
//!
//! Credential persistence remains entirely owned by `connector-secrets::FileStore`. Exchange owns
//! the logical formats of its other durable ports, so they share this small creation/inspection
//! boundary instead of each approximating Unix modes or Windows ACLs independently.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use connector_secrets::StoreError;

#[cfg(unix)]
#[path = "private_fs/unix.rs"]
mod platform;
#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "private_fs/windows.rs"]
mod platform;

#[cfg(not(any(unix, windows)))]
compile_error!("local state needs a platform-native owner-only filesystem implementation");

#[cfg(unix)]
const FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

pub(crate) fn ensure_directory(path: &Path) -> Result<(), StoreError> {
    platform::ensure_directory(path)
}

pub(crate) fn read(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, StoreError> {
    let Some(mut file) = platform::open_existing(path)? else {
        return Ok(None);
    };
    let take = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(take)
        .read_to_end(&mut bytes)
        .map_err(|error| unreachable(path, &error))?;
    if bytes.len() > limit {
        return Err(StoreError::Backend {
            path: path.display().to_string(),
            reason: format!("{} exceeds the {limit}-byte store limit", path.display()),
        });
    }
    Ok(Some(bytes))
}

pub(crate) fn ensure_file(path: &Path) -> Result<(), StoreError> {
    if platform::open_existing(path)?.is_none() {
        write_atomic(path, &[])?;
    }
    Ok(())
}

pub(crate) fn verify_file(path: &Path) -> Result<(), StoreError> {
    match platform::open_existing(path)? {
        Some(file) => {
            drop(file);
            Ok(())
        }
        None => Err(StoreError::Unreachable {
            path: path.display().to_string(),
            reason: "the owner-only state file does not exist".to_owned(),
        }),
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    platform::ensure_directory(directory)?;
    platform::validate_destination(path)?;

    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = platform::create_new(&temporary, path)?;
        file.write_all(bytes)
            .map_err(|error| unreachable(path, &error))?;
        platform::flush(&file, path)?;
        drop(file);
        platform::replace(&temporary, path)?;
        platform::sync_directory(directory);
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), sequence))
}

fn unreachable(path: &Path, error: &std::io::Error) -> StoreError {
    StoreError::Unreachable {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}
