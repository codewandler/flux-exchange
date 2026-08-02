//! Tenant-scoped workflow authoring and immutable publication.
//!
//! Drafts are data, never executable authority. Every method takes a validated [`Tenant`] and the
//! file format nests records below that tenant; callers cannot smuggle a tenant through a workflow
//! id or document body. Execution lives beside the ordinary invocation path in `invoke.rs`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use flux_lang::editor::{EditorDiagnostic, EditorFlow};
use flux_lang::opspec::{OpCatalog, OpSignature};
use flux_runtime::ToolRegistry;
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::paths::{enclosing_working_tree, resolve};
use crate::Tenant;

/// The setting a composing binary reads the workflow directory from.
pub const WORKFLOW_STORE_SETTING: &str = "FLUX_EXCHANGE_WORKFLOWS";

/// Upstream's editor wire schema, served by the Exchange rather than reimplemented in TypeScript.
pub const EDITOR_SCHEMA_VERSION: u32 = flux_lang::editor::EDITOR_SCHEMA_VERSION;

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_ID_BYTES: usize = 96;
const MAX_TITLE_BYTES: usize = 160;

/// One authoring direction. Source is retained byte-for-byte; graph edits are lowered upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WorkflowEdit {
    /// Exact Flux source.
    Source {
        /// The bytes the author supplied.
        source: String,
    },
    /// An upstream versioned editor graph.
    Graph {
        /// The graph to lower and validate.
        graph: EditorFlow,
    },
}

/// A palette operation derived from an executable registry entry, never from UI metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorOperation {
    /// Flux operation name.
    pub id: String,
    /// `connector` or the admitted built-in group `cognition`.
    pub kind: String,
    /// Connector/provider grouping, or `cognition`.
    pub group: String,
    /// Human-readable operation description.
    pub description: String,
    /// Declared JSON input schema.
    pub input_schema: Value,
    /// Declared JSON output schema, when one exists.
    pub output_schema: Option<Value>,
    /// Published risk token.
    pub risk: String,
    /// Published idempotency token.
    pub idempotency: String,
    /// Host effects this operation declares.
    pub effects: Vec<String>,
    /// Host access declarations this operation carries.
    pub access: Vec<Value>,
}

/// A validated registry containing only the upstream pure cognition group.
#[derive(Clone)]
pub struct PureEditorTools {
    registry: ToolRegistry,
}

impl std::fmt::Debug for PureEditorTools {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PureEditorTools")
            .field("operations", &self.registry.specs().len())
            .finish()
    }
}

impl PureEditorTools {
    /// Admit a registry only when every entry is pure, deterministic cognition.
    pub fn new(registry: ToolRegistry) -> Result<Self, WorkflowRefusal> {
        for spec in registry.specs() {
            let value = serde_json::to_value(&spec)
                .map_err(|error| WorkflowRefusal::InvalidPureTool(error.to_string()))?;
            let group = value
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let risk = token(&value, "risk");
            let idempotency = token(&value, "idempotency");
            let effects_empty = value
                .get("effects")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty);
            let access_empty = value
                .get("access")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty);
            if group != "cognition"
                || risk != "low"
                || idempotency != "idempotent"
                || !effects_empty
                || !access_empty
            {
                return Err(WorkflowRefusal::InvalidPureTool(format!(
                    "operation `{}` is not low-risk, idempotent, effect-free cognition",
                    spec.name
                )));
            }
        }
        Ok(Self { registry })
    }

    /// The validated registry used by Flux dispatch.
    pub fn registry(&self) -> ToolRegistry {
        self.registry.clone()
    }
}

/// Return the upstream editor schema for clients that render the graph contract.
pub fn editor_schema() -> Value {
    serde_json::to_value(schema_for!(EditorFlow)).expect("a derived schema serialises")
}

