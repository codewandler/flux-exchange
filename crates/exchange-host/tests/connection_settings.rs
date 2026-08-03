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
//! Re-measured against catalogue 0.16 by X-98: the exact refusal and closed-choice sets are pinned
//! below. `asterisk` joins the whole-authority refusals; two region-host fields remain closed sets
//! published by the vendor catalogue. [`only_an_exactly_declared_choice_may_be_supplied`] drives the
//! latter rather than trusting their labels.
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
    Principal::new(PrincipalKind::ServiceAccount, "triage-bot", tenant())
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
///
/// # `newrelic` stays here after X-70, and on purpose
///
/// Its template is still `{host}`; what changed is that the catalogue now publishes the two region
/// hostnames it permits, so the refusal below is `NotADeclaredChoice` rather than
/// `WouldNameTheHost`. The claim this test makes is unchanged and is the one that matters — a
/// tenant offering `evil.example` gets nothing stored, nothing dispatched and no credential on the
/// wire — and a connector whose values are a closed set is exactly where a regression would be
/// easiest to miss.
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
/// # The fifth arrived by upstream bump, and X-70 measured what it actually was
///
/// It was four against catalogue 0.9. Moving to 0.10 turned this red rather than quietly widening
/// what a tenant may configure, which is what X-47's design said a host-template change *should*
/// do. `intercom` was the arrival: upstream C-225 made its `base_url` `https://{host}` so an EU or
/// AU workspace can be connected at all, and a bare placeholder is the whole authority.
///
/// The same upstream change published `config_choices`, and re-reading the catalogue for them moved
/// **two** connectors rather than one: `intercom`'s three region hosts and `newrelic`'s two. Both
/// now land in [`HostPinning::ChosenFrom`], which is censused below beside the refusals — a set
/// that grows is a widening of what a tenant may supply and has to be as hard to do by accident as
/// the refusals are.
///
/// `algolia`, the 54th provider, is in neither list: it ships `{app_id}.algolia.net`, which pins two
/// labels and lands in [`HostPinning::PinnedTo`] with the seven that were already there. Worth
/// stating because the estimate the 0.10 bump was planned against read its template as unpinnable
/// and expected a sixth refusal; the measurement is what decided it, not the estimate.
#[test]
fn no_shipped_connector_lets_a_tenant_supply_its_whole_authority() {
    let mut refused = Vec::new();
    let mut chosen = Vec::new();

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
                // The value is one of a closed set the **catalogue** publishes, so it is a choice
                // rather than a destination. The set itself is quoted, because "a closed set" that
                // nobody looked at is how a widening would arrive unnoticed.
                HostPinning::ChosenFrom(choices) => {
                    assert!(
                        !choices.is_empty(),
                        "`{}` {} claims a closed set with nothing in it, which admits nothing and \
                         should have stayed a template question",
                        provider.id,
                        declared.binds(),
                    );
                    chosen.push(format!(
                        "{}/{} ({})",
                        provider.id,
                        declared.binds(),
                        choices.join(", "),
                    ));
                }
                HostPinning::WholeAuthority(template) => {
                    refused.push(format!("{}/{} ({template})", provider.id, declared.binds()));
                }
            }
        }
    }

    // Pinned exactly, so that a catalogue change in either direction is a failing test rather than
    // a silent hole: a fifth connector arriving unpinned, or one of these four gaining a suffix
    // upstream and staying refused for no reason. It has already fired once for real — `intercom`
    // arrived here when catalogue 0.10 moved its host, and this assertion is how anybody found out.
    assert_eq!(
        refused,
        vec![
            "asterisk/endpoint.host ({host}:8089)".to_owned(),
            "docusign/endpoint.account_host ({account_host})".to_owned(),
            "freshdesk/endpoint.domain ({domain})".to_owned(),
            "okta/endpoint.domain ({domain})".to_owned(),
        ],
        "the set of connectors a tenant may not configure has changed",
    );

    // And the other half of the same census. Every value here is the vendor's own published string:
    // a diff that adds one is upstream widening a closed set, and a diff that adds a *connector* is
    // a field this host used to refuse and now admits.
    assert_eq!(
        chosen,
        vec![
            "intercom/endpoint.host (api.intercom.io, api.eu.intercom.io, api.au.intercom.io)"
                .to_owned(),
            "newrelic/endpoint.host (api.newrelic.com, api.eu.newrelic.com)".to_owned(),
        ],
        "the set of settings whose values come from a catalogue-declared closed set has changed",
    );
}

