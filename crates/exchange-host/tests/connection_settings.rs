//! **A connector with a templated host can actually be invoked** (X-47).
//!
//! X-12 made this host execute and immediately exposed that seventeen of the fifty-three shipped
//! connectors cannot be invoked at all: each needs a per-connection value — a vendor subdomain, a
//! workspace slug, the non-secret half of a Basic credential — and there was **nowhere for a tenant
//! to put one**. The invoker bound an empty `MemoryConfig`, so those connectors refused by name. The
//! refusal was right; the surface still ran thirty-six of fifty-three.
//!
//! Thirteen of the seventeen are made configurable here. **Four are refused on purpose** — see
//! [`a_setting_cannot_become_the_destination_authority`], which is the whole of why.
//!
//! This file is the whole of that claim, driven the way `invoke.rs` drives its own: through a
//! transport that records instead of sending, so "the request went to the origin this tenant
//! configured" and "the refusal dispatched nothing" are counts rather than sentences.
//!
//! # What it does *not* assert
//!
//! That configuration is safe to accept. It is not this host's job to decide whether a subdomain
//! composes into a hostname — `connector-pack` decides that at the one substitution point, and
//! [`no_setting_can_move_the_destination_host`] pins that the refusal arrives here rather than being
//! something this repository would have had to invent. That distinction is the whole of
//! `AGENTS.md`'s "this host constructs no request of its own": a host that validated hosts would be
//! a host that composes them.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use exchange_host::{
    declared_settings, host_pinning, ConnectionSettings, ConnectorDeclaration, Contexts,
    DeclaredCredential, DeclaredSetting, Deployment, Egress, Grant, GrantRefusal, Grants,
    HostPinning, InvokeRefusal, Invoker, Principal, PrincipalKind, Secret, SecretStore, Selector,
    Sent, SettingKind, SettingsStore, Tenant, MAX_SETTING_VALUE_BYTES,
};
use flux_runtime::{Tool, ToolContext};
use serde_json::{json, Value};

/// The tenant every test invokes as. Which one it is does not matter; that it comes off the
/// resolved principal is the whole point.
const TENANT: &str = "acme";

/// A credential value long enough for flux's redactor to hold, carrying none of flux's known
/// credential prefixes — `invoke.rs`'s sentinel, and kept identical for its reason.
const SENTINEL: &str = "quiggle-marrow-plimth-42";

// ---------------------------------------------------------------------------------------------
// The composition: a counting transport, a scratch store, and a caller
// ---------------------------------------------------------------------------------------------

/// What the transport was asked to carry, and how often.
#[derive(Default)]
struct Wire {
    calls: Vec<Value>,
}

impl Wire {
    /// The origin — scheme and authority — of the `n`th recorded URL.
    fn origin(&self, n: usize) -> String {
        let url = self.calls[n]["url"]
            .as_str()
            .expect("`http.request` is always called with a url");
        let after_scheme = url.find("://").expect("an absolute url") + 3;
        let end = url[after_scheme..]
            .find('/')
            .map_or(url.len(), |offset| after_scheme + offset);
        url[..end].to_owned()
    }

    fn count(&self) -> usize {
        self.calls.len()
    }
}

/// An egress that answers with a fixed body and records what it was asked to carry.
///
/// The spec is borrowed from a real `flux_web::http::HttpRequestTool` — built and never executed —
/// so this fake advertises the identical contract the live transport does.
fn silent_egress() -> (Arc<Mutex<Wire>>, Egress) {
    let wire = Arc::new(Mutex::new(Wire::default()));
    let spec = flux_web::http::HttpRequestTool::new(&flux_web::WebOptions::default()).spec();

    let recorded = wire.clone();
    let tool = flux_runtime::tool_fn(spec, move |params: Value| {
        let recorded = recorded.clone();
        async move {
            recorded.lock().expect("the wire lock").calls.push(params);
            Ok(json!({ "status": 200, "headers": {}, "body": { "ok": true } }))
        }
    });

    (wire, Egress::new(tool))
}

/// An egress that echoes the request's own headers back in its body, and records what it carried.
///
/// `invoke.rs` keeps one for the reason this file needs one: it is how "the credential stayed off
/// the wire" is told apart from "the credential stayed off the *response*". A placed credential is
/// in the recorded params either way, which is what
/// [`a_setting_cannot_become_the_destination_authority`] asserts against.
fn echoing_egress() -> (Arc<Mutex<Wire>>, Egress) {
    let wire = Arc::new(Mutex::new(Wire::default()));
    let spec = flux_web::http::HttpRequestTool::new(&flux_web::WebOptions::default()).spec();

    let recorded = wire.clone();
    let tool = flux_runtime::tool_fn(spec, move |params: Value| {
        let recorded = recorded.clone();
        let echoed = params.get("headers").cloned().unwrap_or(Value::Null);
        async move {
            recorded.lock().expect("the wire lock").calls.push(params);
            Ok(json!({ "status": 200, "headers": {}, "body": { "you_sent": echoed } }))
        }
    });

    (wire, Egress::new(tool))
}

/// A fresh [`ToolContext`] per invocation, over a workspace nothing writes to.
fn contexts() -> Arc<dyn Contexts> {
    Arc::new(|| {
        let workspace = flux_system::Workspace::new(std::env::temp_dir())
            .expect("the temp directory is a usable workspace root");
        ToolContext::new(Arc::new(flux_system::System::new(workspace)))
    })
}

