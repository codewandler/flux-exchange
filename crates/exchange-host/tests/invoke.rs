//! **Invoking an operation** (X-12) — driven through a transport that records instead of sending.
//!
//! Every test here is `docs/designs/invoke.md` §2 lock 3: a counting transport, wrapped as an
//! [`Egress`], which answers without opening a socket and remembers exactly what it was asked to
//! carry. Two things follow that nothing else in this repository can assert:
//!
//! - a successful invoke calls it **once**, and the recorded origin is the connector's declared one
//!   whatever the parameters were;
//! - **every refusal leaves the count at zero.** That is what "the request was never sent" means as
//!   a test rather than as a sentence in an error message.
//!
//! Lock 3's limit is stated here because it is the one most likely to be over-read: it proves
//! things about the paths these tests drive, never about paths they do not know exist.
//! `no_second_request_path.rs` is what speaks to absence, and the two fail differently.

use std::sync::{Arc, Mutex};

use exchange_host::{
    admit_runtime, ConnectorSurface, Contexts, Deployment, Egress, Grant, GrantRefusal, Grants,
    InvokeRefusal, Invoker, MemoryConfig, Principal, PrincipalKind, Risk, Runtime, Secret,
    SecretStore, Selector, Sent, Tenant,
};
use flux_runtime::{Tool, ToolContext};
use serde_json::{json, Value};

/// The tenant every test invokes as. Which one it is does not matter; that it comes off the
/// principal is the whole point.
const TENANT: &str = "acme";

/// A credential value long enough for flux's redactor to hold, carrying **none** of flux's known
/// credential prefixes.
///
/// `connector-pack`'s own `tests/credentials.rs` keeps a sentinel for exactly this reason and the
/// reason is worth copying rather than just the constant: flux's redactor runs a second,
/// shape-based pass over tokens it was never told about (`sk-ant-`, `xoxb-`, `ghp_`, `AKIA`,
/// `eyJ`, …), so a sentinel that *looked* like a credential would be scrubbed whether or not
/// anybody registered it — and the test would pass while asserting nothing.
///
/// [`the_sentinel_is_only_redacted_because_it_was_registered`] is what holds that claim.
const SENTINEL: &str = "quiggle-marrow-plimth-42";

/// Five characters once trimmed, which is one short of what flux's redactor will hold.
///
/// A credential this short registers "successfully" and protects nothing, so `connector-pack`
/// refuses to send it. Deliberately not spelled with a length in any assertion message: a length is
/// a fingerprint.
const TOO_SHORT_TO_REDACT: &str = "ab3d9";

// ---------------------------------------------------------------------------------------------
// The counting transport, and the rest of the composition
// ---------------------------------------------------------------------------------------------

/// What the transport was asked to carry, and how often.
#[derive(Default)]
struct Wire {
    /// One entry per dispatch, each the `{url, method, headers, body}` params the pack built.
    calls: Vec<Value>,
}

impl Wire {
    /// The origin — scheme and authority — of the `n`th recorded URL.
    ///
    /// Compared as a prefix rather than parsed, deliberately: parsing would mean a URL crate, and
    /// the assertion this feeds is "the origin is *unchanged*", which a literal prefix states more
    /// directly than a structural comparison does.
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
}

/// A shared recorder plus the [`Egress`] that writes to it.
///
/// The spec is borrowed from a real `flux_web::http::HttpRequestTool` — built and never executed,
/// exactly as `engine_line.rs` builds one — rather than hand-written. That is not a shortcut: it
/// means this fake advertises the identical contract the live transport does, so a params shape the
/// real tool would refuse cannot slip past here unnoticed.
fn recording_egress(
    reply: impl Fn(&Value) -> Value + Send + Sync + 'static,
) -> (Arc<Mutex<Wire>>, Egress) {
    let wire = Arc::new(Mutex::new(Wire::default()));
    let spec = flux_web::http::HttpRequestTool::new(&flux_web::WebOptions::default()).spec();

    let recorded = wire.clone();
    let tool = flux_runtime::tool_fn(spec, move |params: Value| {
        let recorded = recorded.clone();
        let answer = reply(&params);
        async move {
            recorded.lock().expect("the wire lock").calls.push(params);
            Ok(answer)
        }
    });

    (wire, Egress::new(tool))
}

