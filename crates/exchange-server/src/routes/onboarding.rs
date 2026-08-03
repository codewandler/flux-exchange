//! What this service is, what it can do today and how to authenticate to it — as a document an
//! agent can fetch instead of a page a human reads.
//!
//! ```text
//! GET /api/onboarding    the agent descriptor
//! ```
//!
//! # This widens the anonymous surface, and that is the whole of the story
//!
//! X-41 rendered these facts as a console page. Publishing them *from the service* is a different
//! act: it is a disclosure decision on a host that holds other people's credentials, which is why
//! `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous` has an entry for this
//! route with the argument written beside it, and why every field below carries its own.
//!
//! It has to be anonymous. `docs/designs/agent-onboarding.md` §1: an agent that must already be
//! authenticated to learn how to authenticate is a closed loop, and a human evaluating this
//! platform should not need an account to read what it offers.
//!
//! # What is in it, and what is deliberately not
//!
//! Everything except one boolean is `&'static` and **identical in every deployment of this build**.
//! It is not read from this composition, from the store, from the catalogue or from the request:
//! [`DERIVED`] is a compile-time artifact generated from the console's own surface declaration, so
//! there is no code path here by which a tenant, a principal, an address, a count, a connector or a
//! configured endpoint could reach a caller. `tests::the_document_is_identical_with_two_tenants_connected`
//! drives that adversarially rather than trusting the reading.
//!
//! The one field that varies is [`Published::sign_in_available`], and it is deliberately a field
//! this surface **already publishes** at `/api/signin/availability` (X-43) rather than a new fact:
//! embedding it costs a stranger nothing they could not already learn from one more request, and
//! `crate::state::SignIn::available` carries the argument for why the three states collapse to a
//! boolean before anyone outside sees them. Without it this document would be dishonest on the
//! composition that motivates the Acceptance's last item — a host with no identity provider
//! configured still serves `/health` and the catalogue, and its descriptor must say sign-in is
//! unavailable rather than describe a mint step that begins with a human signing in.
//!
//! The software version is **not** here. `/health` publishes it and one disclosure of a fact is
//! enough; a second copy is a second thing to keep true.
//!
//! # Why not a well-known URI
//!
//! `docs/stories/X-42-agent-descriptor.md` says not to invent a standard and to prefer an existing
//! one. There is no registered `/.well-known/` URI (RFC 8615 keeps a registry, and it is not a
//! free-for-all) that means "how an agent authenticates to this service and what it may call".
//! Neighbouring ecosystems do serve agent cards under `/.well-known/`, and that is precisely the
//! argument *against* borrowing one of those names: a client that knows such a standard would parse
//! this document against a schema it does not follow and act on the result. A wrong answer a
//! machine trusts is worse than an unfamiliar name it has to be pointed at.
//!
//! So: a private document, under `/api` where the rest of this surface lives (and where the
//! console's dev-server proxy sends it — see [`super::signin`]), naming itself and carrying its own
//! `version` so adopting a standard later is an addition rather than a break.
//!
//! # One truth, two renderings
//!
//! [`DERIVED`] is written by `console/scripts/agent-descriptor.mjs` from `console/src/onboarding.mts`
//! — the same model `AgentOnboarding.mts` renders the page from, which in turn derives every "can
//! you do this yet" from `console/src/surfaces.mts`. A committed copy can rot, so it is not trusted:
//! `console/test/descriptor.test.mjs` re-derives the document and fails when this artifact differs,
//! **and** compares this artifact against the rendered page field by field. That second test is the
//! one the design asks for — two renderings that can drift is the failure mode every "docs plus
//! SDK" pair eventually has, and the assertion is that they agree, not that each is separately
//! correct.
//!
//! Which is also why the types below are `deny_unknown_fields`: a field added to the console model
//! and regenerated into this artifact does not quietly appear on the anonymous surface. It fails to
//! parse until somebody adds it here, next to the argument for publishing it.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::error;

use super::{Access, Module, Route};
use crate::state::AppState;

/// The descriptor, as generated from the console's onboarding model.
///
/// `include_str!` rather than a read at startup: a file this host had to open could be missing, be
/// replaced, or differ between two machines running the same build, and none of those are states a
/// document describing *the build* should be able to be in.
const DERIVED: &str = include_str!("onboarding.json");

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "onboarding",
    routes: &[Route {
        path: "/api/onboarding",
        // Anonymous, and it has to be — see the module documentation. The argument is repeated
        // where it is enforced, in `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous`.
        access: Access::Anonymous,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    get(onboarding)
}

/// What this service is, as an agent can parse it.
///
/// Every field is `&'static` — the same bytes in every deployment of this build. The fields are
/// ordered the way a caller meets them: what this document is, where it lives, what the service is,
/// how to authenticate, and what may be done.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Onboarding {
    /// What this document is. A private name; see the module documentation for why not a
    /// well-known one.
    descriptor: String,
    /// This document's own version. Bumped when a field changes meaning or leaves.
    version: u32,
    /// Where this document is served. Published so a caller that was handed the document out of
    /// band knows where the live one is, and pinned to the route by
    /// [`tests::the_endpoint_the_document_names_is_the_route_that_serves_it`].
    endpoint: String,
    /// What this service is, in one name and one sentence.
    service: Service,
    /// How a caller presents a token, and whether presenting one identifies it yet.
    authentication: Authentication,
    /// What an agent may do here, and what it may not. The Acceptance's "which capabilities are
    /// live".
    capabilities: Vec<Capability>,
}