/// A scratch directory under the system temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "exchange-host-settings-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("a scratch directory");
        Self(path.canonicalize().expect("a resolvable scratch directory"))
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A settings store of this host's own, at a fresh path.
fn settings_store(label: &str) -> (Scratch, Arc<SettingsStore>) {
    let scratch = Scratch::new(label);
    let store = SettingsStore::bind(scratch.join("state").join("settings"))
        .expect("a fresh settings store");
    (scratch, Arc::new(store))
}

/// Zendesk's declaration, as the address of its credential is derived from.
fn zendesk_declaration() -> ConnectorDeclaration<'static> {
    ConnectorDeclaration {
        connector: "zendesk",
        authority: Some("com.zendesk.api"),
        credentials: &[DeclaredCredential {
            name: "zendesk.api_token",
            leaf: "api_token",
        }],
    }
}

/// A credential store holding zendesk's api token for [`TENANT`].
async fn zendesk_credentials() -> Arc<dyn SecretStore> {
    let store = Arc::new(connector_pack::MemoryStore::new());
    let reference = zendesk_declaration()
        .address_of(&tenant(), "zendesk.api_token")
        .expect("zendesk declares an authority and this credential");
    store
        .put(&reference, &Secret::new(SENTINEL))
        .await
        .expect("a memory store accepts a write");
    store
}

fn tenant() -> Tenant {
    Tenant::new(TENANT).expect("`acme` is a usable tenant")
}

/// The caller. An agent, because agents are what call operations all day.
fn caller() -> Principal {
    Principal::new(PrincipalKind::Agent, "triage-bot", tenant())
}

/// One of zendesk's declared settings, by its `binds` spelling.
fn zendesk_setting(binds: &str) -> DeclaredSetting {
    setting_of(zendesk(), binds)
}

fn zendesk() -> &'static connector_catalog::Provider {
    provider("zendesk")
}

/// One catalogue provider by id.
fn provider(id: &str) -> &'static connector_catalog::Provider {
    connector_catalog::provider(connector_catalog::ProviderKey::id(id))
        .unwrap_or_else(|| panic!("the catalogue carries `{id}`"))
}

/// One of a connector's declared settings, by its `binds` spelling.
fn setting_of(provider: &'static connector_catalog::Provider, binds: &str) -> DeclaredSetting {
    declared_settings(provider)
        .expect("the connector's operations rehearse")
        .into_iter()
        .find(|declared| declared.binds() == binds)
        .unwrap_or_else(|| panic!("`{}` declares `{binds}`", provider.id))
}

/// A credential store holding [`SENTINEL`] at `credential`'s address under `authority`, or holding
/// nothing when the connector declares no credential.
async fn credential_store(
    authority: &'static str,
    credential: Option<(&'static str, &'static str)>,
) -> Arc<dyn SecretStore> {
    let store = Arc::new(connector_pack::MemoryStore::new());

    if let Some((name, leaf)) = credential {
        let credentials = [DeclaredCredential { name, leaf }];
        let declaration = ConnectorDeclaration {
            connector: "fixture",
            authority: Some(authority),
            credentials: &credentials,
        };
        let reference = declaration
            .address_of(&tenant(), name)
            .expect("the connector declares an authority and this credential");
        store
            .put(&reference, &Secret::new(SENTINEL))
            .await
            .expect("a memory store accepts a write");
    }

    store
}

/// Grants held for every tenant that asks, so a test about *configuration* observes the refusal it
/// is about.
///
/// This file is not about the grant gate — `tests/invoke.rs` and `grant.rs` are — and a tenant
/// holding nothing would refuse every invocation below at a gate one step earlier than the one
/// under test. What it grants is deliberately wide and deliberately still connector-scoped: a
/// wildcard connector is a thing [`Grant`] does not have, and a test helper is not the place to
/// invent one.
struct Granting(Vec<Grant>);

impl Granting {
    /// Everything the connectors in this file publish.
    fn everything() -> Self {
        Self(
            connector_catalog::providers()
                .iter()
                .map(|provider| Grant::for_connector(provider.id, Selector::any()))
                .collect(),
        )
    }
}

impl Grants for Granting {
    fn held(&self, _: &Tenant) -> Vec<Grant> {
        self.0.clone()
    }

    fn set(&self, _: &Tenant, _: &[Grant]) -> Result<(), GrantRefusal> {
        unreachable!("no test in this file edits a grant")
    }
}

/// An invoker over one credential store, one settings store and a recording egress.
fn invoker(
    credentials: Arc<dyn SecretStore>,
    settings: Arc<SettingsStore>,
    egress: Egress,
) -> Invoker {
    Invoker::new(
        Deployment::MultiTenant,
        egress,
        credentials,
        settings,
        Arc::new(Granting::everything()),
        contexts(),
    )
}

/// Run `zendesk-ticket-show` and hand back the wire and the outcome.
async fn show_a_ticket(
    credentials: Arc<dyn SecretStore>,
    settings: Arc<SettingsStore>,
) -> (
    Arc<Mutex<Wire>>,
    Result<exchange_host::Invocation, InvokeRefusal>,
) {
    let (wire, egress) = silent_egress();
    let outcome = invoker(credentials, settings, egress)
        .invoke(
            &caller(),
            "zendesk-ticket-show",
            json!({ "ticket_id": "1" }),
        )
        .await;
    (wire, outcome)
}