// ---------------------------------------------------------------------------------------------
// A closed set the catalogue publishes is not the caller naming a host (X-70)
// ---------------------------------------------------------------------------------------------

/// **The failing-first test.** A tenant on Intercom's EU region can configure their connection, and
/// the request goes to the region they chose.
///
/// Upstream C-225 made intercom's `base_url` `https://{host}`, and a bare placeholder is the whole
/// authority — so X-47's rule refused it, correctly under the rule and wrongly about intercom. The
/// same upstream change published `config_choices`: `{host}` is a closed set of **three vendor
/// hostnames**, and choosing among them is choosing a region from a dropdown rather than naming a
/// destination.
///
/// What makes that admissible is *not* that the hostnames look safe. It is that the choice set is a
/// second piece of declared catalogue data from the same source the host rule is already derived
/// from — so admitting a value because the catalogue declares it as one of a closed set is still
/// deciding from the catalogue, which is the property X-47 exists to keep.
#[tokio::test]
async fn a_tenant_may_choose_intercoms_declared_region_and_the_request_goes_there() {
    let (_scratch, settings) = settings_store("intercom-region");
    let host = setting_of(provider("intercom"), "endpoint.host");

    settings
        .set(&tenant(), "intercom", &host, "api.eu.intercom.io")
        .expect("a hostname the catalogue itself declares as one of intercom's three regions");

    let credentials = credential_store(
        "com.intercom.api",
        Some(("intercom.access_token", "access_token")),
    )
    .await;
    let (wire, egress) = silent_egress();
    let outcome = invoker(credentials, settings, egress)
        .invoke(
            &caller(),
            "intercom-contact-get",
            json!({ "contact_id": "1" }),
        )
        .await;

    let invocation = outcome.expect("a configured region, a stored credential, and one operation");
    assert_eq!(invocation.operation, "intercom-contact-get");

    let wire = wire.lock().expect("the wire lock");
    assert_eq!(wire.count(), 1, "exactly one dispatch, for one invoke");
    assert_eq!(
        wire.origin(0),
        "https://api.eu.intercom.io",
        "the origin is the region this tenant chose out of the catalogue's own closed set",
    );
}

/// **The equality edge, driven rather than read.** Only a value that is *exactly* one of the
/// declared choices is admitted.
///
/// Not a prefix of one, not an extension of one, not a case-folded one, not one with whitespace
/// around it. Each of those is a value a caller composed rather than a value the catalogue
/// published, and `api.eu.intercom.io.evil.example` is the shape that matters: it *contains* a
/// declared choice and resolves wherever its registrant says.
///
/// The refusal names the address and never repeats what was sent — the same standard every other
/// refusal on this surface holds to. It does quote the choices, which are the catalogue's own data
/// and the whole of what makes the refusal actionable.
#[test]
fn only_an_exactly_declared_choice_may_be_supplied() {
    let intercom = provider("intercom");
    let host = setting_of(intercom, "endpoint.host");

    for admitted in [
        "api.intercom.io",
        "api.eu.intercom.io",
        "api.au.intercom.io",
    ] {
        let (_scratch, settings) = settings_store("choice-exact");
        settings
            .set(&tenant(), "intercom", &host, admitted)
            .unwrap_or_else(|refusal| panic!("`{admitted}` is a declared choice: {refusal}"));
        assert!(settings.is_set(&tenant(), "intercom", &host));
    }

    // The refusal names the *address* and the catalogue's own choices, and carries nothing of what
    // was offered — asserted as an equality against one expected value rather than as a search
    // through the message, because a refusal that is byte-identical whatever was sent is a refusal
    // that cannot have repeated it. (Some near-misses below are substrings of a declared choice, so
    // "the message does not contain the value" would be the wrong question to ask.)
    let expected = exchange_host::SettingsRefusal::NotADeclaredChoice {
        connector: "intercom".to_owned(),
        setting: "endpoint.host".to_owned(),
        choices: vec![
            "api.intercom.io".to_owned(),
            "api.eu.intercom.io".to_owned(),
            "api.au.intercom.io".to_owned(),
        ],
    };

    for refused in [
        "api.eu.intercom.io.evil.example",
        "evil.example.api.eu.intercom.io",
        "API.EU.INTERCOM.IO",
        " api.eu.intercom.io",
        "api.eu.intercom.io ",
        "api.eu.intercom.i",
        "eu.intercom.io",
        "evil.example",
    ] {
        let (_scratch, settings) = settings_store("choice-near-miss");
        let refusal = settings
            .set(&tenant(), "intercom", &host, refused)
            .expect_err("a value the catalogue does not declare is not a choice");

        assert_eq!(
            refusal, expected,
            "`{refused}` was refused with something other than the one refusal this address has — \
             a refusal that varies with the value is a refusal that repeats it",
        );

        let message = refusal.to_string();
        assert!(
            message.contains("intercom") && message.contains("endpoint.host"),
            "the refusal must name the address: {message}",
        );
        assert!(
            !message.contains("evil.example"),
            "the refusal must not repeat the value it refused: {message}",
        );
        assert!(
            !settings.is_set(&tenant(), "intercom", &host),
            "`{refused}` was stored despite being refused",
        );
    }
}

