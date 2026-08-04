//! Durable, value-free credential revision heads.
//!
//! The caller supplies an owner-authenticated private root. This module deliberately accepts no
//! credential store or presence callback: a held label has a head even when its credential
//! partition is empty or wholly absent.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const STORE_DIRECTORY: &str = "credential-heads-v1";
const IMAGE_FILE: &str = "image.json";
const MARKER_FILE: &str = "migration-complete";
const MARKER: &[u8] = b"exchange.credential-heads.v1\n";
const SCHEMA: &str = "exchange.credential-heads.v1";
const MAX_IMAGE_BYTES: u64 = 1_048_576;
const MAX_LABELS_PER_CONNECTOR: usize = 256;

/// One tenant-scoped labelled connection whose credential head is retained.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub(crate) struct CredentialHeadKey {
    tenant: String,
    connector: String,
    label: String,
}

impl CredentialHeadKey {
    pub(crate) fn new(
        tenant: impl Into<String>,
        connector: impl Into<String>,
        label: impl Into<String>,
    ) -> Result<Self, CredentialHeadError> {
        let key = Self {
            tenant: tenant.into(),
            connector: connector.into(),
            label: label.into(),
        };
        key.validate()?;
        Ok(key)
    }

    fn validate(&self) -> Result<(), CredentialHeadError> {
        if self.tenant.is_empty() || self.connector.is_empty() || self.label.is_empty() {
            return Err(CredentialHeadError::InvalidKeySet);
        }
        Ok(())
    }
}

/// An opaque nonzero 256-bit per-label credential revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CredentialHead(String);

