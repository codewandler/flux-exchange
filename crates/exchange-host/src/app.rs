//! Immutable App Packages, atomic tenant installations and opaque runtime authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use flux_lang::program::Module;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
    ConnectionLabel, Idempotency, InstanceId, OperationFacts, Principal, PrincipalKind, Risk,
    Selector, Tenant,
};

/// Setting naming the owner-only installed-App directory.
pub const APP_STORE_SETTING: &str = "FLUX_EXCHANGE_APPS";
const MAX_PACKAGE_SOURCE_BYTES: usize = 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Publisher evidence attached to one immutable package revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageProvenance {
    /// Stable curated publisher name.
    pub publisher: String,
    /// Source repository recorded by the curated index.
    pub repository: String,
    /// Immutable source revision.
    pub revision: String,
}

/// One required tenant Connection slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRequirement {
    /// Package-local binding name.
    pub name: String,
    /// Connector the selected Connection must install.
    pub connector: String,
}

/// One reviewed operation layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessLayer {
    /// Package-local layer name.
    pub name: String,
    /// Connector whose metadata is considered.
    pub connector: String,
    /// Metadata selector resolved at installation.
    pub selector: Selector,
    /// Exact operations required. Empty freezes every selector match.
    pub required_operations: Vec<String>,
    /// Whether an install may omit this layer.
    pub required: bool,
}

/// One package datasource slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasourceRequirement {
    /// Package-local binding name.
    pub name: String,
    /// Required datasource kind.
    pub kind: String,
    /// Whether an install may omit this slot.
    pub required: bool,
}

/// A trigger's declared target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum TriggerTarget {
    /// A declared Journey.
    Journey(String),
    /// A declared Agent hosted by Exchange.
    ManagedAgent(String),
}

impl TriggerTarget {
    fn name(&self) -> &str {
        match self {
            Self::Journey(name) | Self::ManagedAgent(name) => name,
        }
    }
}

/// A package-declared Event Type binding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageTrigger {
    /// Event label admitted by the installed App.
    pub event_type: String,
    /// Declared Program target.
    pub target: TriggerTarget,
}

/// Tenant-independent requirements carried by a package.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRequirements {
    /// Required Connection slots.
    pub connections: Vec<ConnectionRequirement>,
    /// Required and optional operation layers.
    pub access_layers: Vec<AccessLayer>,
    /// Required and optional datasource slots.
    pub datasources: Vec<DatasourceRequirement>,
    /// Whether a Model Profile must be selected.
    pub model_profile_required: bool,
    /// Declared trigger bindings.
    pub triggers: Vec<PackageTrigger>,
}

/// An immutable package revision from the curated registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppPackage {
    /// Registry package id.
    pub id: String,
    /// Immutable semantic revision.
    pub version: String,
    /// Exact Flux Program source.
    pub program: String,
    /// SHA-256 over the canonical package document.
    pub integrity: String,
    /// Publisher provenance.
    pub provenance: PackageProvenance,
    /// Tenant-independent requirements.
    pub requirements: PackageRequirements,
}

#[derive(Serialize)]
struct IntegrityDocument<'a> {
    id: &'a str,
    version: &'a str,
    program: &'a str,
    provenance: &'a PackageProvenance,
    requirements: &'a PackageRequirements,
}

impl AppPackage {
    /// Construct and validate a curated immutable package revision.
    pub fn signed(
        id: impl Into<String>,
        version: impl Into<String>,
        program: impl Into<String>,
        provenance: PackageProvenance,
        requirements: PackageRequirements,
    ) -> Result<Self, AppRefusal> {
        let mut package = Self {
            id: id.into(),
            version: version.into(),
            program: program.into(),
            integrity: String::new(),
            provenance,
            requirements,
        };
        package.validate_document()?;
        package.refresh_integrity();
        Ok(package)
    }

    /// Recompute integrity after constructing a curated revision.
    pub fn refresh_integrity(&mut self) {
        self.integrity = self.expected_integrity();
    }

    fn expected_integrity(&self) -> String {
        digest(&IntegrityDocument {
            id: &self.id,
            version: &self.version,
            program: &self.program,
            provenance: &self.provenance,
            requirements: &self.requirements,
        })
    }

    fn verify(&self) -> Result<(), AppRefusal> {
        self.validate_document()?;
        if self.integrity != self.expected_integrity() {
            return Err(AppRefusal::PackageIntegrity {
                package: self.id.clone(),
                version: self.version.clone(),
            });
        }
        Ok(())
    }

    fn validate_document(&self) -> Result<(), AppRefusal> {
        admit_id("App Package", &self.id)?;
        admit_id("App Package version", &self.version)?;
        if self.program.len() > MAX_PACKAGE_SOURCE_BYTES {
            return Err(AppRefusal::InvalidPackage(format!(
                "Flux Program is {} bytes; limit is {MAX_PACKAGE_SOURCE_BYTES}",
                self.program.len()
            )));
        }
        let program = parse_program(&self.program)?;
        program
            .validate_trigger_targets()
            .map_err(AppRefusal::InvalidPackage)?;
        for trigger in &self.requirements.triggers {
            let binding_exists = program.triggers.iter().any(|candidate| {
                candidate.on == trigger.event_type
                    && match &trigger.target {
                        TriggerTarget::Journey(name) => {
                            candidate.agent.is_none() && candidate.run == *name
                        }
                        TriggerTarget::ManagedAgent(name) => {
                            candidate.agent.as_deref() == Some(name.as_str())
                        }
                    }
            });
            if !binding_exists {
                return Err(AppRefusal::InvalidPackage(format!(
                    "Event Type `{}` is not bound to declared target `{}`",
                    trigger.event_type,
                    trigger.target.name(),
                )));
            }
        }
        Ok(())
    }
}