/// Build the editor palette from the connector catalogue and one validated pure registry.
pub fn editor_catalog(pure: &PureEditorTools) -> Result<Vec<EditorOperation>, WorkflowRefusal> {
    let mut out = Vec::new();
    for operation in connector_catalog::operations() {
        let spec = connector_pack::project(operation)
            .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
        let value = serde_json::to_value(&spec)
            .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
        out.push(editor_operation(
            spec.name,
            spec.description,
            spec.input_schema,
            spec.output_schema,
            value,
            "connector",
            operation.provider,
        ));
    }
    for spec in pure.registry.specs() {
        let value = serde_json::to_value(&spec)
            .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
        out.push(editor_operation(
            spec.name,
            spec.description,
            spec.input_schema,
            spec.output_schema,
            value,
            "cognition",
            "cognition",
        ));
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

fn editor_operation(
    id: String,
    description: String,
    input_schema: Value,
    output_schema: Option<Value>,
    value: Value,
    kind: &str,
    group: &str,
) -> EditorOperation {
    EditorOperation {
        id,
        kind: kind.to_owned(),
        group: group.to_owned(),
        description,
        input_schema,
        output_schema,
        risk: token(&value, "risk"),
        idempotency: token(&value, "idempotency"),
        effects: string_array(&value, "effects"),
        access: value
            .get("access")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    }
}

fn token(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn string_array(value: &Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// One referenced operation and the exact executable contract publication saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenOperation {
    /// Flux operation id.
    pub id: String,
    /// `connector` or `cognition`.
    pub kind: String,
    /// Canonical serialized `ToolSpec`; equality detects drift without hiding fields in a hash.
    pub contract: String,
}

/// A valid authoring result, suitable for saving as a draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedWorkflow {
    /// Exact source for source edits, canonical source for graph edits.
    pub source: String,
    /// A complete graph, absent when valid source uses unsupported syntax or trivia.
    pub graph: Option<EditorFlow>,
    /// Non-fatal projection limitations.
    pub diagnostics: Vec<EditorDiagnostic>,
    /// Authoritative runtime path to stable editor node id.
    pub node_map: BTreeMap<String, String>,
    /// Operations and contracts resolved during validation/publication.
    pub operations: Vec<FrozenOperation>,
    /// JSON Schema for run parameters derived from the authored flow declaration.
    pub input_schema: Value,
}

#[derive(Default)]
pub(crate) struct Catalog {
    signatures: BTreeMap<String, OpSignature>,
    specs: BTreeMap<String, Value>,
    kinds: BTreeMap<String, String>,
}

impl Catalog {
    pub(crate) fn contract(&self, operation: &str) -> Option<String> {
        self.specs
            .get(operation)
            .and_then(|spec| serde_json::to_string(spec).ok())
    }

    pub(crate) fn kind(&self, operation: &str) -> Option<&str> {
        self.kinds.get(operation).map(String::as_str)
    }
}

impl OpCatalog for Catalog {
    fn lookup(&self, name: &str) -> Option<OpSignature> {
        self.signatures.get(name).cloned()
    }

    fn param_format(&self, operation: &str, parameter: &str) -> Option<String> {
        self.specs
            .get(operation)?
            .get("input_schema")?
            .get("properties")?
            .get(parameter)?
            .get("format")?
            .as_str()
            .map(str::to_owned)
    }
}

pub(crate) fn catalog(pure: &PureEditorTools) -> Result<Catalog, WorkflowRefusal> {
    let mut catalog = Catalog::default();
    for operation in connector_catalog::operations() {
        let spec = connector_pack::project(operation)
            .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
        catalog.kinds.insert(spec.name.clone(), "connector".into());
        catalog
            .signatures
            .insert(spec.name.clone(), OpSignature::from_spec(&spec));
        let value = serde_json::to_value(&spec)
            .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
        catalog.specs.insert(spec.name.clone(), value);
    }
    for spec in pure.registry.specs() {
        catalog.kinds.insert(spec.name.clone(), "cognition".into());
        catalog
            .signatures
            .insert(spec.name.clone(), OpSignature::from_spec(&spec));
        let value = serde_json::to_value(&spec)
            .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
        catalog.specs.insert(spec.name.clone(), value);
    }
    Ok(catalog)
}

pub(crate) fn connector_entry_for_tool(
    tool_name: &str,
) -> Option<&'static connector_catalog::Operation> {
    connector_catalog::operations().find(|operation| {
        connector_pack::project(operation).is_ok_and(|spec| spec.name == tool_name)
    })
}

/// Project/lower and analyze one edit against the executable editor catalogue.
pub fn validate_workflow(
    edit: WorkflowEdit,
    previous: Option<&EditorFlow>,
    pure: &PureEditorTools,
) -> Result<ValidatedWorkflow, WorkflowRefusal> {
    let (source, graph, diagnostics) = match edit {
        WorkflowEdit::Source { source } => {
            admit_source(&source)?;
            let projection = flux_lang::editor::project_source(&source, previous)
                .map_err(|error| WorkflowRefusal::Invalid(vec![error.to_string()]))?;
            (source, projection.graph, projection.diagnostics)
        }
        WorkflowEdit::Graph { graph } => {
            validate_node_ids(&graph)?;
            let source = flux_lang::editor::lower_source(&graph)
                .map_err(|error| WorkflowRefusal::Invalid(vec![error.to_string()]))?;
            admit_source(&source)?;
            (source, Some(graph), Vec::new())
        }
    };

    let ast = flux_lang::parse::parse(&source)
        .map_err(|error| WorkflowRefusal::Invalid(vec![error.to_string()]))?;
    let catalog = catalog(pure)?;
    flux_lang::analyze::lower(&ast, &catalog, &HashSet::new()).map_err(|diagnostics| {
        WorkflowRefusal::Invalid(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect(),
        )
    })?;

    let ast_value = serde_json::to_value(&ast)
        .map_err(|error| WorkflowRefusal::Invalid(vec![error.to_string()]))?;
    let mut called = BTreeSet::new();
    collect_calls(&ast_value, &mut called);
    let operations = called
        .into_iter()
        .map(|id| {
            let spec = catalog.specs.get(&id).ok_or_else(|| {
                WorkflowRefusal::Invalid(vec![format!("unknown operation `{id}`")])
            })?;
            let contract = serde_json::to_string(spec)
                .map_err(|error| WorkflowRefusal::Catalog(error.to_string()))?;
            Ok(FrozenOperation {
                kind: catalog.kinds.get(&id).cloned().unwrap_or_default(),
                id,
                contract,
            })
        })
        .collect::<Result<Vec<_>, WorkflowRefusal>>()?;

    let input_schema = input_schema(&ast.params);
    let node_map = graph.as_ref().map(EditorFlow::node_map).unwrap_or_default();
    Ok(ValidatedWorkflow {
        source,
        graph,
        diagnostics,
        node_map,
        operations,
        input_schema,
    })
}

fn admit_source(source: &str) -> Result<(), WorkflowRefusal> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(WorkflowRefusal::SourceTooLarge {
            bytes: source.len(),
            limit: MAX_SOURCE_BYTES,
        });
    }
    Ok(())
}