// ---------------------------------------------------------------------------------------------
// The Acceptance's first line
// ---------------------------------------------------------------------------------------------

/// **The failing-first test.** A connector whose `base_url` is templated is refused before its
/// tenant can supply the value, and runs once the tenant has.
///
/// Every state is asserted against the **wire**, because that is the only place the claim can be
/// checked rather than believed: each refusal must leave the count at zero, and the success must
/// carry the origin this tenant's own value composed — `https://acme.zendesk.com`, not
/// `https://{subdomain}.zendesk.com` and not anybody else's.
///
/// All three states run against the **same** store, so what makes the difference is the write in
/// between and nothing about how the invokers were built.
///
/// Zendesk needs **two** values, and the order the pack refuses them in is its own: the credential's
/// user half is resolved before the endpoint is substituted, so an unconfigured connection reports
/// the user half first and the subdomain only once that is supplied. Both are driven here rather
/// than only the second, because "supply the one field it named and it still refuses" is exactly
/// the experience a half-built version of this story would produce.
#[tokio::test]
async fn a_templated_connector_is_invoked_once_its_tenant_supplies_the_value() {
    let (_scratch, settings) = settings_store("invoked");
    let credentials = zendesk_credentials().await;

    // Nothing supplied. It refuses by name and dispatches nothing — the state the whole of X-12's
    // finding describes, and the one this story exists to make escapable.
    let (wire, outcome) = show_a_ticket(credentials.clone(), settings.clone()).await;
    let refusal = outcome.expect_err("nothing supplies zendesk's connection values yet");
    let message = refusal.to_string();
    assert!(
        message.contains("zendesk.api_token"),
        "the refusal must name the value an operator has to go and supply: {message}",
    );
    assert_eq!(refusal.sent(), Sent::No);
    assert!(!refusal.retryable());
    assert_eq!(
        wire.lock().expect("the wire lock").count(),
        0,
        "a refusal must leave nothing dispatched",
    );

    // The user half of zendesk's Basic credential: an account name, not a secret. The token it is
    // joined with is in the credential store, and stays there.
    settings
        .set(
            &tenant(),
            "zendesk",
            &zendesk_setting("username.zendesk.api_token"),
            "ops@acme.test",
        )
        .expect("a declared setting of a declared service");

    // Still refused, and now by the name this story is about: the `{subdomain}` its base URL is
    // templated on. Supplying one value does not weaken the refusal for the other.
    let (wire, outcome) = show_a_ticket(credentials.clone(), settings.clone()).await;
    let refusal = outcome.expect_err("nothing supplies zendesk's subdomain yet");
    let message = refusal.to_string();
    assert!(
        message.contains("endpoint.subdomain"),
        "the refusal must name the field an operator has to go and supply: {message}",
    );
    assert!(
        message.contains("zendesk"),
        "and the connector it belongs to: {message}",
    );
    assert_eq!(refusal.sent(), Sent::No);
    assert_eq!(wire.lock().expect("the wire lock").count(), 0);

    // The value with no home until this story: the vendor subdomain.
    settings
        .set(
            &tenant(),
            "zendesk",
            &zendesk_setting("endpoint.subdomain"),
            "acme",
        )
        .expect("a declared setting of a declared service");

    // The same operation runs, and the origin is the one this tenant's value composed.
    let (wire, outcome) = show_a_ticket(credentials, settings).await;
    let invocation = outcome.expect("a supplied subdomain, a stored credential, and one operation");

    assert_eq!(invocation.operation, "zendesk-ticket-show");
    assert!(!invocation.is_error);

    let wire = wire.lock().expect("the wire lock");
    assert_eq!(wire.count(), 1, "exactly one dispatch, for one invoke");
    assert_eq!(
        wire.origin(0),
        "https://acme.zendesk.com",
        "the origin is composed from this tenant's own configured value",
    );
}

/// A value one tenant supplied is not a value another tenant holds.
///
/// The address every setting lives at carries the tenant, and the tenant comes off the resolved
/// principal — so this is the settings half of the property `connections.rs` asserts for
/// credentials, made where it is decided rather than only over HTTP.
#[tokio::test]
async fn a_setting_belongs_to_one_tenant() {
    let (_scratch, settings) = settings_store("tenants");
    let globex = Tenant::new("globex").expect("a plain tenant id");

    settings
        .set(
            &tenant(),
            "zendesk",
            &zendesk_setting("endpoint.subdomain"),
            "acme",
        )
        .expect("a declared setting");

    assert!(settings.is_set(&tenant(), "zendesk", &zendesk_setting("endpoint.subdomain")));
    assert!(
        !settings.is_set(&globex, "zendesk", &zendesk_setting("endpoint.subdomain")),
        "one tenant's subdomain must not be another tenant's",
    );

    // And the store answers the pack the same way, at the address the pack composes.
    use exchange_host::ConfigStore as _;
    assert_eq!(
        settings.get(
            TENANT,
            "zendesk",
            "default",
            exchange_host::Field::Endpoint("subdomain")
        ),
        Some("acme".to_owned()),
    );
    assert_eq!(
        settings.get(
            "globex",
            "zendesk",
            "default",
            exchange_host::Field::Endpoint("subdomain")
        ),
        None,
    );
}