fn parse_program(source: &str) -> Result<flux_lang::program::Program, AppRefusal> {
    match Module::parse_str(source)
        .map_err(|error| AppRefusal::InvalidPackage(error.to_string()))?
    {
        Module::Program(program) => Ok(program),
        Module::Flow(_) => Err(AppRefusal::InvalidPackage(
            "an App Package must carry a Program, not a bare flow".into(),
        )),
    }
}

/// Immutable package revisions admitted by a trusted curated index.
#[derive(Debug, Clone)]
pub struct PackageRegistry {
    packages: BTreeMap<(String, String), AppPackage>,
}

impl PackageRegistry {
    /// Verify and index package revisions, refusing a conflicting occupied key.
    pub fn new(packages: impl IntoIterator<Item = AppPackage>) -> Result<Self, AppRefusal> {
        let mut indexed = BTreeMap::new();
        for package in packages {
            package.verify()?;
            let key = (package.id.clone(), package.version.clone());
            if let Some(existing) = indexed.insert(key, package.clone()) {
                if existing != package {
                    return Err(AppRefusal::ConflictingPackage {
                        package: package.id,
                        version: package.version,
                    });
                }
            }
        }
        Ok(Self { packages: indexed })
    }

    /// The built-in key-free Slack-bot-style template.
    pub fn curated() -> Self {
        let source = r#"agent assistant
  model "installed"
  tools ["slack.chat.post.message"]
  description "A tenant-installed Slack assistant."

trigger chat
  on "chat"
  agent assistant

trigger slack
  on "slack"
  agent assistant
"#;
        let package = AppPackage::signed(
            "exchange-apps/slack-bot",
            "1.0.0",
            source,
            PackageProvenance {
                publisher: "codewandler".into(),
                repository: "https://github.com/codewandler/exchange-apps".into(),
                revision: "built-in-v1".into(),
            },
            PackageRequirements {
                connections: vec![ConnectionRequirement {
                    name: "slack".into(),
                    connector: "slack".into(),
                }],
                access_layers: vec![AccessLayer {
                    name: "reply".into(),
                    connector: "slack".into(),
                    selector: Selector::at_most(Risk::High),
                    required_operations: vec!["slack-chat-post-message".into()],
                    required: true,
                }],
                datasources: Vec::new(),
                model_profile_required: true,
                triggers: vec![
                    PackageTrigger {
                        event_type: "chat".into(),
                        target: TriggerTarget::ManagedAgent("assistant".into()),
                    },
                    PackageTrigger {
                        event_type: "slack".into(),
                        target: TriggerTarget::ManagedAgent("assistant".into()),
                    },
                ],
            },
        )
        .expect("the built-in package is compile-time valid");
        Self::new([package]).expect("the built-in package has valid integrity")
    }

    /// List every immutable package revision.
    pub fn list(&self) -> Vec<AppPackage> {
        self.packages.values().cloned().collect()
    }

    /// Read one exact package revision.
    pub fn get(&self, id: &str, version: &str) -> Option<&AppPackage> {
        self.packages.get(&(id.to_owned(), version.to_owned()))
    }
}

/// One tenant-owned model selection without provider credential material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    /// Tenant-local profile id.
    pub id: String,
    /// Provider binding name.
    pub provider: String,
    /// Provider model id.
    pub model: String,
    /// Monotonic resource revision.
    pub revision: u64,
    /// Key-free response for the demo provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_reply: Option<String>,
}

impl ModelProfile {
    /// Build the key-free deterministic checkout profile.
    pub fn static_reply(id: impl Into<String>, reply: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider: "static".into(),
            model: "static".into(),
            revision: 1,
            static_reply: Some(reply.into()),
        }
    }
}

/// One tenant-owned datasource binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Datasource {
    /// Tenant-local id.
    pub id: String,
    /// Published backend kind.
    pub kind: String,
    /// Monotonic resource revision.
    pub revision: u64,
}

impl Datasource {
    /// Construct revision one.
    pub fn new(id: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            revision: 1,
        }
    }
}

/// One Connection already proven to belong to the resolved tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableConnection {
    /// Connector id.
    pub connector: String,
    /// Operator label.
    pub label: ConnectionLabel,
    /// Host-minted immutable instance id.
    pub instance: InstanceId,
}

impl AvailableConnection {
    /// Construct a proven available connection.
    pub fn new(connector: impl Into<String>, label: ConnectionLabel, instance: InstanceId) -> Self {
        Self {
            connector: connector.into(),
            label,
            instance,
        }
    }

    /// Deterministic fixture connection.
    #[doc(hidden)]
    pub fn for_test(connector: &str, label: &str) -> Self {
        Self {
            connector: connector.into(),
            label: ConnectionLabel::new(label).expect("fixture label"),
            instance: InstanceId::parse("00000000-0000-4000-8000-000000000001")
                .expect("fixture UUID"),
        }
    }
}