/// An egress that answers with a fixed body and records what it was asked to carry.
fn silent_egress() -> (Arc<Mutex<Wire>>, Egress) {
    recording_egress(|_| json!({ "status": 200, "headers": {}, "body": {"ok": true} }))
}

/// An egress that echoes the request's own headers back in its body — a loopback vendor of the
/// least ceremonious kind.
///
/// Several real vendors do exactly this in an error body, which is why the design asks for it: it
/// is how "the pack kept the credential off the wire" is told apart from "it stayed off every
/// surface".
fn echoing_egress() -> (Arc<Mutex<Wire>>, Egress) {
    recording_egress(|params| {
        json!({
            "status": 200,
            "headers": {},
            "body": { "you_sent": params.get("headers").cloned().unwrap_or(Value::Null) },
        })
    })
}

/// A fresh [`ToolContext`] per invocation, over a workspace nothing writes to.
///
/// This is the composition's half of the [`Contexts`] port. `System::new` disables the sandbox and
/// touches nothing until a tool asks it to; no tool here does.
fn contexts() -> Arc<dyn Contexts> {
    Arc::new(|| {
        let workspace = flux_system::Workspace::new(std::env::temp_dir())
            .expect("the temp directory is a usable workspace root");
        ToolContext::new(Arc::new(flux_system::System::new(workspace)))
    })
}

/// A store holding `value` at `declaration`'s address for [`TENANT`], or holding nothing.
///
/// The address is composed by this host's own [`ConnectorDeclaration::address_of`] rather than
/// written out as a literal, so what a test stores is what the pack looks for — the two derivations
/// are one, or the store is stocked at an address nothing reads from and every test here refuses
/// for the wrong reason.
async fn store_at(
    declaration: exchange_host::ConnectorDeclaration<'static>,
    credential: &str,
    value: Option<&str>,
) -> Arc<dyn SecretStore> {
    let store = Arc::new(connector_pack::MemoryStore::new());

    if let Some(value) = value {
        let reference = declaration
            .address_of(
                &Tenant::new(TENANT).expect("`acme` is a usable tenant"),
                credential,
            )
            .expect("the connector declares an authority and this credential");

        store
            .put(&reference, &Secret::new(value))
            .await
            .expect("a memory store accepts a write");
    }

    store
}

/// A store holding `value` at github's declared address, or holding nothing.
async fn github_store(value: Option<&str>) -> Arc<dyn SecretStore> {
    store_at(
        exchange_host::ConnectorDeclaration {
            connector: "github",
            authority: Some("com.github.api"),
            credentials: &[exchange_host::DeclaredCredential {
                name: "github.token",
                leaf: "token",
            }],
        },
        "github.token",
        value,
    )
    .await
}

/// A store holding `value` at zendesk's declared address.
async fn zendesk_store(value: &str) -> Arc<dyn SecretStore> {
    store_at(
        exchange_host::ConnectorDeclaration {
            connector: "zendesk",
            authority: Some("com.zendesk.api"),
            credentials: &[exchange_host::DeclaredCredential {
                name: "zendesk.api_token",
                leaf: "api_token",
            }],
        },
        "zendesk.api_token",
        Some(value),
    )
    .await
}

/// The caller. An agent, because agents are what call operations all day.
fn caller() -> Principal {
    Principal::new(
        PrincipalKind::Agent,
        "triage-bot",
        Tenant::new(TENANT).expect("`acme` is a usable tenant"),
    )
}

/// The grants a tenant holds, as the port, so a test says what this tenant may run in one place.
///
/// A local type rather than something published from the crate: `exchange-host` deliberately ships
/// no in-memory grant store, for the reason `AGENTS.md` refuses a credential store that falls back
/// to memory — a store that forgets what a tenant may run is a host that refuses everything after a
/// restart, and publishing one would put that composition one line away.
struct Held(Vec<Grant>);

impl Grants for Held {
    fn held(&self, _: &Tenant) -> Vec<Grant> {
        self.0.clone()
    }

    fn set(&self, _: &Tenant, _: &[Grant]) -> Result<(), GrantRefusal> {
        unreachable!(
            "these tests hold grants rather than editing them; `grant.rs` drives the store"
        )
    }
}

