//! Composition of installed App authority with Flux's ordinary App runtime.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use exchange_host::{
    AppInstallation, AppRefusal, AppRuntimeToken, AppStore, EventDelivery, Invoker, Tenant,
};
use flux_app::App;
use flux_core::{Chunk, ContentBlock, StopReason};
use flux_events::{EventKind, EventStore, NewEvent};
use flux_lang::program::Module;
use flux_provider::{ChunkStream, Provider, Request};
use flux_runtime::{Tool, ToolContext, ToolResult};
use flux_secret::Redactor;
use flux_spec::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// One completed delivery answer. The inbox payload and runtime token are absent by construction.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedAppReply {
    /// Durable delivery state.
    pub delivery: EventDelivery,
    /// App result intended for the caller.
    pub reply: String,
    /// Conversation/session key used by the Managed Agent.
    pub session: String,
    /// Installed activation state.
    pub activation: String,
}

/// A Session/Run projection folded from one installed App's Flux event log.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedSession {
    /// Flux's durable session stream id.
    pub id: String,
    /// Caller-visible conversation key bound to this stream by the delivery event.
    pub conversation: String,
    /// Managed Agent declaration.
    pub agent: String,
    /// Completed or refused turns projected from managed-run custom facts.
    pub runs: usize,
    /// Current conversation message count projected by Flux.
    pub messages: usize,
    /// Last projected outcome.
    pub last_outcome: String,
}

/// Value-free Activity projected from durable custom facts in Flux's event log.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedActivity {
    /// Flux event id.
    pub id: String,
    /// Flux session stream.
    pub session: String,
    /// Stable activity kind.
    pub kind: String,
    /// Durable Event Delivery id.
    pub delivery: String,
    /// Completion state.
    pub outcome: String,
    /// Flux event timestamp.
    pub at_ms: i64,
}

/// A refusal while activating or driving an installed App.
#[derive(Debug, thiserror::Error)]
pub enum ManagedAppRefusal {
    /// The tenant installation or durable inbox refused the operation.
    #[error(transparent)]
    Store(#[from] AppRefusal),
    /// Flux could not assemble or execute the installed immutable Program.
    #[error("installed App execution refused: {0}")]
    Execution(String),
    /// This build has no provider binding for the selected profile.
    #[error("Model Profile provider `{0}` is not bound by this deployment")]
    Provider(String),
    /// Durable Flux events could not be opened.
    #[error("installed App event store unavailable: {0}")]
    Events(String),
    /// The runtime cache could not be read safely.
    #[error("installed App runtime state unavailable")]
    RuntimeState,
}

struct InstalledRuntime {
    app: App,
    agent: String,
    events: Arc<EventStore>,
    // Flux App owns the conversation→session map. Serializing deliveries around the before/after
    // event-log fold lets this host correlate its durable delivery fact to the exact Flux stream.
    deliveries: tokio::sync::Mutex<()>,
}

/// Tenant-scoped installed App supervisor.
///
/// The supervisor never receives a credential or address. Its operation tools spend an opaque
/// [`AppRuntimeToken`] to obtain a frozen catalogue id and immutable Connection instance, then
/// delegate to the same [`Invoker`] every direct call uses.
pub struct ManagedAppSupervisor {
    store: Arc<AppStore>,
    invoker: Option<Arc<Invoker>>,
    event_root: Option<PathBuf>,
    runtimes: Mutex<HashMap<(String, String, u64), Arc<InstalledRuntime>>>,
}

impl ManagedAppSupervisor {
    /// Bind the durable installation store, existing invocation boundary and Flux event root.
    pub fn new(
        store: Arc<AppStore>,
        invoker: Option<Arc<Invoker>>,
        event_root: Option<PathBuf>,
    ) -> Result<Self, ManagedAppRefusal> {
        if let Some(root) = &event_root {
            create_private_directory(root)?;
        }
        Ok(Self {
            store,
            invoker,
            event_root,
            runtimes: Mutex::new(HashMap::new()),
        })
    }

