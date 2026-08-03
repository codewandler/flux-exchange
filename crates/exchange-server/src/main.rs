//! The `flux-exchange` binary — one composition of [`exchange_host`].
//!
//! # What it serves
//!
//! An HTTP surface bound to loopback by default: a health route, the connector catalogue — what
//! this binary *could* run, never what a caller may run, which is why every operation it serves
//! carries `admitted: null` — and, since X-12, `POST /api/operations/{operation}/invoke`, which
//! runs one of them for the caller's tenant **if a grant its tenant holds admits it** (X-13). The
//! README carries the itemized inventory of what is still not built; the large one here is that a
//! grant is a file an operator writes — no route and no console screen edits one.
//!
//! What it does carry is the rule that makes the rest safe to add: **a reachable bind with no way to
//! resolve a principal is refused at startup**, not warned about and served anyway. See
//! [`bind::admit_bind`] and `docs/designs/http-surface.md`.

mod audit;
mod auth_posture;
mod bind;
pub mod channel;
mod connection_guard;
pub mod credential_acquisition;
mod dev_identity;
mod entropy;
mod execution;
mod local_identity;
mod managed_apps;
mod oidc;
mod operator;
mod routes;
mod service_account;
mod session;
pub mod state;
mod tenancy;
mod traffic;
mod workflow_runs;

use std::ffi::OsStr;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use exchange_host::{Deployment, Runtime};
use tokio::net::TcpListener;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::audit::{AuditJournal, AUDIT_SETTING};
use crate::bind::{admit_audit, admit_bind, StartupRefusal, BIND_ENV, DEFAULT_BIND};
use crate::channel::{
    CatalogueChannelDeclarations, ChannelSupervisor, DeploymentChannelPlacement,
    GeneratedChannelRunner,
};
use crate::dev_identity::{DevIdentity, DEV_IDENTITY_ENV};
use crate::execution::{channel_execution_system, invoker};
use crate::local_identity::{
    generate as generate_local_user, LocalUserRefusal, LocalUsers, LOCAL_USERS_SETTING,
};
use crate::managed_apps::ManagedAppSupervisor;
use crate::oidc::config::{ConfigRefusal, OidcConfig};
use crate::oidc::http_exchange::HttpTokenExchange;
use crate::oidc::Oidc;
use crate::operator::{OperatorPolicy, OPERATOR_SUBJECTS_ENV};
use crate::service_account::{ServiceAccountStore, SERVICE_ACCOUNT_STORE_SETTING};
use crate::state::AppState;
use crate::tenancy::{Tenancy, TENANT_SETTING};

/// The binary flag that declares the zero-configuration, one-tenant development composition.
const DEV_FLAG: &str = "--dev";

/// The conventional startup-user setting from which [`DEV_FLAG`] names its one human.
const USER_ENV: &str = "USER";

/// Startup choices that must agree across identity and runtime admission.
#[derive(Debug, PartialEq, Eq)]
enum Startup {
    /// The ordinary provider composition, with tenancy selected independently from identity.
    Configured { tenancy: Tenancy },
    /// One loopback-only development principal, fixed to the one `dev` tenant at startup.
    Development {
        roster: String,
        operator_subject: String,
        tenancy: Tenancy,
    },
}

impl Startup {
    /// Read the process arguments and environment once, before any port is composed.
    fn configured() -> Result<Self, StartupRefusal> {
        let requested = requests_development(std::env::args_os().skip(1));
        let explicit_roster = std::env::var_os(DEV_IDENTITY_ENV).is_some();
        let user = std::env::var(USER_ENV).ok();
        let tenant = std::env::var(TENANT_SETTING).ok();

        Self::select(
            requested,
            explicit_roster,
            user.as_deref(),
            tenant.as_deref(),
        )
    }

    /// Select a startup shape from already-read inputs, so tests never race over process state.
    fn select(
        requested: bool,
        explicit_roster: bool,
        user: Option<&str>,
        configured_tenant: Option<&str>,
    ) -> Result<Self, StartupRefusal> {
        let configured_tenancy =
            configured_tenant
                .map(Tenancy::single)
                .transpose()
                .map_err(|source| StartupRefusal::Tenancy {
                    reason: source.to_string(),
                })?;
        // An explicit roster is the operator's more precise declaration. Do not overwrite it or
        // silently collapse principals from several tenants into `dev`.
        if explicit_roster || !requested {
            return Ok(Self::Configured {
                tenancy: configured_tenancy.unwrap_or_default(),
            });
        }

        let Some(user) = user.filter(|user| !user.is_empty()) else {
            return Err(StartupRefusal::DevelopmentMode {
                reason: format!(
                    "{DEV_FLAG} cannot name its local principal because {USER_ENV} is unset or \
                     empty. Set {USER_ENV}, or configure {DEV_IDENTITY_ENV} explicitly"
                ),
            });
        };

        let tenancy = Tenancy::single("dev").map_err(|source| StartupRefusal::Tenancy {
            reason: source.to_string(),
        })?;
        if configured_tenancy
            .as_ref()
            .is_some_and(|configured| configured != &tenancy)
        {
            return Err(StartupRefusal::Tenancy {
                reason: format!(
                    "{DEV_FLAG} declares tenant `dev`, but {TENANT_SETTING} declares a different tenant; remove one declaration or make them agree"
                ),
            });
        }

        Ok(Self::Development {
            roster: format!("user:{user}@dev"),
            operator_subject: user.to_owned(),
            tenancy,
        })
    }

    /// The runtime class selected by this startup declaration.
    const fn deployment(&self) -> Deployment {
        match self {
            Self::Configured { tenancy } | Self::Development { tenancy, .. } => {
                tenancy.deployment()
            }
        }
    }

    /// The tenancy policy applied at the identity boundary.
    fn tenancy(&self) -> &Tenancy {
        match self {
            Self::Configured { tenancy } | Self::Development { tenancy, .. } => tenancy,
        }
    }

    /// The sole automatic development user, who is safely the operator on a loopback-only host.
    fn development_operator(&self) -> Option<&str> {
        match self {
            Self::Development {
                operator_subject, ..
            } => Some(operator_subject),
            Self::Configured { .. } => None,
        }
    }

    /// An implied development roster, or `None` when the normal identity configuration applies.
    fn development_roster(&self) -> Option<&str> {
        match self {
            Self::Configured { .. } => None,
            Self::Development { roster, .. } => Some(roster),
        }
    }
}