fn validate_node_ids(graph: &EditorFlow) -> Result<(), WorkflowRefusal> {
    for id in graph.node_map().values() {
        if id.is_empty()
            || id.len() > MAX_ID_BYTES
            || !id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
        {
            return Err(WorkflowRefusal::InvalidNodeId(id.clone()));
        }
    }
    Ok(())
}

fn collect_calls(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some("lit") {
                return;
            }
            if let Some(operation) = object.get("op").and_then(Value::as_str) {
                out.insert(operation.to_owned());
            }
            for child in object.values() {
                collect_calls(child, out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_calls(child, out);
            }
        }
        _ => {}
    }
}

fn input_schema(params: &[flux_lang::ast::Param]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for param in params {
        properties.insert(param.name.0.clone(), json!({}));
        required.push(Value::String(param.name.0.clone()));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// A mutable workflow draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDraft {
    /// Tenant-local workflow id.
    pub id: String,
    /// Operator-facing title.
    pub title: String,
    /// Optimistic concurrency revision.
    pub revision: u64,
    /// Latest published version, if any.
    pub published_version: Option<u64>,
    /// Exact or upstream-canonical Flux source.
    pub source: String,
    /// Complete editable graph when supported.
    pub graph: Option<EditorFlow>,
    /// Non-fatal projection limitations.
    pub diagnostics: Vec<EditorDiagnostic>,
    /// Run input contract.
    pub input_schema: Value,
}

/// An immutable published workflow version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowVersion {
    /// Tenant-local workflow id.
    pub workflow_id: String,
    /// Monotonic version within the workflow.
    pub version: u64,
    /// Title at publication.
    pub title: String,
    /// Frozen source.
    pub source: String,
    /// Frozen graph when supported.
    pub graph: Option<EditorFlow>,
    /// Frozen runtime-path to editor-id map.
    pub node_map: BTreeMap<String, String>,
    /// Frozen operation contracts.
    pub operations: Vec<FrozenOperation>,
    /// Frozen run input contract.
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRecord {
    draft: WorkflowDraft,
    validated: ValidatedWorkflow,
    versions: BTreeMap<u64, WorkflowVersion>,
}

type Definitions = BTreeMap<String, BTreeMap<String, WorkflowRecord>>;

/// Owner-only, atomic file binding for tenant-scoped workflow definitions.
#[derive(Debug)]
pub struct WorkflowStore {
    path: PathBuf,
    definitions: RwLock<Definitions>,
}

impl WorkflowStore {
    /// Bind a configured workflow directory.
    pub fn bind_configured(configured: Option<&str>) -> Result<Self, WorkflowRefusal> {
        let root = configured
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(WorkflowRefusal::Unconfigured {
                setting: WORKFLOW_STORE_SETTING,
            })?;
        Self::bind(root)
    }

    /// Bind `root/definitions.json`, refusing a location inside a working tree.
    pub fn bind(root: impl AsRef<Path>) -> Result<Self, WorkflowRefusal> {
        let requested = root.as_ref();
        if requested.as_os_str().is_empty() {
            return Err(WorkflowRefusal::Unconfigured {
                setting: WORKFLOW_STORE_SETTING,
            });
        }
        let root = resolve(requested).map_err(|error| WorkflowRefusal::Unusable {
            path: requested.display().to_string(),
            reason: error.to_string(),
        })?;
        if let Some(worktree) = enclosing_working_tree(&root) {
            return Err(WorkflowRefusal::InsideWorkingTree {
                path: root.display().to_string(),
                root: worktree.display().to_string(),
            });
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .map_err(|error| WorkflowRefusal::Unusable {
                path: root.display().to_string(),
                reason: error.to_string(),
            })?;
        let path = root.join("definitions.json");
        let definitions = match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|error| WorkflowRefusal::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                })?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                return Err(WorkflowRefusal::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                })
            }
        };
        Ok(Self {
            path,
            definitions: RwLock::new(definitions),
        })
    }

    /// Bound definitions file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// List one tenant's drafts.
    pub fn list(&self, tenant: &Tenant) -> Result<Vec<WorkflowDraft>, WorkflowRefusal> {
        let definitions = self.read()?;
        Ok(definitions
            .get(tenant.as_str())
            .into_iter()
            .flat_map(|records| records.values())
            .map(|record| record.draft.clone())
            .collect())
    }

    /// Read one tenant's draft.
    pub fn get(&self, tenant: &Tenant, id: &str) -> Result<WorkflowDraft, WorkflowRefusal> {
        let definitions = self.read()?;
        Ok(self.record(&definitions, tenant, id)?.draft.clone())
    }

    /// Create revision one.
    pub fn create(
        &self,
        tenant: &Tenant,
        id: &str,
        title: &str,
        validated: ValidatedWorkflow,
    ) -> Result<WorkflowDraft, WorkflowRefusal> {
        admit_id(id)?;
        admit_title(title)?;
        let mut definitions = self.write()?;
        let records = definitions.entry(tenant.to_string()).or_default();
        if records.contains_key(id) {
            return Err(WorkflowRefusal::AlreadyExists(id.to_owned()));
        }
        let draft = draft(id, title, 1, None, &validated);
        records.insert(
            id.to_owned(),
            WorkflowRecord {
                draft: draft.clone(),
                validated,
                versions: BTreeMap::new(),
            },
        );
        self.persist(&definitions)?;
        Ok(draft)
    }

    /// Replace a draft only when `expected` is current.
    pub fn save(
        &self,
        tenant: &Tenant,
        id: &str,
        expected: u64,
        title: &str,
        validated: ValidatedWorkflow,
    ) -> Result<WorkflowDraft, WorkflowRefusal> {
        admit_title(title)?;
        let mut definitions = self.write()?;
        let record = self.record_mut(&mut definitions, tenant, id)?;
        check_revision(expected, record.draft.revision)?;
        let revision = record.draft.revision.saturating_add(1);
        let published = record.draft.published_version;
        record.draft = draft(id, title, revision, published, &validated);
        record.validated = validated;
        let answer = record.draft.clone();
        self.persist(&definitions)?;
        Ok(answer)
    }

    /// Freeze the current draft as the next immutable version.
    pub fn publish(
        &self,
        tenant: &Tenant,
        id: &str,
        expected: u64,
    ) -> Result<WorkflowVersion, WorkflowRefusal> {
        let mut definitions = self.write()?;
        let record = self.record_mut(&mut definitions, tenant, id)?;
        check_revision(expected, record.draft.revision)?;
        let version = record.versions.keys().next_back().copied().unwrap_or(0) + 1;
        let published = WorkflowVersion {
            workflow_id: id.to_owned(),
            version,
            title: record.draft.title.clone(),
            source: record.validated.source.clone(),
            graph: record.validated.graph.clone(),
            node_map: record.validated.node_map.clone(),
            operations: record.validated.operations.clone(),
            input_schema: record.validated.input_schema.clone(),
        };
        record.versions.insert(version, published.clone());
        record.draft.published_version = Some(version);
        self.persist(&definitions)?;
        Ok(published)
    }

    /// List immutable versions, newest first.
    pub fn versions(
        &self,
        tenant: &Tenant,
        id: &str,
    ) -> Result<Vec<WorkflowVersion>, WorkflowRefusal> {
        let definitions = self.read()?;
        let mut versions: Vec<_> = self
            .record(&definitions, tenant, id)?
            .versions
            .values()
            .cloned()
            .collect();
        versions.reverse();
        Ok(versions)
    }

    /// Read one immutable version.
    pub fn version(
        &self,
        tenant: &Tenant,
        id: &str,
        version: u64,
    ) -> Result<WorkflowVersion, WorkflowRefusal> {
        let definitions = self.read()?;
        self.record(&definitions, tenant, id)?
            .versions
            .get(&version)
            .cloned()
            .ok_or_else(|| WorkflowRefusal::UnknownVersion {
                id: id.into(),
                version,
            })
    }

    /// Delete a draft and all of its versions with a revision precondition.
    pub fn delete(&self, tenant: &Tenant, id: &str, expected: u64) -> Result<(), WorkflowRefusal> {
        let mut definitions = self.write()?;
        let records = definitions
            .get_mut(tenant.as_str())
            .ok_or_else(|| WorkflowRefusal::UnknownWorkflow(id.into()))?;
        let current = records
            .get(id)
            .ok_or_else(|| WorkflowRefusal::UnknownWorkflow(id.into()))?
            .draft
            .revision;
        check_revision(expected, current)?;
        records.remove(id);
        self.persist(&definitions)
    }

    fn record<'a>(
        &self,
        definitions: &'a Definitions,
        tenant: &Tenant,
        id: &str,
    ) -> Result<&'a WorkflowRecord, WorkflowRefusal> {
        definitions
            .get(tenant.as_str())
            .and_then(|records| records.get(id))
            .ok_or_else(|| WorkflowRefusal::UnknownWorkflow(id.into()))
    }

    fn record_mut<'a>(
        &self,
        definitions: &'a mut Definitions,
        tenant: &Tenant,
        id: &str,
    ) -> Result<&'a mut WorkflowRecord, WorkflowRefusal> {
        definitions
            .get_mut(tenant.as_str())
            .and_then(|records| records.get_mut(id))
            .ok_or_else(|| WorkflowRefusal::UnknownWorkflow(id.into()))
    }

    fn read(&self) -> Result<std::sync::RwLockReadGuard<'_, Definitions>, WorkflowRefusal> {
        self.definitions
            .read()
            .map_err(|_| WorkflowRefusal::Unavailable)
    }

    fn write(&self) -> Result<std::sync::RwLockWriteGuard<'_, Definitions>, WorkflowRefusal> {
        self.definitions
            .write()
            .map_err(|_| WorkflowRefusal::Unavailable)
    }

    fn persist(&self, definitions: &Definitions) -> Result<(), WorkflowRefusal> {
        let encoded = serde_json::to_vec_pretty(definitions).map_err(|error| {
            WorkflowRefusal::Unwritable {
                path: self.path.display().to_string(),
                reason: error.to_string(),
            }
        })?;
        let temporary = self.path.with_extension("tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| self.unwritable(error))?;
        file.write_all(&encoded)
            .map_err(|error| self.unwritable(error))?;
        file.sync_all().map_err(|error| self.unwritable(error))?;
        drop(file);
        fs::rename(&temporary, &self.path).map_err(|error| self.unwritable(error))
    }

    fn unwritable(&self, error: std::io::Error) -> WorkflowRefusal {
        WorkflowRefusal::Unwritable {
            path: self.path.display().to_string(),
            reason: error.to_string(),
        }
    }
}