/// A setting outlives the process that wrote it.
///
/// The same argument `CredentialStore` makes for having no in-memory fallback, and it applies here
/// with one difference worth naming: a lost subdomain is not a lost secret, it is a connector that
/// silently stops resolving. An operator who restarts this host and finds thirteen connectors
/// refusing again has a durability bug reported to them as a configuration one.
#[test]
fn a_setting_survives_a_restart() {
    let scratch = Scratch::new("restart");
    let path = scratch.join("state").join("settings");

    let store = SettingsStore::bind(&path).expect("a fresh store");
    store
        .set(
            &tenant(),
            "zendesk",
            &zendesk_setting("endpoint.subdomain"),
            "acme",
        )
        .expect("the write lands");
    drop(store);

    let restarted = SettingsStore::bind(&path).expect("the store reopens");
    assert!(restarted.is_set(&tenant(), "zendesk", &zendesk_setting("endpoint.subdomain")));

    restarted
        .clear(&tenant(), "zendesk", &zendesk_setting("endpoint.subdomain"))
        .expect("the delete lands");
    drop(restarted);

    let restarted = SettingsStore::bind(&path).expect("the store reopens");
    assert!(
        !restarted.is_set(&tenant(), "zendesk", &zendesk_setting("endpoint.subdomain")),
        "a cleared setting must not come back",
    );
}

/// The settings store obeys the credential store's rule about where it may sit — because it is the
/// same rule, shared rather than copied.
///
/// A subdomain is not a credential, and a tenant's list of vendor accounts committed to a repository
/// is still a leak. `crate::paths` is the one walk both stores ask, so this is what says the shared
/// helper is actually wired here rather than only imported.
#[test]
fn a_settings_store_inside_a_working_tree_is_refused() {
    let scratch = Scratch::new("working-tree");
    let checkout = scratch.join("checkout");
    fs::create_dir_all(checkout.join(".git")).expect("a working tree");
    let path = checkout.join("var").join("settings");

    let refused =
        SettingsStore::bind(&path).expect_err("a store inside a checkout must be refused");
    let message = refused.to_string();
    assert!(message.contains("working tree"), "{message}");

    // Refused before anything was created: a store that had already written the directory would
    // have left the exposure it refused.
    assert!(!path.exists());
    assert!(!path.parent().expect("a parent").exists());
}

/// Nothing configured is a startup error naming the setting and an example, not an in-memory
/// fallback.
///
/// X-09's rule, applied to the second store. The consequence differs and the message says so: a
/// lost credential is a lost secret, and a lost subdomain is thirteen connectors that stop resolving.
#[test]
fn an_unconfigured_settings_store_refuses_and_names_what_would_have_worked() {
    for configured in [None, Some(""), Some("   ")] {
        let refused = SettingsStore::bind_configured(configured)
            .expect_err("an unconfigured store must refuse");
        let message = refused.to_string();
        assert!(message.contains("FLUX_EXCHANGE_SETTINGS"), "{message}");
        assert!(message.contains("in-memory"), "{message}");
    }
}

// ---------------------------------------------------------------------------------------------
// Supplying configuration does not become a way to name a host
// ---------------------------------------------------------------------------------------------

/// **The invariant this story is most able to break.** A value supplied through the settings
/// surface cannot move the destination origin.
///
/// Zendesk's origin is `https://{subdomain}.zendesk.com`, composed from a tenant's own value, so
/// this is where a settings surface would hand a caller the host if it were going to. The measured
/// case is the first: `acme.zendesk.com@evil.example` composes
/// `https://acme.zendesk.com@evil.example.zendesk.com/…`, where the `@` turns everything before it
/// into userinfo and the request reaches `evil.example.zendesk.com`.
///
/// **The refusal is `connector-pack`'s and not this host's**, which is the point rather than an
/// accident: the pack validates the composed authority at the one substitution point, and a second
/// opinion here would be a second spelling of one rule. What this asserts is that the refusal
/// arrives, and that nothing is dispatched when it does — the same standard
/// `invoke.rs::no_parameter_can_move_the_destination_host` holds parameters to, applied to the axis
/// this story opened.
#[tokio::test]
async fn no_setting_can_move_the_destination_host() {
    let credentials = zendesk_credentials().await;

    for hostile in [
        "acme.zendesk.com@evil.example",
        "acme.zendesk.com:8443",
        "acme/../evil.example",
        "acme%2eevil.example",
        "evil.example#",
        "acme evil.example",
    ] {
        let (_scratch, settings) = settings_store("hostile");
        settings
            .set(
                &tenant(),
                "zendesk",
                &zendesk_setting("endpoint.subdomain"),
                hostile,
            )
            .expect("this host stores what it is given; the pack decides what may be substituted");
        settings
            .set(
                &tenant(),
                "zendesk",
                &zendesk_setting("username.zendesk.api_token"),
                "ops@acme.test",
            )
            .expect("a declared setting");

        let (wire, outcome) = show_a_ticket(credentials.clone(), settings).await;
        let wire = wire.lock().expect("the wire lock");

        match outcome {
            Ok(_) => assert_eq!(
                wire.origin(0),
                "https://acme.zendesk.com",
                "`{hostile}` moved the origin, which is the confused deputy this host exists to \
                 refuse",
            ),
            Err(refusal) => assert_eq!(
                wire.count(),
                0,
                "`{hostile}` was refused ({refusal}) — and a refusal must leave nothing dispatched",
            ),
        }
    }
}