/// Whether the binary arguments request the local single-tenant composition.
fn requests_development<I, S>(arguments: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == DEV_FLAG)
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    if std::env::args_os().nth(1).as_deref() == Some(OsStr::new("audit-query")) {
        return match audit_query(std::env::args().skip(2)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(refusal) => {
                error!("{refusal}");
                ExitCode::FAILURE
            }
        };
    }

    if std::env::args_os().nth(1).as_deref() == Some(OsStr::new("local-user-secret")) {
        return match local_user_secret(std::env::args().skip(2)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(refusal) => {
                error!("{refusal}");
                ExitCode::FAILURE
            }
        };
    }

    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(refusal) => {
            // The refusal is the product here: it names the address it would not serve and what
            // would have worked, so an operator does not have to guess which half to change.
            error!("{refusal}");
            ExitCode::FAILURE
        }
    }
}

/// Mint one opaque local-user credential and its ready-to-store verifier entry.
fn local_user_secret(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let user = arguments
        .next()
        .ok_or_else(|| "usage: flux-exchange local-user-secret <user> <tenant>".to_owned())?;
    let tenant = arguments
        .next()
        .ok_or_else(|| "usage: flux-exchange local-user-secret <user> <tenant>".to_owned())?;
    if arguments.next().is_some() {
        return Err("usage: flux-exchange local-user-secret <user> <tenant>".to_owned());
    }
    let (secret, entry) = generate_local_user(&user, &tenant).map_err(|error| error.to_string())?;
    let entry = serde_json::to_string_pretty(&vec![entry]).map_err(|error| error.to_string())?;
    println!("secret (shown once): {}", secret.expose_once());
    println!("users file entry:\n{entry}");
    Ok(())
}

/// Query retained evidence locally without adding an HTTP enumeration surface.
fn audit_query(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    enum Query {
        Event(String),
        Actor(String),
        Target(String),
    }

    let mut arguments = arguments.peekable();
    let mut query = None;
    let mut limit = 100_u16;
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag.as_str() {
            "--event-id" if query.is_none() => query = Some(Query::Event(value)),
            "--actor" if query.is_none() => query = Some(Query::Actor(value)),
            "--target" if query.is_none() => query = Some(Query::Target(value)),
            "--limit" => {
                limit = value
                    .parse::<u16>()
                    .ok()
                    .filter(|limit| (1..=1000).contains(limit))
                    .ok_or_else(|| "--limit must be between 1 and 1000".to_owned())?;
            }
            "--event-id" | "--actor" | "--target" => {
                return Err("choose exactly one of --event-id, --actor or --target".to_owned())
            }
            _ => return Err(format!("unknown audit-query option `{flag}`")),
        }
    }
    let query =
        query.ok_or_else(|| "choose exactly one of --event-id, --actor or --target".to_owned())?;
    let configured = std::env::var(AUDIT_SETTING)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{AUDIT_SETTING} does not name an audit journal"))?;
    let journal = AuditJournal::open_read_only(&configured).map_err(|error| error.to_string())?;
    let records = match query {
        Query::Event(event_id) => journal.by_event_id(&event_id),
        Query::Actor(actor) => {
            let parts: Vec<&str> = actor.split('/').collect();
            let [tenant, kind, id] = parts.as_slice() else {
                return Err("--actor must be tenant/kind/id".to_owned());
            };
            journal.by_actor(tenant, kind, id, limit)
        }
        Query::Target(target) => {
            let Some((kind, value)) = target.split_once('/') else {
                return Err("--target must be kind/value".to_owned());
            };
            journal.by_target(kind, value, limit)
        }
    }
    .map_err(|error| error.to_string())?;
    for record in records {
        println!(
            "{}",
            serde_json::to_string(&record).map_err(|error| error.to_string())?
        );
    }
    Ok(())
}

/// Start the server, or refuse and say why.
async fn serve() -> Result<(), StartupRefusal> {
    let startup = Startup::configured()?;
    let bind = configured_bind()?;
    let state = compose(&startup, bind)?;

    report_deployment(startup.deployment());
    report_surface();

    let listener = TcpListener::bind(bind)
        .await
        .map_err(|source| StartupRefusal::BindUnavailable { bind, source })?;
    let local = listener
        .local_addr()
        .map_err(|source| StartupRefusal::BindUnavailable { bind, source })?;

    info!(%local, "flux-exchange is listening");

    let console = configured_console();
    match &console {
        Some(directory) => info!(directory = %directory.display(), "serving the console at /"),
        // Not a warning. A checkout runs this way deliberately: the dev server proxies `/api` and
        // rebuilds on save, which is the faster loop. The console is a deployment concern.
        None => info!(
            "no console directory configured; serving the API only (set {})",
            CONSOLE_SETTING
        ),
    }

    axum::serve(
        listener,
        routes::app_with_console(state, console.as_deref()),
    )
    .with_graceful_shutdown(stop_requested())
    .await
    .map_err(|source| StartupRefusal::Serving { source })
}

/// Where the built console lives, if this deployment serves one.
pub const CONSOLE_SETTING: &str = "FLUX_EXCHANGE_CONSOLE";