    /// The package and installation store exposed through the operator routes.
    pub fn store(&self) -> &Arc<AppStore> {
        &self.store
    }

    /// Persist, execute and project one declared Event Type.
    pub async fn deliver(
        &self,
        tenant: &Tenant,
        app: &str,
        event_type: &str,
        payload: Value,
        session: &str,
    ) -> Result<ManagedAppReply, ManagedAppRefusal> {
        let delivery = self
            .store
            .enqueue_delivery(tenant, app, event_type, payload)?;
        let (installed_app, installed_event, private_payload) =
            self.store.begin_delivery(tenant, &delivery.id)?;
        let runtime = match self.runtime(tenant, &installed_app) {
            Ok(runtime) => runtime,
            Err(error) => {
                self.store
                    .finish_delivery(tenant, &delivery.id, false, "activation_refused")?;
                return Err(error);
            }
        };
        let _delivery_guard = runtime.deliveries.lock().await;
        let before = flux_sessions(&runtime.events)?;
        let runs = match runtime.app.deliver(installed_event, private_payload).await {
            Ok(runs) => runs,
            Err(error) => {
                self.store
                    .finish_delivery(tenant, &delivery.id, false, "execution_refused")?;
                append_run_fact(&runtime, &before, &delivery.id, session, "refused")?;
                return Err(ManagedAppRefusal::Execution(error.to_string()));
            }
        };
        self.store
            .finish_delivery(tenant, &delivery.id, true, "completed")?;
        append_run_fact(&runtime, &before, &delivery.id, session, "completed")?;
        let installation = self.store.get(tenant, app)?;
        Ok(ManagedAppReply {
            delivery: self.store.delivery(tenant, &delivery.id)?,
            reply: runs
                .into_iter()
                .map(|run| run.result)
                .collect::<Vec<_>>()
                .join("\n"),
            session: session.into(),
            activation: installation.activation,
        })
    }

    /// Retry a failed safe delivery and execute its retained private inbox payload.
    pub async fn retry(
        &self,
        tenant: &Tenant,
        delivery: &str,
        session: &str,
    ) -> Result<ManagedAppReply, ManagedAppRefusal> {
        self.store.retry_delivery(tenant, delivery)?;
        let view = self.store.delivery(tenant, delivery)?;
        let (app, event_type, payload) = self.store.begin_delivery(tenant, delivery)?;
        let runtime = match self.runtime(tenant, &app) {
            Ok(runtime) => runtime,
            Err(error) => {
                self.store
                    .finish_delivery(tenant, delivery, false, "activation_refused")?;
                return Err(error);
            }
        };
        let _delivery_guard = runtime.deliveries.lock().await;
        let before = flux_sessions(&runtime.events)?;
        let runs = match runtime.app.deliver(event_type, payload).await {
            Ok(runs) => runs,
            Err(error) => {
                self.store
                    .finish_delivery(tenant, delivery, false, "execution_refused")?;
                append_run_fact(&runtime, &before, delivery, session, "refused")?;
                return Err(ManagedAppRefusal::Execution(error.to_string()));
            }
        };
        self.store
            .finish_delivery(tenant, delivery, true, "completed")?;
        append_run_fact(&runtime, &before, delivery, session, "completed")?;
        let installation = self.store.get(tenant, &app)?;
        Ok(ManagedAppReply {
            delivery: self.store.delivery(tenant, &view.id)?,
            reply: runs
                .into_iter()
                .map(|run| run.result)
                .collect::<Vec<_>>()
                .join("\n"),
            session: session.into(),
            activation: installation.activation,
        })
    }

    fn runtime(
        &self,
        tenant: &Tenant,
        app: &str,
    ) -> Result<Arc<InstalledRuntime>, ManagedAppRefusal> {
        let installation = self.store.get(tenant, app)?;
        let key = (tenant.to_string(), app.to_owned(), installation.revision);
        if let Some(runtime) = self
            .runtimes
            .lock()
            .map_err(|_| ManagedAppRefusal::RuntimeState)?
            .get(&key)
            .cloned()
        {
            return Ok(runtime);
        }
        let runtime = Arc::new(self.activate(tenant, &installation)?);
        self.runtimes
            .lock()
            .map_err(|_| ManagedAppRefusal::RuntimeState)?
            .insert(key, runtime.clone());
        Ok(runtime)
    }