impl CredentialHead {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(value: String) -> Result<Self, CredentialHeadError> {
        if value.len() != 64
            || value
                .bytes()
                .any(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            || value.bytes().all(|byte| byte == b'0')
        {
            return Err(CredentialHeadError::Corrupt);
        }
        Ok(Self(value))
    }
}

/// A value-free refusal from the credential-head store.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CredentialHeadError {
    #[error("credential-head migration has not completed")]
    Unmigrated,
    #[error("credential-head state is corrupt")]
    Corrupt,
    #[error("credential-head key set is invalid")]
    InvalidKeySet,
    #[error("credential-head label is unknown")]
    UnknownKey,
    #[error("credential-head label already exists")]
    AlreadyExists,
    #[error("credential-head compare failed")]
    CompareFailed,
    #[error("credential-head candidate is invalid")]
    InvalidCandidate,
    #[error("credential-head random source is unavailable")]
    RandomUnavailable,
    #[error("credential-head storage is unavailable")]
    Io(#[source] io::Error),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredImage {
    schema: String,
    migration_complete: bool,
    heads: Vec<StoredHead>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredHead {
    key: CredentialHeadKey,
    current: String,
    retired: Vec<String>,
}

/// The process-local serialization point for one durable credential-head image.
pub(crate) struct CredentialHeadStore {
    directory: PathBuf,
    mutation: Mutex<()>,
}

impl CredentialHeadStore {
    /// Atomically initializes every held legacy label without consulting credential presence.
    ///
    /// Retrying after a crash before the directory rename sees no published store and creates a
    /// new complete image. After the rename, both the marker and image are present together.
    pub(crate) fn migrate_legacy(
        private_root: &Path,
        keys: &[CredentialHeadKey],
    ) -> Result<Self, CredentialHeadError> {
        validate_key_set(keys)?;
        let directory = private_root.join(STORE_DIRECTORY);
        if directory.exists() {
            let store = Self::open(private_root)?;
            store.require_exact_keys(keys)?;
            return Ok(store);
        }

        let temporary = private_root.join(format!(
            ".{STORE_DIRECTORY}.migrate-{}",
            random_lowerhex(8)?
        ));
        create_private_directory(&temporary)?;

        let mut image = StoredImage {
            schema: SCHEMA.to_owned(),
            migration_complete: true,
            heads: Vec::with_capacity(keys.len()),
        };
        for key in keys {
            let current = fresh_head(&image)?;
            image.heads.push(StoredHead {
                key: key.clone(),
                current: current.0,
                retired: Vec::new(),
            });
        }
        image.heads.sort_by(|left, right| left.key.cmp(&right.key));

        write_new_file(&temporary.join(MARKER_FILE), MARKER)?;
        write_new_file(&temporary.join(IMAGE_FILE), &serialize_image(&image)?)?;
        sync_directory(&temporary)?;
        match fs::rename(&temporary, &directory) {
            Ok(()) => sync_directory(private_root)?,
            Err(_error) if directory.exists() => {
                let store = Self::open(private_root)?;
                store.require_exact_keys(keys)?;
                return Ok(store);
            }
            Err(error) => return Err(CredentialHeadError::Io(error)),
        }

        let store = Self::open(private_root)?;
        store.require_exact_keys(keys)?;
        Ok(store)
    }

    /// Opens an already marked store. Absence never triggers migration or regeneration.
    pub(crate) fn open(private_root: &Path) -> Result<Self, CredentialHeadError> {
        let directory = private_root.join(STORE_DIRECTORY);
        if !directory.exists() {
            return Err(CredentialHeadError::Unmigrated);
        }
        require_plain_directory(&directory)?;
        require_marker(&directory.join(MARKER_FILE))?;
        let image = read_image(&directory.join(IMAGE_FILE))?;
        validate_image(&image)?;
        Ok(Self {
            directory,
            mutation: Mutex::new(()),
        })
    }

    pub(crate) fn current(
        &self,
        key: &CredentialHeadKey,
    ) -> Result<CredentialHead, CredentialHeadError> {
        let _guard = self.lock()?;
        let image = self.read()?;
        let entry = image
            .heads
            .iter()
            .find(|entry| &entry.key == key)
            .ok_or(CredentialHeadError::UnknownKey)?;
        CredentialHead::parse(entry.current.clone())
    }

    /// Allocates a value-free next head for a coordinator journal without publishing it.
    pub(crate) fn allocate_next(
        &self,
        key: &CredentialHeadKey,
    ) -> Result<CredentialHead, CredentialHeadError> {
        let _guard = self.lock()?;
        let image = self.read()?;
        if !image.heads.iter().any(|entry| &entry.key == key) {
            return Err(CredentialHeadError::UnknownKey);
        }
        fresh_head(&image)
    }

    /// Allocates the initial head for a new label without consulting secret needs or presence.
    pub(crate) fn allocate_new(
        &self,
        key: &CredentialHeadKey,
    ) -> Result<CredentialHead, CredentialHeadError> {
        key.validate()?;
        let _guard = self.lock()?;
        let image = self.read()?;
        if image.heads.iter().any(|entry| &entry.key == key) {
            return Err(CredentialHeadError::AlreadyExists);
        }
        fresh_head(&image)
    }

    /// Publishes the initial head for a newly committed label, including a zero-secret label.
    pub(crate) fn insert_new(
        &self,
        key: CredentialHeadKey,
        head: CredentialHead,
    ) -> Result<(), CredentialHeadError> {
        key.validate()?;
        let _guard = self.lock()?;
        let mut image = self.read()?;
        if image.heads.iter().any(|entry| entry.key == key) {
            return Err(CredentialHeadError::AlreadyExists);
        }
        if image
            .heads
            .iter()
            .filter(|entry| entry.key.tenant == key.tenant && entry.key.connector == key.connector)
            .count()
            == MAX_LABELS_PER_CONNECTOR
        {
            return Err(CredentialHeadError::InvalidKeySet);
        }
        require_unused(&image, &head)?;
        image.heads.push(StoredHead {
            key,
            current: head.0,
            retired: Vec::new(),
        });
        image.heads.sort_by(|left, right| left.key.cmp(&right.key));
        self.write(&image)
    }

    /// Atomically advances exactly the expected current head.
    pub(crate) fn compare_and_advance(
        &self,
        key: &CredentialHeadKey,
        expected: &CredentialHead,
        next: CredentialHead,
    ) -> Result<(), CredentialHeadError> {
        let _guard = self.lock()?;
        let mut image = self.read()?;
        require_unused(&image, &next)?;
        let entry = image
            .heads
            .iter_mut()
            .find(|entry| &entry.key == key)
            .ok_or(CredentialHeadError::UnknownKey)?;
        if entry.current != expected.0 {
            return Err(CredentialHeadError::CompareFailed);
        }
        entry
            .retired
            .push(std::mem::replace(&mut entry.current, next.0));
        self.write(&image)
    }

    fn require_exact_keys(&self, keys: &[CredentialHeadKey]) -> Result<(), CredentialHeadError> {
        let _guard = self.lock()?;
        let image = self.read()?;
        let actual = image
            .heads
            .iter()
            .map(|entry| &entry.key)
            .collect::<HashSet<_>>();
        let expected = keys.iter().collect::<HashSet<_>>();
        if actual != expected {
            return Err(CredentialHeadError::Corrupt);
        }
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>, CredentialHeadError> {
        self.mutation
            .lock()
            .map_err(|_| CredentialHeadError::Corrupt)
    }

    fn read(&self) -> Result<StoredImage, CredentialHeadError> {
        require_marker(&self.directory.join(MARKER_FILE))?;
        let image = read_image(&self.directory.join(IMAGE_FILE))?;
        validate_image(&image)?;
        Ok(image)
    }

    fn write(&self, image: &StoredImage) -> Result<(), CredentialHeadError> {
        validate_image(image)?;
        let temporary = self
            .directory
            .join(format!(".{IMAGE_FILE}.{}", random_lowerhex(8)?));
        write_new_file(&temporary, &serialize_image(image)?)?;
        replace_file(&temporary, &self.directory.join(IMAGE_FILE))?;
        sync_directory(&self.directory)
    }
}

fn validate_key_set(keys: &[CredentialHeadKey]) -> Result<(), CredentialHeadError> {
    let mut unique = HashSet::with_capacity(keys.len());
    let mut connector_counts = HashMap::new();
    for key in keys {
        key.validate()?;
        if !unique.insert(key) {
            return Err(CredentialHeadError::InvalidKeySet);
        }
        let count = connector_counts
            .entry((&key.tenant, &key.connector))
            .or_insert(0_usize);
        *count += 1;
        if *count > MAX_LABELS_PER_CONNECTOR {
            return Err(CredentialHeadError::InvalidKeySet);
        }
    }
    Ok(())
}

fn validate_image(image: &StoredImage) -> Result<(), CredentialHeadError> {
    if image.schema != SCHEMA || !image.migration_complete {
        return Err(CredentialHeadError::Corrupt);
    }
    let mut keys = HashSet::with_capacity(image.heads.len());
    let mut values = HashSet::new();
    let mut connector_counts = HashMap::new();
    let mut previous = None;
    for entry in &image.heads {
        entry
            .key
            .validate()
            .map_err(|_| CredentialHeadError::Corrupt)?;
        if !keys.insert(&entry.key)
            || previous.is_some_and(|key| key >= &entry.key)
            || !values.insert(CredentialHead::parse(entry.current.clone())?.0)
        {
            return Err(CredentialHeadError::Corrupt);
        }
        let count = connector_counts
            .entry((&entry.key.tenant, &entry.key.connector))
            .or_insert(0_usize);
        *count += 1;
        if *count > MAX_LABELS_PER_CONNECTOR {
            return Err(CredentialHeadError::Corrupt);
        }
        previous = Some(&entry.key);
        let mut local = HashSet::new();
        for retired in &entry.retired {
            let retired = CredentialHead::parse(retired.clone())?.0;
            if !local.insert(retired.clone()) || !values.insert(retired) {
                return Err(CredentialHeadError::Corrupt);
            }
        }
    }
    Ok(())
}

fn fresh_head(image: &StoredImage) -> Result<CredentialHead, CredentialHeadError> {
    for _ in 0..128 {
        let candidate = CredentialHead(random_lowerhex(32)?);
        if require_unused(image, &candidate).is_ok() {
            return Ok(candidate);
        }
    }
    Err(CredentialHeadError::RandomUnavailable)
}

fn require_unused(
    image: &StoredImage,
    candidate: &CredentialHead,
) -> Result<(), CredentialHeadError> {
    CredentialHead::parse(candidate.0.clone())
        .map_err(|_| CredentialHeadError::InvalidCandidate)?;
    if image.heads.iter().any(|entry| {
        entry.current == candidate.0 || entry.retired.iter().any(|retired| retired == &candidate.0)
    }) {
        return Err(CredentialHeadError::InvalidCandidate);
    }
    Ok(())
}

fn serialize_image(image: &StoredImage) -> Result<Vec<u8>, CredentialHeadError> {
    let bytes = serde_json::to_vec(image).map_err(|_| CredentialHeadError::Corrupt)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(CredentialHeadError::Corrupt);
    }
    Ok(bytes)
}

fn read_image(path: &Path) -> Result<StoredImage, CredentialHeadError> {
    require_plain_file(path)?;
    let metadata = fs::metadata(path).map_err(|_| CredentialHeadError::Corrupt)?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(CredentialHeadError::Corrupt);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|_| CredentialHeadError::Corrupt)?;
    serde_json::from_slice(&bytes).map_err(|_| CredentialHeadError::Corrupt)
}

fn require_marker(path: &Path) -> Result<(), CredentialHeadError> {
    require_plain_file(path)?;
    if fs::metadata(path)
        .map_err(|_| CredentialHeadError::Corrupt)?
        .len()
        != MARKER.len() as u64
    {
        return Err(CredentialHeadError::Corrupt);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|_| CredentialHeadError::Corrupt)?;
    if bytes != MARKER {
        return Err(CredentialHeadError::Corrupt);
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), CredentialHeadError> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(CredentialHeadError::Io)?;
    file.write_all(bytes).map_err(CredentialHeadError::Io)?;
    file.sync_all().map_err(CredentialHeadError::Io)
}

fn create_private_directory(path: &Path) -> Result<(), CredentialHeadError> {
    #[cfg(unix)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(CredentialHeadError::Io)
}

fn require_plain_directory(path: &Path) -> Result<(), CredentialHeadError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialHeadError::Corrupt)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(CredentialHeadError::Corrupt);
    }
    #[cfg(unix)]
    require_private_mode(&metadata, 0o700)?;
    Ok(())
}