/// The console directory this deployment was told to serve, or `None`.
///
/// Read by name, following X-27: one variable, one reader, and the name spelled once. Unset is the
/// shape a checkout runs in and is not an error.
///
/// # Why an absent directory is not refused here
///
/// Every other store in this composition refuses at startup rather than starting degraded, and this
/// deliberately does not follow them. The reason is what each protects: a missing credential store
/// means a service that looks like it works and loses everything, while a missing console directory
/// is a `404` at `/` with the whole API answering correctly. `ServeDir` reports it per request,
/// which is where the operator will be looking, and refusing to start would make a mistyped path in
/// a cosmetic setting take the platform down.
///
/// Empty counts as unset, for the reason `CREDENTIAL_STORE_SETTING` gives: `FLUX_EXCHANGE_CONSOLE=`
/// in an environment file is an operator who has not chosen a path, not one who chose `""`.
fn configured_console() -> Option<PathBuf> {
    let value = std::env::var(CONSOLE_SETTING).ok()?;
    let trimmed = value.trim();

    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Bind the ports this composition serves with: an identity provider, a credential store, and an
/// Service Account store.
///
/// Four ports, all independent, and every one of them defaults to *bound to nothing* with no
/// fallback. The argument is the same in each case and it is the one that decides the shape: a
/// safety property that depends on a setting staying at its default is one setting away from gone,
/// so nothing here has to be turned *off* to be safe — only turned on to be useful.
///
/// The development identity is armed by [`DEV_FLAG`] or [`DEV_IDENTITY_ENV`]; failing that, OIDC
/// sign-in is offered if it was configured; the credential store and Service Account store are bound by
/// their own settings. Unset and unconfigured binds nothing, which is the state a reachable bind is
/// already refused in.
///
/// The development identity is checked first and wins. An operator who armed a roster is working
/// locally, and quietly federating instead would be the more surprising of the two.
fn compose(startup: &Startup, bind: SocketAddr) -> Result<AppState, StartupRefusal> {
    // Read deployment policy before binding any store or background authority. No released
    // connector declares a hazard yet; X-75 hands this posture to the acquisition binding when the
    // first one does. Parsing it now makes an unknown opt-in a startup refusal instead of a policy
    // that silently lost an entry.
    let auth_posture =
        auth_posture::configured().map_err(|source| StartupRefusal::AuthPosture {
            reason: source.to_string(),
        })?;
    let mut state = compose_identity(startup)?
        .with_tenancy(startup.tenancy().clone())
        .with_credential_acquisition(
            auth_posture,
            Arc::new(credential_acquisition::AcquisitionBindings::default()),
        );
    let operators = startup
        .development_operator()
        .map_or_else(OperatorPolicy::from_env, |subject| {
            OperatorPolicy::one(subject.to_owned())
        });
    if !operators.available() {
        warn!(
            "no usable operator policy is bound; administrative routes refuse every caller (set {OPERATOR_SUBJECTS_ENV} to comma-separated immutable OIDC subjects)"
        );
    }
    if startup.development_operator().is_some() {
        warn!(
            armed_by = DEV_FLAG,
            "the sole automatic development user is this loopback-only deployment's operator"
        );
    }
    state = state.with_operator_policy(operators);
    let audit = audit_store()?;
    if let Some(store) = service_account_store()? {
        // A Service Account verifier is an identity binding in its own right, so it must be bound
        // before reachable-address admission is decided.
        state = state.with_service_accounts(store);
    }

    // Before any credential, grant, workflow or channel store is restored. A composition that
    // will refuse its public socket must not exercise background authority on the way there. The
    // identity refusal stays first because it is the older, more fundamental public-bind rule.
    admit_bind(bind, state.identity_binding())?;
    admit_audit(bind, audit.is_some())?;
    if let Some(audit) = audit {
        state = state.with_audit(audit);
    }

    if let Some(registry) = connection_registry()? {
        state = state.with_connection_registry(registry);
    }

    // Bound before the invoker, because the invoker reads it. A composition with no settings store
    // still builds one — it gets an empty configuration, which is X-12's behaviour: the connectors
    // that need nothing per connection run, and the ones that do refuse by name.
    let settings = settings_store()?;
    if let Some(store) = settings.clone() {
        state = state.with_settings(store);
    }
    let configuration = settings.map_or_else(
        || Arc::new(exchange_host::MemoryConfig::new()) as Arc<dyn exchange_host::ConfigStore>,
        |store| store as Arc<dyn exchange_host::ConfigStore>,
    );

    // Bound before the invoker, because the invoker requires it. **No grant store, no invoker** —
    // see `grant_store`: an invoker built without one could only be built by choosing what to do in
    // its absence, and the only available choice is to admit everything.
    let grants = grant_store()?;

    let channels = channel_store()?;
    if let Some(store) = credential_store()? {
        // The invoker is built from the same store the connections surface writes to, and only
        // when there is one. A composition with no store could still resolve a principal and look
        // an operation up, and would then send every request unauthenticated — a fail-closed `401`
        // from the vendor, but one an agent treating `401` as retryable loops on forever. Binding
        // nothing means `POST /api/operations/{operation}/invoke` refuses with `503` and names the
        // setting, which is the honest answer rather than a hole.
        if let Some(grants) = grants {
            // An empty configuration when nothing was bound, and deliberately not a refusal: the
            // settings store is what the *templated* connectors need, and a host without one is
            // still a working host for the rest of the catalogue. What it must not do is pretend —
            // the seventeen that need a value refuse by name, quoting the field and the service.
            state = state.with_invoker(Arc::new(
                invoker(
                    startup.deployment(),
                    store.clone(),
                    Arc::clone(&configuration),
                    grants,
                )
                .map_err(|reason| StartupRefusal::Invoker { reason })?,
            ));
        }

        if let Some(channels) = channels {
            let planner = Arc::new(exchange_host::ConnectorChannelPlanner::new(
                store.clone(),
                Arc::clone(&configuration),
            ));
            let execution_system = channel_execution_system()
                .map_err(|reason| StartupRefusal::ChannelRuntime { reason })?;
            let runner = Arc::new(GeneratedChannelRunner::new(
                planner,
                execution_system,
                flux_system::net::PrivateNetAllow::None,
            ));
            let supervisor = ChannelSupervisor::new(
                channels,
                Arc::new(CatalogueChannelDeclarations),
                Arc::new(DeploymentChannelPlacement::new(startup.deployment())),
                runner,
            );
            supervisor.restore();
            state = state.with_channels(supervisor);
        }

        // Bound whether or not an invoker was, and that combination is a real composition rather
        // than an oversight: a host with credentials and no grants is one an operator can connect a
        // vendor to and nobody can run anything on, which is the honest state to be in on the way
        // to granting something.
        state = state.with_credentials(store);
    } else if channels.is_some() {
        warn!(
            "a channel store is bound but no credential store is available, so channel management \
             refuses instead of supervising unauthenticated vendor connections"
        );
    }
    if let Some((workflows, pure, runs)) = workflow_store()? {
        state = state.with_workflows(workflows, pure, runs);
    }
    if let Some(apps) = app_store(state.invoker().cloned())? {
        state = state.with_apps(apps);
    }

    Ok(state)
}

/// Bind installed App declarations and the per-App durable Flux event logs, or bind neither.
#[cfg(unix)]
fn app_store(
    invoker: Option<Arc<exchange_host::Invoker>>,
) -> Result<Option<Arc<ManagedAppSupervisor>>, StartupRefusal> {
    let Ok(configured) = std::env::var(exchange_host::APP_STORE_SETTING) else {
        warn!(
            "no installed App store is bound ({} is unset), so App installation and chat refuse",
            exchange_host::APP_STORE_SETTING
        );
        return Ok(None);
    };
    let store = exchange_host::AppStore::bind_configured(
        Some(&configured),
        exchange_host::PackageRegistry::curated(),
    )
    .map_err(|error| StartupRefusal::AppStore {
        reason: error.to_string(),
    })?;
    let event_root = store
        .path()
        .and_then(std::path::Path::parent)
        .expect("a bound App file has a parent")
        .join("flux-events");
    let supervisor = ManagedAppSupervisor::new(Arc::new(store), invoker, Some(event_root.clone()))
        .map_err(|error| StartupRefusal::AppStore {
            reason: error.to_string(),
        })?;
    info!(path = %configured, "installed Apps: owner-only atomic bindings");
    info!(path = %event_root.display(), "installed App activity: tenant-isolated Flux event logs");
    Ok(Some(Arc::new(supervisor)))
}

#[cfg(not(unix))]
fn app_store(
    _invoker: Option<Arc<exchange_host::Invoker>>,
) -> Result<Option<Arc<ManagedAppSupervisor>>, StartupRefusal> {
    Ok(None)
}

/// Bind the durable application audit journal, or bind none for a loopback composition.
#[cfg(unix)]
fn audit_store() -> Result<Option<Arc<AuditJournal>>, StartupRefusal> {
    let Ok(configured) = std::env::var(AUDIT_SETTING) else {
        warn!(
            "no durable audit journal is bound ({AUDIT_SETTING} is unset); loopback remains \
             available, but a reachable bind will refuse"
        );
        return Ok(None);
    };
    let journal = AuditJournal::bind(&configured).map_err(|error| StartupRefusal::AuditStore {
        reason: error.to_string(),
    })?;
    info!(path = %journal.path().display(), "audit evidence: owner-only SQLite journal, minimum 30-day retention");
    Ok(Some(Arc::new(journal)))
}

#[cfg(not(unix))]
fn audit_store() -> Result<Option<Arc<AuditJournal>>, StartupRefusal> {
    Ok(None)
}

/// Bind persistent channel declarations, or bind none. Unset is an unavailable capability rather
/// than an in-memory fallback; configured and unreadable refuses startup.
#[cfg(unix)]
fn channel_store() -> Result<Option<Arc<dyn exchange_host::Channels>>, StartupRefusal> {
    let Ok(configured) = std::env::var(exchange_host::CHANNEL_STORE_SETTING) else {
        warn!(
            "no channel store is bound ({} is unset), so persistent connector channels refuse",
            exchange_host::CHANNEL_STORE_SETTING
        );
        return Ok(None);
    };
    let store =
        exchange_host::ChannelStore::bind_configured(Some(&configured)).map_err(|error| {
            StartupRefusal::ChannelStore {
                reason: error.to_string(),
            }
        })?;
    info!(path = %store.path().display(), "connector channels: owner-only atomic file store");
    Ok(Some(Arc::new(store)))
}

#[cfg(not(unix))]
fn channel_store() -> Result<Option<Arc<dyn exchange_host::Channels>>, StartupRefusal> {
    Ok(None)
}

/// Bind workflow definitions and the audited pure cognition pack, or bind neither.
type WorkflowBinding = (
    Arc<exchange_host::WorkflowStore>,
    Arc<exchange_host::PureEditorTools>,
    Arc<crate::workflow_runs::WorkflowRunStore>,
);

#[cfg(unix)]
fn workflow_store() -> Result<Option<WorkflowBinding>, StartupRefusal> {
    let Ok(configured) = std::env::var(exchange_host::WORKFLOW_STORE_SETTING) else {
        warn!(
            "no workflow store is bound ({} is unset), so workflow authoring and runs refuse",
            exchange_host::WORKFLOW_STORE_SETTING
        );
        return Ok(None);
    };
    let store =
        exchange_host::WorkflowStore::bind_configured(Some(&configured)).map_err(|error| {
            StartupRefusal::WorkflowStore {
                reason: error.to_string(),
            }
        })?;
    let mut registry = exchange_host::ToolRegistry::new();
    flux_tools::cognition::try_register_cognition(&mut registry).map_err(|error| {
        StartupRefusal::WorkflowStore {
            reason: error.to_string(),
        }
    })?;
    let pure = exchange_host::PureEditorTools::new(registry).map_err(|error| {
        StartupRefusal::WorkflowStore {
            reason: error.to_string(),
        }
    })?;
    let run_path = store
        .path()
        .parent()
        .expect("a bound definitions file has a parent")
        .join("runs.sqlite");
    let runs = crate::workflow_runs::WorkflowRunStore::bind(&run_path)
        .map_err(|reason| StartupRefusal::WorkflowStore { reason })?;
    info!(path = %store.path().display(), "workflow definitions: owner-only atomic file store");
    info!(path = %run_path.display(), "workflow activity: SQLite run store");
    Ok(Some((Arc::new(store), Arc::new(pure), Arc::new(runs))))
}

#[cfg(not(unix))]
fn workflow_store() -> Result<Option<WorkflowBinding>, StartupRefusal> {
    Ok(None)
}

/// Bind the Service Account store the environment names, or bind none.
///
/// The same three states as the credential store, for the same reason and with one difference worth
/// naming. **Unset binds nothing**, and `/api/service-accounts` then refuses with `503` quoting this
/// setting — which is not the in-memory fallback X-09 refuses, because nothing is served from
/// somewhere else: the host says it cannot hold the record, and a token it could not record is one
/// nobody could ever revoke. **Set and unusable refuses to start**, since a store the operator named
/// and this process could not open is a mistake with no later moment at which it announces itself —
/// and, here, one of the ways it can be unusable is a mode that would let somebody else plant a
/// verifier, which is an authentication bypass rather than an inconvenience. See `crate::service_account`.
///
/// Not `#[cfg(unix)]`, unlike the credential store. What protects a *credential* in that file is the
/// mode and nothing else, so a platform that cannot spell one gets no store at all; what protects an
/// Service Account token here is that the store holds a digest rather than the token, which holds on every
/// platform. The mode still matters — it is what stops a planted verifier — and `crate::service_account`
/// states plainly what is lost where it cannot be checked.
fn service_account_store() -> Result<Option<Arc<ServiceAccountStore>>, StartupRefusal> {
    let canonical = std::env::var(SERVICE_ACCOUNT_STORE_SETTING).ok();
    let configured = match service_account_store_path(canonical.as_deref())? {
        Some(configured) => configured,
        None => {
            warn!(
                "no Service Account store is bound ({SERVICE_ACCOUNT_STORE_SETTING} is unset), so management and bearer authentication will refuse"
            );
            return Ok(None);
        }
    };

    let store = ServiceAccountStore::open(&configured).map_err(|source| {
        StartupRefusal::ServiceAccountStore {
            reason: source.to_string(),
        }
    })?;

    info!("{}", store.banner());
    Ok(Some(Arc::new(store)))
}

/// Resolve the canonical setting without reading global state.
fn service_account_store_path(canonical: Option<&str>) -> Result<Option<String>, StartupRefusal> {
    let configured = match canonical {
        None => {
            return Ok(None);
        }
        Some(canonical) => canonical.trim(),
    };
    Ok(Some(configured.to_owned()))
}

/// Bind the credential store the environment names, or bind none.
///
/// The same three states as the development identity, and for the same reason. **Unset binds
/// nothing**, and every route that would have used a store then refuses with `503` naming this
/// setting — which is not the in-memory fallback X-09 refuses, because nothing is served from
/// somewhere else; the host says it cannot hold a credential. **Set and unusable refuses to
/// start**, since a store the operator named and this process could not open is a mistake with no
/// later moment at which it announces itself.
///
/// `#[cfg(unix)]` because `CredentialStore` is: what protects a value in the file store is `0600`
/// and `0700`, and a platform that cannot spell those would get a store implying a safety it does
/// not have. The *port* is not gated, so another platform's composition binds its own.
#[cfg(unix)]
fn credential_store() -> Result<Option<Arc<dyn exchange_host::SecretStore>>, StartupRefusal> {
    use exchange_host::{CredentialStore, CREDENTIAL_STORE_SETTING};

    let Ok(configured) = std::env::var(CREDENTIAL_STORE_SETTING) else {
        warn!(
            "no credential store is bound ({CREDENTIAL_STORE_SETTING} is unset), so connecting a \
             connector will refuse. Set it to a path outside every working tree to hold \
             credentials",
        );
        return Ok(None);
    };

    let store = CredentialStore::bind_configured(Some(&configured)).map_err(|source| {
        StartupRefusal::CredentialStore {
            reason: source.to_string(),
        }
    })?;

    // Read back off the bound store, so this line cannot name a file this process did not open.
    info!("{}", store.banner());

    Ok(Some(store.secrets()))
}

/// No file store on this platform; a composition here binds its own or holds none.
#[cfg(not(unix))]
fn credential_store() -> Result<Option<Arc<dyn exchange_host::SecretStore>>, StartupRefusal> {
    Ok(None)
}

/// Bind the grant store the environment names, or bind none.
///
/// The same three states as the credential store — **unset binds nothing**, **set and unusable
/// refuses to start** — and one consequence that is this store's alone: unset means this host runs
/// **nothing**, because there is nowhere for a grant to live and therefore no grant that could admit
/// an operation. `POST /api/operations/{operation}/invoke` refuses with `503` naming this setting.
///
/// That is a deliberate choice against the alternative, which is worth naming because it is the one
/// somebody will propose. An unset grant store could have meant "no grants configured, so admit
/// everything", and that is exactly the exposure X-13 exists to close, reintroduced as a default.
/// **The safe state is the one you get by doing nothing**; the useful one is the one you have to
/// configure.
///
/// `#[cfg(unix)]` because the file binding is, for [`credential_store`]'s reason with a different
/// thing at stake: nothing in this file is a secret, and somebody who can *write* to it decides what
/// this host will run with a tenant's credentials. The port is not gated, so another platform's
/// composition binds its own.
#[cfg(unix)]
fn grant_store() -> Result<Option<Arc<dyn exchange_host::Grants>>, StartupRefusal> {
    use exchange_host::{GrantStore, GRANT_STORE_SETTING};

    let Ok(configured) = std::env::var(GRANT_STORE_SETTING) else {
        warn!(
            "no grant store is bound ({GRANT_STORE_SETTING} is unset), so this host runs no \
             operation for anybody: an invocation is admitted by a grant, and there is nowhere for \
             one to live. Set it to a path outside every working tree. It holds no secrets — it \
             holds what each tenant may run",
        );
        return Ok(None);
    };

    let store = GrantStore::bind_configured(Some(&configured)).map_err(|source| {
        StartupRefusal::GrantStore {
            reason: source.to_string(),
        }
    })?;

    // Read back off the bound store, so this line cannot name a file this process did not open.
    info!("{}", store.banner());

    Ok(Some(Arc::new(store)))
}

/// No file store on this platform; a composition here binds its own or holds none.
#[cfg(not(unix))]
fn grant_store() -> Result<Option<Arc<dyn exchange_host::Grants>>, StartupRefusal> {
    Ok(None)
}

/// Bind the connection-settings store the environment names, or bind none.
///
/// The same three states as the credential store, and the same argument for each — **unset binds
/// nothing**, **set and unusable refuses to start** — with one difference stated plainly, because
/// it is the whole of why this is a second function and not a second path passed to the first.
///
/// **What this file holds is not secret.** A subdomain, a workspace slug, an account name: each is
/// in the URL of every request the connector makes and in the vendor's own dashboard. It is kept
/// apart from the credential store because mixing them would make `held` and the tenant occupancy
/// bound each mean two things at once, not because it needs the same protection —
/// `exchange_host::settings` carries that argument at length.
///
/// Unset is therefore a *warning* rather than a silent absence, and it names the consequence
/// precisely: seventeen connectors refuse by name until this is set, and the rest are unaffected.
///
/// `#[cfg(unix)]` because the file binding is, for [`credential_store`]'s reason with less at stake:
/// the modes there are what protects a credential, and here they are hygiene for a customer's data.
/// The port is not gated, so another platform's composition binds its own.
#[cfg(unix)]
fn settings_store() -> Result<Option<Arc<dyn exchange_host::ConnectionSettings>>, StartupRefusal> {
    use exchange_host::{SettingsStore, CONNECTION_SETTINGS_SETTING};

    let Ok(configured) = std::env::var(CONNECTION_SETTINGS_SETTING) else {
        warn!(
            "no connection-settings store is bound ({CONNECTION_SETTINGS_SETTING} is unset), so \
             every connector whose base URL is templated on a per-connection value — zendesk, \
             shopify, jira and fourteen others — will refuse by name. Set it to a path outside \
             every working tree. It holds no secrets; credentials stay in the credential store",
        );
        return Ok(None);
    };

    let store = SettingsStore::bind_configured(Some(&configured)).map_err(|source| {
        StartupRefusal::SettingsStore {
            reason: source.to_string(),
        }
    })?;

    // Read back off the bound store, so this line cannot name a file this process did not open.
    info!("{}", store.banner());

    Ok(Some(Arc::new(store)))
}

/// Bind the durable label-to-UUID overlay, or bind none for sole-connection compatibility.
#[cfg(unix)]
fn connection_registry(
) -> Result<Option<Arc<dyn exchange_host::ConnectionRegistry>>, StartupRefusal> {
    use exchange_host::{ConnectionRegistryStore, CONNECTION_REGISTRY_SETTING};

    let Ok(configured) = std::env::var(CONNECTION_REGISTRY_SETTING) else {
        warn!(
            "no connection registry is bound ({CONNECTION_REGISTRY_SETTING} is unset); sole legacy connections still work, but labels and multiple instances refuse"
        );
        return Ok(None);
    };
    let store = ConnectionRegistryStore::bind_configured(Some(&configured)).map_err(|source| {
        StartupRefusal::ConnectionRegistry {
            reason: source.to_string(),
        }
    })?;
    info!("{}", store.banner());
    Ok(Some(Arc::new(store)))
}

#[cfg(not(unix))]
fn connection_registry(
) -> Result<Option<Arc<dyn exchange_host::ConnectionRegistry>>, StartupRefusal> {
    Ok(None)
}

/// No file store on this platform; a composition here binds its own or holds none.
#[cfg(not(unix))]
fn settings_store() -> Result<Option<Arc<dyn exchange_host::ConnectionSettings>>, StartupRefusal> {
    Ok(None)
}

/// Bind the identity port this composition serves with.
///
/// Two on offer. The development one is armed by [`DEV_FLAG`] or [`DEV_IDENTITY_ENV`] and wins when
/// either selects it; failing that, OIDC federates if it is configured and its token exchange could
/// be built. Unset and unconfigured binds nothing, which is the state a reachable bind is already
/// refused in.
fn compose_identity(startup: &Startup) -> Result<AppState, StartupRefusal> {
    if let Ok(path) = std::env::var(LOCAL_USERS_SETTING) {
        return compose_local_users(LocalUsers::open(&path));
    }

    let implied = startup
        .development_roster()
        .map(DevIdentity::from_roster)
        .transpose()?;
    let Some(dev) = implied.or(DevIdentity::armed()?) else {
        return Ok(compose_oidc(OidcConfig::from_env()));
    };

    // Named as such at startup, and at `warn` rather than `info`: this line is the difference
    // between a host whose principals are authenticated and one whose principals are asserted, and
    // an operator who scrolls past it should still have seen it.
    let roster: Vec<String> = dev
        .roster()
        .map(|(handle, principal)| format!("{handle} -> {principal}"))
        .collect();
    let armed_by = if startup.development_roster().is_some() {
        DEV_FLAG
    } else {
        DEV_IDENTITY_ENV
    };

    warn!(
        armed_by,
        roster = %roster.join(", "),
        "DEVELOPMENT identity armed. Any caller presenting one of these \
         handles becomes that principal, with no secret required. This host will refuse to serve \
         on any address but loopback while it is armed",
    );

    if startup.development_roster().is_some() {
        Ok(AppState::with_automatic_development_identity(Arc::new(dev)))
    } else {
        Ok(AppState::with_development_identity(Arc::new(dev)))
    }
}

/// Turn a loaded verifier file into its distinct identity state, or make any file refusal a
/// startup refusal. Taking the result keeps malformed-startup behavior testable without racing on
/// process environment.
fn compose_local_users(
    loaded: Result<LocalUsers, LocalUserRefusal>,
) -> Result<AppState, StartupRefusal> {
    let users = loaded.map_err(|source| StartupRefusal::LocalUsers {
        reason: source.to_string(),
    })?;
    info!(
        users = users.len(),
        "verifier-backed local human sign-in is configured"
    );
    Ok(AppState::with_local_users(Arc::new(users)))
}

/// Offer OIDC sign-in, or say precisely why it is not on offer.
///
/// **Never a `StartupRefusal`.** Unconfigured sign-in is an absent feature, not a hole: `/health`
/// and the catalogue still answer, and exiting here would take them down to punish an operator who
/// has not set up federation yet. The Acceptance is explicit that this must be a startup message
/// and an explanatory page rather than a panic — and, just as explicitly, that it must not be a
/// login that looks fine and dies at the callback. Both branches below refuse at `/api/signin`.
///
/// Takes the configuration rather than reading it, so the "never a `StartupRefusal`" claim is
/// testable without a test mutating the process environment out from under its neighbours — see
/// `a_cleartext_endpoint_refusal_keeps_the_rest_of_the_host_serving`. The only production caller
/// passes [`OidcConfig::from_env`].
fn compose_oidc(configured: Result<OidcConfig, ConfigRefusal>) -> AppState {
    match configured {
        Err(refusal) => {
            // Names every unset variable in one message, so an operator fixes them in one pass
            // rather than one restart at a time. At `warn` rather than `error`: on a host nobody
            // configured for sign-in this is a statement of fact, not a fault.
            warn!("{refusal}");
            AppState::without_identity()
        }
        Ok(config) => match HttpTokenExchange::new(&config) {
            Ok(exchange) => {
                info!(
                    issuer = config.issuer(),
                    tenant = %config.tenant(),
                    "OIDC sign-in is configured and the token exchange is bound",
                );
                AppState::with_oidc(Arc::new(Oidc::new(config, Arc::new(exchange))))
            }
            // The configuration is good and the HTTP client could not be built — a missing TLS
            // backend, most plausibly. Still not a `StartupRefusal`: the same reasoning as an
            // unconfigured provider, since /health and the catalogue are no less serveable for
            // sign-in being unavailable. `/api/signin` explains rather than sending a browser to a
            // provider this build could not return from.
            Err(reason) => {
                error!(
                    issuer = config.issuer(),
                    reason,
                    "OIDC sign-in is configured but the token exchange could not be built, so no \
                     authorization code could be redeemed and no sign-in can complete",
                );
                AppState::oidc_without_a_token_exchange()
            }
        },
    }
}

/// Where to listen, from the environment or from the loopback default.
fn configured_bind() -> Result<SocketAddr, StartupRefusal> {
    let Ok(configured) = std::env::var(BIND_ENV) else {
        return Ok(DEFAULT_BIND);
    };

    configured
        .parse()
        .map_err(|source| StartupRefusal::UnreadableBind {
            value: configured,
            source,
        })
}

/// Wait for the operator to ask for a stop, so in-flight requests finish rather than being cut.
async fn stop_requested() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        // Never returns: a process that cannot hear its own stop signal must keep serving rather
        // than read the failure as "stop now" and drop every request in flight.
        error!(%error, "cannot listen for ctrl-c; graceful shutdown is unavailable");
        std::future::pending::<()>().await;
    }
}