    fn activate(
        &self,
        tenant: &Tenant,
        installation: &AppInstallation,
    ) -> Result<InstalledRuntime, ManagedAppRefusal> {
        let package = self
            .store
            .package(&installation.package, &installation.version)?;
        let program = match Module::parse_str(&package.program)
            .map_err(|error| ManagedAppRefusal::Execution(error.to_string()))?
        {
            Module::Program(program) => program,
            Module::Flow(_) => {
                return Err(ManagedAppRefusal::Execution(
                    "package source is not a Program".into(),
                ))
            }
        };
        let agent = match program.agents.as_slice() {
            [agent] => agent.name.clone(),
            [] => {
                return Err(ManagedAppRefusal::Execution(
                    "installed Program declares no Managed Agent".into(),
                ))
            }
            _ => {
                return Err(ManagedAppRefusal::Execution(
                    "installed Program must name one Managed Agent".into(),
                ))
            }
        };
        let token = self.store.runtime_token(tenant, &installation.id, &agent)?;
        let provider: Arc<dyn Provider> = match installation.model_profile.provider.as_str() {
            "static" => Arc::new(FixedReplyProvider {
                reply: installation
                    .model_profile
                    .static_reply
                    .clone()
                    .unwrap_or_default(),
            }),
            provider => return Err(ManagedAppRefusal::Provider(provider.into())),
        };
        let tools = installation
            .operations
            .iter()
            .map(|operation| {
                let spec: ToolSpec = serde_json::from_str(&operation.contract)
                    .map_err(|error| ManagedAppRefusal::Execution(error.to_string()))?;
                Ok(Arc::new(InstalledOperationTool {
                    spec,
                    store: self.store.clone(),
                    token: token.clone(),
                    invoker: self.invoker.clone(),
                }) as Arc<dyn Tool>)
            })
            .collect::<Result<Vec<_>, ManagedAppRefusal>>()?;
        let events = Arc::new(self.events(tenant, installation)?);
        let app = App::try_with_events(
            program,
            Some(provider),
            installation.model_profile.model.clone(),
            false,
            tools,
            None,
            Redactor::new(),
            events.clone(),
        )
        .map_err(|error| ManagedAppRefusal::Execution(error.to_string()))?;
        Ok(InstalledRuntime {
            app,
            agent,
            events,
            deliveries: tokio::sync::Mutex::new(()),
        })
    }

    /// Sessions and Runs folded from this App's tenant-isolated durable Flux event store.
    pub fn sessions(
        &self,
        tenant: &Tenant,
        app: &str,
    ) -> Result<Vec<ManagedSession>, ManagedAppRefusal> {
        let runtime = self.runtime(tenant, app)?;
        let summaries = runtime
            .events
            .list(1_000)
            .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?;
        summaries
            .into_iter()
            .map(|summary| {
                let events = runtime
                    .events
                    .load_stream(&summary.id, None)
                    .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?;
                let facts: Vec<_> = events.iter().filter_map(managed_run_fact).collect();
                let Some(last) = facts.last() else {
                    return Ok(None);
                };
                Ok(Some(ManagedSession {
                    id: summary.id,
                    conversation: last.conversation.clone(),
                    agent: last.agent.clone(),
                    runs: facts.len(),
                    messages: summary.messages,
                    last_outcome: last.outcome.clone(),
                }))
            })
            .filter_map(|item: Result<Option<_>, ManagedAppRefusal>| item.transpose())
            .collect()
    }