/// Operator choices for one atomic install or upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    /// Tenant-local App id.
    pub id: String,
    /// Package id.
    pub package: String,
    /// Exact package version.
    pub version: String,
    /// Package Connection slot to operator label.
    pub connections: BTreeMap<String, String>,
    /// Tenant Model Profile id.
    pub model_profile: String,
    /// Required plus selected optional access layers.
    pub access_layers: BTreeSet<String>,
    /// Package datasource slot to tenant Datasource id.
    pub datasources: BTreeMap<String, String>,
    /// Operator-reviewed risk ceiling.
    pub risk_ceiling: Risk,
    /// Installation-local scopes.
    pub scopes: BTreeSet<String>,
    /// Required fingerprint when an upgrade widens authority.
    #[serde(default)]
    pub review: Option<String>,
}

/// One executable operation frozen at review time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenAppOperation {
    /// Connector catalogue id used by [`crate::Invoker`].
    pub catalogue_id: String,
    /// Flux tool name exposed to the Program.
    pub tool_name: String,
    /// Connector id.
    pub connector: String,
    /// Immutable Connection instance id, never a credential address.
    pub connection_instance: String,
    /// Canonical projected Flux tool contract.
    pub contract: String,
    /// Metadata frozen for review and retry decisions.
    pub facts: OperationFacts,
}

/// A complete tenant installation revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInstallation {
    /// Tenant-local id.
    pub id: String,
    /// Package id.
    pub package: String,
    /// Exact package version.
    pub version: String,
    /// Exact package integrity.
    pub package_integrity: String,
    /// Immutable Connection instances by package slot.
    pub connections: BTreeMap<String, String>,
    /// Frozen Model Profile.
    pub model_profile: ModelProfile,
    /// Frozen operations.
    pub operations: Vec<FrozenAppOperation>,
    /// Frozen Datasources by package slot.
    pub datasources: BTreeMap<String, Datasource>,
    /// Installed triggers.
    pub triggers: Vec<PackageTrigger>,
    /// Risk ceiling.
    pub risk_ceiling: Risk,
    /// Scopes.
    pub scopes: BTreeSet<String>,
    /// Stable fingerprint over authority-bearing fields.
    pub review_fingerprint: String,
    /// Monotonic installation revision.
    pub revision: u64,
    /// Activation state.
    pub activation: String,
}

/// Process-local Managed Agent authority. Deliberately not serializable.
#[derive(Clone, PartialEq, Eq)]
pub struct AppRuntimeToken {
    tenant: Tenant,
    app: String,
    agent: String,
    revision: u64,
    nonce: u64,
}

impl std::fmt::Debug for AppRuntimeToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AppRuntimeToken(<opaque>)")
    }
}

/// Operation authority obtained by spending an opaque runtime token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOperation {
    /// Catalogue id the existing invoker accepts.
    pub catalogue_id: String,
    /// Flux tool name.
    pub tool_name: String,
    /// Immutable Connection instance id.
    pub connection_instance: InstanceId,
    /// Managed principal for ordinary grant and invocation gates.
    pub principal: Principal,
}

/// One durable trigger occurrence. Its payload remains private inbox state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventDelivery {
    /// Host-minted id.
    pub id: String,
    /// Installed App id.
    pub app: String,
    /// Event Type.
    pub event_type: String,
    /// Durable state.
    pub status: String,
    /// Attempt count.
    pub attempts: u32,
    /// Whether retry is safe from frozen authority.
    pub retry_safe: bool,
    /// Value-free terminal detail.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredDelivery {
    id: String,
    app: String,
    event_type: String,
    status: String,
    attempts: u32,
    retry_safe: bool,
    detail: Option<String>,
    payload: Value,
}

impl StoredDelivery {
    fn view(&self) -> EventDelivery {
        EventDelivery {
            id: self.id.clone(),
            app: self.app.clone(),
            event_type: self.event_type.clone(),
            status: self.status.clone(),
            attempts: self.attempts,
            retry_safe: self.retry_safe,
            detail: self.detail.clone(),
        }
    }
}