/// The service, named and summarised.
///
/// A stranger may learn both: the name is on the crate, the binary, the console and crates.io, and
/// the sentence is `docs/vision.md`'s own public description of what this software does. Neither
/// says anything about *this* deployment.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Service {
    name: String,
    summary: String,
}

/// How to present a token.
///
/// A stranger may learn this because the guard already answers it: `super::presented` reads
/// `Authorization: Bearer <material>`, so anyone may discover the scheme by sending one request. It
/// is published so an agent author does not have to.
///
/// [`live`](Self::live) is derived from the declared identity surface. Service Account bearer
/// authentication is live; keeping the flag derived means a future regression withdraws the claim
/// from both the page and this document.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Authentication {
    scheme: String,
    header: String,
    capability: String,
    live: bool,
}

/// One thing an agent may or may not do on this build.
///
/// A stranger may learn all of it: it describes what this *software version* can do, which is the
/// same for everyone running it, and it names no instance of anything. The endpoints are
/// parameterless by construction — a path with a segment in it would be a connector, a credential
/// or a tenant — and `tests::no_endpoint_the_document_names_could_hold_a_value` holds that over
/// every entry rather than over the ones that exist today.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    id: String,
    title: String,
    summary: String,
    /// Whether this build has it. Derived from the console's surface declaration, never written.
    live: bool,
    /// How to do it, or `null` — `null` exactly when it is not live. An endpoint published beside
    /// a capability that does not exist is an invitation to call nothing.
    call: Option<Call>,
    /// Why it cannot be done, in the console's own words. Empty exactly when it is live.
    withheld: String,
}

/// The one call a capability is done by.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Call {
    /// The HTTP method, spelled as a caller sends it.
    ///
    /// Held by [`tests::every_published_call_reaches_a_handler_for_the_method_it_names`], which
    /// **drives** it rather than comparing it. [`Route`] carries a `fn() -> MethodRouter` and a
    /// `MethodRouter` cannot be asked what it answers — the same property that made `super`'s
    /// modules hand over tables rather than routers — so the only reading of this field that is
    /// not a second declaration is the surface's own answer to a request naming it. What that
    /// holds is that the method reaches a handler at the published endpoint, and not that it is
    /// the only method that route serves.
    method: String,
    /// The route to send it to, in the route table's own spelling. Pinned to the route that
    /// actually serves this capability by
    /// [`tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`], and not
    /// merely to *some* route this host publishes.
    endpoint: String,
    /// Who makes this call, and with what standing — the field an agent author gets wrong, since
    /// minting is a call a *human* makes.
    caller: String,
    note: String,
    /// The one thing not to skim past, or empty.
    warn: String,
}

/// The document as it goes on the wire: everything derived, plus the one thing the console cannot
/// know.
///
/// The split is the design. What the page renders is a fact about the **build**; whether anyone can
/// sign in is a fact about the **deployment**, and only this process knows it. Keeping them in
/// separate fields is what lets `console/test/descriptor.test.mjs` compare the page against the
/// derived core byte for byte without the console having to model a composition it cannot see.
#[derive(Debug, Serialize)]
struct Published<'a> {
    #[serde(flatten)]
    derived: &'a Onboarding,
    /// Whether a human could complete a sign-in against **this** deployment.
    ///
    /// The only field here that differs between two hosts running one build, and it is one this
    /// surface already publishes at `/api/signin/availability`. It is in this document because
    /// without it the document is dishonest on the composition the Acceptance names: a host with no
    /// identity provider configured serves `/health`, serves the catalogue, and serves a mint
    /// capability whose caller is "a signed-in human". Saying nothing there would let it read as a
    /// host somebody could actually be minted on.
    ///
    /// Spelled `sign_in_available` and not `available`, which is the spelling
    /// `crate::routes::signin::availability` chose so that it would still say what it means when a
    /// later story embedded it in a wider answer. This is that story.
    sign_in_available: bool,
}

/// Serve the descriptor.
async fn onboarding(State(state): State<AppState>) -> Response {
    match document(DERIVED, state.sign_in().available()) {
        Ok(document) => Json(document).into_response(),
        // Refuse; never repair. Half a descriptor is worse than none: an agent that reads a
        // capability list missing its last entry concludes the capability is not there. The reason
        // names this host's own artifact, so it goes to the log rather than to the caller.
        Err(error) => {
            error!(%error, "the compiled-in agent descriptor is not a document this host can serve");
            super::refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "this host cannot serve its agent descriptor right now",
            )
        }
    }
}