/// An invoker over a store, an egress, an empty configuration and `grants`.
///
/// `MultiTenant` throughout, because that is the deployment the invariants are about — and because
/// a test that quietly ran single-tenant would be exercising the permissive branch of every gate.
fn invoker_granting(
    credentials: Arc<dyn SecretStore>,
    egress: Egress,
    grants: Vec<Grant>,
) -> Invoker {
    Invoker::new(
        Deployment::MultiTenant,
        egress,
        credentials,
        Arc::new(MemoryConfig::new()),
        Arc::new(Held(grants)),
        contexts(),
    )
}

/// An invoker whose tenant may run anything github and zendesk publish.
///
/// The grant is what the tests below that are **not** about the grant gate need: since X-13 an
/// invocation is admitted by a grant, so a helper that granted nothing would make every one of them
/// refuse for a reason that has nothing to do with what it asserts. It is still connector-scoped —
/// there is no wildcard grant, deliberately, and a test helper is not the place to invent one.
fn invoker(credentials: Arc<dyn SecretStore>, egress: Egress) -> Invoker {
    invoker_granting(
        credentials,
        egress,
        vec![
            Grant::for_connector("github", Selector::any()),
            Grant::for_connector("zendesk", Selector::any()),
        ],
    )
}

// ---------------------------------------------------------------------------------------------
// The route runs an operation
// ---------------------------------------------------------------------------------------------

/// **The Acceptance's first line.** One catalogue operation runs for the caller's tenant and the
/// result comes back.
///
/// `github-repo-get` rather than `zendesk-ticket-show` on purpose: github's base URL carries no
/// `{placeholder}`, so this asserts the invoke path without also asserting a connection-settings
/// surface that does not exist yet.
#[tokio::test]
async fn one_operation_runs_and_returns_its_result() {
    let (wire, egress) = silent_egress();
    let invoker = invoker(github_store(Some(SENTINEL)).await, egress);

    let invocation = invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect("a stored credential and a declared operation");

    assert_eq!(invocation.operation, "github-repo-get");
    assert!(!invocation.is_error);
    assert!(
        invocation.content.contains("\"ok\":true"),
        "the transport's answer is returned whole: {}",
        invocation.content,
    );

    let wire = wire.lock().expect("the wire lock");
    assert_eq!(wire.calls.len(), 1, "exactly one dispatch, for one invoke");
    assert_eq!(wire.origin(0), "https://api.github.com");
}

/// An id the catalogue does not spell is refused, terminally, and nothing is dispatched.
#[tokio::test]
async fn an_unknown_operation_is_refused_and_nothing_is_dispatched() {
    let (wire, egress) = silent_egress();
    let invoker = invoker(github_store(Some(SENTINEL)).await, egress);

    let refusal = invoker
        .invoke(&caller(), "github-repo-obliterate", json!({}))
        .await
        .expect_err("nothing in the catalogue spells that");

    assert!(matches!(refusal, InvokeRefusal::UnknownOperation { .. }));
    assert_eq!(refusal.sent(), Sent::No);
    assert!(!refusal.retryable());
    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 0);
}

// ---------------------------------------------------------------------------------------------
// No request field lets a caller influence the destination host
// ---------------------------------------------------------------------------------------------