/// Log the surface this process publishes, and which part of it needs a principal.
fn report_surface() {
    for (module, route) in routes::published() {
        info!(
            module = module.name,
            path = route.path,
            access = ?route.access,
            "route",
        );
    }
}

/// Log which runtimes the selected deployment shape serves, and which it refuses.
///
/// Kept from the pre-service binary deliberately. It is the one line of startup output that shows
/// the tenancy rule is in force and decided at startup, rather than from a request field.
fn report_deployment(deployment: Deployment) {
    const RUNTIMES: [Runtime; 6] = [
        Runtime::Http,
        Runtime::Socket,
        Runtime::Process,
        Runtime::Container,
        Runtime::Plugin,
        Runtime::Remote,
    ];

    let (served, refused): (Vec<_>, Vec<_>) = RUNTIMES
        .iter()
        .partition(|runtime| deployment.admits(**runtime).is_ok());

    info!(
        deployment = ?deployment,
        serves = %names(&served),
        refuses = %names(&refused),
        "runtime admission",
    );
}

fn names(runtimes: &[&Runtime]) -> String {
    if runtimes.is_empty() {
        return "nothing".to_string();
    }

    runtimes
        .iter()
        .map(|runtime| format!("{runtime:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::ErrorKind;

    use axum::body::Body;
    use axum::http::{header, Request as HttpRequest, StatusCode};
    use tokio::net::TcpStream;
    use tower::{Service, ServiceExt as _};

    use crate::oidc::config::AUTHORIZATION_ENDPOINT_ENV;

    #[test]
    fn a_malformed_local_users_entry_refuses_process_composition_and_names_the_entry() {
        let marker = "plaintext-must-not-be-echoed";
        let loaded = LocalUsers::from_json(&format!(
            r#"[{{"user":"alice","tenant":"acme","verifier":"{marker}"}}]"#
        ));
        let refusal = match compose_local_users(loaded) {
            Err(refusal) => refusal.to_string(),
            Ok(_) => panic!("a malformed verifier file was accepted"),
        };
        assert!(refusal.contains("entry 1"), "{refusal}");
        assert!(!refusal.contains(marker), "{refusal}");
    }

    #[test]
    fn service_account_store_setting_has_no_legacy_alias() {
        assert_eq!(
            service_account_store_path(Some(" /srv/accounts.json "))
                .expect("the canonical setting is valid"),
            Some("/srv/accounts.json".to_owned()),
        );
        assert_eq!(
            service_account_store_path(None).expect("an unset canonical setting is valid"),
            None,
        );
    }

    /// **X-59's failing-first test.** The flag is one declaration carrying both axes of the local
    /// deployment: who the human is and that every address belongs to the one `dev` tenant. The
    /// explicit-roster control keeps the existing multi-tenant development path intact.
    #[test]
    fn dev_declares_the_startup_user_and_one_dev_tenant() {
        assert!(requests_development(["--dev"]));
        assert!(requests_development(["ignored-before", "--dev"]));
        assert!(!requests_development(["--development"]));

        let dev = Startup::select(true, false, Some("timo"), None)
            .expect("a startup user makes the development shorthand usable");
        assert_eq!(dev.deployment(), Deployment::SingleTenant);
        assert_eq!(dev.development_roster(), Some("user:timo@dev"));

        let explicit = Startup::select(true, true, Some("timo"), None)
            .expect("an explicit development roster remains authoritative");
        assert_eq!(explicit.deployment(), Deployment::MultiTenant);
        assert_eq!(explicit.development_roster(), None);
    }

    /// The production declaration is independent from the provider: both OIDC/local users and an
    /// explicit development roster reach the same runtime and identity-boundary policy.
    #[test]
    fn one_tenant_is_selected_independently_from_authentication() {
        let hosted = Startup::select(false, false, None, Some("acme"))
            .expect("a provider-independent tenant");
        assert_eq!(hosted.deployment(), Deployment::SingleTenant);
        assert_eq!(
            hosted.tenancy().tenant().map(|tenant| tenant.as_str()),
            Some("acme")
        );
        assert_eq!(hosted.development_roster(), None);

        let rostered = Startup::select(true, true, Some("ignored"), Some("acme"))
            .expect("an explicit roster may use the same independent declaration");
        assert_eq!(rostered.deployment(), Deployment::SingleTenant);
        assert_eq!(rostered.development_roster(), None);

        let refusal = Startup::select(true, false, Some("timo"), Some("other"))
            .expect_err("--dev always means dev and must not be silently redefined")
            .to_string();
        assert!(refusal.contains(DEV_FLAG), "{refusal}");
        assert!(refusal.contains(TENANT_SETTING), "{refusal}");
    }

    /// A missing startup user is a named refusal, not a silently anonymous development host and
    /// not a made-up default credential shared by every checkout.
    #[test]
    fn dev_refuses_when_the_startup_user_cannot_be_named() {
        for user in [None, Some("")] {
            let message = Startup::select(true, false, user, None)
                .expect_err("the shorthand needs a real startup user")
                .to_string();
            assert!(message.contains(DEV_FLAG), "{message}");
            assert!(message.contains(USER_ENV), "{message}");
            assert!(message.contains(DEV_IDENTITY_ENV), "{message}");
        }
    }

    /// The shortcut reaches the guard as an ordinary principal whose tenant was fixed at startup.
    /// Hostile request fields are present so this pins X-59's tenant-vector acceptance against the
    /// new entry point rather than inheriting X-03's roster test by assertion.
    #[tokio::test]
    async fn dev_resolves_the_startup_user_to_dev_and_no_request_can_rename_it() {
        let startup =
            Startup::select(true, false, Some("timo"), None).expect("a development startup");
        let state = compose_identity(&startup).expect("the implied roster is valid");
        let mut service = routes::app(state).into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method("POST")
            .uri("/api/session?tenant=attacker")
            .header("Authorization", "Bearer timo")
            .header("X-Tenant", "attacker")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"tenant":"attacker"}"#))
            .expect("a well-formed request");
        let response = service.call(request).await.expect("an infallible router");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a response body");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("the session response is JSON");

        assert_eq!(body["principal"]["id"], "timo", "{body}");
        assert_eq!(body["principal"]["kind"], "user", "{body}");
        assert_eq!(body["principal"]["tenant"], "dev", "{body}");
        assert!(!body.to_string().contains("attacker"), "{body}");
    }

    /// The `--dev` shorthand knows there is exactly one local principal, so the browser path must
    /// complete the same session exchange as the bearer API without asking a human to construct an
    /// Authorization header. The response carries the token only as an HttpOnly cookie.
    #[tokio::test]
    async fn dev_signin_is_a_real_browser_action_and_not_an_instruction_page() {
        let startup =
            Startup::select(true, false, Some("timo"), None).expect("a development startup");
        let state = compose_identity(&startup).expect("the implied roster is valid");
        let app = routes::app(state);

        let page = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/signin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let page_body = axum::body::to_bytes(page.into_body(), usize::MAX)
            .await
            .unwrap();
        let page_body = String::from_utf8(page_body.to_vec()).unwrap();
        assert!(page_body.contains("<form"), "{page_body}");
        assert!(page_body.contains("method=\"post\""), "{page_body}");

        let signed_in = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/signin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(signed_in.status(), StatusCode::OK);
        let planted = signed_in
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        let cookie = planted.split(';').next().unwrap().to_owned();
        let signed_in_body = axum::body::to_bytes(signed_in.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&signed_in_body).contains("token"));

        let session = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/session")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);
        let session_body = axum::body::to_bytes(session.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_slice(&session_body).unwrap();
        assert_eq!(session["principal"]["id"], "timo");
        assert_eq!(session["principal"]["tenant"], "dev");
    }

    /// Single-tenant changes runtime admission, never address layout. Rendering the default tenant
    /// literally pins the migration property: the same `Tenant("dev")` under the existing
    /// multi-tenant composition looks in exactly this location after `--dev` is removed.
    #[test]
    fn dev_credentials_keep_the_multi_tenant_address_layout() {
        const CREDENTIALS: &[exchange_host::DeclaredCredential<'static>] =
            &[exchange_host::DeclaredCredential {
                name: "zendesk.api_token",
                leaf: "api_token",
            }];
        let declaration = exchange_host::ConnectorDeclaration {
            connector: "zendesk",
            authority: Some("com.zendesk.api"),
            credentials: CREDENTIALS,
        };
        let tenant = exchange_host::Tenant::new("dev").expect("the fixed tenant is addressable");
        let reference = declaration
            .address_of(&tenant, "zendesk.api_token")
            .expect("a declared credential");

        assert_eq!(
            exchange_host::address_path(&reference),
            "tenants/dev/com.zendesk.api/api_token",
        );
        assert_eq!(reference.tenant(), "dev");
    }

    /// Send a whole request.
    ///
    /// Spelled against tokio's readiness API rather than `AsyncWriteExt` because this crate does not
    /// carry tokio's `io-util` feature, and the manifest is not this story's to change.
    async fn send(stream: &TcpStream, mut remaining: &[u8]) {
        while !remaining.is_empty() {
            stream
                .writable()
                .await
                .expect("the socket becomes writable");

            match stream.try_write(remaining) {
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == ErrorKind::WouldBlock => continue,
                Err(error) => panic!("writing the request failed: {error}"),
            }
        }
    }

    /// Read until the server closes the connection, which `Connection: close` makes it do.
    async fn receive(stream: &TcpStream) -> String {
        let mut received = Vec::new();
        let mut chunk = [0_u8; 1024];

        loop {
            stream
                .readable()
                .await
                .expect("the socket becomes readable");

            match stream.try_read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => received.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => continue,
                Err(error) => panic!("reading the response failed: {error}"),
            }
        }

        String::from_utf8_lossy(&received).into_owned()
    }

    /// End to end over a real socket. The router tests prove what the surface answers; this proves
    /// the process listens where the default says it does, and that a plain HTTP request to it gets
    /// an answer.
    ///
    /// Port `0` rather than the default `8080`: the address under test is the *interface*, and a
    /// fixed port would make this test fail whenever anything else on the machine holds it.
    #[tokio::test]
    async fn health_answers_over_a_socket_on_the_default_interface() {
        let bind = SocketAddr::new(DEFAULT_BIND.ip(), 0);
        assert!(bind.ip().is_loopback(), "the default bind must be loopback");

        let listener = TcpListener::bind(bind).await.expect("loopback is bindable");
        let local = listener
            .local_addr()
            .expect("a bound listener has an address");
        let server = tokio::spawn(async move {
            axum::serve(listener, routes::app(AppState::without_identity())).await
        });

        let stream = TcpStream::connect(local)
            .await
            .expect("the server is listening");
        send(
            &stream,
            format!("GET /health HTTP/1.1\r\nHost: {local}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await;

        let response = receive(&stream).await;

        server.abort();

        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains(r#""status":"ok""#), "{response}");
    }

    /// Drive one anonymous `GET` through a fully assembled app and report what it answered.
    ///
    /// The same shape as `routes::tests::anonymous_get`, over an [`AppState`] this module composed
    /// rather than one written out in the test — which is the whole point of it being here.
    async fn anonymous_get(state: AppState, path: &str) -> StatusCode {
        let mut service = routes::app(state).into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .expect("a well-formed request");

        service
            .call(request)
            .await
            .expect("a router is infallible")
            .status()
    }

    /// **X-23.** A cleartext endpoint is refused, and the rest of the host goes on serving.
    ///
    /// This is where "the refusal is a `ConfigRefusal`" stops being a type and becomes a behaviour.
    /// `oidc::config`'s module documentation requires that an OIDC problem cost the operator sign-in
    /// and nothing else — killing the process would take `/health` and the catalogue down to punish
    /// somebody who mistyped a scheme — and [`compose_oidc`] is the only place that promise is kept
    /// or broken. It composes a state; there is no exit path through it.
    ///
    /// The chain has two links and this is the second. That a cleartext authorization endpoint
    /// produces exactly this refusal is
    /// `oidc::config::tests::every_transport_checked_variable_is_actually_enforced_and_no_other`'s
    /// claim, asserted there because `OidcConfig::read` is private to that module.
    #[tokio::test]
    async fn a_cleartext_endpoint_refusal_keeps_the_rest_of_the_host_serving() {
        let state = compose_oidc(Err(ConfigRefusal::InsecureEndpoint {
            insecure: vec![AUTHORIZATION_ENDPOINT_ENV],
        }));

        for (path, expected) in [
            ("/health", StatusCode::OK),
            ("/api/catalogue/connectors", StatusCode::OK),
            // Sign-in is the one thing that is gone, and it says so rather than sending a browser
            // to a provider over a channel this host refused.
            ("/api/signin", StatusCode::SERVICE_UNAVAILABLE),
        ] {
            assert_eq!(
                anonymous_get(state.clone(), path).await,
                expected,
                "for {path}",
            );
        }
    }

    /// The bind an operator gets when they set nothing.
    #[test]
    fn the_default_is_used_when_nothing_is_configured() {
        // Only meaningful with the variable unset, which is the state a test process runs in unless
        // something set it; assert the branch rather than mutating the process environment, which
        // would race the other tests in this binary.
        assert!(
            std::env::var(BIND_ENV).is_err(),
            "{BIND_ENV} must not be set"
        );
        assert_eq!(
            configured_bind().expect("an unset variable falls back to the default"),
            DEFAULT_BIND,
        );
    }
}
