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

mod agent;
mod audit;
mod bind;
mod connection_guard;
mod dev_identity;
mod entropy;
mod execution;
mod oidc;
mod routes;
mod session;
mod state;
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

use crate::agent::{AgentStore, AGENT_STORE_SETTING};
use crate::bind::{admit_bind, StartupRefusal, BIND_ENV, DEFAULT_BIND};
use crate::dev_identity::{DevIdentity, DEV_IDENTITY_ENV};
use crate::execution::invoker;
use crate::oidc::config::{ConfigRefusal, OidcConfig};
use crate::oidc::http_exchange::HttpTokenExchange;
use crate::oidc::Oidc;
use crate::state::AppState;

/// The binary flag that declares the zero-configuration, one-tenant development composition.
const DEV_FLAG: &str = "--dev";

/// The conventional startup-user setting from which [`DEV_FLAG`] names its one human.
const USER_ENV: &str = "USER";

/// Startup choices that must agree across identity and runtime admission.
#[derive(Debug, PartialEq, Eq)]
enum Startup {
    /// The existing composition: an explicit roster or OIDC may resolve several tenants, and local
    /// runtimes are refused.
    MultiTenant,
    /// One loopback-only development principal, fixed to the one `dev` tenant at startup.
    Development { roster: String },
}

impl Startup {
    /// Read the process arguments and environment once, before any port is composed.
    fn configured() -> Result<Self, StartupRefusal> {
        let requested = requests_development(std::env::args_os().skip(1));
        let explicit_roster = std::env::var_os(DEV_IDENTITY_ENV).is_some();
        let user = std::env::var(USER_ENV).ok();

        Self::select(requested, explicit_roster, user.as_deref())
    }

    /// Select a startup shape from already-read inputs, so tests never race over process state.
    fn select(
        requested: bool,
        explicit_roster: bool,
        user: Option<&str>,
    ) -> Result<Self, StartupRefusal> {
        // An explicit roster is the operator's more precise declaration. Do not overwrite it or
        // silently collapse principals from several tenants into `dev`.
        if explicit_roster || !requested {
            return Ok(Self::MultiTenant);
        }

        let Some(user) = user.filter(|user| !user.is_empty()) else {
            return Err(StartupRefusal::DevelopmentMode {
                reason: format!(
                    "{DEV_FLAG} cannot name its local principal because {USER_ENV} is unset or \
                     empty. Set {USER_ENV}, or configure {DEV_IDENTITY_ENV} explicitly"
                ),
            });
        };

        Ok(Self::Development {
            roster: format!("user:{user}@dev"),
        })
    }

    /// The runtime class selected by this startup declaration.
    const fn deployment(&self) -> Deployment {
        match self {
            Self::MultiTenant => Deployment::MultiTenant,
            Self::Development { .. } => Deployment::SingleTenant,
        }
    }