fn require_plain_file(path: &Path) -> Result<(), CredentialHeadError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialHeadError::Corrupt)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(CredentialHeadError::Corrupt);
    }
    #[cfg(unix)]
    require_private_mode(&metadata, 0o600)?;
    Ok(())
}

#[cfg(unix)]
fn require_private_mode(metadata: &fs::Metadata, expected: u32) -> Result<(), CredentialHeadError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.mode() & 0o777 != expected || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(CredentialHeadError::Corrupt);
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), CredentialHeadError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(CredentialHeadError::Io)
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> Result<(), CredentialHeadError> {
    Ok(())
}

#[cfg(unix)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), CredentialHeadError> {
    fs::rename(source, destination).map_err(CredentialHeadError::Io)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), CredentialHeadError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(CredentialHeadError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

fn random_lowerhex(bytes: usize) -> Result<String, CredentialHeadError> {
    let mut random = vec![0_u8; bytes];
    fill_random(&mut random)?;
    let mut encoded = String::with_capacity(bytes * 2);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").map_err(|_| CredentialHeadError::RandomUnavailable)?;
    }
    if encoded.bytes().all(|byte| byte == b'0') {
        return Err(CredentialHeadError::RandomUnavailable);
    }
    Ok(encoded)
}

#[cfg(unix)]
fn fill_random(bytes: &mut [u8]) -> Result<(), CredentialHeadError> {
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(bytes))
        .map_err(|_| CredentialHeadError::RandomUnavailable)
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> Result<(), CredentialHeadError> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let length = u32::try_from(bytes.len()).map_err(|_| CredentialHeadError::RandomUnavailable)?;
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(CredentialHeadError::RandomUnavailable);
    }
    Ok(())
}
