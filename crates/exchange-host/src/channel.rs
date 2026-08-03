//! Tenant-owned connector channels and their persistent store port.
//!
//! This module stores declarations only. It opens no socket and reads no credential: the composing
//! service binds a runner after catalogue, grant, connection and placement checks have passed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::Tenant;

const MAX_ID_BYTES: usize = 96;
const MAX_NAME_BYTES: usize = 128;
const MAX_EVENTS: usize = 512;

/// An opaque channel identifier minted by the host, never interpreted as a tenant or address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(String);

impl ChannelId {
    /// Validate an opaque id. IDs are safe path/log tokens but are not used as filesystem paths.
    pub fn new(value: impl Into<String>) -> Result<Self, ChannelRefusal> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ChannelRefusal::InvalidId);
        }
        Ok(Self(value))
    }

    /// The opaque wire spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Persistent operator-owned state for one vendor channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelRecord {
    id: ChannelId,
    tenant: Tenant,
    connector: String,
    connection: String,
    binding: String,
    events: BTreeSet<String>,
}

impl ChannelRecord {
    /// Construct a record after the caller has checked the binding and selected event subset
    /// against the catalogue.
    pub fn new(
        id: ChannelId,
        tenant: Tenant,
        connector: impl Into<String>,
        connection: impl Into<String>,
        binding: impl Into<String>,
        events: BTreeSet<String>,
    ) -> Result<Self, ChannelRefusal> {
        let connector = declared_name(connector.into())?;
        let connection = declared_name(connection.into())?;
        let binding = declared_name(binding.into())?;
        if events.is_empty() || events.len() > MAX_EVENTS {
            return Err(ChannelRefusal::InvalidEventSet);
        }
        if events
            .iter()
            .any(|event| declared_name(event.clone()).is_err())
        {
            return Err(ChannelRefusal::InvalidEventSet);
        }
        Ok(Self {
            id,
            tenant,
            connector,
            connection,
            binding,
            events,
        })
    }

    /// Opaque host-minted id.
    pub fn id(&self) -> &ChannelId {
        &self.id
    }

    /// Tenant derived from the authenticated operator that created the channel.
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    /// Connector catalogue id.
    pub fn connector(&self) -> &str {
        &self.connector
    }

    /// Existing connection reference within the record's tenant.
    pub fn connection(&self) -> &str {
        &self.connection
    }

    /// Declared connector binding.
    pub fn binding(&self) -> &str {
        &self.binding
    }

    /// Closed selected subset of the binding's declared events.
    pub fn events(&self) -> &BTreeSet<String> {
        &self.events
    }

    /// Replace only the selected event subset.
    pub fn with_events(mut self, events: BTreeSet<String>) -> Result<Self, ChannelRefusal> {
        if events.is_empty()
            || events.len() > MAX_EVENTS
            || events
                .iter()
                .any(|event| declared_name(event.clone()).is_err())
        {
            return Err(ChannelRefusal::InvalidEventSet);
        }
        self.events = events;
        Ok(self)
    }
}

fn declared_name(value: String) -> Result<String, ChannelRefusal> {
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(ChannelRefusal::InvalidDeclaration);
    }
    Ok(value)
}

/// Persistent channel storage. Every lookup is tenant-scoped; `all` exists only for startup
/// restoration by the host, never for a request route.
pub trait Channels: Send + Sync {
    /// This tenant's channels, ordered by id.
    fn held(&self, tenant: &Tenant) -> Vec<ChannelRecord>;
    /// One tenant-owned channel, or `None` for both unknown and cross-tenant ids.
    fn get(&self, tenant: &Tenant, id: &ChannelId) -> Option<ChannelRecord>;
    /// Create or replace a record whose embedded tenant is authoritative.
    fn set(&self, record: ChannelRecord) -> Result<(), ChannelRefusal>;
    /// Remove one tenant-owned channel.
    fn delete(&self, tenant: &Tenant, id: &ChannelId) -> Result<bool, ChannelRefusal>;
    /// Startup restoration view across tenants. This must never be exposed as an API response.
    fn all(&self) -> Vec<ChannelRecord>;
}

/// In-memory binding for tests and ephemeral single-process compositions.
#[derive(Default)]
pub struct MemoryChannels {
    held: RwLock<BTreeMap<(String, ChannelId), ChannelRecord>>,
}

impl Channels for MemoryChannels {
    fn held(&self, tenant: &Tenant) -> Vec<ChannelRecord> {
        self.held
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .filter(|((owner, _), _)| owner == tenant.as_str())
            .map(|(_, record)| record.clone())
            .collect()
    }

