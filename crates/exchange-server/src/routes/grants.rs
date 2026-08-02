//! What a tenant may run, read and edited.
//!
//! ```text
//! GET  /api/grants          the grants this tenant holds, and what each one currently admits
//! PUT  /api/grants          replace them, whole-set
//! POST /api/grants/preview  what a proposed grant would admit, before it is saved
//! ```
//!
//! # Why this exists (X-62)
//!
//! X-13 closed a real exposure and closed it **fail-closed**: an operation runs only if one of the
//! caller's tenant's grants admits it. What it left behind is that a deployment now runs *nothing*
//! until somebody hand-writes `FLUX_EXCHANGE_GRANTS`, and until this module there was no route, no
//! screen and no command that wrote one. Invoke was reachable, correct, gated — and unusable
//! through the product.
//!
//! # This surface expresses a **selector**, and never a list of operation ids
//!
//! The rule is X-13's Goal — a grant is *"decided from the operation's declared metadata, not from a
//! list of names"* — and it is not a style preference. It is the same reasoning that makes X-47's
//! host rule read the catalogue instead of a hand-written list, where it caught four vulnerable
//! connectors and a list would have caught two. A grant written as `risk <= low` covers a connector's
//! next operation correctly on the day it lands; a grant written as five ids silently stops covering
//! it, and nobody finds out.
//!
//! So the body this surface takes is a connector and three axes — `max_risk`, `effects_within`,
//! `idempotency` — and a request that names an operation id is **refused** rather than narrowed.
//! [`Selector`] does carry `allow_ids` and `deny_ids`, deliberately, as an operator's last-resort
//! exception in a file they edit by hand; a route that deserialised `Selector` verbatim would let a
//! console write ids straight back into the model, and the property the gate was built around would
//! be gone through the one path that edits it.
//!
//! # The preview is most of the value
//!
//! A grant nobody can evaluate before saving is a grant somebody sets too wide. So every answer here
//! carries **which operations the grant admits**, derived from
//! [`OperationFacts::of`](exchange_host::OperationFacts::of) and
//! [`ConnectorSurface::admitted`] — the same projection and the same predicate
//! `exchange_host::admit_grant` decides on, rather than a second copy of the rules living beside the
//! screen that renders them. `super::tests::a_grant_written_through_the_surface_admits_exactly_what_the_gate_admits`
//! is what holds the two together, and it asserts against `admit_grant` itself.
//!
//! # Where the store comes from, and why it is not a port of its own
//!
//! Through [`Invoker::grants`](exchange_host::Invoker::grants), which is the same `Arc` the gate
//! reads. `exchange_host::Grants` states the hazard in as many words — *"a composition that could
//! bind a reader to the invoker and a writer to a surface would have two stores that disagree about
//! what a tenant is allowed to do"* — and binding a second port on `AppState` beside the invoker is
//! exactly the composition that could. One binding means the grant an operator sees here and the
//! grant that decides an invocation are the same object, by construction rather than by wiring.
//!
//! It also keeps the Acceptance's last item true for free: an invoker exists exactly when a
//! credential store **and** a grant store are bound, so a deployment missing either answers `503`
//! here naming both settings — the same fact `super::invoke` reports, from the same binding.
//!
//! # Whoever may edit a grant decides what the tenant runs
//!
//! [`MAY_GRANT`] carries the argument. In short: it is strictly more authority than supplying a
//! credential, so it is gated at least as narrowly (X-54's `MAY_SUPPLY_A_CREDENTIAL`), and the
//! **read** is gated too — which is the half that is easy to get wrong.
//!
//! # Nothing here reaches an anonymous caller
//!
//! What a tenant is granted is tenant data. Both paths are `Access::PrincipalOfKind`, the catalogue
//! next door still answers `admitted: null` for every operation and still never reads a grant, and
//! `routes::onboarding::tests::the_document_is_identical_with_two_tenants_connected` drives the
//! descriptor adversarially against two tenants' state. Nothing in this module is reachable from any
//! of them.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{Provider, ProviderKey};
use exchange_host::{
    ConnectorSurface, Effect, Grant, Grants, Idempotency, OperationFacts, Principal, PrincipalKind,
    Risk, Selector,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use super::{Access, Module, Route};
// The setting that names the grant store, quoted in every refusal that cannot find one. Spelled
// through `invoke`'s constant, which is spelled through the host's own, so the refusals that quote
// it here and there cannot drift into two names.
use super::invoke::GRANT_SETTING;
use crate::state::AppState;

/// **Who may read or edit what a tenant may run: a `User`, and nothing else.**
///
/// The same kinds as [`MAY_SUPPLY_A_CREDENTIAL`](super::connections::MAY_SUPPLY_A_CREDENTIAL), and
/// deliberately not one kind wider. `super::tests::the_surface_edits_a_grant_and_the_write_is_no_wider_than_supplying_a_credential`
/// pins the two together rather than writing this list out a second time, because the Acceptance's
/// wording is *at least as narrow as* — and a widening over there should not silently widen this.
///
/// # Why the write is at least that authority
///
/// X-54's argument for gating credential supply is that a caller which decides **which** credential
/// a tenant's operations run under has been granted the credential position, whether or not it ever
/// sees a value. Editing a grant is that argument's larger sibling: it decides **which operations
/// run at all**, for every principal of the tenant, across every connection it holds. An agent that
/// could write here would grant itself the remainder of the catalogue in one request — which makes
/// every other kind gate on this surface advisory, including the one that stops it minting
/// successors.
///
/// `Service` is refused on `agents::MAY_MINT`'s reasoning rather than a new one: nothing in this
/// repository mints, verifies, lists or revokes a service credential, so a policy decision this host
/// cannot attribute to a revocable caller is an incomplete remedy one level out of sight.
///
/// # Why the **read** is gated too, which is the half that is easy to get wrong
///
/// `GET /api/connections` answers every kind, and the argument there is good: an agent that can see
/// *"this tenant has no zendesk connection"* can say so instead of failing an invocation for a
/// reason nobody can act on. It does not carry over, and `exchange_host::admit_grant` says why in
/// its own `# Errors` section: the refusal it produces names neither the grants the tenant holds nor
/// the axis that refused, because *"an agent learning which predicate turned it down can enumerate a
/// tenant's policy one call at a time"*. A read open to every kind would hand that whole policy over
/// in a single request, and undo a decision X-13 made deliberately.
///
/// What it costs is real and is worth naming: an agent cannot discover in advance what it may run,
/// so it finds out by being refused. The refusal is terminal, names the operation, and says what to
/// do about it — ask whoever holds the tenant — which is the same remedy either way.
///
/// # What it does *not* close: there is no operator kind
///
/// `User` is **every** signed-in human of the tenant, so this says *a human, not a bot*; it cannot
/// say *the human who set this tenant up*. That is the same within-tenant gap
/// [`MAY_SUPPLY_A_CREDENTIAL`](super::connections::MAY_SUPPLY_A_CREDENTIAL) and
/// [`MAY_CONFIGURE`](super::connections::MAY_CONFIGURE) both record, and it is the same gap rather
/// than a third one: answering it needs a notion of who *administers* a tenant, which is a policy
/// model this identity vocabulary does not have and which no kind gate can supply.
pub(super) const MAY_GRANT: &[PrincipalKind] = &[PrincipalKind::User];

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "grants",
    routes: &[
        Route {
            // Read and write on **one** path and in one declaration, rather than a `Route` per
            // verb. `super::connections` publishes `/api/connections/{connector}` twice because its
            // verbs differ in [`Access`]; these do not, and X-61 records what a duplicated path
            // costs — the second declaration is invisible to the surface-wide anonymous probe,
            // which drives one `GET` per entry and therefore answers for the first declaration
            // twice. One declaration is one thing for that enumeration to see.
            //
            // No parameter, and there is nowhere one could go: the tenant is read off the resolved
            // principal, and a grant is addressed by the connector *inside* the body it carries.
            path: "/api/grants",
            access: Access::PrincipalOfKind(MAY_GRANT),
            method_router: grants_route,
        },
        Route {
            // What a proposed grant would admit, decided but not stored. A path of its own so that
            // evaluating a policy is not one typo away from applying it, and so the answer is a
            // `POST` with a body rather than a selector smuggled through a query string.
            path: "/api/grants/preview",
            access: Access::PrincipalOfKind(MAY_GRANT),
            method_router: preview_route,
        },
    ],
};