/// **The rework's failing-first test.** A tenant-supplied setting cannot become the destination
/// authority — measured on the connectors where the property is *not* structurally free.
///
/// `no_setting_can_move_the_destination_host` above drives zendesk, whose template is
/// `{subdomain}.zendesk.com`: a pinned suffix means every composed authority is a zendesk one
/// whatever the value, so the property holds there for free and the test proves nothing about the
/// shape that matters.
///
/// **Four shipped connectors template the whole authority.** `newrelic` declares
/// `hosts: ["{host}"]`, `okta` and `freshdesk` `["{domain}"]`, `docusign` `["{account_host}"]` —
/// there is no literal suffix to keep the origin at the vendor, so `evil.example` is a *valid
/// hostname* and `connector-pack`'s character allow-list admits it. That check constrains the
/// characters of a value, never the identity of the host, and against these four it is vacuous.
///
/// What that bought a caller, before this test: `newrelic-application-list` dispatched to
/// `https://evil.example/v2/applications.json` carrying the tenant's `X-Api-Key`. The writer needs
/// no special standing — the settings route is `Access::Principal`, which admits any kind, and an
/// agent token resolves to one. That is `AGENTS.md`'s *"an agent's token grants access to an
/// operation, never to a credential"*, broken through a configuration field.
///
/// The assertions are ordered so the **dispatch** is checked before the refusal: a run against code
/// that stores the value fails by reporting the origin it reached and the credential it carried,
/// which is the failure worth reading.
#[tokio::test]
async fn a_setting_cannot_become_the_destination_authority() {
    // Every connector whose host template pins no suffix, with an operation and the credential it
    // would have carried. `freshdesk` declares no credential at all — it cannot leak one, and it is
    // driven anyway because "a caller named the host this process connects to" is the same defect
    // with or without a secret attached.
    /// A connector whose host template pins no suffix: what to set, what to run, and the
    /// credential the request would have carried.
    struct Unpinned {
        connector: &'static str,
        binds: &'static str,
        operation: &'static str,
        authority: &'static str,
        /// `(name, leaf)`, or `None` for a connector that declares no credential at all.
        credential: Option<(&'static str, &'static str)>,
    }

    let cases = [
        Unpinned {
            connector: "newrelic",
            binds: "endpoint.host",
            operation: "newrelic-application-list",
            authority: "com.newrelic.api",
            credential: Some(("newrelic.api_key", "api_key")),
        },
        Unpinned {
            connector: "okta",
            binds: "endpoint.domain",
            operation: "okta-user-list",
            authority: "com.okta.api",
            credential: Some(("okta.api_token", "api_token")),
        },
        Unpinned {
            connector: "docusign",
            binds: "endpoint.account_host",
            operation: "docusign-envelope-list",
            authority: "com.docusign.api",
            credential: Some(("docusign.access_token", "access_token")),
        },
        Unpinned {
            connector: "freshdesk",
            binds: "endpoint.domain",
            operation: "freshdesk-ticket-list",
            authority: "com.freshdesk.api",
            credential: None,
        },
    ];

    for case in cases {
        let Unpinned {
            connector,
            binds,
            operation,
            authority,
            credential,
        } = case;
        let (_scratch, settings) = settings_store("authority");
        let provider = provider(connector);
        let declared = setting_of(provider, binds);

        // The attempt. Recorded rather than asserted here, so the wire assertions below are what a
        // failing run reports first.
        let stored = settings.set(&tenant(), connector, &declared, "evil.example");

        // Whatever the store did, supply every *other* value this connector needs, so the
        // invocation below is refused — if it is — by the authority rule and not by an unrelated
        // missing field. Without this a vulnerable build could still refuse for the wrong reason
        // and the test would pass while asserting nothing.
        for other in declared_settings(provider).expect("the connector's operations rehearse") {
            if other != declared {
                let _ = settings.set(&tenant(), connector, &other, "supplied");
            }
        }

        let credentials = credential_store(authority, credential).await;
        let (wire, egress) = echoing_egress();
        let outcome = invoker(credentials, settings, egress)
            .invoke(&caller(), operation, json!({}))
            .await;

        let wire = wire.lock().expect("the wire lock");

        // **The property.** Nothing this process sent went to a host the tenant named.
        for n in 0..wire.count() {
            assert_ne!(
                wire.origin(n),
                "https://evil.example",
                "`{connector}` dispatched to a host the tenant supplied — a caller named the \
                 destination through a configuration field, which is the confused deputy this host \
                 exists to refuse (outcome: {outcome:?})",
            );
        }

        // And the credential is not on the wire at all, wherever it went.
        assert!(
            !format!("{:?}", wire.calls).contains(SENTINEL),
            "`{connector}` put this tenant's credential on a request whose host it did not choose",
        );

        // The store refused the write, which is what keeps the two assertions above true rather
        // than lucky. A build that stored the value and happened not to dispatch is one refactor
        // away from dispatching.
        let refusal = stored.expect_err(
            "a value that is the entire destination authority must not be storable at all",
        );
        let message = refusal.to_string();
        assert!(
            message.contains(connector),
            "the refusal must name the connector: {message}",
        );
        assert!(
            !message.contains("evil.example"),
            "the refusal must not repeat the value it refused: {message}",
        );
    }
}