/// The document, from the derived artifact and this composition's one runtime fact.
///
/// Takes the artifact as an argument rather than reading [`DERIVED`] directly, so
/// [`tests::an_artifact_this_host_cannot_read_refuses_rather_than_serving_half_a_document`] can
/// reach the refusal above. A branch no test can enter is a branch nobody knows the shape of.
///
/// Parsed per request rather than once at startup. It costs a few microseconds on a route nothing
/// calls in a loop, and it keeps the failure a *refusal* — a document parsed once and cached would
/// have to decide what to do about a failure at a point where there is no caller to refuse.
fn document(derived: &str, sign_in_available: bool) -> Result<Value, serde_json::Error> {
    let derived: Onboarding = serde_json::from_str(derived)?;

    serde_json::to_value(Published {
        derived: &derived,
        sign_in_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{header, Method, Request as HttpRequest};
    use axum::Router;
    use exchange_host::{
        address_path, async_trait, CredentialRef, CredentialScope, Layout, Secret, SecretStore,
        StoreError, TenantLayout,
    };
    use serde_json::json;
    use tower::Service;

    use super::super::published;
    use crate::dev_identity::DevIdentity;

    /// Two tenants, so a document that leaked one could be caught leaking it.
    const ROSTER: &str = "user:alice@acme,user:bob@globex";

    /// The route under test, read from the declaration rather than written out again.
    const ONBOARDING: &str = super::MODULE.routes[0].path;

    /// A store that lives in the test.
    ///
    /// Hand-rolled for `routes::connections::tests::TestStore`'s reason: `exchange_host` must not
    /// be made to re-export an in-memory store a production composition could then bind. This one
    /// is the smallest thing that can hold two tenants' credentials.
    #[derive(Default)]
    struct TestStore(Mutex<HashMap<String, String>>);

    #[async_trait]
    impl SecretStore for TestStore {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            let path = address_path(reference);
            self.0
                .lock()
                .expect("no test poisons this")
                .get(&path)
                .map(Secret::new)
                .ok_or(StoreError::NotFound { path })
        }

        async fn put(&self, reference: &CredentialRef, secret: &Secret) -> Result<(), StoreError> {
            self.0
                .lock()
                .expect("no test poisons this")
                .insert(address_path(reference), secret.expose_secret().to_string());
            Ok(())
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            self.0
                .lock()
                .expect("no test poisons this")
                .remove(&address_path(reference));
            Ok(())
        }

        async fn references(
            &self,
            scope: &CredentialScope,
        ) -> Result<Vec<CredentialRef>, StoreError> {
            let mut references = Vec::new();
            for path in self.0.lock().expect("no test poisons this").keys() {
                let reference = TenantLayout
                    .parse(path)
                    .map_err(|reason| StoreError::Layout { reason })?;
                if scope.contains(&reference) {
                    references.push(reference);
                }
            }
            references.sort();
            Ok(references)
        }
    }

    /// Drive one request through a fully assembled app and hand back what a caller sees.
    async fn call(
        app: &Router,
        method: Method,
        path: &str,
        handle: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let mut service = app.clone().into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let mut request = HttpRequest::builder().method(method).uri(path);

        if let Some(handle) = handle {
            request = request.header(header::AUTHORIZATION, format!("Bearer {handle}"));
        }

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

        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The descriptor, as a stranger sees it.
    async fn fetched(app: &Router) -> (StatusCode, String) {
        call(app, Method::GET, ONBOARDING, None, None).await
    }

    /// The derived artifact, parsed — what this host serves before it adds its one runtime field.
    fn artifact() -> Value {
        serde_json::from_str(DERIVED).expect("the compiled-in descriptor is a JSON document")
    }

    // ---------------------------------------------------------------------------------------
    // Nothing tenant-specific, asserted adversarially.
    // ---------------------------------------------------------------------------------------

    /// **The Acceptance's second item.** Two tenants connect, and the descriptor does not move.
    ///
    /// The comparison is on the **bytes**, not on a field somebody thought to check: a leak this
    /// test is worth having would not arrive as a `tenant` key, it would arrive as a count, a
    /// connector id in a list of "what you can call here", or an endpoint assembled from a
    /// configured value. Byte identity is the only assertion that covers a field nobody has thought
    /// of yet.
    ///
    /// The connects are driven through the real route and asserted to have landed, so this cannot
    /// pass on a host where connecting is broken and neither tenant has anything.
    #[tokio::test]
    async fn the_document_is_identical_with_two_tenants_connected() {
        let store = Arc::new(TestStore::default());
        let app = super::super::app(
            AppState::with_development_identity(Arc::new(
                DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
            ))
            .with_credentials(store.clone()),
        );

        let (_, before) = fetched(&app).await;

        for (handle, connector, credential, value) in [
            ("alice", "zendesk", "zendesk.api_token", "ACME-TOKEN"),
            ("bob", "slack", "slack.bot_token", "GLOBEX-TOKEN"),
        ] {
            let (status, body) = call(
                &app,
                Method::POST,
                &format!("/api/connections/{connector}"),
                Some(handle),
                Some(json!({ "credentials": { credential: value } })),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::CREATED,
                "`{handle}` must be able to connect `{connector}`, or this test asserts nothing: \
                 {body}",
            );
        }

        // The store really is holding two tenants' worth of state now.
        let held = store.0.lock().expect("no test poisons this").len();
        assert_eq!(held, 2, "two tenants are connected");

        let (status, after) = fetched(&app).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            after, before,
            "the descriptor changed once tenants connected, so something in it is read from what \
             this host holds rather than from what this build is",
        );

        // And spelled out, for the fields a reader would look for first.
        for forbidden in [
            "acme",
            "globex",
            "alice",
            "bob",
            "zendesk",
            "slack",
            "ACME-TOKEN",
            "GLOBEX-TOKEN",
            "tenants/",
        ] {
            assert!(
                !after.contains(forbidden),
                "the descriptor names `{forbidden}`, which belongs to a tenant and not to this \
                 build: {after}",
            );
        }
    }

    /// Every path parameter that may appear in an endpoint this document publishes, and why it is
    /// not a value.
    ///
    /// Written out rather than pattern-matched, so admitting a second one is a decision somebody
    /// makes here. The rule it enforces is not "no parameters" — that was the first spelling, and
    /// it was the right instinct one notch too tight, since it would have meant either omitting the
    /// one route that runs an operation or describing it in prose to get past a test. The rule is
    /// **no value**: no tenant, no principal, no credential, no address. `/api/connections/{connector}`
    /// in this document would be a tenant's contents wearing the shape of a route, and `{connector}`
    /// is not on this list.
    const CATALOGUE_KEYS: &[&str] = &[
        // The catalogue's own operation id — `zendesk-ticket-show`. It selects *what* runs; the
        // tenant, the host and the credential are each derived from something the caller did not
        // supply. The same vocabulary `/api/catalogue/connectors` already publishes anonymously,
        // so it discloses nothing this surface was keeping.
        "operation",
    ];

    /// Every endpoint this document names is a route this host actually publishes, and none of them
    /// has a place a *value* could go.
    ///
    /// The first half is the stronger one and it is new in the rework: comparing against
    /// [`published`] rather than against a shape means a typo, a renamed route, or a document
    /// describing an endpoint this build does not serve is a failure here rather than a `404` in
    /// somebody's agent.
    #[test]
    fn every_endpoint_the_document_names_is_a_published_route_and_names_no_value() {
        let document: Onboarding = serde_json::from_str(DERIVED).expect("a JSON document");

        let endpoints: Vec<String> = document
            .capabilities
            .iter()
            .filter_map(|capability| capability.call.as_ref())
            .map(|call| call.endpoint.clone())
            .chain(std::iter::once(document.endpoint.clone()))
            .collect();

        assert!(
            endpoints.len() > 1,
            "no endpoints were checked, so this proves nothing",
        );

        for endpoint in endpoints {
            assert!(
                published().any(|(_, route)| route.path == endpoint),
                "the descriptor tells an agent to call `{endpoint}`, which is not a route this \
                 host publishes",
            );

            for parameter in endpoint
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{'))
                .filter_map(|segment| segment.strip_suffix('}'))
            {
                assert!(
                    CATALOGUE_KEYS.contains(&parameter),
                    "`{endpoint}` carries `{{{parameter}}}`, which is not on the list of catalogue \
                     keys this document may spell — a parameter that is not a catalogue key is a \
                     tenant, a principal, a credential or an address, and this document describes \
                     the shape of the service and never its contents",
                );
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // The rework's central claim: a capability's liveness is measured against the route table.
    // ---------------------------------------------------------------------------------------

    /// Every capability this document publishes, and the route on **this host's surface** that
    /// serves it. `None` means no route does, and each of those carries its argument.
    ///
    /// This table is the seam the derivation crosses. The document is generated from the console's
    /// `surfaces.mts`, which is a statement about the console; whether a capability is live is a
    /// statement about the **route table**, and those are two different facts that agreed for every
    /// entry until `invoke` shipped a route with no screen behind it. This is where they are made
    /// to agree again, in the language that owns the route table, in the gate.
    const SERVED_BY: &[(&str, Option<&str>)] = &[
        ("read-the-catalogue", Some("/api/catalogue/connectors")),
        ("create-service-account", Some("/api/service-accounts")),
        // Resolving the bearer is exercised through the session route: it is the smallest call
        // that both runs authentication and returns the principal it resolved.
        ("authenticate", Some("/api/session")),
        ("connections", Some("/api/connections")),
        ("grants", Some("/api/grants")),
        ("invoke", Some("/api/operations/{operation}/invoke")),
        ("subscribe", Some("/api/subscribe")),
        ("workflows", Some("/api/workflows")),
        // Workflow runs are durable and tenant-scoped; the collection is the capability's entry.
        ("read-what-happened", Some("/api/workflow-runs")),
        // These are part of the intended surface, but no route serves either one yet. The
        // descriptor's withheld reason points at the story that would make each row live.
        ("agents", None),
        ("leases", None),
    ];

    /// Every published route that backs **no** capability, and why it is not one.
    ///
    /// The half that would have caught `invoke` on the day it shipped. Without it the table above
    /// is a list somebody maintains: a route can land, serve a capability this document calls
    /// unavailable, and nothing goes red — which is exactly what happened between v0.7.0 and this
    /// story. With it, a new route fails the gate until somebody either publishes it as a
    /// capability or writes a sentence here saying why an agent author is not being told about it.
    const NOT_A_CAPABILITY: &[(&str, &str)] = &[
        (
            "/health",
            "liveness, for an operator's monitor. Not something an agent does with this service.",
        ),
        (
            "/metrics",
            "fixed-cardinality process saturation for an operator's monitor. It describes host \
             load and grants an agent no authority.",
        ),
        (
            "/api/catalogue/connectors/{id}/operations",
            "the second page of the catalogue capability, named in its own note rather than as a \
             capability of its own — one capability per thing an agent *does*, not per route.",
        ),
        (
            "/api/catalogue/connectors/{id}/credentials",
            "what a connector declares it needs. An operator supplies those; an agent never sees a \
             credential at all, which is the vision's fourth principle.",
        ),
        (
            "/api/catalogue/connectors/{id}/channels",
            "the public generated binding and event choices an operator uses to configure a channel; \
             it publishes no handshake, auth, endpoint, payload mapping or tenant state and is the \
             declaration half of the catalogue rather than a separate agent capability.",
        ),
        (
            "/api/signin",
            "a browser redirect into an identity provider. Not a call an agent makes, and the one \
             fact about it an agent needs is the `sign_in_available` field.",
        ),
        (
            "/api/signin/callback",
            "the provider's redirect back, mid-sign-in. Nothing calls it deliberately.",
        ),
        (
            "/api/signin/availability",
            "published in this document as `sign_in_available`, so it is not a capability as well.",
        ),
        (
            "/api/signin/local",
            "the verifier-backed human form target. It creates the browser session through the same sign-in capability fact and is never a call an agent makes.",
        ),
        (
            "/api/service-accounts/{id}",
            "revocation is the item half of the create-service-account capability and an operator \
             action, not a separate capability an Agent receives.",
        ),
        (
            "/api/connections/{connector}",
            "the connector item is create, inspect and delete beneath the connections capability; \
             it is operator work rather than another public capability.",
        ),
        (
            "/api/connections/{connector}/label",
            "as above; naming the sole connection is operator metadata.",
        ),
        (
            "/api/connections/{connector}/instances/{label}",
            "as above; creating, inspecting, renaming or removing a named connection remains an operator action.",
        ),
        (
            "/api/connections/{connector}/instances/{label}/settings",
            "as above; it exposes only declared setting names and set state, never values.",
        ),
        (
            "/api/connections/{connector}/instances/{label}/settings/{service}/{field}",
            "as above; selecting which endpoint receives this connection's credential is an operator action.",
        ),
        (
            "/api/connections/{connector}/instances/{label}/credentials/{credential}",
            "as above, and it accepts a credential value which the agent descriptor must never invite an agent to supply.",
        ),
        (
            "/api/connections/{connector}/credentials/{credential}",
            "as above, and it takes a credential value — the one thing this document must never \
             invite anyone to send here.",
        ),
        ("/api/connections/{connector}/settings", "as above."),
        (
            "/api/connections/{connector}/settings/{service}/{field}",
            "as above.",
        ),
        (
            "/api/grants/preview",
            "what a proposed grant would admit before it is saved; it is the preview half of the \
             grants capability rather than another capability.",
        ),
        (
            "/api/workflows/editor-catalog",
            "the authoring vocabulary beneath the workflows capability, not a capability of its own.",
        ),
        (
            "/api/workflows/{workflow}",
            "the draft item beneath the workflows capability; agents never mutate its stored Program.",
        ),
        (
            "/api/workflows/{workflow}/validate",
            "the validation action beneath the workflows capability, not a separate capability.",
        ),
        (
            "/api/workflows/{workflow}/publish",
            "the publication action beneath the workflows capability, not a separate capability.",
        ),
        (
            "/api/workflows/{workflow}/versions",
            "immutable publication history is an authoring and inspection affordance for a \
             signed-in human, not authority granted to an agent.",
        ),
        ("/api/workflows/{workflow}/versions/{version}", "as above."),
        (
            "/api/workflows/{workflow}/runs",
            "a signed-in human's workflow-specific run control. An agent invokes the published \
             workflow through the ordinary operation route and its ordinary grant.",
        ),
        (
            "/api/workflow-runs/{run}",
            "the detail page of the read-what-happened capability, not a separate thing an agent \
             does.",
        ),
        (
            "/api/workflow-runs/{run}/cancel",
            "a signed-in human's control over a live run, not authority handed to an agent.",
        ),
        (
            "/api/channels",
            "operator-owned lifecycle for persistent vendor channels. Agents consume selected \
             events through subscribe and never create or widen the underlying connection.",
        ),
        (
            "/api/channels/{id}",
            "the item half of the same operator-owned channel lifecycle, not an agent capability.",
        ),
        (
            "/api/onboarding",
            "this document itself. It names its own endpoint in `endpoint`, which is where a \
             caller looks for it, rather than as something an agent is told to go and do.",
        ),
    ];

    /// **A capability is live exactly when a route on this surface serves it** — and it is
    /// published at that route.
    ///
    /// The test the rework exists for. The page and the descriptor already agreed with each other;
    /// they agreed while both were wrong, because both derived from a console flag that answers a
    /// question about screens. This one derives from [`published`] — the route table itself — so it
    /// cannot agree with a mistake.
    ///
    /// It would have gone red on the diff that shipped `invoke` as `live: false`.
    ///
    /// **The second half is X-52's, and the name is why it is here rather than in a test of its
    /// own.** Until it existed this compared `capability.live` against *does the [`SERVED_BY`] path
    /// exist* and never against the capability's own `call.endpoint`, so republishing `be-minted`
    /// at `/api/session` — a route this host really does publish, and one [`NOT_A_CAPABILITY`]
    /// argues is not a capability — left every test in this crate green. All three live endpoints
    /// happened to be pinned by a hand-written assertion in `console/test/onboarding.test.mjs`, so
    /// nothing was wrong; the test's *name* claimed the thing it did not check, which is the
    /// failure this repository has now corrected in five separate stories.
    #[test]
    fn a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it() {
        let document: Onboarding = serde_json::from_str(DERIVED).expect("a JSON document");

        // The table covers the document exactly, so a capability added to the console model cannot
        // slip past unmeasured, and a stale row cannot sit here asserting nothing.
        let published_ids: Vec<&str> = document
            .capabilities
            .iter()
            .map(|capability| capability.id.as_str())
            .collect();
        let measured_ids: Vec<&str> = SERVED_BY.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            published_ids, measured_ids,
            "the descriptor publishes a different set of capabilities from the one this table \
             measures; every capability needs a row saying which route serves it",
        );

        for (id, path) in SERVED_BY {
            let capability = document
                .capabilities
                .iter()
                .find(|capability| capability.id == *id)
                .expect("the table and the document agree, asserted above");

            let served = path.is_some_and(|path| published().any(|(_, route)| route.path == path));

            assert_eq!(
                capability.live,
                served,
                "`{id}` is published as live={} and this host {} a route serving it ({path:?}). A \
                 capability that is live with no route is an invitation to call nothing; one that \
                 is withheld while its route sits in the surface is this service telling the \
                 caller the vision calls primary that it cannot do the thing it does.",
                capability.live,
                if served { "publishes" } else { "publishes no" },
            );

            // And the route named above is the route the document tells an agent to call. The two
            // are deliberately stated twice — once here, from the language that owns the route
            // table, and once in the console model the artifact is generated from — so that the
            // pair has to be kept in step rather than one of them silently drifting.
            match (*path, capability.call.as_ref()) {
                (Some(path), Some(call)) => assert_eq!(
                    call.endpoint, path,
                    "`{id}` tells an agent to call `{}`, and the route this table says serves it \
                     is `{path}`. An endpoint that exists but does not serve the capability beside \
                     it is worse than one that does not exist at all: the caller gets an answer, \
                     from something else.",
                    call.endpoint,
                ),
                (Some(path), None) => panic!(
                    "`{id}` is served by `{path}` and names no call, so this document publishes a \
                     capability an agent has no way to reach",
                ),
                (None, Some(call)) => panic!(
                    "`{id}` is served by no route on this surface and names `{}` anyway, which is \
                     an invitation to call nothing",
                    call.endpoint,
                ),
                (None, None) => {}
            }
        }
    }

    /// A path a request can actually be sent to: `/api/operations/{operation}/invoke` matches
    /// nothing literally, and a `404` in the probe below would read as a method this host serves.
    ///
    /// The same substitution `super::super::tests::probe_path` makes, written again because that
    /// one is private to a sibling test module. Six lines said twice is the cheaper of the two
    /// mistakes on offer — the other is widening a helper's visibility in `routes/mod.rs`, which is
    /// the merge site every module's story already has to touch.
    fn probe(path: &str) -> String {
        let mut probe = String::with_capacity(path.len());
        let mut in_parameter = false;

        for character in path.chars() {
            match character {
                '{' => {
                    in_parameter = true;
                    probe.push('x');
                }
                '}' => in_parameter = false,
                _ if in_parameter => {}
                _ => probe.push(character),
            }
        }

        probe
    }

    /// A method this build serves at none of the endpoints below, used as the probe's own control.
    ///
    /// Without it, `not a 405` would be satisfied by a surface that answered every method — and a
    /// check that cannot fail is the thing this story exists to stop shipping. If a later story
    /// gives one of these routes a `PATCH` handler this goes red, and the fix is to pick another
    /// method rather than to drop the control.
    const UNSERVED: Method = Method::PATCH;

    /// **Every call this document publishes reaches a handler for the method it names.**
    ///
    /// The name is the whole claim, and it is deliberately narrower than "the method is correct":
    /// a path serving both `GET` and `POST` would pass here with either spelling published. Every
    /// endpoint this document names serves exactly one method on this build, so the two coincide
    /// today — and what this holds is the half that costs a caller something either way, which is
    /// a published method the route does not answer at all.
    ///
    /// **Why a request and not a comparison.** [`Route`] carries a `fn() -> MethodRouter`, so the
    /// method is not statically readable from the route table; `super`'s own documentation says
    /// why — "axum's `Router` cannot be asked what it answers". The alternative shape was a
    /// `method` field on [`Route`], declared beside the `method_router` it describes. That would
    /// have pinned this document to a declaration and left the declaration pinned to nothing: two
    /// values in one struct that can disagree, with the guard on the wrong side of the disagreement.
    /// Driving the published call asks the surface itself, which is the thing an agent will ask.
    ///
    /// **The probe is driven by a resolved caller, and that is not a convenience.** The first
    /// spelling of this test drove it anonymously and the control caught it: on a guarded route the
    /// `route_layer` runs *before* the method router, so an unidentified caller gets `401` for
    /// every method and a `PATCH` this host serves nowhere is indistinguishable from the `POST` it
    /// does. Anonymously, this test would have passed for a nonexistent method on the guarded
    /// Service Account route — the exact defect it exists to catch. A rostered `user:` handle is
    /// the weakest caller that gets past its operator guard, and nothing is minted, stored or
    /// dispatched to get there: no Service Account store and no credential store are bound, and
    /// the requests carry no body.
    #[tokio::test]
    async fn every_published_call_reaches_a_handler_for_the_method_it_names() {
        let document: Onboarding = serde_json::from_str(DERIVED).expect("a JSON document");
        let app = super::super::app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        )));
        // The roster's first handle, which is a `user:` — see above for why the caller has to be
        // one this host resolves at all.
        let caller = Some("alice");

        let calls: Vec<&Call> = document
            .capabilities
            .iter()
            .filter_map(|capability| capability.call.as_ref())
            .collect();

        assert!(
            !calls.is_empty(),
            "no call was driven, so this proves nothing",
        );

        for published in calls {
            let method: Method = published
                .method
                .parse()
                .unwrap_or_else(|_| panic!("`{}` is not an HTTP method", published.method));

            assert_ne!(
                method, UNSERVED,
                "the document publishes the method this test uses as its control, so the control \
                 below proves nothing",
            );

            let path = probe(&published.endpoint);
            let (status, body) = call(&app, method, &path, caller, None).await;

            assert_ne!(
                status,
                StatusCode::NOT_FOUND,
                "`{} {}` reached no route at all — `{path}` is not published, which \
                 `every_endpoint_the_document_names_is_a_published_route_and_names_no_value` says \
                 more about: {body}",
                published.method,
                published.endpoint,
            );
            assert_ne!(
                status,
                StatusCode::METHOD_NOT_ALLOWED,
                "the descriptor tells an agent to send `{} {}` and this host serves no handler for \
                 that method there. An agent that follows this document gets a 405 from the one \
                 field it cannot discover any other way: {body}",
                published.method,
                published.endpoint,
            );

            // The control, at the same path: a method this surface does not serve there is a 405,
            // so the assertion above is reading the method and not merely the path.
            let (control, _) = call(&app, UNSERVED, &path, caller, None).await;
            assert_eq!(
                control,
                StatusCode::METHOD_NOT_ALLOWED,
                "`{UNSERVED} {path}` was not refused for its method, so this host answers \
                 `{path}` whatever is sent to it and the check above cannot tell a served method \
                 from an unserved one",
            );
        }
    }

    /// **Every published route is a capability this document names, or is argued not to be.**
    ///
    /// The other direction, and the one with teeth over time. The table above can only measure
    /// capabilities that already exist; this notices a route that landed and that nobody told an
    /// agent author about — which is precisely how `invoke` came to be described as unavailable for
    /// two releases while it ran operations.
    #[test]
    fn every_published_route_is_a_capability_or_is_argued_not_to_be() {
        let backing: Vec<&str> = SERVED_BY.iter().filter_map(|(_, path)| *path).collect();

        let unaccounted: Vec<&str> = published()
            .map(|(_, route)| route.path)
            .filter(|path| {
                !backing.contains(path)
                    && !NOT_A_CAPABILITY
                        .iter()
                        .any(|(excluded, _)| excluded == path)
            })
            .collect();

        assert!(
            unaccounted.is_empty(),
            "these routes are published and this document neither offers them as a capability nor \
             says why not: {unaccounted:?}. Add a capability, or add a line to NOT_A_CAPABILITY \
             with the argument — a route an agent author is never told about is a decision, and it \
             should be one somebody made.",
        );

        // And the exclusions are all live routes, so a stale argument for a route that no longer
        // exists cannot sit here making the check above look narrower than it is.
        let stale: Vec<&str> = NOT_A_CAPABILITY
            .iter()
            .map(|(path, _)| *path)
            .filter(|path| !published().any(|(_, route)| route.path == *path))
            .collect();

        assert!(
            stale.is_empty(),
            "these routes are argued not to be capabilities and are not published at all: \
             {stale:?}",
        );
    }

    /// A Service Account store is an identity binding even without browser sign-in.
    #[test]
    fn a_service_account_store_is_a_live_bearer_identity_binding() {
        let directory = std::env::temp_dir().join(format!(
            "flux-exchange-onboarding-service-accounts-{}",
            std::process::id()
        ));
        let store = Arc::new(
            crate::service_account::ServiceAccountStore::open(
                directory.join("service-accounts.json"),
            )
            .expect("a fresh store"),
        );

        let state = AppState::without_identity().with_service_accounts(store);

        assert_eq!(
            state.identity_binding(),
            crate::bind::IdentityBinding::Bound
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    // ---------------------------------------------------------------------------------------
    // The Acceptance's last item: a deployment with no identity provider configured.
    // ---------------------------------------------------------------------------------------

    /// A host that can sign nobody in says so, and a host that can says so — the same document
    /// either way, differing in exactly one field.
    ///
    /// Both directions in one test on purpose. "It says `false`" is satisfied by a host that says
    /// `false` unconditionally, which would be a different lie on the deployment where sign-in
    /// works.
    #[tokio::test]
    async fn the_descriptor_says_whether_this_deployment_can_sign_anyone_in() {
        let unconfigured = super::super::app(AppState::without_identity());
        let (status, without) = fetched(&unconfigured).await;
        assert_eq!(status, StatusCode::OK, "{without}");

        let without: Value = serde_json::from_str(&without).expect("a JSON document");
        assert_eq!(
            without["sign_in_available"], false,
            "a host with no identity provider configured must say sign-in is unavailable rather \
             than describe a mint step that begins with a human signing in: {without}",
        );

        // The same host still serves the rest of what it promises — the Acceptance's wording is
        // that this host "still exists and still serves /health and the catalogue".
        for (path, expected) in [("/health", StatusCode::OK), (ONBOARDING, StatusCode::OK)] {
            let (status, body) = call(&unconfigured, Method::GET, path, None, None).await;
            assert_eq!(status, expected, "`{path}` on an unconfigured host: {body}");
        }

        let configured = super::super::app(AppState::with_oidc(Arc::new(crate::oidc::Oidc::new(
            crate::oidc::config::OidcConfig::for_test(
                "https://accounts.example.com",
                "flux-exchange",
                "acme",
            ),
            Arc::new(NoExchange),
        ))));
        let (status, with) = fetched(&configured).await;
        assert_eq!(status, StatusCode::OK, "{with}");

        let with: Value = serde_json::from_str(&with).expect("a JSON document");
        assert_eq!(
            with["sign_in_available"], true,
            "a host that can complete a sign-in must say so, or the field is a constant wearing a \
             boolean: {with}",
        );

        // And nothing else moved. The configured host was given an issuer, a client id and a
        // tenant, and none of them may appear anywhere — which is the same discipline
        // `signin::tests::the_availability_answer_discloses_nothing_about_the_configuration` holds
        // for the route this field came from.
        let mut with_only_the_boolean = with.clone();
        let mut without_only_the_boolean = without.clone();
        for document in [&mut with_only_the_boolean, &mut without_only_the_boolean] {
            document
                .as_object_mut()
                .expect("a JSON object")
                .remove("sign_in_available");
        }

        assert_eq!(
            with_only_the_boolean, without_only_the_boolean,
            "two compositions answered with two different documents; the only thing this document \
             may say about a deployment is whether sign-in works",
        );
        assert_eq!(
            with_only_the_boolean,
            artifact(),
            "what this host serves is no longer the derived artifact plus one field, so something \
             is being composed at runtime that the page cannot see and the console tests cannot \
             compare against",
        );
    }

    /// **X-57.** The descriptor stays the derived artifact plus one field on a host whose identity
    /// is the development one — and that field says a caller can sign in.
    ///
    /// The composition the sibling test above cannot reach: it drives "no provider" and "a bound
    /// OIDC provider", which were the only two answers `sign_in_available` had. A host with the
    /// development identity armed mints principals from a roster and exchanges one for a session at
    /// `POST /api/session`, so a descriptor telling an agent author that nobody can sign in here
    /// describes a mint step — whose caller is "a signed-in human" — as unreachable on a host where
    /// it is not.
    ///
    /// The second assertion is the one that keeps this story honest: everything **except** that
    /// boolean must still be the artifact, byte for byte. A variant added to `SignIn` may widen
    /// what a caller is told about the *service* and must not widen what it is told about the
    /// *deployment*.
    #[tokio::test]
    async fn a_host_with_the_development_identity_says_a_caller_can_sign_in() {
        let app = super::super::app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        )));

        let (status, body) = fetched(&app).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let mut document: Value = serde_json::from_str(&body).expect("a JSON document");
        assert_eq!(
            document["sign_in_available"], true,
            "the development identity is armed, so this deployment can turn a caller into a \
             principal and the descriptor must say so: {body}",
        );

        document
            .as_object_mut()
            .expect("a JSON object")
            .remove("sign_in_available");

        assert_eq!(
            document,
            artifact(),
            "what this host serves is no longer the derived artifact plus one field, so arming a \
             local identity has widened what a stranger learns about this deployment beyond \
             whether sign-in works",
        );
    }

    /// A token exchange that is never called: deciding sign-in *availability* redeems nothing.
    struct NoExchange;

    #[async_trait]
    impl crate::oidc::exchange::TokenExchange for NoExchange {
        async fn redeem(
            &self,
            _: crate::oidc::exchange::Redemption<'_>,
        ) -> Result<crate::oidc::exchange::SignedClaims, crate::oidc::exchange::ExchangeError>
        {
            unreachable!("serving a descriptor redeems no authorization code")
        }
    }

    // ---------------------------------------------------------------------------------------
    // The artifact, and the seam it crosses.
    // ---------------------------------------------------------------------------------------

    /// The document names the route that serves it.
    ///
    /// The cheap half of the cross-language pin. `console/src/onboarding.mts` holds the endpoint the
    /// **page** tells an agent author to fetch, and it is generated into the artifact; this asserts
    /// the route table agrees. Moving the route without regenerating, or regenerating without moving
    /// the route, fails here.
    #[test]
    fn the_endpoint_the_document_names_is_the_route_that_serves_it() {
        let document: Onboarding = serde_json::from_str(DERIVED).expect("a JSON document");

        assert_eq!(
            document.endpoint, ONBOARDING,
            "the descriptor tells a caller it lives somewhere this host does not serve it",
        );
    }

    /// A capability is live exactly when it says how, and withheld exactly when it says why.
    ///
    /// The console asserts this over the model it derives from; this asserts it over the artifact
    /// that is actually served, which is the copy a caller acts on. A capability that is live with
    /// no call is a "you can now" with no instruction; one that is not live and names an endpoint
    /// anyway is an invitation to call nothing.
    #[test]
    fn a_capability_says_how_exactly_when_it_is_live() {
        let document: Onboarding = serde_json::from_str(DERIVED).expect("a JSON document");

        assert!(
            document
                .capabilities
                .iter()
                .any(|capability| capability.live),
            "no capability is live, so this document tells an agent author nothing",
        );
        for capability in &document.capabilities {
            if capability.live {
                assert!(
                    capability.call.is_some(),
                    "`{}` is live and names no call",
                    capability.id,
                );
                assert!(
                    capability.withheld.is_empty(),
                    "`{}` is live and carries a reason it cannot be done",
                    capability.id,
                );
            } else {
                assert!(
                    capability.call.is_none(),
                    "`{}` is not live and names an endpoint anyway",
                    capability.id,
                );
                assert!(
                    !capability.withheld.is_empty(),
                    "`{}` is withheld with no reason beside it, which reads as a broken document \
                     rather than as a statement about the service",
                    capability.id,
                );
            }
        }

        // The auth scheme is not a claim standing on its own: it defers to a capability, and that
        // capability's own liveness is what it reports.
        let backing = document
            .capabilities
            .iter()
            .find(|capability| capability.id == document.authentication.capability)
            .expect("the auth scheme names a capability this document publishes");

        assert_eq!(
            document.authentication.live, backing.live,
            "the auth scheme and the capability it defers to disagree about whether presenting a \
             token identifies a caller",
        );
    }

    /// An artifact this host cannot read refuses, rather than serving half a document.
    ///
    /// Reachable only through [`document`], which is why that function takes the artifact as an
    /// argument. A branch no test can enter is a branch nobody knows the shape of — and the shape
    /// matters here: an agent that reads a capability list missing its last entry concludes the
    /// capability is not there.
    #[test]
    fn an_artifact_this_host_cannot_read_refuses_rather_than_serving_half_a_document() {
        assert!(
            document("{ \"descriptor\": ", true).is_err(),
            "a truncated artifact must not produce a document",
        );

        // The one that matters for a repository where the artifact is generated: a field that
        // appeared in the console model and was never argued for here does not reach a caller.
        let widened = DERIVED.replacen('{', "{ \"tenant\": \"acme\",", 1);
        assert!(
            document(&widened, true).is_err(),
            "a field nobody added to `Onboarding` reached the anonymous surface; \
             `deny_unknown_fields` is what makes widening this document a deliberate act",
        );
    }
}
