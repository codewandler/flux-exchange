//! Owner-only Unix filesystem primitives for the portable logical store.

use std::fs::{DirBuilder, File};
use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _, MetadataExt as _};
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags};

use super::{unreachable, StoreError, DIR_MODE, FILE_MODE};

#[derive(Clone, Copy)]
enum Expected {
    Directory,
    File,
}

pub(super) fn ensure_directory(directory: &Path) -> Result<(), StoreError> {
    match std::fs::symlink_metadata(directory) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            DirBuilder::new()
                .recursive(true)
                .mode(DIR_MODE)
                .create(directory)
                .map_err(|error| unreachable(directory, &error))?;
        }
        Err(error) => return Err(unreachable(directory, &error)),
    }

    inspect_path(directory, Expected::Directory, DIR_MODE)?;
    let descriptor = rustix::fs::open(
        directory,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| unreachable(directory, &std::io::Error::from(error)))?;
    inspect_handle(&descriptor, directory, Expected::Directory, DIR_MODE)
}

pub(super) fn open_existing(path: &Path) -> Result<Option<File>, StoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => inspect_path(path, Expected::File, FILE_MODE)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreachable(path, &error)),
    }

    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| unreachable(path, &std::io::Error::from(error)))?;
    inspect_handle(&descriptor, path, Expected::File, FILE_MODE)?;
    Ok(Some(File::from(descriptor)))
}

pub(super) fn validate_destination(path: &Path) -> Result<(), StoreError> {
    let existing = open_existing(path)?;
    drop(existing);
    Ok(())
}

pub(super) fn create_new(temporary: &Path, store: &Path) -> Result<File, StoreError> {
    let descriptor = rustix::fs::open(
        temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| unreachable(store, &std::io::Error::from(error)))?;
    inspect_handle(&descriptor, store, Expected::File, FILE_MODE)?;
    Ok(File::from(descriptor))
}

pub(super) fn flush(file: &File, store: &Path) -> Result<(), StoreError> {
    file.sync_all().map_err(|error| unreachable(store, &error))
}

pub(super) fn replace(temporary: &Path, store: &Path) -> Result<(), StoreError> {
    // Revalidate the destination immediately before replacement. A path widened, chowned or
    // replaced after `FileStore::open` is evidence to preserve, not state to repair with a rename.
    if open_existing(store)?.is_some() {
        // The validated handle drops before rename; the one-process contract excludes a second
        // cooperative writer, while O_NOFOLLOW/path+handle checks close the accidental case.
    }
    std::fs::rename(temporary, store).map_err(|error| unreachable(store, &error))?;
    let Some(file) = open_existing(store)? else {
        return Err(StoreError::Unreachable {
            path: store.display().to_string(),
            reason: "the atomically installed store disappeared before it could be revalidated"
                .to_owned(),
        });
    };
    drop(file);
    Ok(())
}

pub(super) fn sync_directory(directory: &Path) {
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
}

fn inspect_path(path: &Path, expected: Expected, widest: u32) -> Result<(), StoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| unreachable(path, &error))?;
    if metadata.file_type().is_symlink() {
        return denied(
            path,
            "it is a symbolic link, which this store never follows",
        );
    }
    let right_kind = match expected {
        Expected::Directory => metadata.file_type().is_dir(),
        Expected::File => metadata.file_type().is_file() && !metadata.file_type().is_socket(),
    };
    if !right_kind {
        return denied(path, expected.kind_reason());
    }
    inspect_owner_mode(path, metadata.uid(), metadata.mode(), widest)
}

// Darwin's `st_mode` is `u16` while Linux's is `u32`; this conversion is deliberately portable.
#[allow(clippy::useless_conversion)]
fn inspect_handle(
    descriptor: &impl std::os::fd::AsFd,
    path: &Path,
    expected: Expected,
    widest: u32,
) -> Result<(), StoreError> {
    let metadata = rustix::fs::fstat(descriptor)
        .map_err(|error| unreachable(path, &std::io::Error::from(error)))?;
    let right_kind = match expected {
        Expected::Directory => FileType::from_raw_mode(metadata.st_mode) == FileType::Directory,
        Expected::File => FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile,
    };
    if !right_kind {
        return denied(path, expected.kind_reason());
    }
    inspect_owner_mode(path, metadata.st_uid, u32::from(metadata.st_mode), widest)
}

fn inspect_owner_mode(
    path: &Path,
    owner: u32,
    raw_mode: u32,
    widest: u32,
) -> Result<(), StoreError> {
    let mode = raw_mode & 0o777;
    if mode & !widest != 0 {
        let reason = if widest == DIR_MODE {
            format!(
                "its mode is {mode:04o}, wider than {widest:04o}; create an owner-only child \
                 directory or use a conventional per-user state root. Never narrow a shared \
                 ancestor for this store"
            )
        } else {
            format!(
                "its mode is {mode:04o}, wider than {widest:04o}; inspect the exposure before \
                 deliberately relocating it or changing its permissions"
            )
        };
        return denied(path, reason);
    }
    let expected_owner = rustix::process::geteuid().as_raw();
    if owner != expected_owner {
        return denied(
            path,
            format!(
                "it is owned by uid {owner}, not the current process uid {expected_owner}; ownership \
                 is never repaired automatically"
            ),
        );
    }
    Ok(())
}

fn denied(path: &Path, reason: impl Into<String>) -> Result<(), StoreError> {
    Err(StoreError::Denied {
        path: path.display().to_string(),
        reason: reason.into(),
    })
}

impl Expected {
    fn kind_reason(self) -> &'static str {
        match self {
            Self::Directory => "it is not a directory",
            Self::File => "it is not a regular file",
        }
    }
}