/// **The `get`-side guard, which until now was held by nothing.**
///
/// The rule is enforced twice — `ConnectionSettings::set` refuses on the way in, and
/// `ConfigStore::get` refuses again on the way out — and the design calls the second one *"the one
/// that matters"*. That is because `set` is not the only way bytes reach the file: an **edited
/// store**, a **backup restored from before the rule existed**, and a **value written by an older
/// build** all arrive without passing it, which is what makes the property belong to the port rather
/// than to one write path.
///
/// Deleting that branch used to leave the whole gate green. Every other test on this axis drives
/// `set`, so all of them were satisfied by the first enforcement point alone and the second could
/// be removed by a refactor with a clean CI. This one therefore never calls `set`: the value reaches
/// the file **the way those three scenarios reach it**, written straight into it by something that
/// is not this store.
///
/// Four things are measured, in the order a failing run should report them: the plant landed, the
/// port refuses it, nothing dispatched, and the file is byte-identical afterwards. The last is
/// *refuse; never repair* — a store that quietly rewrote a file it found suspicious would destroy
/// the evidence of how the value got there, on the one path where somebody has to find that out.
#[tokio::test]
async fn a_planted_whole_authority_value_is_refused_on_the_way_out() {
    use exchange_host::ConfigStore as _;

    let scratch = Scratch::new("planted");
    let path = scratch.join("state").join("settings");
    let newrelic = provider("newrelic");
    let unpinned = setting_of(newrelic, "endpoint.host");

    // The whole of newrelic's declared surface, written into the store file directly — no `set`, no
    // bound, no rule. Every *other* value is supplied too, so that a build without the guard is
    // refused by the guard or by nothing: a connection still missing a field would dispatch nothing
    // for an unrelated reason, and this test would pass while asserting it.
    let mut planted = json!({ TENANT: { "newrelic": {} } });
    for declared in declared_settings(newrelic).expect("newrelic's operations rehearse") {
        let value = if declared == unpinned {
            "evil.example"
        } else {
            "supplied"
        };
        planted[TENANT]["newrelic"][&declared.service][declared.binds()] = json!(value);
    }

    fs::create_dir_all(path.parent().expect("the store path has a parent"))
        .expect("a scratch directory");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&planted).expect("a serialisable store"),
    )
    .expect("a planted store file");
    let on_disk = fs::read(&path).expect("the planted file is readable");

    let settings = Arc::new(
        SettingsStore::bind(&path).expect("the store reopens over a file it did not write"),
    );

    // The bytes really are in the store. Without this, a plant that silently failed would make
    // every assertion below pass for the wrong reason.
    assert!(
        settings.held_bytes(&tenant()) >= "evil.example".len(),
        "nothing was planted, so nothing below is a test of the guard",
    );

    // **The guard.** The port answers the invoker with nothing, for a value that never met `set`.
    assert_eq!(
        settings.get(TENANT, "newrelic", &unpinned.service, unpinned.field()),
        None,
        "a value that is the whole destination authority reached the invoker out of a file `set` \
         never saw — an edited store, a restored backup and an older build all arrive this way",
    );

    // And the same claim as a dispatch, which is the part that costs a tenant its credential.
    let credentials =
        credential_store("com.newrelic.api", Some(("newrelic.api_key", "api_key"))).await;
    let (wire, egress) = echoing_egress();
    let outcome = invoker(credentials, settings, egress)
        .invoke(&caller(), "newrelic-application-list", json!({}))
        .await;

    let wire = wire.lock().expect("the wire lock");
    for n in 0..wire.count() {
        assert_ne!(
            wire.origin(n),
            "https://evil.example",
            "a planted value became the origin this host sent a request to (outcome: {outcome:?})",
        );
    }
    assert!(
        !format!("{:?}", wire.calls).contains(SENTINEL),
        "this tenant's credential went onto a request whose host it did not choose",
    );
    assert_eq!(
        wire.count(),
        0,
        "the connector is unconfigured as far as the port is concerned, so it must refuse by name \
         and dispatch nothing",
    );

    let refusal = outcome.expect_err("a refused value leaves the connection unconfigured");
    assert_eq!(refusal.sent(), Sent::No);

    // Refuse; never repair. The file is exactly what was found.
    assert_eq!(
        fs::read(&path).expect("the file is still readable"),
        on_disk,
        "the store rewrote a file it found a refused value in, destroying the evidence of how the \
         value got there",
    );
}