fn grants_route() -> MethodRouter<AppState> {
    get(held).put(replace)
}

fn preview_route() -> MethodRouter<AppState> {
    post(preview)
}

// ---------------------------------------------------------------------------------------------
// What a caller may send
// ---------------------------------------------------------------------------------------------

/// The whole of what a tenant may run, as a caller states it.
///
/// Whole-set rather than add-one, which is [`exchange_host::Grants::set`]'s own decision restated at
/// the wire: a grant is an authorisation decision, and what an operator needs to be able to state is
/// *what this tenant may do*, entire. A `revoke(one)` beside a `grant(one)` is a sequence nobody can
/// see the end state of.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposedGrants {
    /// One grant per connector. An empty list is a tenant that may run nothing, which is a decision
    /// a caller is allowed to make and is the state every deployment starts in.
    grants: Vec<ProposedGrant>,
}

/// One grant, as a caller states it: a connector and a predicate over declared metadata.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposedGrant {
    /// The connector this grant reaches, in the catalogue's own spelling. Never a wildcard — see
    /// [`Grant::connector`].
    connector: String,
    /// Which of its operations.
    selector: ProposedSelector,
}

/// The three axes this surface expresses, and **only** those.
///
/// Deliberately not [`Selector`], which also carries `allow_ids` and `deny_ids`. Those exist for an
/// operator editing the file by hand and they are exactly what this surface must not write — see the
/// module documentation. `deny_unknown_fields` is the mechanism, and [`names_an_operation`] is what
/// turns the resulting serde message into a refusal that says *why*.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposedSelector {
    /// Admit only operations at or below this risk. Absent admits every level.
    #[serde(default)]
    max_risk: Option<Risk>,
    /// Admit only operations whose effects are a subset of this set. Absent admits any effects.
    #[serde(default)]
    effects_within: Option<BTreeSet<Effect>>,
    /// Admit only operations with this idempotency. Absent admits any.
    #[serde(default)]
    idempotency: Option<Idempotency>,
}

impl ProposedSelector {
    /// The host's own selector, with both exception lists empty **by construction**.
    ///
    /// `..Selector::default()` rather than naming the two lists, so a field added to [`Selector`]
    /// later arrives here defaulted rather than caller-settable. That is the safe direction: a new
    /// axis this surface has not decided about must not become expressible by a caller because a
    /// struct literal happened to be exhaustive.
    fn resolve(self) -> Selector {
        Selector {
            max_risk: self.max_risk,
            effects_within: self.effects_within,
            idempotency: self.idempotency,
            ..Selector::default()
        }
    }
}

/// The spellings a caller might reach for when it wants to name an operation.
///
/// Written out rather than inferred, and checked **before** `deny_unknown_fields` gets to say
/// "unknown field": a serde message is the right refusal for a typo and the wrong one for this,
/// which is not a mistake but a different model of what a grant is. The list covers the two fields
/// [`Selector`] really has and the four spellings somebody writing a client would try.
const NAMES_AN_OPERATION: &[&str] = &[
    "allow_ids",
    "deny_ids",
    "allow",
    "deny",
    "operation",
    "operations",
];

/// The first key anywhere in `value` that names operations, or `None`.
///
/// Recursive, so nesting cannot hide one. A grant body is a handful of objects; walking it costs
/// nothing next to being wrong about what was asked for.
fn names_an_operation(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(fields) => NAMES_AN_OPERATION
            .iter()
            .find(|named| fields.contains_key(**named))
            .copied()
            .or_else(|| fields.values().find_map(names_an_operation)),
        Value::Array(items) => items.iter().find_map(names_an_operation),
        _ => None,
    }
}