    /// Value-free Activity folded only from custom facts in Flux's durable event log.
    pub fn activity(
        &self,
        tenant: &Tenant,
        app: &str,
    ) -> Result<Vec<ManagedActivity>, ManagedAppRefusal> {
        let runtime = self.runtime(tenant, app)?;
        let mut activity = Vec::new();
        for stream in runtime
            .events
            .all_streams()
            .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?
        {
            for event in runtime
                .events
                .load_stream(&stream, None)
                .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?
            {
                if let Some(fact) = managed_run_fact(&event) {
                    activity.push(ManagedActivity {
                        id: event.id,
                        session: stream.clone(),
                        kind: "managed_run_finished".into(),
                        delivery: fact.delivery,
                        outcome: fact.outcome,
                        at_ms: event.ts_ms,
                    });
                }
            }
        }
        activity.sort_by_key(|event| event.at_ms);
        Ok(activity)
    }

    fn events(
        &self,
        tenant: &Tenant,
        installation: &AppInstallation,
    ) -> Result<EventStore, ManagedAppRefusal> {
        let Some(root) = &self.event_root else {
            return EventStore::in_memory()
                .map_err(|error| ManagedAppRefusal::Events(error.to_string()));
        };
        let mut digest = Sha256::new();
        digest.update(tenant.as_str().as_bytes());
        digest.update([0]);
        digest.update(installation.id.as_bytes());
        let name = format!(
            "{}.sqlite",
            digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        EventStore::open(root.join(name))
            .map_err(|error| ManagedAppRefusal::Events(error.to_string()))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ManagedRunFact {
    delivery: String,
    conversation: String,
    agent: String,
    outcome: String,
}

fn flux_sessions(events: &EventStore) -> Result<HashMap<String, (i64, usize)>, ManagedAppRefusal> {
    events
        .list(1_000)
        .map_err(|error| ManagedAppRefusal::Events(error.to_string()))
        .map(|sessions| {
            sessions
                .into_iter()
                .map(|session| (session.id, (session.updated_at_ms, session.messages)))
                .collect()
        })
}

fn append_run_fact(
    runtime: &InstalledRuntime,
    before: &HashMap<String, (i64, usize)>,
    delivery: &str,
    conversation: &str,
    outcome: &str,
) -> Result<(), ManagedAppRefusal> {
    let after = runtime
        .events
        .list(1_000)
        .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?;
    let stream = after
        .iter()
        .find(|session| {
            before
                .get(&session.id)
                .is_none_or(|previous| *previous != (session.updated_at_ms, session.messages))
        })
        .or_else(|| after.first())
        .ok_or_else(|| {
            ManagedAppRefusal::Events("Flux execution produced no session stream".into())
        })?;
    runtime
        .events
        .append(
            &stream.id,
            NewEvent::new(EventKind::Custom {
                name: "exchange.managed_run".into(),
                payload: serde_json::json!({
                    "delivery": delivery,
                    "conversation": conversation,
                    "agent": runtime.agent,
                    "outcome": outcome,
                }),
            }),
        )
        .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?;
    Ok(())
}

fn managed_run_fact(event: &flux_events::StoredEvent) -> Option<ManagedRunFact> {
    match &event.kind {
        EventKind::Custom { name, payload } if name == "exchange.managed_run" => {
            serde_json::from_value(payload.clone()).ok()
        }
        _ => None,
    }
}

struct InstalledOperationTool {
    spec: ToolSpec,
    store: Arc<AppStore>,
    token: AppRuntimeToken,
    invoker: Option<Arc<Invoker>>,
}

/// Key-free provider that follows Flux's intent-routing protocol before returning fixed prose.
struct FixedReplyProvider {
    reply: String,
}

#[async_trait]
impl Provider for FixedReplyProvider {
    fn name(&self) -> &str {
        "static"
    }

    async fn stream(&self, request: Request) -> flux_core::Result<ChunkStream> {
        let intent = request.tools.len() == 1 && request.tools[0].name == "declare_intent";
        let chunks = if intent {
            vec![
                Chunk::Block(ContentBlock::ToolUse {
                    id: "installed-intent".into(),
                    name: "declare_intent".into(),
                    input: serde_json::json!({
                        "intent": "answer the current message",
                        "capability_families": [],
                    }),
                }),
                Chunk::Done {
                    stop_reason: Some(StopReason::ToolUse),
                },
            ]
        } else {
            vec![
                Chunk::TextDelta(self.reply.clone()),
                Chunk::Done {
                    stop_reason: Some(StopReason::EndTurn),
                },
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(
            chunks.into_iter().map(Ok),
        )))
    }
}

#[async_trait]
impl Tool for InstalledOperationTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        params: Value,
    ) -> flux_core::Result<ToolResult> {
        let authority = self
            .store
            .authorize_operation(&self.token, &self.spec.name)
            .map_err(|error| flux_core::Error::Other(error.to_string()))?;
        let invoker = self.invoker.as_ref().ok_or_else(|| {
            flux_core::Error::Config("operation invocation is not configured".into())
        })?;
        let result = invoker
            .invoke_for_instance(
                &authority.principal,
                &authority.catalogue_id,
                &authority.connection_instance,
                params,
            )
            .await
            .map_err(|error| flux_core::Error::Other(error.to_string()))?;
        Ok(ToolResult {
            content: result.content,
            view: result.view,
            is_error: result.is_error,
        })
    }
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> Result<(), ManagedAppRefusal> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|error| ManagedAppRefusal::Events(error.to_string()))?;
    let metadata =
        fs::metadata(path).map_err(|error| ManagedAppRefusal::Events(error.to_string()))?;
    if metadata.mode() & 0o077 != 0 {
        return Err(ManagedAppRefusal::Events(format!(
            "{} is not owner-only",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> Result<(), ManagedAppRefusal> {
    fs::create_dir_all(path).map_err(|error| ManagedAppRefusal::Events(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use exchange_host::{
        AvailableConnection, InstallRequest, ModelProfile, PackageRegistry, Principal,
        PrincipalKind, Risk, Tenant,
    };
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn chat_is_driven_by_flux_and_projects_payload_free_activity() {
        let store = Arc::new(AppStore::in_memory(PackageRegistry::curated()));
        let tenant = Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new("acme").expect("tenant"),
        )
        .tenant()
        .clone();
        store
            .put_model_profile(
                &tenant,
                ModelProfile::static_reply("demo", "Hello from Flux"),
            )
            .expect("profile");
        store
            .install(
                &tenant,
                InstallRequest {
                    id: "assistant".into(),
                    package: "exchange-apps/slack-bot".into(),
                    version: "1.0.0".into(),
                    connections: BTreeMap::from([("slack".into(), "team".into())]),
                    model_profile: "demo".into(),
                    access_layers: BTreeSet::from(["reply".into()]),
                    datasources: BTreeMap::new(),
                    risk_ceiling: Risk::High,
                    scopes: BTreeSet::new(),
                    review: None,
                },
                &[AvailableConnection::for_test("slack", "team")],
            )
            .expect("install");
        let supervisor = ManagedAppSupervisor::new(store.clone(), None, None).expect("supervisor");

        let reply = supervisor
            .deliver(
                &tenant,
                "assistant",
                "chat",
                json!({"text": "secret prompt", "conversation": "s-1"}),
                "s-1",
            )
            .await
            .expect("chat");

        assert_eq!(reply.reply, "Hello from Flux");
        assert_eq!(reply.delivery.status, "succeeded");
        supervisor
            .deliver(
                &tenant,
                "assistant",
                "chat",
                json!({"text": "second turn", "conversation": "s-1"}),
                "s-1",
            )
            .await
            .expect("second chat");
        let projected =
            serde_json::to_string(&supervisor.activity(&tenant, "assistant").expect("activity"))
                .expect("activity json");
        assert!(!projected.contains("secret prompt"));
        let sessions = supervisor
            .sessions(&tenant, "assistant")
            .expect("Flux sessions");
        assert_eq!(
            sessions.len(),
            1,
            "the supplied conversation key reuses one Flux stream"
        );
        assert_eq!(sessions[0].conversation, "s-1");
        assert_eq!(sessions[0].runs, 2);
        assert_eq!(sessions[0].messages, 4);
    }
}