fn draft(
    id: &str,
    title: &str,
    revision: u64,
    published_version: Option<u64>,
    validated: &ValidatedWorkflow,
) -> WorkflowDraft {
    WorkflowDraft {
        id: id.into(),
        title: title.into(),
        revision,
        published_version,
        source: validated.source.clone(),
        graph: validated.graph.clone(),
        diagnostics: validated.diagnostics.clone(),
        input_schema: validated.input_schema.clone(),
    }
}

fn admit_id(id: &str) -> Result<(), WorkflowRefusal> {
    if id.is_empty()
        || id.len() > MAX_ID_BYTES
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err(WorkflowRefusal::InvalidWorkflowId(id.into()));
    }
    Ok(())
}

fn admit_title(title: &str) -> Result<(), WorkflowRefusal> {
    if title.trim().is_empty() || title.len() > MAX_TITLE_BYTES {
        return Err(WorkflowRefusal::InvalidTitle);
    }
    Ok(())
}

fn check_revision(expected: u64, current: u64) -> Result<(), WorkflowRefusal> {
    if expected != current {
        return Err(WorkflowRefusal::RevisionConflict { expected, current });
    }
    Ok(())
}

/// Why workflow authoring or persistence refused.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowRefusal {
    /// The workflow id is not one safe tenant-local segment.
    #[error("invalid workflow id `{0}`")]
    InvalidWorkflowId(String),
    /// The title is empty or too long.
    #[error("workflow title must be 1..={MAX_TITLE_BYTES} bytes")]
    InvalidTitle,
    /// A graph node id is not a bounded opaque token.
    #[error("invalid editor node id `{0}`")]
    InvalidNodeId(String),
    /// Source exceeded the authoring bound.
    #[error("workflow source is {bytes} bytes; the limit is {limit}")]
    SourceTooLarge {
        /// Supplied bytes.
        bytes: usize,
        /// Maximum bytes.
        limit: usize,
    },
    /// Parse or analysis refused the workflow.
    #[error("workflow is invalid: {0:?}")]
    Invalid(Vec<String>),
    /// The executable catalogue could not be projected.
    #[error("editor catalogue is unusable: {0}")]
    Catalog(String),
    /// A supposedly pure tool failed the hard purity contract.
    #[error("pure editor tool refused: {0}")]
    InvalidPureTool(String),
    /// The tenant already owns this id.
    #[error("workflow `{0}` already exists")]
    AlreadyExists(String),
    /// No workflow with this id exists for the resolved tenant.
    #[error("workflow `{0}` does not exist")]
    UnknownWorkflow(String),
    /// No immutable version exists.
    #[error("workflow `{id}` has no version {version}")]
    UnknownVersion {
        /// Workflow id.
        id: String,
        /// Requested version.
        version: u64,
    },
    /// Optimistic concurrency failed.
    #[error("draft revision conflict: expected {expected}, current {current}")]
    RevisionConflict {
        /// Revision supplied by the writer.
        expected: u64,
        /// Current durable revision.
        current: u64,
    },
    /// No workflow directory was configured.
    #[error("{setting} is not configured")]
    Unconfigured {
        /// Setting name.
        setting: &'static str,
    },
    /// A durable store must not be reachable by `git add`.
    #[error("workflow store `{path}` is inside working tree `{root}`")]
    InsideWorkingTree {
        /// Resolved store directory.
        path: String,
        /// Enclosing checkout.
        root: String,
    },
    /// The configured store cannot be opened.
    #[error("workflow store `{path}` is unusable: {reason}")]
    Unusable {
        /// Resolved or requested path.
        path: String,
        /// IO/parser reason without stored content.
        reason: String,
    },
    /// A write could not become durable.
    #[error("workflow store `{path}` is unwritable: {reason}")]
    Unwritable {
        /// Bound path.
        path: String,
        /// IO reason.
        reason: String,
    },
    /// The process-local lock was poisoned.
    #[error("workflow store is unavailable")]
    Unavailable,
}