/// The admitted set comes from the catalogue and from **nothing written in this repository**.
///
/// Walked over the whole catalogue in both directions, so neither half can be satisfied by a name
/// somebody typed here:
///
/// - a setting the catalogue publishes a non-empty closed set for admits **every** value in that
///   set and refuses one outside it;
/// - a setting this host refuses outright has **no** closed set published for it — which is the
///   Acceptance's third line, derived rather than enumerated: a connector whose choice set is empty
///   or absent is still refused as the whole authority.
#[test]
fn what_a_tenant_may_supply_is_read_off_the_catalogues_own_choices() {
    for provider in connector_catalog::providers() {
        for declared in declared_settings(provider).expect("every connector's operations rehearse")
        {
            let published = provider
                .choices_for(&declared.service, declared.kind.as_str(), &declared.name)
                .map_or(&[][..], |entry| entry.choices);

            if published.is_empty() {
                // Nothing to admit from, so the template is the whole of the answer — and an
                // unpinned one stays refused whatever is offered.
                if let HostPinning::WholeAuthority(template) = host_pinning(provider, &declared) {
                    assert!(
                        template.contains('{'),
                        "`{}` {} is refused against a template that carries no placeholder",
                        provider.id,
                        declared.binds(),
                    );
                    let (_scratch, settings) = settings_store("no-choices");
                    settings
                        .set(&tenant(), provider.id, &declared, "evil.example")
                        .expect_err(
                            "a connector with no declared choices templates the whole authority, \
                             so no value may be supplied",
                        );
                }
                continue;
            }

            for choice in published {
                let (_scratch, settings) = settings_store("catalogue-choice");
                settings
                    .set(&tenant(), provider.id, &declared, choice.value)
                    .unwrap_or_else(|refusal| {
                        panic!(
                            "`{}` publishes `{}` as a choice for {} and this host refused it: \
                             {refusal}",
                            provider.id,
                            choice.value,
                            declared.binds(),
                        )
                    });
            }

            let (_scratch, settings) = settings_store("catalogue-non-choice");
            let _ = settings.set(&tenant(), provider.id, &declared, "evil.example");
            assert!(
                !settings.is_set(&tenant(), provider.id, &declared),
                "`{}` accepted a value for {} that its own catalogue entry does not declare",
                provider.id,
                declared.binds(),
            );
        }
    }
}