/// **Failing-first, Acceptance line 3.** No parameter a caller supplies moves the origin.
///
/// The three shapes the design names are driven together, against the operation's real path
/// parameters: `@evil.example`, which turns everything before it into userinfo when it lands in an
/// *authority*; `../`, which walks a path; and `{subdomain}`, which spells a configuration variable.
/// In a path segment none of them can reach the origin — substitution lands after the authority is
/// fixed in the `base` literal — and this asserts that against the **URL observed on the wire**
/// rather than against an error message, because an error message is a thing that can be right
/// while the request is wrong.
///
/// Members the operation does not declare are driven too. There is nowhere for them to land: the
/// body *is* the parameter object, and the pack projects an input schema from the operation's own
/// compiled Flux.
#[tokio::test]
async fn no_parameter_can_move_the_destination_host() {
    let hostile = [
        json!({ "owner": "codewandler@evil.example", "repo": "flux-exchange" }),
        json!({ "owner": "../../../evil.example", "repo": "flux-exchange" }),
        json!({ "owner": "codewandler", "repo": "{subdomain}" }),
        json!({ "owner": "codewandler", "repo": "x", "url": "https://evil.example/pwned" }),
        json!({ "owner": "codewandler", "repo": "x", "base_url": "https://evil.example" }),
        json!({ "owner": "codewandler", "repo": "x", "host": "evil.example" }),
    ];

    let mut dispatched = 0;

    for params in hostile {
        let (wire, egress) = silent_egress();
        let invoker = invoker(github_store(Some(SENTINEL)).await, egress);

        let outcome = invoker
            .invoke(&caller(), "github-repo-get", params.clone())
            .await;

        let wire = wire.lock().expect("the wire lock");
        match outcome {
            Ok(_) => {
                dispatched += 1;
                assert_eq!(
                    wire.origin(0),
                    "https://api.github.com",
                    "`{params}` moved the origin, which is the confused deputy this host exists \
                     to refuse",
                );
            }
            Err(refusal) => assert_eq!(
                wire.calls.len(),
                0,
                "`{params}` was refused ({refusal}) — and a refusal must leave nothing dispatched",
            ),
        }
    }

    // A test where every case is refused would assert only that refusals do not dispatch, which is
    // a different claim from the one in the name. At least one hostile value has to reach the wire
    // and be observed there for "the origin is unchanged" to have been tested at all.
    assert!(
        dispatched > 0,
        "every hostile parameter was refused, so the recorded origin was never examined",
    );
}

/// The same claim where it is sharpest: a connector whose **host** is templated.
///
/// Zendesk's base URL is `https://{subdomain}.zendesk.com`, so its origin is composed from a
/// tenant's own configured value. A parameter carrying `{subdomain}` puts a brace back into a URL
/// every variable had already filled, and the pack refuses rather than sending a request naming a
/// variable nobody resolved. Either way the origin is never the caller's.
#[tokio::test]
async fn a_templated_host_is_still_not_the_callers_to_name() {
    // Both halves of zendesk's connection: the `{subdomain}` its base URL needs, and the non-secret
    // user half its Basic credential joins with. Without them every case below refuses for a reason
    // that has nothing to do with the origin, and the test would assert nothing.
    let settings = MemoryConfig::new()
        .with_endpoint(TENANT, "zendesk", "default", "subdomain", "acme")
        .with_username(
            TENANT,
            "zendesk",
            "default",
            "zendesk.api_token",
            "ops@acme.test",
        );

    let mut dispatched = 0;

    for ticket in [
        json!("{subdomain}"),
        json!("1/../../evil.example"),
        json!("1@evil.example"),
    ] {
        let (wire, egress) = silent_egress();
        let invoker = Invoker::new(
            Deployment::MultiTenant,
            egress,
            zendesk_store(SENTINEL).await,
            Arc::new(settings.clone()),
            Arc::new(Held(vec![Grant::for_connector("zendesk", Selector::any())])),
            contexts(),
        );

        let outcome = invoker
            .invoke(
                &caller(),
                "zendesk-ticket-show",
                json!({ "ticket_id": ticket }),
            )
            .await;

        let wire = wire.lock().expect("the wire lock");
        match outcome {
            Ok(_) => {
                dispatched += 1;
                assert_eq!(
                    wire.origin(0),
                    "https://acme.zendesk.com",
                    "`{ticket}` moved the origin of a templated host",
                );
            }
            Err(refusal) => assert_eq!(
                wire.calls.len(),
                0,
                "`{ticket}` was refused ({refusal}) after dispatching",
            ),
        }
    }

    assert!(
        dispatched > 0,
        "every value was refused, so the composed origin was never examined",
    );
}

// ---------------------------------------------------------------------------------------------
// A missing credential refuses by address, and is terminal
// ---------------------------------------------------------------------------------------------