/// Safe Activity projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Host-minted event id.
    pub id: String,
    /// App id.
    pub app: String,
    /// Stable event kind.
    pub kind: String,
    /// Target resource id.
    pub target: String,
    /// Outcome/status.
    pub outcome: String,
    /// Unix timestamp milliseconds.
    pub at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TenantState {
    profiles: BTreeMap<String, ModelProfile>,
    datasources: BTreeMap<String, Datasource>,
    installations: BTreeMap<String, AppInstallation>,
    deliveries: BTreeMap<String, StoredDelivery>,
    activity: Vec<ActivityEvent>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredState {
    tenants: BTreeMap<String, TenantState>,
}

/// Atomic durable binding for installations, deliveries and safe projections.
#[derive(Debug)]
pub struct AppStore {
    registry: PackageRegistry,
    path: Option<PathBuf>,
    state: RwLock<StoredState>,
}

impl AppStore {
    /// In-memory binding for explicitly ephemeral compositions.
    pub fn in_memory(registry: PackageRegistry) -> Self {
        Self {
            registry,
            path: None,
            state: RwLock::new(StoredState::default()),
        }
    }

    /// Bind an owner-only atomic store at `root/apps.json`.
    #[cfg(unix)]
    pub fn bind(root: impl AsRef<Path>, registry: PackageRegistry) -> Result<Self, AppRefusal> {
        let root = root.as_ref();
        if root.as_os_str().is_empty() {
            return Err(AppRefusal::Unconfigured(APP_STORE_SETTING));
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)
            .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
        admit_owner_only(root)?;
        let path = root.join("apps.json");
        let state = match fs::read(&path) {
            Ok(bytes) => {
                admit_owner_only(&path)?;
                serde_json::from_slice(&bytes)
                    .map_err(|error| AppRefusal::Unavailable(error.to_string()))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoredState::default(),
            Err(error) => return Err(AppRefusal::Unavailable(error.to_string())),
        };
        Ok(Self {
            registry,
            path: Some(path),
            state: RwLock::new(state),
        })
    }

    /// Bind the directory named by [`APP_STORE_SETTING`].
    #[cfg(unix)]
    pub fn bind_configured(
        configured: Option<&str>,
        registry: PackageRegistry,
    ) -> Result<Self, AppRefusal> {
        let root = configured
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(AppRefusal::Unconfigured(APP_STORE_SETTING))?;
        Self::bind(root, registry)
    }

    /// Bound file path, absent for an in-memory store.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Curated immutable packages.
    pub fn packages(&self) -> Vec<AppPackage> {
        self.registry.list()
    }

    /// Exact package source for runtime assembly.
    pub fn package(&self, id: &str, version: &str) -> Result<AppPackage, AppRefusal> {
        self.registry
            .get(id, version)
            .cloned()
            .ok_or_else(|| AppRefusal::MissingPackage {
                package: id.into(),
                version: version.into(),
            })
    }

    /// Store one tenant Model Profile atomically.
    pub fn put_model_profile(
        &self,
        tenant: &Tenant,
        profile: ModelProfile,
    ) -> Result<(), AppRefusal> {
        admit_id("Model Profile", &profile.id)?;
        self.mutate(|state| {
            state
                .tenants
                .entry(tenant.to_string())
                .or_default()
                .profiles
                .insert(profile.id.clone(), profile);
            Ok(())
        })
    }

    /// List tenant Model Profiles.
    pub fn model_profiles(&self, tenant: &Tenant) -> Result<Vec<ModelProfile>, AppRefusal> {
        let state = self.read()?;
        Ok(state
            .tenants
            .get(tenant.as_str())
            .into_iter()
            .flat_map(|state| state.profiles.values().cloned())
            .collect())
    }

    /// Store one tenant Datasource atomically.
    pub fn put_datasource(
        &self,
        tenant: &Tenant,
        datasource: Datasource,
    ) -> Result<(), AppRefusal> {
        admit_id("Datasource", &datasource.id)?;
        self.mutate(|state| {
            state
                .tenants
                .entry(tenant.to_string())
                .or_default()
                .datasources
                .insert(datasource.id.clone(), datasource);
            Ok(())
        })
    }

    /// List tenant Datasources.
    pub fn datasources(&self, tenant: &Tenant) -> Result<Vec<Datasource>, AppRefusal> {
        let state = self.read()?;
        Ok(state
            .tenants
            .get(tenant.as_str())
            .into_iter()
            .flat_map(|state| state.datasources.values().cloned())
            .collect())
    }

    /// Resolve every requirement and atomically write one installation.
    pub fn install(
        &self,
        tenant: &Tenant,
        request: InstallRequest,
        available: &[AvailableConnection],
    ) -> Result<AppInstallation, AppRefusal> {
        admit_id("App", &request.id)?;
        let package = self.package(&request.package, &request.version)?;
        package.verify()?;

        self.mutate(|state| {
            let tenant_state = state.tenants.entry(tenant.to_string()).or_default();
            let (connections, connector_instances) =
                resolve_connections(&package, &request, available)?;
            let profile = tenant_state
                .profiles
                .get(&request.model_profile)
                .cloned()
                .filter(|_| package.requirements.model_profile_required)
                .ok_or_else(|| AppRefusal::MissingModelProfile(request.model_profile.clone()))?;
            let operations = resolve_operations(&package, &request, &connector_instances)?;
            let datasources = resolve_datasources(&package, &request, tenant_state)?;
            let revision = tenant_state
                .installations
                .get(&request.id)
                .map_or(1, |current| current.revision + 1);
            let mut installation = AppInstallation {
                id: request.id.clone(),
                package: package.id.clone(),
                version: package.version.clone(),
                package_integrity: package.integrity.clone(),
                connections,
                model_profile: profile,
                operations,
                datasources,
                triggers: package.requirements.triggers.clone(),
                risk_ceiling: request.risk_ceiling,
                scopes: request.scopes.clone(),
                review_fingerprint: String::new(),
                revision,
                activation: "active".into(),
            };
            installation.review_fingerprint = authority_fingerprint(&installation);
            if let Some(current) = tenant_state.installations.get(&request.id) {
                if widens(current, &installation)
                    && request.review.as_deref() != Some(installation.review_fingerprint.as_str())
                {
                    return Err(AppRefusal::NeedsReview {
                        fingerprint: installation.review_fingerprint.clone(),
                    });
                }
            }
            tenant_state
                .installations
                .insert(request.id.clone(), installation.clone());
            tenant_state.activity.push(activity(
                &installation.id,
                "app_installed",
                &format!("{}@{}", installation.package, installation.version),
                "active",
            ));
            Ok(installation)
        })
    }

    /// List tenant installations.
    pub fn list(&self, tenant: &Tenant) -> Result<Vec<AppInstallation>, AppRefusal> {
        let state = self.read()?;
        Ok(state
            .tenants
            .get(tenant.as_str())
            .into_iter()
            .flat_map(|state| state.installations.values().cloned())
            .collect())
    }

    /// Read one tenant installation.
    pub fn get(&self, tenant: &Tenant, app: &str) -> Result<AppInstallation, AppRefusal> {
        self.read()?
            .tenants
            .get(tenant.as_str())
            .and_then(|state| state.installations.get(app))
            .cloned()
            .ok_or_else(|| AppRefusal::NoSuchApp(app.into()))
    }

    /// Mint opaque process-local authority for one declared Managed Agent.
    pub fn runtime_token(
        &self,
        tenant: &Tenant,
        app: &str,
        agent: &str,
    ) -> Result<AppRuntimeToken, AppRefusal> {
        let installation = self.get(tenant, app)?;
        let program = parse_program(
            &self
                .package(&installation.package, &installation.version)?
                .program,
        )?;
        if !program
            .agents
            .iter()
            .any(|candidate| candidate.name == agent)
        {
            return Err(AppRefusal::NoSuchManagedAgent(agent.into()));
        }
        Ok(AppRuntimeToken {
            tenant: tenant.clone(),
            app: app.into(),
            agent: agent.into(),
            revision: installation.revision,
            nonce: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Spend runtime authority on one frozen operation.
    pub fn authorize_operation(
        &self,
        token: &AppRuntimeToken,
        operation: &str,
    ) -> Result<RuntimeOperation, AppRefusal> {
        let installation = self.get(&token.tenant, &token.app)?;
        if installation.revision != token.revision || token.nonce == 0 {
            return Err(AppRefusal::StaleRuntimeToken);
        }
        let frozen = installation
            .operations
            .iter()
            .find(|candidate| {
                candidate.catalogue_id == operation || candidate.tool_name == operation
            })
            .ok_or_else(|| AppRefusal::OperationNotFrozen(operation.into()))?;
        let current =
            connector_catalog::operation(connector_catalog::OperationKey::id(&frozen.catalogue_id))
                .ok_or_else(|| AppRefusal::OperationContractChanged(frozen.catalogue_id.clone()))?;
        let current_spec = connector_pack::project(current)
            .map_err(|_| AppRefusal::OperationContractChanged(frozen.catalogue_id.clone()))?;
        let current_contract = serde_json::to_string(&current_spec)
            .map_err(|_| AppRefusal::OperationContractChanged(frozen.catalogue_id.clone()))?;
        if current_contract != frozen.contract {
            return Err(AppRefusal::OperationContractChanged(
                frozen.catalogue_id.clone(),
            ));
        }
        let connection_instance = InstanceId::parse(&frozen.connection_instance)
            .map_err(|reason| AppRefusal::Unavailable(reason.to_string()))?;
        Ok(RuntimeOperation {
            catalogue_id: frozen.catalogue_id.clone(),
            tool_name: frozen.tool_name.clone(),
            connection_instance,
            principal: Principal::new(
                PrincipalKind::Service,
                format!("managed-app:{}/{}", token.app, token.agent),
                token.tenant.clone(),
            ),
        })
    }

    /// Persist one admitted trigger occurrence before execution.
    pub fn enqueue_delivery(
        &self,
        tenant: &Tenant,
        app: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<EventDelivery, AppRefusal> {
        self.mutate(|state| {
            let tenant_state = state
                .tenants
                .get_mut(tenant.as_str())
                .ok_or_else(|| AppRefusal::NoSuchApp(app.into()))?;
            let installation = tenant_state
                .installations
                .get(app)
                .ok_or_else(|| AppRefusal::NoSuchApp(app.into()))?;
            if !installation
                .triggers
                .iter()
                .any(|trigger| trigger.event_type == event_type)
            {
                return Err(AppRefusal::NoSuchEventType(event_type.into()));
            }
            let retry_safe = installation.operations.iter().all(|operation| {
                operation.facts.idempotency == Idempotency::Idempotent
                    && operation.facts.risk < Risk::Destructive
            });
            let id = unique_id("delivery");
            let delivery = StoredDelivery {
                id: id.clone(),
                app: app.into(),
                event_type: event_type.into(),
                status: "pending".into(),
                attempts: 0,
                retry_safe,
                detail: None,
                payload,
            };
            tenant_state.deliveries.insert(id.clone(), delivery.clone());
            tenant_state
                .activity
                .push(activity(app, "delivery_enqueued", &id, "pending"));
            Ok(delivery.view())
        })
    }

    /// Read one delivery as a structurally payload-free tenant view.
    pub fn delivery(&self, tenant: &Tenant, delivery: &str) -> Result<EventDelivery, AppRefusal> {
        self.read()?
            .tenants
            .get(tenant.as_str())
            .and_then(|state| state.deliveries.get(delivery))
            .map(StoredDelivery::view)
            .ok_or_else(|| AppRefusal::NoSuchDelivery(delivery.into()))
    }

    /// Mark a pending delivery running and return its private inbox payload.
    pub fn begin_delivery(
        &self,
        tenant: &Tenant,
        delivery: &str,
    ) -> Result<(String, String, Value), AppRefusal> {
        self.mutate(|state| {
            let tenant_state = tenant_state_mut(state, tenant)?;
            let entry = tenant_state
                .deliveries
                .get_mut(delivery)
                .ok_or_else(|| AppRefusal::NoSuchDelivery(delivery.into()))?;
            if entry.status != "pending" {
                return Err(AppRefusal::DeliveryState(entry.status.clone()));
            }
            entry.status = "running".into();
            entry.attempts += 1;
            Ok((
                entry.app.clone(),
                entry.event_type.clone(),
                entry.payload.clone(),
            ))
        })
    }

    /// Complete one delivery without retaining its payload in Activity.
    pub fn finish_delivery(
        &self,
        tenant: &Tenant,
        delivery: &str,
        succeeded: bool,
        detail: &str,
    ) -> Result<(), AppRefusal> {
        self.mutate(|state| {
            let tenant_state = tenant_state_mut(state, tenant)?;
            let entry = tenant_state
                .deliveries
                .get_mut(delivery)
                .ok_or_else(|| AppRefusal::NoSuchDelivery(delivery.into()))?;
            entry.status = if succeeded { "succeeded" } else { "failed" }.into();
            entry.detail = Some(detail.into());
            let app = entry.app.clone();
            let outcome = entry.status.clone();
            tenant_state
                .activity
                .push(activity(&app, "delivery_finished", delivery, &outcome));
            Ok(())
        })
    }

    /// Return a failed delivery to pending only when frozen effects are retry-safe.
    pub fn retry_delivery(&self, tenant: &Tenant, delivery: &str) -> Result<(), AppRefusal> {
        let retry_safe = self.mutate(|state| {
            let tenant_state = tenant_state_mut(state, tenant)?;
            let entry = tenant_state
                .deliveries
                .get_mut(delivery)
                .ok_or_else(|| AppRefusal::NoSuchDelivery(delivery.into()))?;
            if entry.status != "failed" {
                return Err(AppRefusal::DeliveryState(entry.status.clone()));
            }
            if !entry.retry_safe {
                entry.status = "indeterminate".into();
                let app = entry.app.clone();
                tenant_state.activity.push(activity(
                    &app,
                    "delivery_indeterminate",
                    delivery,
                    "indeterminate",
                ));
                return Ok(false);
            }
            entry.status = "pending".into();
            entry.detail = None;
            let app = entry.app.clone();
            tenant_state
                .activity
                .push(activity(&app, "delivery_retried", delivery, "pending"));
            Ok(true)
        })?;
        if retry_safe {
            Ok(())
        } else {
            Err(AppRefusal::UnsafeRetry(delivery.into()))
        }
    }

    /// Value-free tenant Activity.
    pub fn activity(&self, tenant: &Tenant) -> Result<Vec<ActivityEvent>, AppRefusal> {
        Ok(self
            .read()?
            .tenants
            .get(tenant.as_str())
            .map(|state| state.activity.clone())
            .unwrap_or_default())
    }

    fn read(&self) -> Result<RwLockReadGuard<'_, StoredState>, AppRefusal> {
        self.state
            .read()
            .map_err(|_| AppRefusal::Unavailable("the App store lock is poisoned".into()))
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, StoredState>, AppRefusal> {
        self.state
            .write()
            .map_err(|_| AppRefusal::Unavailable("the App store lock is poisoned".into()))
    }

    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut StoredState) -> Result<T, AppRefusal>,
    ) -> Result<T, AppRefusal> {
        let mut held = self.write()?;
        let mut next = held.clone();
        let answer = change(&mut next)?;
        self.persist(&next)?;
        *held = next;
        Ok(answer)
    }

    fn persist(&self, state: &StoredState) -> Result<(), AppRefusal> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let encoded = serde_json::to_vec_pretty(state)
            .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        #[cfg(unix)]
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
        #[cfg(not(unix))]
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
        file.write_all(&encoded)
            .and_then(|()| file.sync_all())
            .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
        fs::rename(&temporary, path).map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
        Ok(())
    }
}

type ResolvedConnections = (BTreeMap<String, String>, BTreeMap<String, String>);

fn resolve_connections(
    package: &AppPackage,
    request: &InstallRequest,
    available: &[AvailableConnection],
) -> Result<ResolvedConnections, AppRefusal> {
    let mut connections = BTreeMap::new();
    let mut connector_instances = BTreeMap::new();
    for requirement in &package.requirements.connections {
        let label = request.connections.get(&requirement.name).ok_or_else(|| {
            AppRefusal::MissingConnection {
                slot: requirement.name.clone(),
                connector: requirement.connector.clone(),
            }
        })?;
        let connection = available
            .iter()
            .find(|candidate| {
                candidate.connector == requirement.connector && candidate.label.as_str() == label
            })
            .ok_or_else(|| AppRefusal::MissingConnection {
                slot: requirement.name.clone(),
                connector: requirement.connector.clone(),
            })?;
        connections.insert(requirement.name.clone(), connection.instance.to_string());
        connector_instances
            .entry(requirement.connector.clone())
            .or_insert_with(|| connection.instance.to_string());
    }
    Ok((connections, connector_instances))
}

fn resolve_operations(
    package: &AppPackage,
    request: &InstallRequest,
    connector_instances: &BTreeMap<String, String>,
) -> Result<Vec<FrozenAppOperation>, AppRefusal> {
    let mut selected_layers = request.access_layers.clone();
    selected_layers.extend(
        package
            .requirements
            .access_layers
            .iter()
            .filter(|layer| layer.required)
            .map(|layer| layer.name.clone()),
    );
    if let Some(unknown) = selected_layers.iter().find(|selected| {
        !package
            .requirements
            .access_layers
            .iter()
            .any(|layer| layer.name == selected.as_str())
    }) {
        return Err(AppRefusal::UnknownAccessLayer(unknown.clone()));
    }
    let mut operations = BTreeMap::new();
    for layer in package
        .requirements
        .access_layers
        .iter()
        .filter(|layer| selected_layers.contains(&layer.name))
    {
        let instance = connector_instances.get(&layer.connector).ok_or_else(|| {
            AppRefusal::MissingConnection {
                slot: layer.name.clone(),
                connector: layer.connector.clone(),
            }
        })?;
        let candidates: Vec<_> = connector_catalog::operations()
            .filter(|operation| operation.provider == layer.connector)
            .filter(|operation| layer.selector.admits(&OperationFacts::of(operation)))
            .filter(|operation| OperationFacts::of(operation).risk <= request.risk_ceiling)
            .filter(|operation| {
                layer.required_operations.is_empty()
                    || layer
                        .required_operations
                        .iter()
                        .any(|required| required == operation.id)
            })
            .collect();
        for required in &layer.required_operations {
            if !candidates.iter().any(|operation| operation.id == required) {
                return Err(AppRefusal::MissingOperation(required.clone()));
            }
        }
        if candidates.is_empty() && layer.required {
            return Err(AppRefusal::MissingOperation(format!(
                "selector `{}`",
                layer.name
            )));
        }
        for operation in candidates {
            let spec = connector_pack::project(operation)
                .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
            let contract = serde_json::to_string(&spec)
                .map_err(|error| AppRefusal::Unavailable(error.to_string()))?;
            operations.insert(
                operation.id.to_owned(),
                FrozenAppOperation {
                    catalogue_id: operation.id.to_owned(),
                    tool_name: spec.name,
                    connector: operation.provider.to_owned(),
                    connection_instance: instance.clone(),
                    contract,
                    facts: OperationFacts::of(operation),
                },
            );
        }
    }
    Ok(operations.into_values().collect())
}

fn resolve_datasources(
    package: &AppPackage,
    request: &InstallRequest,
    tenant_state: &TenantState,
) -> Result<BTreeMap<String, Datasource>, AppRefusal> {
    let mut datasources = BTreeMap::new();
    for requirement in &package.requirements.datasources {
        let selected = request.datasources.get(&requirement.name);
        if selected.is_none() && !requirement.required {
            continue;
        }
        let selected =
            selected.ok_or_else(|| AppRefusal::MissingDatasource(requirement.name.clone()))?;
        let datasource = tenant_state
            .datasources
            .get(selected)
            .filter(|datasource| datasource.kind == requirement.kind)
            .cloned()
            .ok_or_else(|| AppRefusal::MissingDatasource(selected.clone()))?;
        datasources.insert(requirement.name.clone(), datasource);
    }
    Ok(datasources)
}

fn tenant_state_mut<'a>(
    state: &'a mut StoredState,
    tenant: &Tenant,
) -> Result<&'a mut TenantState, AppRefusal> {
    state
        .tenants
        .get_mut(tenant.as_str())
        .ok_or_else(|| AppRefusal::Unavailable("tenant App state is absent".into()))
}

fn widens(current: &AppInstallation, candidate: &AppInstallation) -> bool {
    let current_ops: BTreeSet<_> = current
        .operations
        .iter()
        .map(|operation| (&operation.catalogue_id, &operation.connection_instance))
        .collect();
    let candidate_ops: BTreeSet<_> = candidate
        .operations
        .iter()
        .map(|operation| (&operation.catalogue_id, &operation.connection_instance))
        .collect();
    let current_data: BTreeSet<_> = current
        .datasources
        .values()
        .map(|datasource| (&datasource.id, datasource.revision))
        .collect();
    let candidate_data: BTreeSet<_> = candidate
        .datasources
        .values()
        .map(|datasource| (&datasource.id, datasource.revision))
        .collect();
    !candidate_ops.is_subset(&current_ops)
        || !candidate_data.is_subset(&current_data)
        || !candidate.scopes.is_subset(&current.scopes)
        || candidate.risk_ceiling > current.risk_ceiling
        || candidate.model_profile != current.model_profile
        || candidate.triggers != current.triggers
}

fn authority_fingerprint(installation: &AppInstallation) -> String {
    #[derive(Serialize)]
    struct Authority<'a> {
        connections: &'a BTreeMap<String, String>,
        model_profile: &'a ModelProfile,
        operations: &'a [FrozenAppOperation],
        datasources: &'a BTreeMap<String, Datasource>,
        triggers: &'a [PackageTrigger],
        risk_ceiling: Risk,
        scopes: &'a BTreeSet<String>,
    }
    digest(&Authority {
        connections: &installation.connections,
        model_profile: &installation.model_profile,
        operations: &installation.operations,
        datasources: &installation.datasources,
        triggers: &installation.triggers,
        risk_ceiling: installation.risk_ceiling,
        scopes: &installation.scopes,
    })
}