    fn get(&self, tenant: &Tenant, id: &ChannelId) -> Option<ChannelRecord> {
        self.held
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&(tenant.as_str().to_owned(), id.clone()))
            .cloned()
    }

    fn set(&self, record: ChannelRecord) -> Result<(), ChannelRefusal> {
        let key = (record.tenant.as_str().to_owned(), record.id.clone());
        self.held
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(key, record);
        Ok(())
    }

    fn delete(&self, tenant: &Tenant, id: &ChannelId) -> Result<bool, ChannelRefusal> {
        Ok(self
            .held
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&(tenant.as_str().to_owned(), id.clone()))
            .is_some())
    }

    fn all(&self) -> Vec<ChannelRecord> {
        self.held
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .values()
            .cloned()
            .collect()
    }
}

/// Why channel state was refused. No variant carries endpoint, credential or payload material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelRefusal {
    /// The opaque id is not a bounded token.
    #[error("channel id is invalid")]
    InvalidId,
    /// A connector, connection or binding is not a bounded declared-name token.
    #[error("channel declaration is invalid")]
    InvalidDeclaration,
    /// The selected event set is empty, oversized or contains an invalid declared name.
    #[error("channel event selection is invalid")]
    InvalidEventSet,
    /// Persistent state could not be read or written.
    #[error("channel state is unavailable")]
    Unavailable,
}

#[cfg(unix)]
pub use file::{ChannelStore, ChannelStoreError, CHANNEL_STORE_SETTING};

#[cfg(unix)]
mod file {
    use std::fs;
    use std::io::Write as _;
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::paths::{enclosing_working_tree, resolve};

    /// Configuration setting naming the persistent channel store.
    pub const CHANNEL_STORE_SETTING: &str = "FLUX_EXCHANGE_CHANNELS";

    #[derive(Debug, Serialize, Deserialize)]
    struct WireRecord {
        id: String,
        tenant: String,
        connector: String,
        connection: String,
        binding: String,
        events: BTreeSet<String>,
    }

    impl TryFrom<WireRecord> for ChannelRecord {
        type Error = ChannelStoreError;

        fn try_from(wire: WireRecord) -> Result<Self, Self::Error> {
            ChannelRecord::new(
                ChannelId::new(wire.id).map_err(|_| ChannelStoreError::Invalid)?,
                Tenant::new(wire.tenant).map_err(|_| ChannelStoreError::Invalid)?,
                wire.connector,
                wire.connection,
                wire.binding,
                wire.events,
            )
            .map_err(|_| ChannelStoreError::Invalid)
        }
    }

    impl From<&ChannelRecord> for WireRecord {
        fn from(record: &ChannelRecord) -> Self {
            Self {
                id: record.id.to_string(),
                tenant: record.tenant.to_string(),
                connector: record.connector.clone(),
                connection: record.connection.clone(),
                binding: record.binding.clone(),
                events: record.events.clone(),
            }
        }
    }

    /// File-backed persistent channel records, written atomically with mode `0600`.
    pub struct ChannelStore {
        path: PathBuf,
        held: RwLock<BTreeMap<(String, ChannelId), ChannelRecord>>,
    }

    impl ChannelStore {
        /// Bind the path explicitly chosen by the operator.
        pub fn bind_configured(configured: Option<&str>) -> Result<Self, ChannelStoreError> {
            let path = configured
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .ok_or(ChannelStoreError::Unconfigured)?;
            Self::bind(path)
        }

        /// Open or create a persistent channel store outside a working tree.
        pub fn bind(path: impl AsRef<Path>) -> Result<Self, ChannelStoreError> {
            let requested = path.as_ref();
            if requested.as_os_str().is_empty() {
                return Err(ChannelStoreError::Unconfigured);
            }
            let path = resolve(requested).map_err(|_| ChannelStoreError::Unavailable)?;
            if enclosing_working_tree(&path).is_some() {
                return Err(ChannelStoreError::InsideWorkingTree);
            }
            let directory = path.parent().ok_or(ChannelStoreError::Unavailable)?;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(|_| ChannelStoreError::Unavailable)?;
            let held = read(&path)?;
            Ok(Self {
                path,
                held: RwLock::new(held),
            })
        }

        /// Resolved store location.
        pub fn path(&self) -> &Path {
            &self.path
        }