/// **Failing-first, Acceptance line 4.** No credential at the address → refused by address, never
/// by value, terminal, and never sent.
///
/// Four assertions, and the last is the one the other three rest on: `MissingCredential` being
/// terminal is only meaningful if the request genuinely did not go out, and the counting transport
/// is the only thing that can say so.
#[tokio::test]
async fn a_missing_credential_refuses_by_address_and_is_terminal() {
    let (wire, egress) = silent_egress();
    let invoker = invoker(github_store(None).await, egress);

    let refusal = invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect_err("nothing is stored at github's address for this tenant");

    let message = refusal.to_string();

    // By address.
    assert!(
        message.contains("tenants/acme/com.github.api/token"),
        "the refusal must name the address an operator has to go and put a value at: {message}",
    );
    // Never by value — there is no value, and the refusal must not have invented the shape of one.
    assert!(!message.contains(SENTINEL), "{message}");

    // Terminal. Nothing a retry can change is inside this system.
    assert_eq!(refusal.sent(), Sent::No);
    assert!(
        !refusal.retryable(),
        "a retrying agent against a credential-less connector is a loop against a vendor's rate \
         limit, one layer up from the one `connector-pack` refuses an optional credential port for",
    );

    // The request was never sent, as a count rather than as a sentence.
    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 0);
}

// ---------------------------------------------------------------------------------------------
// The credential is registered with the redactor, and the registration took
// ---------------------------------------------------------------------------------------------

/// **Acceptance line 5, the positive half.** A credential that travels does not reach the response.
///
/// The vendor echoes every request header back in its body, so the `Authorization` header carrying
/// `Bearer <SENTINEL>` comes straight back. It reaches this assertion having gone through the
/// **same** `ctx.redactor` the pack registered the credential with a moment before the request
/// existed — and the sentinel is nowhere in it.
#[tokio::test]
async fn a_credential_that_travels_is_not_on_the_response() {
    let (wire, egress) = echoing_egress();
    let invoker = invoker(github_store(Some(SENTINEL)).await, egress);

    let invocation = invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect("a stored credential and a declared operation");

    // The control: the vendor really did echo the header, so this is not passing on an empty body.
    let sent = &wire.lock().expect("the wire lock").calls[0];
    assert!(
        sent["headers"]["Authorization"]
            .as_str()
            .expect("the pack placed the credential in a header")
            .contains(SENTINEL),
        "the transport must have been handed the real value, or the assertion below is vacuous",
    );

    assert!(
        !invocation.content.contains(SENTINEL),
        "the credential reached the caller's surface: {}",
        invocation.content,
    );
    assert!(
        invocation
            .view
            .as_deref()
            .is_none_or(|view| !view.contains(SENTINEL)),
        "the credential reached the model-facing view",
    );
}

/// The sentinel is scrubbed **because it was registered**, not because it looks like a credential.
///
/// Without this, the test above would pass against flux's shape-based pass — which redacts tokens
/// it was never told about — and would be asserting nothing at all. A redactor nobody has told
/// anything leaves the sentinel exactly as it found it.
#[test]
fn the_sentinel_is_only_redacted_because_it_was_registered() {
    let fresh = contexts().fresh();

    assert_eq!(
        fresh.redactor.redact(SENTINEL),
        SENTINEL,
        "the sentinel is credential-shaped, so every redaction assertion in this file is vacuous",
    );
    assert_eq!(
        fresh.redactor.redact(&format!("Bearer {SENTINEL}")),
        format!("Bearer {SENTINEL}"),
        "and neither is the form that actually travels",
    );
}

/// **Acceptance line 5, the negative half.** A credential too short to redact is refused, not sent.
///
/// flux's `Redactor::add_secret` silently ignores a value under six trimmed characters, so
/// registration can succeed and protect nothing. `connector-pack` asks the redactor whether it took
/// and refuses when it did not; this asserts the observable from here — a refusal, no dispatch, and
/// a message that names the address without naming the value **or its length**.
#[tokio::test]
async fn a_credential_too_short_to_redact_is_refused_rather_than_sent() {
    let (wire, egress) = echoing_egress();
    let invoker = invoker(github_store(Some(TOO_SHORT_TO_REDACT)).await, egress);

    let refusal = invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect_err("a value the host's redactor will not hold does not go on the wire");

    let message = refusal.to_string();
    assert!(
        message.contains("com.github.api") && message.contains(TENANT),
        "the refusal must name the address: {message}",
    );
    assert!(!message.contains(TOO_SHORT_TO_REDACT), "{message}");
    assert!(
        !message.contains(&TOO_SHORT_TO_REDACT.len().to_string()),
        "a length is a fingerprint: {message}",
    );

    assert_eq!(refusal.sent(), Sent::No);
    assert!(!refusal.retryable());
    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 0);
}

// ---------------------------------------------------------------------------------------------
// The grant gate (X-13)
// ---------------------------------------------------------------------------------------------