/// Why a proposed grant is not one.
///
/// A value rather than a [`Response`] built where the fault is found, in the shape
/// `connections::settings_refused` uses: one place turns a refusal into a status, so a variant added
/// later cannot be answered two different ways by the two handlers that raise it. (It is also what
/// keeps a fallible helper's `Err` small enough for clippy's `result_large_err`, which is a real
/// reading of the same design — an error that is already a rendered HTTP response is an error that
/// has made a presentation decision too early.)
#[derive(Debug)]
enum Refusal {
    /// The body names operations rather than declaring a predicate.
    NamesOperations(&'static str),
    /// The body is not a set of grants at all.
    Unreadable(serde_json::Error),
    /// Nothing in this build's catalogue is spelled that way.
    UnknownConnector(String),
    /// One connector, twice, in one set.
    Twice(&'static str),
}

/// One proposed grant, resolved against the catalogue.
///
/// The resolved connector is **the catalogue's spelling and not the caller's string**, which is the
/// gap `exchange_host::Granted` names as uncovered by the type system: a grant whose connector is
/// not a connector would sit in the store admitting nothing and looking like policy. Here it cannot
/// be stored at all.
fn resolve(proposed: ProposedGrant) -> Result<(&'static Provider, Grant), Refusal> {
    let Some(provider) = catalogued(&proposed.connector) else {
        return Err(Refusal::UnknownConnector(proposed.connector));
    };

    Ok((
        provider,
        Grant::for_connector(provider.id, proposed.selector.resolve()),
    ))
}

/// Every proposed grant in a body.
fn resolve_set(body: Value) -> Result<Vec<Grant>, Refusal> {
    if let Some(named) = names_an_operation(&body) {
        return Err(Refusal::NamesOperations(named));
    }

    let proposed: ProposedGrants = serde_json::from_value(body).map_err(Refusal::Unreadable)?;

    let mut resolved: Vec<Grant> = Vec::with_capacity(proposed.grants.len());
    for grant in proposed.grants {
        let (provider, grant) = resolve(grant)?;

        // One grant per connector, which is the shape the store is keyed for: `exchange_host`'s
        // file binding says a second grant at one connector is a shape `Selector` expresses inside a
        // single grant instead. Two here would be two policies for one connector with no rule
        // saying which wins, and the honest answer is to refuse rather than to pick.
        if resolved
            .iter()
            .any(|existing| existing.connector == grant.connector)
        {
            return Err(Refusal::Twice(provider.id));
        }

        resolved.push(grant);
    }

    Ok(resolved)
}

// ---------------------------------------------------------------------------------------------
// The handlers
// ---------------------------------------------------------------------------------------------

/// What this tenant may run.
///
/// Reports what is **stored**, including a grant this surface could not have written — see
/// [`view`]. A read that quietly omitted an id exception would let an operator replace a set they
/// had never been shown.
async fn held(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(store) = store(&state) else {
        return no_grant_store();
    };

    Json(document(&store.held(principal.tenant()))).into_response()
}

/// Replace what this tenant may run.
///
/// The tenant is the guard's and nothing in the body moves it. What a caller supplies is a set of
/// connectors and predicates; the store, the address and the principal are each derived from
/// something the caller did not send.
async fn replace(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> Response {
    let Some(store) = store(&state) else {
        return no_grant_store();
    };

    let proposed = match resolve_set(body) {
        Ok(proposed) => proposed,
        Err(refusal) => return refused(refusal),
    };

    // **Refuse; never repair.** This surface cannot express an id exception, so it must not replace
    // a set that holds one: the write would silently drop a `deny` an operator meant, and the only
    // evidence would be an operation running that used to be refused. The remedy is stated rather
    // than guessed at — take the exception out of the file, or edit the file.
    let existing = store.held(principal.tenant());
    if let Some(grant) = existing.iter().find(|grant| names_ids(&grant.selector)) {
        return would_drop_an_exception(&grant.connector);
    }

    if let Err(refusal) = store.set(principal.tenant(), &proposed) {
        // The refusal names this host's own store path, which is a fact about the deployment and
        // none of a caller's business. It goes to the log; the caller gets the setting's name.
        error!(%refusal, %principal, "a tenant's grants could not be stored");
        return store_refused();
    }

    crate::audit::grants_replaced(&principal, proposed.len());
    Json(document(&proposed)).into_response()
}

/// What a proposed grant would admit, without storing it.
///
/// **Reads nothing tenant-specific and needs no store**, which is why it answers on a composition
/// that bound neither: it is a pure function of the catalogue and of what the caller just typed. An
/// operator evaluating a policy on a host they have not finished configuring is exactly the person
/// this route is for.
async fn preview(Json(body): Json<Value>) -> Response {
    if let Some(named) = names_an_operation(&body) {
        return refused(Refusal::NamesOperations(named));
    }

    let proposed: ProposedGrant = match serde_json::from_value(body) {
        Ok(proposed) => proposed,
        Err(error) => return refused(Refusal::Unreadable(error)),
    };

    match resolve(proposed) {
        Ok((_, grant)) => Json(view(&grant)).into_response(),
        Err(refusal) => refused(refusal),
    }
}

// ---------------------------------------------------------------------------------------------
// What a caller sees
// ---------------------------------------------------------------------------------------------

/// A tenant's whole set, as an operator reads it.
///
/// `editable` is derived from the rows rather than asserted beside them, in the shape
/// `connections::list_settings`' `configurable` uses: a top-level claim that could disagree with the
/// entries under it is a claim a console would render and an operator would act on.
fn document(grants: &[Grant]) -> Value {
    json!({
        "grants": grants.iter().map(view).collect::<Vec<Value>>(),
        // Whether `PUT` on this path would be a faithful replacement of what is stored. False when
        // anything held names an operation id, because this surface cannot write that back.
        "editable": grants.iter().all(|grant| !names_ids(&grant.selector)),
    })
}

/// One grant: the selector this surface speaks, and **which operations it currently admits**.
///
/// The `admits` list is the whole point of the route. It is derived through
/// [`ConnectorSurface::admitted`] — the host's own function, over
/// [`OperationFacts::of`](exchange_host::OperationFacts::of)'s projection — rather than by a
/// predicate written here, because a preview that reimplemented `Selector::admits` would be a second
/// answer to *"what does this grant admit"* and the one an operator reads would be the one that is
/// not deciding.
///
/// `declares` beside it is what stops the list reading as an absolute: *3 admitted* means nothing
/// until you know whether the connector declares 4 or 400.
fn view(grant: &Grant) -> Value {
    let Some(provider) = catalogued(&grant.connector) else {
        // A stored grant naming a connector this build does not carry. Shown rather than dropped —
        // it is in the file, it is what an operator would find there, and a read that hid it would
        // make the `editable` flag below a surprise.
        return json!({
            "connector": grant.connector,
            "selector": selector_view(&grant.selector),
            "expressible": false,
            "reason": format!(
                "this build's catalogue carries no connector `{}`, so nothing here can say what \
                 this grant admits and this surface would refuse to write it back",
                grant.connector,
            ),
            "declares": 0,
            "admits": [],
        });
    };

    // The connector's whole declared surface, projected the way the gate projects it.
    // `ConnectorSurface::of` deliberately leaves `operations` empty — it exists for the runtime
    // decision, which needs none — so they are filled in here from the same `OperationFacts::of`.
    let surface = ConnectorSurface {
        operations: provider.operations.iter().map(OperationFacts::of).collect(),
        ..ConnectorSurface::of(provider)
    };

    let held = [grant.clone()];
    let admitted = surface.admitted(&held);
    let admits: Vec<&OperationFacts> = surface
        .operations
        .iter()
        .filter(|operation| admitted.contains(operation.id.as_str()))
        .collect();

    let mut view = json!({
        "connector": provider.id,
        "vendor": provider.vendor,
        "selector": selector_view(&grant.selector),
        // Whether this surface could have written this grant, and could write it again.
        "expressible": !names_ids(&grant.selector),
        "declares": provider.operations.len(),
        "admits": admits,
    });

    if names_ids(&grant.selector) {
        view["reason"] = json!(
            "this grant names operations explicitly, which this surface does not express — a grant \
             decided from a list of names silently stops covering a connector the moment that \
             connector gains an operation. It is shown as stored, and replacing this tenant's \
             grants here is refused rather than dropping it"
        );
        view["exempt"] = json!({
            "always": grant.selector.allow_ids,
            "never": grant.selector.deny_ids,
        });
    }

    view
}

/// The selector, in the vocabulary this surface takes back.
///
/// Three axes and no id lists, so a caller can read this answer, change one field and `PUT` it
/// without tripping [`names_an_operation`]. Echoing [`Selector`] verbatim would publish two empty
/// arrays whose only effect on a round trip is a refusal.
fn selector_view(selector: &Selector) -> Value {
    json!({
        "max_risk": selector.max_risk,
        "effects_within": selector.effects_within,
        "idempotency": selector.idempotency,
    })
}

/// Whether a selector names operations rather than only declaring a predicate.
fn names_ids(selector: &Selector) -> bool {
    !selector.allow_ids.is_empty() || !selector.deny_ids.is_empty()
}

// ---------------------------------------------------------------------------------------------
// Refusals — every one names the rule, and none of them names another tenant's anything
// ---------------------------------------------------------------------------------------------

/// A refusal as a caller sees it: a status, a reason and the fields it was decided against.
fn refuse(status: StatusCode, reason: impl Into<String>, mut extra: Value) -> Response {
    let body = extra
        .as_object_mut()
        .expect("every refusal here is built from a JSON object");
    body.insert("error".to_owned(), json!(reason.into()));

    (status, Json(extra)).into_response()
}

/// How a [`Refusal`] reaches a caller — **the one place**, so a variant added later cannot be
/// answered two different ways by the two handlers that raise it.
///
/// Every variant is `422` and that is the reading rather than a shortcut: in each the request was
/// well formed and reached a route it is entitled to call, and what it *asks for* is something this
/// surface does not express. `connections::unreadable_field` reads the same way. None of them is
/// `403` — nothing here is about who is asking — and none is `404`, which would hide a route the
/// caller has already reached.
fn refused(refusal: Refusal) -> Response {
    /// The vocabulary a caller is pointed back at, quoted from one place so a refusal cannot list
    /// an axis this surface no longer takes.
    const EXPRESSES: [&str; 3] = ["max_risk", "effects_within", "idempotency"];

    let (reason, extra) = match refusal {
        // The message carries the **argument** and not only the rule, because a caller that sent
        // this has a different model of what a grant is, and "unknown field" would leave them
        // hunting for a spelling mistake.
        Refusal::NamesOperations(named) => (
            format!(
                "`{named}` names operations, and a grant written here selects them by what they \
                 declare rather than by what they are called: a list of names silently stops \
                 covering a connector the moment that connector gains an operation, while \
                 `max_risk`, `effects_within` and `idempotency` cover the new one correctly on the \
                 day it lands. Send a selector — POST it to /api/grants/preview first to see which \
                 operations it admits"
            ),
            json!({ "field": named, "selector_expresses": EXPRESSES }),
        ),
        // serde's own message, which names the field and the position. This host did not compose
        // the document and does not paraphrase what it could not read.
        Refusal::Unreadable(error) => (
            format!(
                "this is not a set of grants: {error}. A grant is `{{\"connector\": <catalogue \
                 id>, \"selector\": {{\"max_risk\": …, \"effects_within\": […], \"idempotency\": \
                 …}}}}`, and every field of the selector may be omitted"
            ),
            json!({ "selector_expresses": EXPRESSES }),
        ),
        // The catalogue that would have answered is anonymous, so pointing at it discloses
        // nothing a stranger could not already read.
        Refusal::UnknownConnector(connector) => (
            format!(
                "this build carries no connector `{connector}`, so a grant for it would admit \
                 nothing and hide the mistake. `GET /api/catalogue/connectors` lists what this \
                 build carries"
            ),
            json!({ "connector": connector }),
        ),
        Refusal::Twice(connector) => (
            format!(
                "this set names connector `{connector}` twice. A tenant holds one grant per \
                 connector — two predicates for one connector is a policy with no stated \
                 precedence, and the shape for \"these operations and also those\" is one selector \
                 that admits both"
            ),
            json!({ "connector": connector }),
        ),
    };

    refuse(StatusCode::UNPROCESSABLE_ENTITY, reason, extra)
}

/// This tenant holds a grant this surface cannot write back.
///
/// `409` rather than `422`: the request is expressible and it is this **tenant's own state** that
/// conflicts with it — the same reading `connections`' in-flight refusals take. And a refusal rather
/// than a write that drops the exception, because the evidence of the drop would be an operation
/// running that used to be refused, which is the failure the whole gate exists to prevent.
///
/// It names the connector, which is this tenant's own state told to a caller who already holds the
/// tenant, and it does not name the operations — those are in the answer to `GET /api/grants`, which
/// is where an operator goes to look.
fn would_drop_an_exception(connector: &str) -> Response {
    refuse(
        StatusCode::CONFLICT,
        format!(
            "this tenant's grant for `{connector}` names operations explicitly, and this surface \
             does not express that: replacing the set here would drop the exception silently, and \
             the only sign of it would be an operation running that used to be refused. Read \
             `GET /api/grants` to see what is held, and remove the exception where it was written"
        ),
        json!({ "connector": connector }),
    )
}

/// This composition holds no grant store, so there is nowhere for a decision to live.
///
/// `503`, naming **both** settings, in the terms `super::invoke::no_invoker` already uses and for
/// its reason: an invoker exists exactly when a credential store and a grant store are both bound,
/// this host does not say which one is missing — that is a fact about the composition and a caller
/// who is not the operator learns nothing useful from it — and one composition fact described two
/// ways would be two sentences to keep in step.
///
/// The rest of this host is unaffected: `/health`, the catalogue and the descriptor all still
/// answer, which is the Acceptance's last item and what
/// `tests::a_host_with_no_grant_store_refuses_here_and_serves_everything_else` drives.
fn no_grant_store() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "this host holds no grants: it needs a grant store (`{GRANT_SETTING}`) for a decision \
             about what a tenant may run to live in, and a credential store (`{}`) to run anything \
             with, and it is missing at least one of them",
            super::connections::STORE_SETTING,
        ),
        json!({ "setting": GRANT_SETTING }),
    )
}

