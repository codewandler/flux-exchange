//! Tenant-owned connector channels and their persistent store port.
//!
//! This module stores declarations only. It opens no socket and reads no credential: the composing
//! service binds a runner after catalogue, grant, connection and placement checks have passed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use connector_address::InstanceId;
use connector_catalog::{provider, ProviderKey};
use connector_pack::{ConfigStore, Configuration, Credentials, PreparedChannelPlan};
use connector_secrets::{CredentialScope, SecretStore};
use serde::ser::SerializeStruct as _;
use serde::{Deserialize, Serialize, Serializer};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRecord {
    id: ChannelId,
    tenant: Tenant,
    connector: String,
    connection: InstanceId,
    binding: String,
    events: BTreeSet<String>,
}

impl Serialize for ChannelRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut record = serializer.serialize_struct("ChannelRecord", 6)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("tenant", &self.tenant)?;
        record.serialize_field("connector", &self.connector)?;
        record.serialize_field("connection", self.connection.as_str())?;
        record.serialize_field("binding", &self.binding)?;
        record.serialize_field("events", &self.events)?;
        record.end()
    }
}

impl ChannelRecord {
    /// Construct a record after the caller has checked the binding and selected event subset
    /// against the catalogue.
    pub fn new(
        id: ChannelId,
        tenant: Tenant,
        connector: impl Into<String>,
        connection: InstanceId,
        binding: impl Into<String>,
        events: BTreeSet<String>,
    ) -> Result<Self, ChannelRefusal> {
        let connector = declared_name(connector.into())?;
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
    pub fn connection(&self) -> &InstanceId {
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

    /// Rebind to another host-resolved immutable connection while replacing the event subset.
    pub fn with_connection_and_events(
        mut self,
        connection: InstanceId,
        events: BTreeSet<String>,
    ) -> Result<Self, ChannelRefusal> {
        self = self.with_events(events)?;
        self.connection = connection;
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

/// The one zero-transport seam from a tenant-owned channel record to connector-pack's generated
/// handshake plan. The composing binary receives the plan only after its placement gate has passed;
/// it never receives either store or a credential value as an independently addressable object.
pub struct ConnectorChannelPlanner {
    credentials: Arc<dyn SecretStore>,
    settings: Arc<dyn ConfigStore>,
}

impl ConnectorChannelPlanner {
    /// Bind the same tenant-scoped stores used by ordinary connector invocation.
    pub fn new(credentials: Arc<dyn SecretStore>, settings: Arc<dyn ConfigStore>) -> Self {
        Self {
            credentials,
            settings,
        }
    }

    /// Resolve the record's generated connector binding into a guarded-runtime plan.
    ///
    /// The tenant comes only from the persistent record, whose tenant was derived from the
    /// authenticated operator. Connector-pack performs every configuration and credential lookup
    /// and returns its redacting wire-value wrappers; this host neither constructs nor logs them.
    pub async fn prepare(
        &self,
        record: &ChannelRecord,
    ) -> Result<PreparedChannelPlan, ChannelPlanRefusal> {
        let tenant = record.tenant().as_str();
        let provider = provider(ProviderKey::id(record.connector())).ok_or(ChannelPlanRefusal)?;
        let authority = provider.authority.ok_or(ChannelPlanRefusal)?;
        let scope = CredentialScope::new(tenant, authority).map_err(|_| ChannelPlanRefusal)?;
        let references = self
            .credentials
            .references(&scope)
            .await
            .map_err(|_| ChannelPlanRefusal)?;
        let mut legacy = false;
        let mut instances = BTreeSet::new();
        for reference in references {
            let declared = reference.is_default_service()
                && provider
                    .auth
                    .iter()
                    .any(|credential| credential.leaf == reference.credential());
            if !declared {
                continue;
            }
            match reference.instance() {
                Some(instance) => {
                    instances.insert(instance.clone());
                }
                None => legacy = true,
            }
        }
        if legacy && !instances.is_empty() {
            return Err(ChannelPlanRefusal);
        }
        let selected = if legacy {
            None
        } else if instances.contains(record.connection()) {
            Some(record.connection())
        } else {
            return Err(ChannelPlanRefusal);
        };
        let (credentials, settings) = match selected {
            Some(instance) => (
                Credentials::for_instance(Arc::clone(&self.credentials), tenant, instance.as_str())
                    .map_err(|_| ChannelPlanRefusal)?,
                Configuration::for_instance(Arc::clone(&self.settings), tenant, instance.as_str())
                    .map_err(|_| ChannelPlanRefusal)?,
            ),
            None => (
                Credentials::new(Arc::clone(&self.credentials), tenant)
                    .map_err(|_| ChannelPlanRefusal)?,
                Configuration::new(Arc::clone(&self.settings), tenant)
                    .map_err(|_| ChannelPlanRefusal)?,
            ),
        };

        connector_pack::channel_plan(record.connector(), record.binding(), credentials, settings)
            .await
            .map_err(|_| ChannelPlanRefusal)
    }
}

/// A generated channel plan could not be prepared. It deliberately carries no upstream message:
/// connector refusals name declarations, but this public boundary must never accidentally preserve
/// a credential-bearing URL or header added to a future diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("connector channel plan was refused")]
pub struct ChannelPlanRefusal;

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

pub use file::{ChannelStore, ChannelStoreError};

mod file {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::paths::{enclosing_working_tree, resolve};
    use crate::{private_fs, CHANNEL_STORE_SETTING};

    const MAX_STORE_BYTES: usize = 1024 * 1024;

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
                InstanceId::parse(&wire.connection).map_err(|_| ChannelStoreError::Invalid)?,
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
                connection: record.connection.to_string(),
                binding: record.binding.clone(),
                events: record.events.clone(),
            }
        }
    }

    /// File-backed persistent channel records, written atomically with native owner-only metadata.
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
            private_fs::ensure_directory(directory).map_err(|_| ChannelStoreError::Unavailable)?;
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
            private_fs::write_atomic(&self.path, &encoded).map_err(|_| ChannelRefusal::Unavailable)
        }
    }

    fn read(
        path: &Path,
    ) -> Result<BTreeMap<(String, ChannelId), ChannelRecord>, ChannelStoreError> {
        let Some(bytes) =
            private_fs::read(path, MAX_STORE_BYTES).map_err(|_| ChannelStoreError::Unavailable)?
        else {
            return Ok(BTreeMap::new());
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
    use std::sync::Mutex;

    use async_trait::async_trait;
    use connector_pack::Field;
    use connector_secrets::{CredentialRef, CredentialScope, InstanceId, Secret, StoreError};

    use super::*;

    const FIRST_INSTANCE: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND_INSTANCE: &str = "22222222-2222-4222-8222-222222222222";

    fn record(tenant: &str, id: &str) -> ChannelRecord {
        ChannelRecord::new(
            ChannelId::new(id).expect("id"),
            Tenant::new(tenant).expect("tenant"),
            "asterisk",
            InstanceId::parse(FIRST_INSTANCE).expect("instance"),
            "ari-events",
            ["channel-created".to_string()].into_iter().collect(),
        )
        .expect("record")
    }

    struct RecordingSecrets {
        references: Vec<CredentialRef>,
        reads: Mutex<Vec<CredentialRef>>,
    }

    #[async_trait]
    impl SecretStore for RecordingSecrets {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            self.reads.lock().expect("reads").push(reference.clone());
            Ok(Secret::new("not-a-real-password"))
        }

        async fn put(&self, _: &CredentialRef, _: &Secret) -> Result<(), StoreError> {
            unreachable!("planning writes no credential")
        }

        async fn delete(&self, _: &CredentialRef) -> Result<(), StoreError> {
            unreachable!("planning deletes no credential")
        }

        async fn references(&self, _: &CredentialScope) -> Result<Vec<CredentialRef>, StoreError> {
            Ok(self.references.clone())
        }
    }

    #[derive(Default)]
    struct RecordingConfig {
        reads: Mutex<Vec<Option<String>>>,
    }

    impl ConfigStore for RecordingConfig {
        fn get(&self, _: &str, _: &str, _: &str, _: Field<'_>) -> Option<String> {
            self.reads.lock().expect("reads").push(None);
            Some("legacy".to_owned())
        }

        fn get_for_instance(
            &self,
            _: &str,
            _: &str,
            instance: Option<&InstanceId>,
            _: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.reads
                .lock()
                .expect("reads")
                .push(instance.map(ToString::to_string));
            let prefix = instance?.as_str().split('-').next()?;
            match field {
                Field::Endpoint("host") => Some(format!("pbx-{prefix}.example.com")),
                Field::Username("asterisk.password") => Some(format!("user-{prefix}")),
                Field::ChannelQuery {
                    channel: "ari-events",
                    parameter: "app",
                } => Some(format!("app-{prefix}")),
                _ => None,
            }
        }
    }

    #[tokio::test]
    async fn two_channels_select_distinct_credential_and_configuration_instances() {
        let tenant = Tenant::new("alpha").expect("tenant");
        let reference = |instance: &str| {
            CredentialRef::for_instance(
                tenant.as_str(),
                "org.asterisk.ari",
                instance,
                "default",
                "password",
            )
            .expect("reference")
        };
        let secrets = Arc::new(RecordingSecrets {
            references: vec![reference(FIRST_INSTANCE), reference(SECOND_INSTANCE)],
            reads: Mutex::new(Vec::new()),
        });
        let configuration = Arc::new(RecordingConfig::default());
        let planner = ConnectorChannelPlanner::new(secrets.clone(), configuration.clone());
        let channel = |id: &str, instance: &str| {
            ChannelRecord::new(
                ChannelId::new(id).expect("id"),
                tenant.clone(),
                "asterisk",
                InstanceId::parse(instance).expect("instance"),
                "ari-events",
                ["channel-created".to_owned()].into_iter().collect(),
            )
            .expect("record")
        };

        planner
            .prepare(&channel("ch_first", FIRST_INSTANCE))
            .await
            .expect("first plan");
        planner
            .prepare(&channel("ch_second", SECOND_INSTANCE))
            .await
            .expect("second plan");

        let credential_reads = secrets.reads.lock().expect("reads");
        assert_eq!(
            *credential_reads,
            vec![reference(FIRST_INSTANCE), reference(SECOND_INSTANCE)]
        );
        let configuration_reads = configuration.reads.lock().expect("reads");
        assert!(configuration_reads.iter().all(Option::is_some));
        assert!(configuration_reads
            .iter()
            .any(|read| read.as_deref() == Some(FIRST_INSTANCE)));
        assert!(configuration_reads
            .iter()
            .any(|read| read.as_deref() == Some(SECOND_INSTANCE)));
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

    #[test]
    fn a_serialized_record_carries_the_uuid_as_an_address_component() {
        let value = serde_json::to_value(record("alpha", "ch_1")).expect("record JSON");
        assert_eq!(value["connection"], FIRST_INSTANCE);
        assert!(value.get("connection_label").is_none());
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