/// The rule is decided from the catalogue, over the **whole** catalogue — so a connector shipped
/// tomorrow whose host is a bare placeholder is refused without anybody adding it to a list.
///
/// This is the generalisation of the test above, and it is the one that would have caught
/// `freshdesk` and `okta`: the review that found this named `newrelic` and `docusign`, and the
/// measurement finds five. A rule enumerated by hand would have shipped three of them.
///
/// # The fifth arrived by upstream bump, which is the mechanism working (X-67)
///
/// It was four against catalogue 0.9. Moving to 0.10 turned this red rather than quietly widening
/// what a tenant may configure, which is what X-47's design said a host-template change *should*
/// do. `intercom` is the arrival: upstream C-225 made its `base_url` `https://{host}` so an EU or
/// AU workspace can be connected at all, and a bare placeholder is the whole authority. The
/// refusal here is not a disagreement with that change — it is this host declining to let a
/// *tenant* be the one who supplies it.
///
/// `algolia`, the 54th provider, is **not** here: it ships `{app_id}.algolia.net`, which pins two
/// labels and lands in [`HostPinning::PinnedTo`] with the seven that were already there. Worth
/// stating because the estimate this bump was planned against read its template as unpinnable and
/// expected a sixth refusal; the measurement is what decided it, not the estimate.
#[test]
fn no_shipped_connector_lets_a_tenant_supply_its_whole_authority() {
    let mut refused = Vec::new();

    for provider in connector_catalog::providers() {
        for declared in declared_settings(provider).expect("every connector's operations rehearse")
        {
            match host_pinning(provider, &declared) {
                // The value never reaches the authority — it lands in a path or a query, where
                // `connector-pack` holds it to that position's own rule.
                HostPinning::OutsideTheAuthority => {}
                // The composed authority always ends in this literal, so the origin stays the
                // vendor's whatever the tenant supplies.
                HostPinning::PinnedTo(suffix) => assert!(
                    suffix.starts_with('.') && suffix.matches('.').count() >= 2,
                    "`{}` {} claims to pin {suffix:?}, which pins nothing an attacker cannot \
                     register under",
                    provider.id,
                    declared.binds(),
                ),
                HostPinning::WholeAuthority(template) => {
                    refused.push(format!("{}/{} ({template})", provider.id, declared.binds()));
                }
            }
        }
    }

    // Pinned exactly, so that a catalogue change in either direction is a failing test rather than
    // a silent hole: a sixth connector arriving unpinned, or one of these five gaining a suffix
    // upstream and staying refused for no reason. It has already fired once for real — `intercom`
    // is here because catalogue 0.10 moved its host, and this assertion is how anybody found out.
    assert_eq!(
        refused,
        vec![
            "docusign/endpoint.account_host ({account_host})".to_owned(),
            "freshdesk/endpoint.domain ({domain})".to_owned(),
            "intercom/endpoint.host ({host})".to_owned(),
            "newrelic/endpoint.host ({host})".to_owned(),
            "okta/endpoint.domain ({domain})".to_owned(),
        ],
        "the set of connectors a tenant may not configure has changed",
    );
}

/// The refusal is actionable: it says which connectors this host will not let a tenant configure,
/// and why, rather than reading as a bug.
///
/// A smaller working surface beats a larger one that leaks — but only if an operator can tell the
/// difference between "refused on purpose" and "broken".
#[test]
fn a_connector_this_host_will_not_configure_says_so() {
    let (_scratch, settings) = settings_store("unsuppliable");
    let newrelic = provider("newrelic");
    let host = setting_of(newrelic, "endpoint.host");

    let refusal = settings
        .set(&tenant(), "newrelic", &host, "acme.newrelic.com")
        .expect_err("newrelic's host is the whole authority, whatever value is offered");

    let message = refusal.to_string();
    assert!(message.contains("newrelic"), "{message}");
    assert!(message.contains("endpoint.host"), "{message}");
    assert!(
        message.contains("{host}"),
        "the refusal must quote the template that pins nothing, so an operator can see why: \
         {message}",
    );

    // Even a value that really is a New Relic host is refused. The rule is about the *template*,
    // not about the value — a rule that inspected values would be a blocklist, and a blocklist is
    // the thing this repository already refuses on the credential side.
    assert!(!settings.is_set(&tenant(), "newrelic", &host));
}

/// The refusal for a missing value is **unchanged** by this story: still by name, still terminal,
/// still nothing sent.
///
/// This story adds a way to *supply* a value; it must not become a way to do without one. The case
/// driven here is the sharp one — a connector with two declared settings where the tenant has
/// supplied only the first, which is exactly the state a half-configured connection is in.
#[tokio::test]
async fn a_connection_missing_a_value_it_declares_is_still_refused_by_name() {
    let (_scratch, settings) = settings_store("half");
    settings
        .set(
            &tenant(),
            "zendesk",
            &zendesk_setting("endpoint.subdomain"),
            "acme",
        )
        .expect("a declared setting");

    let (wire, outcome) = show_a_ticket(zendesk_credentials().await, settings).await;
    let refusal = outcome.expect_err("the user half of zendesk's Basic credential is unsupplied");

    let message = refusal.to_string();
    assert!(
        message.contains("zendesk.api_token"),
        "the refusal must name the value that is missing: {message}",
    );
    assert!(
        !message.contains(SENTINEL),
        "and must never repeat a credential: {message}",
    );
    assert_eq!(refusal.sent(), Sent::No);
    assert!(!refusal.retryable());
    assert_eq!(wire.lock().expect("the wire lock").count(), 0);
}

// ---------------------------------------------------------------------------------------------
// What a connector declares, and what may be written at it
// ---------------------------------------------------------------------------------------------