/// A write github publishes, with every parameter its own Flux declares.
///
/// Complete on purpose: an incomplete parameter object is refused by the *pack*, one gate later, so
/// a test driving one would pass whether or not the grant gate existed. This is the call that runs
/// when nothing refuses it — which is exactly what it did at this story's merge base.
fn a_github_write() -> Value {
    json!({
        "owner": "codewandler",
        "repo": "flux-exchange",
        "title": "no",
        "body": "no",
        "labels": [],
        "assignees": [],
    })
}

/// **Failing-first, Acceptance line 2.** A read-only grant admits a connector's reads and refuses
/// its writes — **with no operation named anywhere in the grant.**
///
/// One grant, `risk <= low`, and two invocations of the same connector with the same credential:
/// the read runs and reaches the wire, the `high`-risk write is refused before anything is
/// resolved. Nothing here enumerates an id, and nothing in the grant does either — the difference
/// between the two calls is entirely what the *catalogue* declares about them, which is what makes
/// this a test of the rule rather than of a list.
///
/// At the merge base this test failed on its second half: the write ran, and the recorder held the
/// `POST` it dispatched.
#[tokio::test]
async fn a_read_only_grant_admits_the_reads_of_a_connector_and_refuses_its_writes() {
    let read_only = || vec![Grant::for_connector("github", Selector::at_most(Risk::Low))];

    let (wire, egress) = silent_egress();
    let invoker = invoker_granting(github_store(Some(SENTINEL)).await, egress, read_only());

    invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect("`github-repo-get` is declared `low`, and a `risk <= low` grant admits it");
    assert_eq!(
        wire.lock().expect("the wire lock").calls.len(),
        1,
        "the read must actually run, or this test is only about refusals",
    );

    let (wire, egress) = silent_egress();
    let invoker = invoker_granting(github_store(Some(SENTINEL)).await, egress, read_only());

    let refusal = invoker
        .invoke(&caller(), "github-issue-create", a_github_write())
        .await
        .expect_err("`github-issue-create` is declared `high`, and no grant here admits it");

    assert_eq!(refusal.label(), "not_granted", "{refusal}");
    assert_eq!(refusal.operation(), Some("github-issue-create"));
    assert_eq!(refusal.sent(), Sent::No);
    assert!(
        !refusal.retryable(),
        "a grant is not something time supplies"
    );

    let message = refusal.to_string();
    assert!(
        message.contains("triage-bot") && message.contains("github-issue-create"),
        "the refusal must name the principal and the operation: {message}",
    );
    assert_eq!(
        wire.lock().expect("the wire lock").calls.len(),
        0,
        "a refused invocation must leave nothing dispatched",
    );
}

/// **Failing-first, Acceptance line 3.** A grant for one connector does not reach another.
///
/// The grant is `Selector::any()` — every predicate satisfied — so the only thing standing between
/// this caller and github is the connector the grant names. A grant that reached every connector
/// would re-admit whatever the next connection added, without anyone deciding to.
#[tokio::test]
async fn a_grant_for_one_connector_does_not_reach_another() {
    let (wire, egress) = silent_egress();
    let invoker = invoker_granting(
        github_store(Some(SENTINEL)).await,
        egress,
        vec![Grant::for_connector("zendesk", Selector::any())],
    );

    let refusal = invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect_err("a grant for zendesk admits nothing of github's");

    assert_eq!(refusal.label(), "not_granted", "{refusal}");
    assert_eq!(refusal.sent(), Sent::No);
    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 0);
}