    /// An implied development roster, or `None` when the normal identity configuration applies.
    fn development_roster(&self) -> Option<&str> {
        match self {
            Self::MultiTenant => None,
            Self::Development { roster } => Some(roster),
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

/// Start the server, or refuse and say why.
async fn serve() -> Result<(), StartupRefusal> {
    let startup = Startup::configured()?;
    let state = compose(&startup)?;
    let bind = configured_bind()?;

    // Before the socket, not after: a listener that opens and then closes has still been open.
    admit_bind(bind, state.identity_binding())?;

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
/// agent store.
///
/// Four ports, all independent, and every one of them defaults to *bound to nothing* with no
/// fallback. The argument is the same in each case and it is the one that decides the shape: a
/// safety property that depends on a setting staying at its default is one setting away from gone,
/// so nothing here has to be turned *off* to be safe — only turned on to be useful.
///
/// The development identity is armed by [`DEV_FLAG`] or [`DEV_IDENTITY_ENV`]; failing that, OIDC
/// sign-in is offered if it was configured; the credential store and the agent store are bound by
/// their own settings. Unset and unconfigured binds nothing, which is the state a reachable bind is
/// already refused in.
///
/// The development identity is checked first and wins. An operator who armed a roster is working
/// locally, and quietly federating instead would be the more surprising of the two.
fn compose(startup: &Startup) -> Result<AppState, StartupRefusal> {
    let mut state = compose_identity(startup)?;

    // Bound before the invoker, because the invoker reads it. A composition with no settings store
    // still builds one — it gets an empty configuration, which is X-12's behaviour: the connectors
    // that need nothing per connection run, and the ones that do refuse by name.
    let settings = settings_store()?;
    if let Some(store) = settings.clone() {
        state = state.with_settings(store);
    }

    // Bound before the invoker, because the invoker requires it. **No grant store, no invoker** —
    // see `grant_store`: an invoker built without one could only be built by choosing what to do in
    // its absence, and the only available choice is to admit everything.
    let grants = grant_store()?;

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
            let configuration = settings.map_or_else(
                || {
                    Arc::new(exchange_host::MemoryConfig::new())
                        as Arc<dyn exchange_host::ConfigStore>
                },
                |store| store as Arc<dyn exchange_host::ConfigStore>,
            );

            state = state.with_invoker(Arc::new(
                invoker(startup.deployment(), store.clone(), configuration, grants)
                    .map_err(|reason| StartupRefusal::Invoker { reason })?,
            ));
        }

        // Bound whether or not an invoker was, and that combination is a real composition rather
        // than an oversight: a host with credentials and no grants is one an operator can connect a
        // vendor to and nobody can run anything on, which is the honest state to be in on the way
        // to granting something.
        state = state.with_credentials(store);
    }
    if let Some(store) = agent_store()? {
        state = state.with_agents(store);
    }
    if let Some((workflows, pure, runs)) = workflow_store()? {
        state = state.with_workflows(workflows, pure, runs);
    }

    Ok(state)
}

/// Bind workflow definitions and the audited pure cognition pack, or bind neither.
#[cfg(unix)]
fn workflow_store() -> Result<
    Option<(
        Arc<exchange_host::WorkflowStore>,
        Arc<exchange_host::PureEditorTools>,
        Arc<crate::workflow_runs::WorkflowRunStore>,
    )>,
    StartupRefusal,
> {
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
fn workflow_store() -> Result<
    Option<(
        Arc<exchange_host::WorkflowStore>,
        Arc<exchange_host::PureEditorTools>,
        Arc<crate::workflow_runs::WorkflowRunStore>,
    )>,
    StartupRefusal,
> {
    Ok(None)
}

/// Bind the agent store the environment names, or bind none.
///
/// The same three states as the credential store, for the same reason and with one difference worth
/// naming. **Unset binds nothing**, and `POST /api/agents` then refuses with `503` quoting this
/// setting — which is not the in-memory fallback X-09 refuses, because nothing is served from
/// somewhere else: the host says it cannot hold the record, and a token it could not record is one
/// nobody could ever revoke. **Set and unusable refuses to start**, since a store the operator named
/// and this process could not open is a mistake with no later moment at which it announces itself —
/// and, here, one of the ways it can be unusable is a mode that would let somebody else plant a
/// verifier, which is an authentication bypass rather than an inconvenience. See `crate::agent`.
///
/// Not `#[cfg(unix)]`, unlike the credential store. What protects a *credential* in that file is the
/// mode and nothing else, so a platform that cannot spell one gets no store at all; what protects an
/// agent token here is that the store holds a digest rather than the token, which holds on every
/// platform. The mode still matters — it is what stops a planted verifier — and `crate::agent`
/// states plainly what is lost where it cannot be checked.
fn agent_store() -> Result<Option<Arc<AgentStore>>, StartupRefusal> {
    let Ok(configured) = std::env::var(AGENT_STORE_SETTING) else {
        warn!(
            "no agent store is bound ({AGENT_STORE_SETTING} is unset), so minting an agent token \
             will refuse. Set it to a path to let this host mint tokens for the agents that call \
             it",
        );
        return Ok(None);
    };

    let store =
        AgentStore::open(configured.trim()).map_err(|source| StartupRefusal::AgentStore {
            reason: source.to_string(),
        })?;

    // Read back off the bound store, so this line cannot name a file this process did not open.
    info!("{}", store.banner());

    Ok(Some(Arc::new(store)))
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

    Ok(AppState::with_development_identity(Arc::new(dev)))
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
    use axum::http::{Request as HttpRequest, StatusCode};
    use tokio::net::TcpStream;
    use tower::Service;

    use crate::oidc::config::AUTHORIZATION_ENDPOINT_ENV;

    /// **X-59's failing-first test.** The flag is one declaration carrying both axes of the local
    /// deployment: who the human is and that every address belongs to the one `dev` tenant. The
    /// explicit-roster control keeps the existing multi-tenant development path intact.
    #[test]
    fn dev_declares_the_startup_user_and_one_dev_tenant() {
        assert!(requests_development(["--dev"]));
        assert!(requests_development(["ignored-before", "--dev"]));
        assert!(!requests_development(["--development"]));

        let dev = Startup::select(true, false, Some("timo"))
            .expect("a startup user makes the development shorthand usable");
        assert_eq!(dev.deployment(), Deployment::SingleTenant);
        assert_eq!(dev.development_roster(), Some("user:timo@dev"));

        let explicit = Startup::select(true, true, Some("timo"))
            .expect("an explicit development roster remains authoritative");
        assert_eq!(explicit.deployment(), Deployment::MultiTenant);
        assert_eq!(explicit.development_roster(), None);
    }

    /// A missing startup user is a named refusal, not a silently anonymous development host and
    /// not a made-up default credential shared by every checkout.
    #[test]
    fn dev_refuses_when_the_startup_user_cannot_be_named() {
        for user in [None, Some("")] {
            let message = Startup::select(true, false, user)
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
        let startup = Startup::select(true, false, Some("timo")).expect("a development startup");
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