        fn persist(
            &self,
            held: &BTreeMap<(String, ChannelId), ChannelRecord>,
        ) -> Result<(), ChannelRefusal> {
            let encoded =
                serde_json::to_vec_pretty(&held.values().map(WireRecord::from).collect::<Vec<_>>())
                    .map_err(|_| ChannelRefusal::Unavailable)?;
            let temporary = self.path.with_extension("tmp");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|_| ChannelRefusal::Unavailable)?;
            file.write_all(&encoded)
                .and_then(|()| file.sync_all())
                .map_err(|_| ChannelRefusal::Unavailable)?;
            drop(file);
            fs::rename(temporary, &self.path).map_err(|_| ChannelRefusal::Unavailable)
        }
    }

    fn read(
        path: &Path,
    ) -> Result<BTreeMap<(String, ChannelId), ChannelRecord>, ChannelStoreError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new())
            }
            Err(_) => return Err(ChannelStoreError::Unavailable),
        };
        if bytes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let wire: Vec<WireRecord> =
            serde_json::from_slice(&bytes).map_err(|_| ChannelStoreError::Invalid)?;
        let mut held = BTreeMap::new();
        for wire in wire {
            let record = ChannelRecord::try_from(wire)?;
            let key = (record.tenant.to_string(), record.id.clone());
            if held.insert(key, record).is_some() {
                return Err(ChannelStoreError::Invalid);
            }
        }
        Ok(held)
    }

    impl Channels for ChannelStore {
        fn held(&self, tenant: &Tenant) -> Vec<ChannelRecord> {
            self.held
                .read()
                .ok()
                .map(|held| {
                    held.iter()
                        .filter(|((owner, _), _)| owner == tenant.as_str())
                        .map(|(_, record)| record.clone())
                        .collect()
                })
                .unwrap_or_default()
        }

        fn get(&self, tenant: &Tenant, id: &ChannelId) -> Option<ChannelRecord> {
            self.held
                .read()
                .ok()
                .and_then(|held| held.get(&(tenant.as_str().to_owned(), id.clone())).cloned())
        }

        fn set(&self, record: ChannelRecord) -> Result<(), ChannelRefusal> {
            let mut held = self.held.write().map_err(|_| ChannelRefusal::Unavailable)?;
            let mut next = held.clone();
            next.insert((record.tenant.to_string(), record.id.clone()), record);
            self.persist(&next)?;
            *held = next;
            Ok(())
        }

        fn delete(&self, tenant: &Tenant, id: &ChannelId) -> Result<bool, ChannelRefusal> {
            let mut held = self.held.write().map_err(|_| ChannelRefusal::Unavailable)?;
            let mut next = held.clone();
            let removed = next
                .remove(&(tenant.as_str().to_owned(), id.clone()))
                .is_some();
            if removed {
                self.persist(&next)?;
                *held = next;
            }
            Ok(removed)
        }

        fn all(&self) -> Vec<ChannelRecord> {
            self.held
                .read()
                .map(|held| held.values().cloned().collect())
                .unwrap_or_default()
        }
    }

    /// Startup refusal for persistent channel state.
    #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
    pub enum ChannelStoreError {
        /// No path was selected.
        #[error("{CHANNEL_STORE_SETTING} is not configured")]
        Unconfigured,
        /// A checked-in channel file would turn deployment state into repository content.
        #[error("refusing a channel store inside a working tree")]
        InsideWorkingTree,
        /// Existing bytes are not valid bounded channel records.
        #[error("the channel store is invalid")]
        Invalid,
        /// Filesystem state could not be read or prepared.
        #[error("the channel store is unavailable")]
        Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tenant: &str, id: &str) -> ChannelRecord {
        ChannelRecord::new(
            ChannelId::new(id).expect("id"),
            Tenant::new(tenant).expect("tenant"),
            "asterisk",
            "asterisk",
            "ari-events",
            ["channel-created".to_string()].into_iter().collect(),
        )
        .expect("record")
    }

    #[test]
    fn channel_ids_are_tenant_scoped_and_cross_tenant_reads_are_indistinguishable() {
        let store = MemoryChannels::default();
        store.set(record("alpha", "ch_1")).expect("store");
        let alpha = Tenant::new("alpha").expect("tenant");
        let beta = Tenant::new("beta").expect("tenant");
        let id = ChannelId::new("ch_1").expect("id");
        assert!(store.get(&alpha, &id).is_some());
        assert!(store.get(&beta, &id).is_none());
        assert_eq!(store.held(&beta), Vec::new());
    }

    #[test]
    fn only_the_selected_events_can_change() {
        let changed = record("alpha", "ch_1")
            .with_events(["channel-destroyed".to_string()].into_iter().collect())
            .expect("event subset");
        assert_eq!(changed.connector(), "asterisk");
        assert_eq!(changed.binding(), "ari-events");
        assert_eq!(
            changed.events(),
            &["channel-destroyed".to_string()].into_iter().collect()
        );
    }

    #[cfg(unix)]
    #[test]
    fn persistent_channels_are_restored_after_rebinding() {
        use std::os::unix::fs::PermissionsExt as _;

        let root =
            std::env::temp_dir().join(format!("flux-exchange-channels-{}", std::process::id()));
        let path = root.join("state/channels.json");
        let _ = std::fs::remove_dir_all(&root);
        let store = ChannelStore::bind(&path).expect("bind store");
        store.set(record("alpha", "ch_1")).expect("persist");
        drop(store);

        let restored = ChannelStore::bind(&path).expect("rebind store");
        assert_eq!(restored.all(), vec![record("alpha", "ch_1")]);
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).expect("remove scratch");
    }
}