/// **Acceptance line 4.** An explicit `deny` beats an explicit `allow`, end to end.
///
/// Both halves are driven, because only the pair says anything: the `allow` really does admit an
/// operation the predicate refuses — a `high`-risk write under `risk <= low`, which runs and
/// reaches the wire — and adding a `deny` for the same id refuses it again. Without the first, a
/// grant that admitted nothing at all would pass the second.
#[tokio::test]
async fn an_explicit_deny_beats_an_explicit_allow_end_to_end() {
    let (wire, egress) = silent_egress();
    let allowed = invoker_granting(
        github_store(Some(SENTINEL)).await,
        egress,
        vec![Grant::for_connector(
            "github",
            Selector::at_most(Risk::Low).allow("github-issue-create"),
        )],
    );

    allowed
        .invoke(&caller(), "github-issue-create", a_github_write())
        .await
        .expect("an explicit allow admits an operation the predicate would refuse");
    assert_eq!(
        wire.lock().expect("the wire lock").calls.len(),
        1,
        "the allow must really admit, or the deny below is refusing something already refused",
    );

    let (wire, egress) = silent_egress();
    let denied = invoker_granting(
        github_store(Some(SENTINEL)).await,
        egress,
        vec![Grant::for_connector(
            "github",
            Selector::at_most(Risk::Low)
                .allow("github-issue-create")
                .deny("github-issue-create"),
        )],
    );

    let refusal = denied
        .invoke(&caller(), "github-issue-create", a_github_write())
        .await
        .expect_err("an operator who has explicitly denied an operation means it");

    assert_eq!(refusal.label(), "not_granted", "{refusal}");
    assert_eq!(refusal.sent(), Sent::No);
    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 0);
}

/// A tenant that holds nothing runs nothing — the state a composition is in before anybody grants
/// anything, and the one a fail-closed gate has to get right.
#[tokio::test]
async fn a_tenant_holding_no_grant_runs_nothing() {
    let (wire, egress) = silent_egress();
    let invoker = invoker_granting(github_store(Some(SENTINEL)).await, egress, Vec::new());

    let refusal = invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect_err("no grant, nothing runs");

    assert_eq!(refusal.label(), "not_granted", "{refusal}");
    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 0);
}

/// The gate sits **before** the credential store, which is where the design puts it.
///
/// Driven against a store that holds nothing: an ungranted call refuses with `not_granted` rather
/// than with the address refusal a granted one gets, and the two statuses from one store are what
/// says the order is this way round. A refusal that happened *after* a secret had been read would
/// have moved that secret into this process's memory for a call that was never going to be made.
#[tokio::test]
async fn a_call_no_grant_admits_is_refused_before_the_credential_store_is_read() {
    let (_, egress) = silent_egress();
    let ungranted = invoker_granting(github_store(None).await, egress, Vec::new());

    let refusal = ungranted
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect_err("no grant, nothing runs");
    assert_eq!(refusal.label(), "not_granted", "{refusal}");

    let (_, egress) = silent_egress();
    let granted = invoker(github_store(None).await, egress);

    let refusal = granted
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect_err("nothing is stored at github's address for this tenant");
    assert_eq!(
        refusal.label(),
        "refused",
        "the same call, granted, reaches the store and refuses by address: {refusal}",
    );
}

// ---------------------------------------------------------------------------------------------
// The deployment gate
// ---------------------------------------------------------------------------------------------

/// **Acceptance line 6.** A connector whose declared runtime this deployment does not admit is
/// refused through [`admit_runtime`] — the same call `Invoker::invoke` makes — and never executed.
///
/// It is driven against a constructed [`ConnectorSurface`] because **no shipped connector declares
/// a locally-executing runtime**: every one of them is `http`, which `invoke`'s own unit tests
/// assert against the whole catalogue. That makes this path *more* worth testing rather than less
/// — it is a fail-closed guarantee with no accidental coverage.
#[test]
fn a_runtime_this_deployment_does_not_admit_is_refused() {
    for runtime in [
        Runtime::Socket,
        Runtime::Process,
        Runtime::Container,
        Runtime::Plugin,
    ] {
        let surface = ConnectorSurface {
            connector: "fixture".into(),
            runtime,
            operations: Vec::new(),
        };

        let refusal = admit_runtime(Deployment::MultiTenant, &surface)
            .expect_err("a shared deployment cannot serve a runtime that executes on this host");

        assert_eq!(refusal.runtime, runtime);
        assert!(
            refusal.to_string().contains("single-tenant"),
            "the refusal has to name the way out",
        );
    }
}

/// And the gate does not refuse what it must serve, or the safe deployment is the unusable one.
#[tokio::test]
async fn a_shared_deployment_still_serves_http() {
    let (wire, egress) = silent_egress();
    let invoker = invoker(github_store(Some(SENTINEL)).await, egress);

    invoker
        .invoke(
            &caller(),
            "github-repo-get",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await
        .expect("github declares http, which a multi-tenant deployment admits");

    assert_eq!(wire.lock().expect("the wire lock").calls.len(), 1);
}