/// The store would not take the write.
///
/// `503` on `connections::store_failed`'s split: a store that cannot be *written* may take the same
/// write next time, which is a different event from one that cannot be read. Nothing was applied —
/// `exchange_host::Grants::set` refuses rather than leaving memory and file disagreeing — so a
/// caller that retries is not repeating a partial change.
fn store_refused() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "this host could not store the grants: nothing was applied, and the store this \
             deployment named in `{GRANT_SETTING}` is where an operator has to look"
        ),
        json!({ "setting": GRANT_SETTING }),
    )
}

// ---------------------------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------------------------

/// The grant store this composition bound, through the invoker that decides against it.
fn store(state: &AppState) -> Option<&Arc<dyn Grants>> {
    state.invoker().map(|invoker| invoker.grants())
}

/// The catalogue entry for `connector`, or `None`.
///
/// Written here rather than shared with `connections`: it is two lines, and a helper reaching across
/// two route modules is a coupling neither of them needs.
fn catalogued(connector: &str) -> Option<&'static Provider> {
    connector_catalog::provider(ProviderKey::id(connector))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::Mutex;

    use axum::body::Body;
    use axum::http::{header, Method, Request as HttpRequest};
    use axum::Router;
    use exchange_host::{
        address_path, async_trait, CredentialRef, GrantRefusal, Secret, SecretStore, StoreError,
        Tenant,
    };
    use tower::Service;

    use crate::dev_identity::DevIdentity;

    /// Two tenants and two kinds, so nothing below can pass by answering the same thing to
    /// everyone.
    const ROSTER: &str = "user:alice@acme,user:bob@globex,agent:bot@acme";

    /// A grant store that lives in the test, keyed by tenant.
    ///
    /// Hand-rolled rather than published from `exchange_host` for `invoke::tests::HeldGrants`'
    /// reason: an in-memory store re-exported from the library crate is a fallback a production
    /// composition could bind, and `AGENTS.md` refuses one.
    #[derive(Default)]
    struct StoredGrants(Mutex<HashMap<String, Vec<Grant>>>);

    impl StoredGrants {
        /// Arm the store with what a tenant already holds — a hand-written file, which is the only
        /// way a grant existed before this story.
        fn holding(tenant: &str, grants: Vec<Grant>) -> Arc<Self> {
            let store = Self::default();
            store
                .0
                .lock()
                .expect("no test poisons this")
                .insert(tenant.to_owned(), grants);
            Arc::new(store)
        }

        fn read(&self, tenant: &str) -> Vec<Grant> {
            self.0
                .lock()
                .expect("no test poisons this")
                .get(tenant)
                .cloned()
                .unwrap_or_default()
        }
    }

    impl Grants for StoredGrants {
        fn held(&self, tenant: &Tenant) -> Vec<Grant> {
            self.read(tenant.as_str())
        }

        fn set(&self, tenant: &Tenant, grants: &[Grant]) -> Result<(), GrantRefusal> {
            self.0
                .lock()
                .expect("no test poisons this")
                .insert(tenant.as_str().to_owned(), grants.to_vec());
            Ok(())
        }
    }

    /// A store that refuses every write, for the one refusal an operator has to be able to read.
    struct UnwritableGrants;

    impl Grants for UnwritableGrants {
        fn held(&self, _: &Tenant) -> Vec<Grant> {
            Vec::new()
        }

        fn set(&self, _: &Tenant, _: &[Grant]) -> Result<(), GrantRefusal> {
            Err(GrantRefusal::Unwritable {
                path: "/var/lib/flux-exchange/grants".to_owned(),
                reason: "read-only file system".to_owned(),
            })
        }
    }

    /// A credential store that holds nothing. Nothing in this module reads one.
    struct NoCredentials;

    #[async_trait]
    impl SecretStore for NoCredentials {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            Err(StoreError::NotFound {
                path: address_path(reference),
            })
        }

        async fn put(&self, _: &CredentialRef, _: &Secret) -> Result<(), StoreError> {
            unreachable!("editing a grant stores no credential")
        }

        async fn delete(&self, _: &CredentialRef) -> Result<(), StoreError> {
            unreachable!("editing a grant destroys no credential")
        }
    }

    /// A composition that can sign the roster in and that holds `grants`.
    fn editing(grants: Arc<dyn Grants>) -> AppState {
        let invoker = Arc::new(
            crate::execution::invoker(
                exchange_host::Deployment::MultiTenant,
                Arc::new(NoCredentials),
                Arc::new(exchange_host::MemoryConfig::new()),
                grants,
            )
            .expect("a usable workspace root"),
        );

        signed_in().with_invoker(invoker)
    }

    /// A composition that can sign the roster in and binds nothing else.
    fn signed_in() -> AppState {
        AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
    }

    /// Drive one request through a fully assembled app and hand back the status and parsed body.
    async fn driven(
        state: AppState,
        method: Method,
        path: &str,
        handle: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let app: Router = super::super::app(state);
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {handle}"));

        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("a well-formed request"),
            None => request.body(Body::empty()).expect("a well-formed request"),
        };

        let response = service
            .call(request)
            .await
            .expect("a router is infallible")
            .into_response();

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a response body");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Drive one `GET` with no credential at all, and hand back the **bytes**.
    ///
    /// Unparsed on purpose: the comparison
    /// `nothing_a_tenant_is_granted_reaches_an_anonymous_caller` makes is byte identity, which is
    /// the only assertion that covers a field nobody has thought of yet.
    async fn anonymously(state: AppState, path: &str) -> (StatusCode, String) {
        let app: Router = super::super::app(state);
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .expect("a well-formed request");

        let response = service
            .call(request)
            .await
            .expect("a router is infallible")
            .into_response();

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a response body");

        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A read-only grant over one connector, as a caller sends it.
    fn read_only(connector: &str) -> Value {
        json!({
            "grants": [{
                "connector": connector,
                "selector": { "max_risk": "low" },
            }],
        })
    }

    /// The operation ids one answer says a grant admits.
    fn admits(document: &Value, index: usize) -> BTreeSet<String> {
        document["grants"][index]["admits"]
            .as_array()
            .unwrap_or_else(|| panic!("a grant carries what it admits: {document}"))
            .iter()
            .map(|facts| {
                facts["id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("an admitted operation carries its id: {document}"))
                    .to_owned()
            })
            .collect()
    }

    // -----------------------------------------------------------------------------------------
    // Reading and editing
    // -----------------------------------------------------------------------------------------

    /// A tenant that has been granted nothing reads an empty set, and it is not an error.
    ///
    /// The state every deployment starts in since X-13, and the one an operator is most likely to
    /// meet first. An empty `200` and not a `404`: the tenant exists and holds nothing, which is a
    /// different fact from "there is no such thing here".
    #[tokio::test]
    async fn a_tenant_granted_nothing_reads_an_empty_set() {
        let (status, body) = driven(
            editing(Arc::new(StoredGrants::default())),
            Method::GET,
            "/api/grants",
            "alice",
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["grants"], json!([]));
        assert_eq!(
            body["editable"], true,
            "a tenant holding nothing can be given something: {body}",
        );
    }

    /// A grant written through the surface is read back, and one tenant's is not another's.
    ///
    /// The tenant comes from the resolved principal on both verbs, so this is also the vector test:
    /// `alice` writes and `bob` — a `User` of another tenant, on the same host and through the same
    /// routes — sees nothing.
    #[tokio::test]
    async fn a_grant_is_written_read_back_and_is_one_tenants_own() {
        let store = Arc::new(StoredGrants::default());

        let (status, body) = driven(
            editing(store.clone()),
            Method::PUT,
            "/api/grants",
            "alice",
            Some(read_only("github")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["grants"][0]["connector"], "github");

        let (status, body) = driven(
            editing(store.clone()),
            Method::GET,
            "/api/grants",
            "alice",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["grants"][0]["selector"]["max_risk"], "low");

        let (status, body) = driven(
            editing(store.clone()),
            Method::GET,
            "/api/grants",
            "bob",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["grants"],
            json!([]),
            "one tenant's grants answered for another's: {body}",
        );

        assert!(
            store.read("globex").is_empty(),
            "the write landed under a tenant the caller did not name and does not belong to",
        );
    }

    /// The set is replaced whole, not appended to.
    ///
    /// [`exchange_host::Grants::set`]'s own decision, driven at the wire: what an operator states is
    /// *what this tenant may do*, entire. A `PUT` that merged would make the end state of two edits
    /// something nobody wrote down.
    #[tokio::test]
    async fn a_write_replaces_the_whole_set() {
        let store = Arc::new(StoredGrants::default());

        for connector in ["github", "slack"] {
            let (status, body) = driven(
                editing(store.clone()),
                Method::PUT,
                "/api/grants",
                "alice",
                Some(read_only(connector)),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }

        let held = store.read("acme");
        assert_eq!(held.len(), 1, "the second write did not replace the first");
        assert_eq!(held[0].connector, "slack");
    }

    // -----------------------------------------------------------------------------------------
    // The preview
    // -----------------------------------------------------------------------------------------

    /// The preview says which operations a proposed grant would admit, and it is the selector that
    /// decides.
    ///
    /// Two selectors over one connector through the same route: `low` admits fewer operations than
    /// `destructive`, both are proper non-empty subsets of what the connector declares, and neither
    /// names an operation anywhere. A preview that answered the same list for both would be a
    /// decoration, and an operator would set a grant far wider than they read.
    #[tokio::test]
    async fn the_preview_answers_what_a_selector_would_admit_and_the_selector_decides() {
        let mut widths = Vec::new();

        for level in ["low", "destructive"] {
            let (status, body) = driven(
                signed_in(),
                Method::POST,
                "/api/grants/preview",
                "alice",
                Some(json!({
                    "connector": "github",
                    "selector": { "max_risk": level },
                })),
            )
            .await;

            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["connector"], "github");

            let admitted: BTreeSet<String> = body["admits"]
                .as_array()
                .unwrap_or_else(|| panic!("the preview carries what it admits: {body}"))
                .iter()
                .map(|facts| facts["id"].as_str().expect("an id").to_owned())
                .collect();

            let declares = body["declares"].as_u64().expect("a count") as usize;
            assert!(
                !admitted.is_empty() && admitted.len() <= declares,
                "`{level}` admits {} of {declares}, which is not a subset anybody can read: {body}",
                admitted.len(),
            );

            widths.push((admitted, declares));
        }

        let (narrow, declares) = &widths[0];
        let (wide, _) = &widths[1];

        assert!(
            narrow.len() < wide.len(),
            "`low` and `destructive` admit the same {} operations, so the selector is not what \
             decides this answer",
            narrow.len(),
        );
        assert!(
            narrow.is_subset(wide),
            "a narrower risk bound admitted something a wider one did not",
        );
        assert!(
            wide.len() <= *declares,
            "the preview admits more than the connector declares",
        );
    }

    /// The preview needs no store bound, because it reads nothing this host holds.
    ///
    /// The composition an operator meets before they have finished configuring anything, and the
    /// structural claim that this route touches no tenant state: a host with no grant store cannot
    /// answer `GET /api/grants` and answers this one in full.
    #[tokio::test]
    async fn the_preview_answers_on_a_host_with_no_store_bound() {
        let (status, body) = driven(
            signed_in(),
            Method::POST,
            "/api/grants/preview",
            "alice",
            Some(json!({ "connector": "slack", "selector": {} })),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(
            !admits(&json!({ "grants": [body.clone()] }), 0).is_empty(),
            "an empty selector admits everything the connector declares: {body}",
        );
    }

    /// The preview stores nothing. Evaluating a policy must not be one typo away from applying it.
    #[tokio::test]
    async fn the_preview_stores_nothing() {
        let store = Arc::new(StoredGrants::default());

        let (status, body) = driven(
            editing(store.clone()),
            Method::POST,
            "/api/grants/preview",
            "alice",
            Some(json!({ "connector": "github", "selector": { "max_risk": "destructive" } })),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(
            store.read("acme").is_empty(),
            "the preview wrote a grant, so an operator evaluating a policy has applied one",
        );
    }

    // -----------------------------------------------------------------------------------------
    // Refusals
    // -----------------------------------------------------------------------------------------

    /// The preview refuses an operation id too, and not only the write.
    ///
    /// The write is the one that matters and `super::super::tests` drives it; this is the path a
    /// client would find first, and a preview that accepted ids would be a surface teaching the
    /// model the write then refuses.
    #[tokio::test]
    async fn the_preview_refuses_a_selector_that_names_an_operation() {
        let (status, body) = driven(
            signed_in(),
            Method::POST,
            "/api/grants/preview",
            "alice",
            Some(json!({
                "connector": "github",
                "selector": { "deny_ids": ["github-repo-get"] },
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["field"], "deny_ids");
        assert!(
            body["error"]
                .as_str()
                .expect("a refusal carries a reason")
                .contains("declare"),
            "the refusal must carry the argument and not only the rule: {body}",
        );
    }

    /// A connector this build does not carry is refused rather than stored.
    ///
    /// A grant for a connector that does not exist admits nothing and reads like policy, which is
    /// the worst of both: an operator believes they have granted something.
    #[tokio::test]
    async fn a_grant_for_a_connector_this_build_does_not_carry_is_refused() {
        let store = Arc::new(StoredGrants::default());

        let (status, body) = driven(
            editing(store.clone()),
            Method::PUT,
            "/api/grants",
            "alice",
            Some(read_only("no-such-connector")),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["connector"], "no-such-connector");
        assert!(store.read("acme").is_empty(), "a refused grant was stored");
    }

    /// One connector, twice, in one set: refused rather than resolved by a rule nobody stated.
    #[tokio::test]
    async fn two_grants_for_one_connector_are_refused() {
        let store = Arc::new(StoredGrants::default());

        let (status, body) = driven(
            editing(store.clone()),
            Method::PUT,
            "/api/grants",
            "alice",
            Some(json!({
                "grants": [
                    { "connector": "github", "selector": { "max_risk": "low" } },
                    { "connector": "github", "selector": { "max_risk": "destructive" } },
                ],
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["connector"], "github");
        assert!(store.read("acme").is_empty(), "a refused set was stored");
    }

    /// A hand-written grant that names operations is **shown**, and replacing the set is refused.
    ///
    /// The one case where this surface and the file disagree about what a grant can be, and it is
    /// answered by refusing rather than by dropping the exception: the only evidence of a silent
    /// drop would be an operation running that used to be refused, which is the failure the gate
    /// exists to prevent. The read stays honest — an operator sees exactly what is in the file.
    #[tokio::test]
    async fn a_grant_that_names_operations_is_shown_and_replacing_it_is_refused() {
        let store = StoredGrants::holding(
            "acme",
            vec![Grant::for_connector(
                "github",
                Selector::at_most(Risk::Low).deny("github-repo-get"),
            )],
        );

        let (status, body) = driven(
            editing(store.clone()),
            Method::GET,
            "/api/grants",
            "alice",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["editable"], false,
            "the read must say this set cannot be written back here: {body}",
        );
        assert_eq!(body["grants"][0]["expressible"], false);
        assert_eq!(
            body["grants"][0]["exempt"]["never"],
            json!(["github-repo-get"]),
            "the read must show what is stored, or an operator replaces a set they never saw: \
             {body}",
        );
        assert!(
            !admits(&body, 0).contains("github-repo-get"),
            "the preview must honour the stored deny: {body}",
        );

        let (status, body) = driven(
            editing(store.clone()),
            Method::PUT,
            "/api/grants",
            "alice",
            Some(read_only("github")),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "replacing a set that holds an exception this surface cannot express must refuse: \
             {body}",
        );

        let held = store.read("acme");
        assert_eq!(held.len(), 1, "the refused write changed the stored set");
        assert!(
            held[0].selector.deny_ids.contains("github-repo-get"),
            "the exception was dropped by a write that was supposed to have been refused",
        );
    }

    /// A store that will not take the write says so, applies nothing, and names no path.
    #[tokio::test]
    async fn a_store_that_refuses_the_write_says_so_and_names_no_path() {
        let (status, body) = driven(
            editing(Arc::new(UnwritableGrants)),
            Method::PUT,
            "/api/grants",
            "alice",
            Some(read_only("github")),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["setting"], GRANT_SETTING);
        assert!(
            !body.to_string().contains("/var/lib"),
            "the refusal names this host's own filesystem to a caller: {body}",
        );
    }

    // -----------------------------------------------------------------------------------------
    // Nothing tenant-specific, asserted adversarially
    // -----------------------------------------------------------------------------------------

    /// **What a tenant is granted is tenant data, and no anonymous route learns it.**
    ///
    /// The shape `routes::onboarding::tests::the_document_is_identical_with_two_tenants_connected`
    /// uses, pointed at the fact this story introduces: two tenants are granted two *different*
    /// things through the real route, and every document this host serves to a stranger is compared
    /// **byte for byte** against what it served before. A leak worth catching would not arrive as a
    /// `grants` key — it would arrive as a count, a connector id in a list of "what you can call
    /// here", or `admitted` quietly ceasing to be `null`.
    ///
    /// `/api/catalogue/connectors/github/operations` is the one that would go first, and it is the
    /// reason this test drives `github` for one tenant and `slack` for the other: the catalogue's
    /// contract is that it answers *what exists*, never *what you may run*, and `admitted: null` on
    /// every operation is that contract on the wire. A grant landing in the store is exactly the
    /// input that would tempt it to become a boolean.
    #[tokio::test]
    async fn nothing_a_tenant_is_granted_reaches_an_anonymous_caller() {
        /// Every route a caller with no principal can read, and which this story could move.
        const ANONYMOUS: &[&str] = &[
            "/api/onboarding",
            "/api/catalogue/connectors",
            "/api/catalogue/connectors/github/operations",
            "/api/catalogue/connectors/slack/operations",
        ];

        let store = Arc::new(StoredGrants::default());

        let mut before = Vec::new();
        for path in ANONYMOUS {
            let (status, body) = anonymously(editing(store.clone()), path).await;
            assert_eq!(status, StatusCode::OK, "`{path}`: {body}");
            before.push(body);
        }

        // Two tenants, granted two different things, through the route this story added — so this
        // cannot pass on a host where granting is broken and neither tenant holds anything.
        for (handle, connector, level) in
            [("alice", "github", "low"), ("bob", "slack", "destructive")]
        {
            let (status, body) = driven(
                editing(store.clone()),
                Method::PUT,
                "/api/grants",
                handle,
                Some(json!({
                    "grants": [{ "connector": connector, "selector": { "max_risk": level } }],
                })),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "`{handle}` must be able to grant `{connector}`, or this test asserts nothing: \
                 {body}",
            );
        }

        assert_eq!(
            store.read("acme").len(),
            1,
            "two tenants really do hold a grant now",
        );
        assert_eq!(store.read("globex").len(), 1);

        for (path, before) in ANONYMOUS.iter().zip(before) {
            let (status, after) = anonymously(editing(store.clone()), path).await;

            assert_eq!(status, StatusCode::OK, "`{path}`: {after}");
            assert_eq!(
                after, before,
                "`{path}` changed once two tenants were granted something, so it reads what this \
                 host holds rather than what this build is",
            );

            // And spelled out, for the fields a reader would look for first.
            for forbidden in ["acme", "globex", "alice", "bob", "max_risk", "\"grants\""] {
                assert!(
                    !after.contains(forbidden),
                    "`{path}` names `{forbidden}`, which belongs to a tenant and not to this \
                     build: {after}",
                );
            }
        }

        // The catalogue's own contract, stated rather than inferred from byte identity: every
        // operation still answers `admitted: null`, which is the third value that says this route
        // never asked the question.
        let (_, operations) = anonymously(
            editing(store),
            "/api/catalogue/connectors/github/operations",
        )
        .await;
        let document: Value = serde_json::from_str(&operations).expect("a JSON document");
        for operation in document["operations"]
            .as_array()
            .expect("the catalogue answers an array of operations")
        {
            assert_eq!(
                operation["admitted"],
                Value::Null,
                "the catalogue started answering what a caller may run: {operation}",
            );
        }
    }

    // -----------------------------------------------------------------------------------------
    // The Acceptance's last item
    // -----------------------------------------------------------------------------------------

    /// A deployment with no grant store refuses **here**, names the settings, and serves the rest.
    ///
    /// X-13 already answers `503` naming both stores at the invoke route; this is the same fact at
    /// the surface that would have fixed it, and the second half is what keeps a missing store from
    /// reading as a broken host.
    #[tokio::test]
    async fn a_host_with_no_grant_store_refuses_here_and_serves_everything_else() {
        let unbound = signed_in();

        for method in [Method::GET, Method::PUT] {
            let body = (method == Method::PUT).then(|| read_only("github"));
            let (status, answer) = driven(
                unbound.clone(),
                method.clone(),
                "/api/grants",
                "alice",
                body,
            )
            .await;

            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
            let reason = answer["error"]
                .as_str()
                .expect("a refusal carries a reason");
            assert!(
                reason.contains(GRANT_SETTING)
                    && reason.contains(super::super::connections::STORE_SETTING),
                "the refusal must name the settings an operator has to set: {answer}",
            );
        }

        // And the rest of the host is unaffected.
        for path in [
            "/health",
            "/api/catalogue/connectors",
            "/api/onboarding",
            "/api/grants/preview",
        ] {
            let body = (path == "/api/grants/preview")
                .then(|| json!({ "connector": "github", "selector": {} }));
            let method = if body.is_some() {
                Method::POST
            } else {
                Method::GET
            };

            let (status, answer) = driven(unbound.clone(), method, path, "alice", body).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "`{path}` stopped answering on a host with no grant store: {answer}",
            );
        }
    }
}