fn digest(value: &impl Serialize) -> String {
    let encoded = serde_json::to_vec(value).expect("canonical package data serializes");
    Sha256::digest(encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn activity(app: &str, kind: &str, target: &str, outcome: &str) -> ActivityEvent {
    ActivityEvent {
        id: unique_id("activity"),
        app: app.into(),
        kind: kind.into(),
        target: target.into(),
        outcome: outcome.into(),
        at_ms: now_ms(),
    }
}

fn unique_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        now_ms(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn admit_id(kind: &'static str, id: &str) -> Result<(), AppRefusal> {
    let valid = !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'));
    if valid {
        Ok(())
    } else {
        Err(AppRefusal::InvalidId {
            kind,
            id: id.into(),
        })
    }
}

#[cfg(unix)]
fn admit_owner_only(path: &Path) -> Result<(), AppRefusal> {
    let mode = fs::metadata(path)
        .map_err(|error| AppRefusal::Unavailable(error.to_string()))?
        .mode();
    if mode & 0o077 != 0 {
        return Err(AppRefusal::Unavailable(format!(
            "App store `{}` permissions allow group or other access",
            path.display()
        )));
    }
    Ok(())
}

/// Why an App operation refused. Every variant refuses and none repairs.
#[derive(Debug, thiserror::Error)]
pub enum AppRefusal {
    /// Store setting absent.
    #[error("App store is not configured; set `{0}`")]
    Unconfigured(&'static str),
    /// Store unavailable.
    #[error("App store unavailable: {0}")]
    Unavailable(String),
    /// Invalid identifier.
    #[error("{kind} id `{id}` is invalid")]
    InvalidId {
        /// Resource kind.
        kind: &'static str,
        /// Refused id.
        id: String,
    },
    /// Invalid package document.
    #[error("invalid App Package: {0}")]
    InvalidPackage(String),
    /// Package digest mismatch.
    #[error("App Package `{package}` version `{version}` failed integrity verification")]
    PackageIntegrity {
        /// Package id.
        package: String,
        /// Version.
        version: String,
    },
    /// Conflicting immutable package revision.
    #[error("App Package `{package}` version `{version}` is occupied by different bytes")]
    ConflictingPackage {
        /// Package id.
        package: String,
        /// Version.
        version: String,
    },
    /// Package absent.
    #[error("App Package `{package}` version `{version}` is missing")]
    MissingPackage {
        /// Package id.
        package: String,
        /// Version.
        version: String,
    },
    /// Connection requirement absent.
    #[error("required Connection `{slot}` for connector `{connector}` is missing")]
    MissingConnection {
        /// Package slot.
        slot: String,
        /// Connector id.
        connector: String,
    },
    /// Model Profile absent.
    #[error("required Model Profile `{0}` is missing")]
    MissingModelProfile(String),
    /// Operation absent.
    #[error("required Operation `{0}` is missing")]
    MissingOperation(String),
    /// Datasource absent.
    #[error("required Datasource `{0}` is missing")]
    MissingDatasource(String),
    /// Layer unknown.
    #[error("access layer `{0}` is not declared by the App Package")]
    UnknownAccessLayer(String),
    /// Upgrade widens authority.
    #[error("the App upgrade widens authority and requires review `{fingerprint}`")]
    NeedsReview {
        /// Required fingerprint.
        fingerprint: String,
    },
    /// App absent.
    #[error("no installed App is called `{0}`")]
    NoSuchApp(String),
    /// Managed Agent absent.
    #[error("no Managed Agent is called `{0}` in this installed App")]
    NoSuchManagedAgent(String),
    /// Token no longer names current revision.
    #[error("the Managed Agent runtime token is stale")]
    StaleRuntimeToken,
    /// Operation is outside frozen authority.
    #[error("Operation `{0}` is not frozen into this installed App")]
    OperationNotFrozen(String),
    /// Frozen operation contract no longer matches the executable catalogue.
    #[error("Operation `{0}` changed after this App revision was reviewed")]
    OperationContractChanged(String),
    /// Event type absent.
    #[error("Event Type `{0}` is not bound by this installed App")]
    NoSuchEventType(String),
    /// Delivery absent.
    #[error("no Event Delivery is called `{0}`")]
    NoSuchDelivery(String),
    /// Delivery state refuses transition.
    #[error("Event Delivery in state `{0}` cannot make that transition")]
    DeliveryState(String),
    /// Retry could repeat an unsafe effect.
    #[error("Event Delivery `{0}` cannot be retried because its effects are not retry-safe")]
    UnsafeRetry(String),
}

impl AppRefusal {
    /// Fingerprint carried only by widening-upgrade refusals.
    pub fn required_review(&self) -> Option<&str> {
        match self {
            Self::NeedsReview { fingerprint } => Some(fingerprint),
            _ => None,
        }
    }
}