/// **The `get` side**, for the fourth answer — a value that reached the file some other way is
/// checked against the choices on the way out too.
///
/// The rule is enforced twice for `a_planted_whole_authority_value_is_refused_on_the_way_out`'s
/// reason, and a closed set is no different: an edited store, a backup taken before this rule
/// existed, or a value written by an older build all bypass `set`. What is new here is that the
/// question the port asks is now about the **value**, so the answer must be right in both
/// directions — a planted extension of a choice is refused, and a planted choice is honoured.
///
/// Both halves run against the same planting mechanism the older test uses: the bytes are written
/// straight into the store file by something that is not this store.
#[test]
fn a_planted_value_is_admitted_only_when_it_is_a_declared_choice() {
    use exchange_host::ConfigStore as _;

    for (planted, admitted) in [
        ("api.eu.intercom.io", true),
        ("api.eu.intercom.io.evil.example", false),
        ("API.EU.INTERCOM.IO", false),
        ("evil.example", false),
    ] {
        let scratch = Scratch::new("planted-choice");
        let path = scratch.join("state").join("settings");
        let intercom = provider("intercom");
        let host = setting_of(intercom, "endpoint.host");

        let mut file = json!({ TENANT: { "intercom": {} } });
        file[TENANT]["intercom"][&host.service][host.binds()] = json!(planted);
        fs::create_dir_all(path.parent().expect("the store path has a parent"))
            .expect("a scratch directory");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&file).expect("a serialisable store"),
        )
        .expect("a planted store file");
        let on_disk = fs::read(&path).expect("the planted file is readable");

        let settings =
            SettingsStore::bind(&path).expect("the store reopens over a file it did not write");
        assert!(
            settings.held_bytes(&tenant()) >= planted.len(),
            "nothing was planted, so nothing below is a test of the guard",
        );

        let read = settings.get(TENANT, "intercom", &host.service, host.field());
        if admitted {
            assert_eq!(
                read,
                Some(planted.to_owned()),
                "a stored value that *is* one of the catalogue's declared choices must reach the \
                 invoker — refusing it would make the closed set unusable",
            );
        } else {
            assert_eq!(
                read, None,
                "`{planted}` is not one of intercom's declared choices and reached the invoker out \
                 of a file `set` never saw",
            );
        }

        // Refuse; never repair — on both paths, and for the same reason.
        assert_eq!(
            fs::read(&path).expect("the file is still readable"),
            on_disk,
            "the store rewrote a file it read a value out of",
        );
    }
}

/// The refusal is actionable: it says which connectors this host will not let a tenant configure,
/// and why, rather than reading as a bug.
///
/// A smaller working surface beats a larger one that leaks — but only if an operator can tell the
/// difference between "refused on purpose" and "broken".
///
/// Driven on `okta` rather than on `newrelic`, which it used to drive: newrelic's `{host}` is a
/// closed set of two region hostnames as of catalogue 0.10, so it is [`HostPinning::ChosenFrom`]
/// and no longer an example of this. Okta declares `{domain}` and publishes no choices for it,
/// which is the state this test is about — the value would be the whole origin and there is nothing
/// declared to pick from.
#[test]
fn a_connector_this_host_will_not_configure_says_so() {
    let (_scratch, settings) = settings_store("unsuppliable");
    let okta = provider("okta");
    let domain = setting_of(okta, "endpoint.domain");

    let refusal = settings
        .set(&tenant(), "okta", &domain, "acme.okta.com")
        .expect_err("okta's domain is the whole authority, whatever value is offered");

    let message = refusal.to_string();
    assert!(message.contains("okta"), "{message}");
    assert!(message.contains("endpoint.domain"), "{message}");
    assert!(
        message.contains("{domain}"),
        "the refusal must quote the template that pins nothing, so an operator can see why: \
         {message}",
    );

    // Even a value that really is an Okta host is refused. Where the catalogue declares no closed
    // set, the rule is about the *template* and not about the value — a rule that inspected values
    // would be a blocklist, and a blocklist is the thing this repository already refuses on the
    // credential side.
    assert!(!settings.is_set(&tenant(), "okta", &domain));
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
            "default/username.zendesk.messaging_key".to_owned(),
            "help-center/endpoint.subdomain".to_owned(),
            "help-center/username.zendesk.api_token".to_owned(),
            "help-center/username.zendesk.messaging_key".to_owned(),
            "messaging/endpoint.appId".to_owned(),
            "messaging/endpoint.subdomain".to_owned(),
            "messaging/username.zendesk.api_token".to_owned(),
            "messaging/username.zendesk.messaging_key".to_owned(),
        ],
        "every service-specific Zendesk binding comes from compiled Flux rather than base URL guessing",
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

/// The socket planner asks for its query value through the same tenant-scoped configuration port
/// as ordinary operations. If the surface omitted this declaration, an operator could connect
/// Asterisk's REST API but its generated event channel would always refuse as unconfigured.
#[test]
fn a_generated_channel_query_is_part_of_the_connection_settings_surface() {
    let asterisk = connector_catalog::provider(connector_catalog::ProviderKey::id("asterisk"))
        .expect("the catalogue carries asterisk");
    let declared = declared_settings(asterisk).expect("asterisk declarations are readable");

    assert!(
        declared.iter().any(|setting| {
            setting.service == "default"
                && setting.kind == SettingKind::ChannelQuery
                && setting.binds() == "channel.ari-events.query.app"
        }),
        "the generated ARI socket's required application query is not configurable: {declared:?}",
    );
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