/// The declared surface is read off the connector's own compiled Flux, not guessed from its
/// `base_url`.
///
/// The distinction is measurable and it is why this is a test rather than a comment: parsing
/// `base_url` for `{placeholders}` finds zendesk's `subdomain` and **misses** bitbucket's
/// `workspace`, cloudflare's `zone_id`, contentful's two `space_id`s, statuspage's `page_id`,
/// vercel's `teamId` and docusign's `account_id` — every one of which is a configuration variable
/// the operation's own Flux carries somewhere other than the base URL. A host that enumerated the
/// surface that way would tell an operator they had supplied everything and then refuse the call.
#[test]
fn the_declared_surface_is_read_from_the_connector_rather_than_from_its_base_url() {
    let zendesk: Vec<String> = declared_settings(zendesk())
        .expect("zendesk's operations rehearse")
        .iter()
        .map(|declared| format!("{}/{}", declared.service, declared.binds()))
        .collect();

    assert_eq!(
        zendesk,
        vec![
            "default/endpoint.subdomain".to_owned(),
            "default/username.zendesk.api_token".to_owned(),
        ],
        "zendesk needs both a subdomain and the non-secret user half of its Basic credential",
    );

    // The variable that does not appear in the base URL at all — the case a `base_url` scan misses.
    let bitbucket = connector_catalog::provider(connector_catalog::ProviderKey::id("bitbucket"))
        .expect("the catalogue carries bitbucket");
    assert!(
        !bitbucket.base_url.contains('{'),
        "bitbucket's base URL carries no placeholder, which is what makes it the interesting case",
    );
    let declared: Vec<String> = declared_settings(bitbucket)
        .expect("bitbucket's operations rehearse")
        .iter()
        .map(|declared| declared.binds())
        .collect();
    assert_eq!(declared, vec!["endpoint.workspace".to_owned()]);
}

/// A connector with a literal base URL and no Basic credential declares nothing to configure, and
/// that is an answer rather than an omission.
#[test]
fn a_connector_that_needs_nothing_declares_nothing() {
    let github = connector_catalog::provider(connector_catalog::ProviderKey::id("github"))
        .expect("the catalogue carries github");

    assert!(
        declared_settings(github)
            .expect("github's operations rehearse")
            .is_empty(),
        "github's base URL is literal and its credential is a bearer token, so there is nothing \
         per-connection to supply",
    );
}

/// Every shipped connector's declared surface can be read, so an operator is never told to go and
/// supply a value this host cannot name.
///
/// The whole catalogue rather than a sample: the day a connector ships whose Flux this host cannot
/// rehearse, that connector is unconfigurable, and finding out here is cheaper than finding out
/// from a `422` an operator cannot act on.
#[test]
fn every_shipped_connector_can_say_what_it_needs() {
    for provider in connector_catalog::providers() {
        assert!(
            declared_settings(provider).is_ok(),
            "`{}` cannot say what it needs configured, so nothing can supply it",
            provider.id,
        );
    }
}

/// A name the connector does not declare has no address here, and the refusal says what would have
/// worked.
///
/// The same rule `ConnectorDeclaration::address_of` holds for credentials, and for the same reason:
/// a value stored under an undeclared name sits where no operation reads it, which is a loss that
/// looks like a success from every side.
#[test]
fn an_undeclared_setting_is_refused_and_the_declared_ones_are_named() {
    let (_scratch, settings) = settings_store("undeclared");

    let refusal = settings
        .set(
            &tenant(),
            "zendesk",
            &DeclaredSetting {
                service: "default".to_owned(),
                kind: SettingKind::Endpoint,
                name: "base_url".to_owned(),
            },
            "https://evil.example",
        )
        .expect_err("zendesk declares no `endpoint.base_url`");

    let message = refusal.to_string();
    assert!(message.contains("endpoint.base_url"), "{message}");
    assert!(message.contains("endpoint.subdomain"), "{message}");
}

/// A service the connector does not declare is refused too, and is a distinct refusal.
///
/// `contentful` is the shipped case for why the service is part of the address at all: its
/// `delivery` and `management` services both spell `endpoint.space_id`, and a value stored under
/// the wrong one is a management write into a space nobody named.
#[test]
fn a_service_the_connector_does_not_declare_is_refused() {
    let (_scratch, settings) = settings_store("service");

    let refusal = settings
        .set(
            &tenant(),
            "zendesk",
            &DeclaredSetting {
                service: "sandbox".to_owned(),
                kind: SettingKind::Endpoint,
                name: "subdomain".to_owned(),
            },
            "acme",
        )
        .expect_err("zendesk declares one service, and it is not `sandbox`");

    let message = refusal.to_string();
    assert!(message.contains("sandbox"), "{message}");
    assert!(message.contains("default"), "{message}");
}

/// A value larger than a connection setting is refused, and the refusal names the bound.
///
/// The bound is **not** the credential one, and this is where that stops being a sentence in a
/// design document: a setting is a hostname or a vendor id, and the largest one anybody ships is
/// two orders of magnitude under a PEM-encoded key.
#[test]
fn a_value_past_the_setting_bound_is_refused() {
    let (_scratch, settings) = settings_store("bound");
    let oversized = "x".repeat(MAX_SETTING_VALUE_BYTES + 1);

    let refusal = settings
        .set(
            &tenant(),
            "zendesk",
            &zendesk_setting("endpoint.subdomain"),
            &oversized,
        )
        .expect_err("a value past the bound is not a connection setting");

    let message = refusal.to_string();
    assert!(
        message.contains(&MAX_SETTING_VALUE_BYTES.to_string()),
        "the refusal must name the bound: {message}",
    );
    assert!(
        !message.contains(&oversized),
        "and must never repeat the value: {message}",
    );
    assert!(
        !settings.is_set(&tenant(), "zendesk", &zendesk_setting("endpoint.subdomain")),
        "a refused write must not have landed",
    );
}
