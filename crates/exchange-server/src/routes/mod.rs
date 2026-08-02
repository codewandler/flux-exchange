//! The HTTP surface, assembled from per-module route tables.
//!
//! # How a later story adds routes
//!
//! Write a module beside [`health`], give it a `pub(super) const MODULE: Module`, and add one entry
//! to [`MODULES`]. Nothing else in this file changes. That is the point: two stories can add routes
//! at the same time without editing the same lines.
//!
//! # Why a module hands over a table rather than a `Router`
//!
//! axum's `Router` cannot be asked what it answers. A module that built its own router privately
//! could publish a route reachable without a principal, and no test could see it — the enumeration
//! the Acceptance asks for would be enumerating its own assumptions. Here a module declares its
//! routes as data and its `Router` is *derived* from them, so [`published`] is the whole surface by
//! construction. The seam is the same; only the direction of the dependency changed.

mod agents;
mod catalogue;
mod connections;
mod grants;
mod health;
mod identity;
mod invoke;
mod onboarding;
mod signin;

use std::path::Path;

use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, MethodRouter};
use axum::{Json, Router};
use exchange_host::{IdentityError, PrincipalKind};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::warn;

use crate::session;
use crate::state::AppState;

/// The feature modules this app is assembled from. **This is the merge site.**
const MODULES: &[Module] = &[
    health::MODULE,
    catalogue::MODULE,
    identity::MODULE,
    signin::MODULE,
    connections::MODULE,
    agents::MODULE,
    invoke::MODULE,
    grants::MODULE,
    onboarding::MODULE,
];

/// A feature module's contribution to the surface.
pub struct Module {
    /// What it is called, so an enumeration failure names the module that caused it.
    pub name: &'static str,
    /// Every route it publishes.
    pub routes: &'static [Route],
}

impl Module {
    /// This module's own router, guards included.
    fn router(&self, state: &AppState) -> Router<AppState> {
        self.routes.iter().fold(Router::new(), |router, route| {
            router.merge(route.router(state))
        })
    }
}

/// One route, and whether it answers a caller this host has not identified.
pub struct Route {
    /// The path axum matches.
    pub path: &'static str,
    /// Whether a principal is required, and of which kinds. This field is what wires the guard — a
    /// route is not guarded by its handler remembering to ask.
    pub access: Access,
    /// How it answers. A function rather than a value so a module's table can be a `const`.
    pub method_router: fn() -> MethodRouter<AppState>,
}

impl Route {
    fn router(&self, state: &AppState) -> Router<AppState> {
        let route = Router::new().route(self.path, (self.method_router)());

        let admitted = match self.access {
            Access::Anonymous => return route,
            Access::Principal => None,
            Access::PrincipalOfKind(kinds) => Some(kinds),
        };

        // `route_layer` and not `layer`: the guard must run for this route and leave an
        // unmatched path a plain 404, rather than answering 401 for paths that do not exist.
        route.route_layer(middleware::from_fn_with_state(
            (state.clone(), admitted),
            require_principal,
        ))
    }
}

/// Which kinds of principal a guarded route admits. `None` is every kind.
///
/// `None` rather than a written-out list of all three, so [`Access::Principal`] cannot silently
/// stop admitting a kind that gets added to [`PrincipalKind`] later — a route that admits everyone
/// says so by holding nothing, not by holding a list somebody has to remember to extend.
type Admitted = Option<&'static [PrincipalKind]>;

/// Whether a route answers a caller the host could not resolve to a principal, and which kinds of
/// principal it answers at all.
///
/// # Why the kind is declared here and not decided by the handler
///
/// The same reason [`Access`] exists at all: a route is not guarded by its handler remembering to
/// ask. Declaring it as data makes the whole surface enumerable, so
/// `tests::the_kind_gated_surface_is_only_what_was_declared` can walk [`published`] and compare
/// against a list with an argument written beside every entry — the shape
/// `the_anonymous_surface_is_only_what_was_declared_anonymous` already uses for the other axis.
///
/// # This is not the grant model, and does not wait for it
///
/// `docs/designs/agent-access.md` defers **authorization** to X-13, and that deferral holds for
/// *what an agent may call*. [`Access::PrincipalOfKind`] asks a different question — *what kind of
/// principal is calling* — which this host answers today from the credential it issued, with no
/// grant, no connector metadata and no policy. When X-13 lands it decides what a principal may do
/// with a connection; this decides whether a principal may exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Answers without a principal. Health is the only one, and a test enforces that.
    Anonymous,
    /// Refused unless the identity port resolves a principal — of **any** kind.
    ///
    /// Almost everything. `identity::MODULE` is the first route declared this way, and
    /// `tests::the_surface_publishes_a_route_that_requires_a_principal` keeps at least one so the
    /// enumeration above never becomes a comparison against an empty set.
    Principal,
    /// Refused unless the identity port resolves a principal **and** its kind is one of these.
    ///
    /// The narrower form, for a route where the *kind* of caller is the question. `/api/agents` is
    /// the one that exists: minting creates a principal, and a principal that can create principals
    /// is one whose revocation is not a complete remedy. See [`agents`] for the argument.
    PrincipalOfKind(&'static [PrincipalKind]),
}

/// Every route the assembled app publishes, paired with the module that publishes it.
///
/// The enumeration test walks this, and so does startup logging — an operator can see the surface
/// and its access classes without reading the source.
pub fn published() -> impl Iterator<Item = (&'static Module, &'static Route)> {
    MODULES
        .iter()
        .flat_map(|module| module.routes.iter().map(move |route| (module, route)))
}

/// The assembled application, serving the API and nothing else.
///
/// **This is the surface every guard is written against**, and that is the whole reason it survives
/// as its own function now that `main` calls [`app_with_console`]. A checkout serves exactly this —
/// no console directory, no static route — so the enumeration in
/// `tests::the_anonymous_surface_is_only_what_was_declared_anonymous` walks the same router a
/// developer runs. `cfg(test)` because a deployment always answers through `app_with_console`, and a
/// second production entry point differing only in what it omits is one somebody would eventually
/// serve by mistake.
#[cfg(test)]
pub fn app(state: AppState) -> Router {
    app_with_console(state, None)
}

/// The assembled application, optionally serving a built console at `/`.
///
/// # Why the console is served by the host it talks to
///
/// `console/src/service.mts` addresses every endpoint as a **same-origin relative path**, and
/// [`session::host_cookie`](crate::session::host_cookie) issues the session cookie
/// `SameSite=Strict`. A browser does not attach a `Strict` cookie to a request that originated from
/// another origin — not because a CORS header forbids it, but because the cookie is never sent. So
/// hosting the console anywhere other than here cannot work, and the fix is not a CORS layer. X-15
/// and X-40 chose `Strict` deliberately; relaxing it to split the origins would be a security
/// decision wearing a deployment costume.
///
/// # `/api` is answered before the fallback, and that is load-bearing
///
/// A single-page application needs a fallback serving `index.html` so a deep link survives a
/// refresh. Left to itself that fallback also answers **every unmatched `/api` path with `200` and
/// a page of HTML**, which every client reads as success. So an explicit catch-all under `/api`
/// refuses first, and `tests::an_unknown_api_path_refuses_rather_than_serving_the_console` is what
/// keeps it there — the defect is silent, and a test is the only thing that sees it.
///
/// `None` is the shape a checkout runs in: no console built, no static route, and the surface is
/// exactly what the guards enumerate.
pub fn app_with_console(state: AppState, console: Option<&Path>) -> Router {
    let api = MODULES.iter().fold(Router::new(), |app, module| {
        app.merge(module.router(&state))
    });

    let app = match console {
        // The catch-all is registered *before* the fallback so an unmatched `/api` path reaches a
        // refusal rather than the page. Both must exist: without the catch-all the fallback claims
        // `/api`, and without the fallback a refreshed deep link is a 404.
        Some(directory) => api
            .route(API_CATCH_ALL, any(no_such_api_route))
            .route(API_ROOT, any(no_such_api_route))
            .route(API_ROOT_SLASH, any(no_such_api_route))
            .fallback_service(
                ServeDir::new(directory)
                    .fallback(ServeFile::new(directory.join(CONSOLE_ENTRY_POINT))),
            ),
        None => api,
    };

    app.layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Attach the browser policy at the outermost application boundary, including errors and static
/// fallbacks. API responses opt out of storage; fingerprinted console assets remain cacheable.
async fn security_headers(request: Request, next: Next) -> Response {
    let no_store = request.uri().path() == API_ROOT
        || request.uri().path() == API_ROOT_SLASH
        || request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    for (name, value) in [
        (
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'; object-src 'none'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'",
            ),
        ),
        (
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ),
        (
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(
                "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
            ),
        ),
    ] {
        headers.insert(name, value);
    }
    if no_store {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }

    response
}

/// The path patterns that keep the console's fallback off the API.
///
/// Spelled once, and read by the test that proves they work, so a rename cannot leave the test
/// passing against a pattern nothing serves.
///
/// **Three patterns, and the third is why this is a test and not a review comment.** A wildcard
/// matches one segment or more, so `/api/{*unmatched}` covers `/api/nope` and leaves both `/api`
/// and — the one that is easy to miss — `/api/` falling through to the console's entry point with a
/// `200`. X-83's failing-first test drove all three and found the trailing-slash case after the
/// wildcard was already in place.
pub(super) const API_CATCH_ALL: &str = "/api/{*unmatched}";

/// The API prefix with nothing after it.
pub(super) const API_ROOT: &str = "/api";

/// The API prefix with a trailing slash and nothing after it — a distinct route to axum.
pub(super) const API_ROOT_SLASH: &str = "/api/";

/// The document a client-side router is served for any path it owns.
pub(super) const CONSOLE_ENTRY_POINT: &str = "index.html";

/// Refuse an `/api` path this host does not serve.
///
/// A `404` that says so, rather than the console's `index.html` with a `200`. The refusal names no
/// path back to the caller: it is their own input, and echoing it is how a static surface becomes a
/// reflection.
async fn no_such_api_route() -> Response {
    refuse(StatusCode::NOT_FOUND, "no such route on this host")
}

/// A bounded-work refusal, carrying the standard delay and no deployment counters.
pub(super) fn rate_limited(refusal: crate::traffic::TrafficRefusal) -> Response {
    let retry_after = HeaderValue::from_str(&refusal.retry_after().to_string())
        .unwrap_or_else(|_| HeaderValue::from_static("1"));
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_after)],
        Json(json!({ "error": "this host is at its request limit; retry later" })),
    )
        .into_response()
}

/// What [`require_principal`] logs when it identifies a caller and refuses it for its kind.
///
/// Stated once because two things read it: the guard emits it, and
/// `connections::tests::an_agent_may_not_write_a_connection_setting_and_the_refusal_is_logged`
/// asserts an operator would see it. A test that spelled the sentence a second time would go on
/// passing after the guard stopped emitting anything at all.
pub(super) const KIND_REFUSED: &str = "a principal of a kind this route does not admit was refused";

/// Refuse anything this host cannot attribute to a principal — or attributes to the wrong kind of
/// one.
async fn require_principal(
    State((state, admitted)): State<(AppState, Admitted)>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(identity) = state.identity() else {
        // Refuse; never repair. With no identity port bound there is nothing that could resolve a
        // caller, and answering anyway would hand a credential-holding surface to an anonymous one.
        return refuse(
            StatusCode::UNAUTHORIZED,
            "this host has no identity provider configured, so no caller can be resolved to a principal",
        );
    };

    let (presented, carrier) = presented(&request).unwrap_or(("", Carrier::Authorization));
    // Bound before the match so the borrow of `request` ends here and the resolved principal can be
    // attached below.
    let resolved = identity.resolve(presented).await;

    match resolved {
        Ok(Some(principal)) => match admitted {
            Some(kinds) if !kinds.contains(&principal.kind()) => {
                // Identified, and refused anyway. To the log, because an agent reaching for a
                // route only a human may call is the shape of a leaked token being used — and an
                // operator who cannot see it happening has nothing to revoke. The caller's own id
                // and tenant belong here and not in the answer.
                warn!(%principal, "{KIND_REFUSED}");
                refuse_kind(kinds)
            }
            _ => {
                request.extensions_mut().insert(principal);
                // How the caller authenticated, for the one route that mints credentials. The
                // guard is the only thing that inserts this, so a handler cannot be lied to
                // about it.
                request.extensions_mut().insert(carrier);
                next.run(request).await
            }
        },
        Ok(None) => refuse(
            StatusCode::UNAUTHORIZED,
            "this route requires a principal and none was presented",
        ),
        Err(IdentityError::Rejected) => refuse(StatusCode::UNAUTHORIZED, "credential rejected"),
        // Distinct on purpose, and distinct all the way out to the caller: an operator answers an
        // outage and a bad token in opposite ways, and a 401 for an unreachable provider reads to
        // everyone as "your login is broken".
        Err(IdentityError::Unreachable(reason)) => {
            // The reason goes to the log, not to the caller — it describes this host's dependencies.
            warn!(%reason, "identity provider unreachable");
            refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "identity provider unreachable",
            )
        }
    }
}

/// How a caller carried the credential it presented.
///
/// This exists because the two carriers are not equally powerful in the hands of an attacker, and a
/// route that mints credentials has to be able to tell them apart. See
/// [`identity`](crate::routes::identity) for the rule it enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carrier {
    /// An `Authorization` header, which the caller attached deliberately and can therefore read.
    Authorization,
    /// The session cookie, which the browser attaches ambiently and script **cannot** read.
    Cookie,
}

/// The credential material a request presents, and how it carried it.
///
/// Two ways to carry one session, because the two callers this host serves carry things
/// differently: an **agent** sets an `Authorization` header, and a **browser** sends the cookie it
/// was given. Both arrive here as an opaque string, which is the only thing the identity port sees
/// — this function decides *where* a credential was found, never what it means.
///
/// The header wins when both are present. An `Authorization` header is something the caller
/// deliberately attached; a cookie is ambient, attached by the browser on the caller's behalf. When
/// they disagree, the deliberate one is the one that was meant.
///
/// **Nothing about the tenant is read here, from either.** The credential resolves to a principal
/// and the tenant comes from that principal — which is why this returns the material and not a
/// caller identity.
fn presented(request: &Request) -> Option<(&str, Carrier)> {
    let headers = request.headers();

    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|material| (material, Carrier::Authorization));

    bearer.or_else(|| {
        headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| session::from_cookie_header(value, session::SESSION_COOKIE))
            .map(|material| (material, Carrier::Cookie))
    })
}

/// A refusal as the caller sees it: a status and a reason, never a value.
fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

/// Refuse a caller this host **did** identify, because its kind is not one this route admits.
///
/// `403` and not `401`: the credential was good, and telling a caller to authenticate again when
/// authenticating again cannot help is how an operator spends an afternoon rotating a working
/// token.
///
/// # What it says, and what it must not
///
/// It quotes the **rule** — which kinds this route admits, in the wire spelling `PrincipalKind`
/// serialises — and nothing else. Not the caller's own id, not its tenant, not what this host
/// holds, not whether the thing it asked to create already exists. That is
/// `an_anonymous_caller_is_refused_and_told_nothing`'s discipline applied one step later: an
/// identified caller may be told what would have worked, in the same way an unusable identifier is
/// refused by quoting the rule rather than the value, but a refusal is never a place to learn what
/// exists. Derived from the declaration rather than written out, so the rule and the sentence
/// describing it cannot drift.
///
/// `pub(super)` so `agents` answers its own unreachable store-level refusal in these exact terms —
/// see [`agents::MAY_MINT`].
pub(super) fn refuse_kind(admitted: &'static [PrincipalKind]) -> Response {
    let admitted: Vec<String> = admitted.iter().map(PrincipalKind::to_string).collect();

    refuse(
        StatusCode::FORBIDDEN,
        &format!(
            "this route admits only a principal of kind: {}",
            admitted.join(", "),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{BTreeSet, HashMap};
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{Method, Request as HttpRequest};
    use axum::routing::{get, post};
    use exchange_host::{
        address_path, admit_grant, admit_runtime, async_trait, ConnectorSurface, CredentialRef,
        Deployment, Grant, GrantRefusal, Grants, OperationFacts, Principal, Secret, SecretStore,
        StoreError, Tenant,
    };
    use serde_json::Value;
    use tower::Service;

    /// Drive one anonymous `GET` through a fully assembled app and report what it answered.
    async fn anonymous_get(app: Router, path: &str) -> StatusCode {
        anonymous_request(app, Method::GET, path).await
    }

    /// Drive one anonymous request of `method` through a fully assembled app and report what it
    /// answered.
    ///
    /// [`anonymous_get`]'s general form, and the method is the whole of the difference: a path
    /// declared twice serves a different **declaration** on each verb, so a probe that always sent
    /// `GET` reported on whichever declaration happened to serve `GET` and on no other. See
    /// [`a_second_declaration_at_one_path_cannot_hide_from_the_enumeration`].
    async fn anonymous_request(app: Router, method: Method, path: &str) -> StatusCode {
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("a well-formed request");

        service
            .call(request)
            .await
            .expect("a router is infallible")
            .status()
    }

    /// Drive one anonymous `GET` through a fully assembled app and report the status **and** the
    /// body.
    ///
    /// The body half is what [`anonymous_get`] deliberately drops, and a route that exists to
    /// publish a document to strangers cannot be checked without it: "answered 200" is not the
    /// claim, "answered with this and nothing more" is.
    async fn anonymous_get_body(app: Router, path: &str) -> (StatusCode, String) {
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

    /// Drive one `GET` carrying a development roster handle, and report the status and the body —
    /// a refusal aimed at an identified caller is only worth anything if what it says can be read.
    async fn authenticated_get(app: Router, path: &str, handle: &str) -> (StatusCode, String) {
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {handle}"))
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

    /// Drive one anonymous `POST` through a fully assembled app and report what it answered, body
    /// included — a refusal is only worth anything if what it says can be read.
    async fn anonymous_post(app: Router, path: &str) -> (StatusCode, String) {
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method("POST")
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

    /// A path a request can actually be sent to: `/x/{id}` matches nothing literally, and a route
    /// that 404s would be miscounted as unreachable. Later stories will add parameterised paths;
    /// this keeps the enumeration total when they do.
    fn probe_path(path: &str) -> String {
        let mut probe = String::with_capacity(path.len());
        let mut in_param = false;

        for character in path.chars() {
            match character {
                '{' => {
                    in_param = true;
                    probe.push('x');
                }
                '}' => in_param = false,
                _ if in_param => {}
                _ => probe.push(character),
            }
        }

        probe
    }

    /// The app `modules` assemble into, with no identity provider bound.
    ///
    /// [`app`] for an arbitrary set of modules, so a spy module written to defeat the enumeration
    /// is walked through a *merged* router rather than a hand-built one — which is the only place
    /// a path declared twice behaves the way it does in production.
    fn assembled(modules: &'static [Module]) -> Router {
        let state = AppState::without_identity();

        modules
            .iter()
            .fold(Router::new(), |app, module| {
                app.merge(module.router(&state))
            })
            .with_state(state)
    }

    /// A method nothing on this surface serves, used to ask a method router what it *does* serve
    /// without running any of it.
    ///
    /// `TRACE` echoes a request back by definition, so a route that answered one would be
    /// disclosing rather than doing — nothing here has any business serving it, and
    /// [`methods_served`] asserts that rather than assuming it.
    const UNSERVED: Method = Method::TRACE;

    /// Which HTTP methods **this declaration** answers.
    ///
    /// Read out of the `Allow` header axum puts on the refusal, against a router built from this
    /// one declaration and nothing else — which is the point. Asking the assembled app is what
    /// [`the_anonymous_surface_is_only_what_was_declared_anonymous`] used to do, and there a
    /// duplicated path answers whichever declaration serves the verb that was sent.
    ///
    /// It is deliberately the declaration's own [`Route::method_router`] and **not** its guarded
    /// router: `route_layer` wraps the method router's `405` fallback too, so a guarded route
    /// refuses an unserved method with `401` before the fallback can name what it serves. That is
    /// the same fallback-ownership the note on X-61 records.
    ///
    /// Sending [`UNSERVED`] means discovery executes no handler — the method router refuses it
    /// before dispatch, and the refusal still carries `Allow`. One request per declaration, and
    /// nothing runs.
    async fn methods_served(route: &Route) -> Vec<Method> {
        let state = AppState::without_identity();
        let router: Router = Router::new()
            .route(route.path, (route.method_router)())
            .with_state(state);

        let mut service = router.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method(UNSERVED)
            .uri(probe_path(route.path))
            .body(Body::empty())
            .expect("a well-formed request");

        let response = service.call(request).await.expect("a router is infallible");

        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "`{}` answers {UNSERVED}, so it is no longer the method nothing serves — this \
             discovery has just run a handler instead of asking one, and `Allow` may be missing; \
             pick a sentinel nothing serves",
            route.path,
        );

        let allow = response
            .headers()
            .get(header::ALLOW)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_else(|| {
                panic!(
                    "`{}` refused a method without naming what it does serve, so this enumeration \
                     would probe it with nothing at all and report it guarded for free — a method \
                     router built from `any` or from a bare fallback skips the `Allow` header",
                    route.path,
                )
            });

        let served: Vec<Method> = allow
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| {
                Method::from_bytes(name.as_bytes())
                    .expect("axum names real HTTP methods in `Allow`")
            })
            .collect();

        assert!(
            !served.is_empty(),
            "`{}` serves no method at all, so probing it proves nothing",
            route.path,
        );

        served
    }

    /// Every declaration in `modules` that answers a caller this host cannot identify, named by the
    /// module that published it.
    ///
    /// **Per declaration, and probed with a method that declaration actually serves** — which is
    /// X-61, and the difference between a guard that enumerates declarations and one that
    /// enumerates paths. [`methods_served`] asks each declaration's own method router what it
    /// answers; the probe then drives those methods through the **assembled** app, so what is
    /// measured is what a caller would actually get from the merged router rather than what a
    /// declaration would have answered alone.
    ///
    /// One reachable method is enough to report the declaration, so the result carries one entry
    /// per declaration exactly as it did when the probe was a single `GET`.
    ///
    /// Extracted from [`the_anonymous_surface_is_only_what_was_declared_anonymous`] so that
    /// [`a_second_declaration_at_one_path_cannot_hide_from_the_enumeration`] can drive the same
    /// code against a surface built to defeat it — the guard's own guard, in the shape
    /// [`the_declared_access_is_what_decides_the_answer`] uses.
    async fn anonymously_reachable(
        modules: &'static [Module],
        assemble: fn() -> Router,
    ) -> Vec<(&'static str, &'static str)> {
        let mut reachable = Vec::new();

        for module in modules {
            for route in module.routes {
                let probe = probe_path(route.path);

                for method in methods_served(route).await {
                    let status = anonymous_request(assemble(), method, &probe).await;

                    if status != StatusCode::UNAUTHORIZED {
                        reachable.push((module.name, route.path));
                        break;
                    }
                }
            }
        }

        reachable
    }

    /// **X-61's failing-first test.** A second declaration at an already-declared path is visible
    /// to the enumeration that exists to see it.
    ///
    /// X-54 gated `POST /api/connections/{connector}` by declaring that path **twice** — the open
    /// verbs at [`Access::Principal`], the `POST` at [`Access::PrincipalOfKind`] — because
    /// [`Access`] is per [`Route`] and a check inside the handler is what [`Access`] exists to
    /// refuse. That is sound, and it is not what this test is about.
    ///
    /// What it is about is that
    /// [`the_anonymous_surface_is_only_what_was_declared_anonymous`] probed every declaration with
    /// a `GET`. Both declarations at one path answer the same `GET` — served by whichever of them
    /// declares it — so the other one's access was **unobservable to the test whose entire job is
    /// to notice it**: setting the `POST` entry to [`Access::Anonymous`] left the guard green.
    ///
    /// The spy below is that shape with nothing else in it: one path, two declarations, the second
    /// anonymous and reachable only by `POST`. A guard that probes paths rather than declarations
    /// reports an empty set here.
    #[tokio::test]
    async fn a_second_declaration_at_one_path_cannot_hide_from_the_enumeration() {
        fn guarded() -> MethodRouter<AppState> {
            get(|| async { "reached" })
        }

        fn widening() -> MethodRouter<AppState> {
            post(|| async { "reached" })
        }

        const SPIES: &[Module] = &[Module {
            name: "spy",
            routes: &[
                Route {
                    path: "/spy-shared",
                    access: Access::Principal,
                    method_router: guarded,
                },
                // The widening. Same path, different verb, and the only thing that distinguishes
                // it from the entry above is the declared access.
                Route {
                    path: "/spy-shared",
                    access: Access::Anonymous,
                    method_router: widening,
                },
            ],
        }];

        assert_eq!(
            anonymously_reachable(SPIES, || assembled(SPIES)).await,
            vec![("spy", "/spy-shared")],
            "a declaration that answers a caller with no principal did not appear in the \
             enumeration, because a sibling declaration at the same path answers the verb the \
             probe sent; widening the anonymous surface is supposed to be something somebody sees",
        );
    }

    /// Every route that answers a caller this host cannot identify, and the argument for each one
    /// being on the list.
    ///
    /// **Health, and the catalogue.** X-02 wrote this asserting health was the only one; X-06 added
    /// the catalogue and this is the deliberate widening, not a paper-over. The case, in short:
    /// `crate::routes::catalogue` serves `&'static` data compiled in from a published crates.io
    /// package, identical in every deployment of this version. It names no tenant, no principal and
    /// no credential, it never reads a grant, and it never filters — `admitted: null` on every
    /// operation says on the wire what the code does structurally. Requiring a principal would not
    /// make it stricter, because no identity provider binds until X-03; it would make it `401`
    /// forever. `crate::routes::catalogue`'s module documentation carries the long form.
    ///
    /// The test keeps its teeth either way: it walks [`published`] and compares against a set
    /// written out **here**, so a route that becomes anonymous without anyone arguing for it in
    /// this list still fails.
    ///
    /// # What it probes, and what it therefore does not (X-61)
    ///
    /// It enumerates **declarations, not paths**, and drives each one with a method that
    /// declaration actually serves — [`methods_served`] asks the declaration's own method router,
    /// and [`anonymously_reachable`] sends the answer through the assembled app. Until X-61 the
    /// probe was a single `GET`, and a path declared twice answers one `GET` served by one of the
    /// two: setting X-54's `POST` entry to [`Access::Anonymous`] left this green. It no longer
    /// does, and [`a_second_declaration_at_one_path_cannot_hide_from_the_enumeration`] is what
    /// keeps that true rather than this paragraph.
    ///
    /// Two things this still does not reach, stated rather than left to be discovered:
    ///
    /// - **A method no declaration serves.** On a duplicated path the merged router's `405`
    ///   fallback belongs to whichever declaration was merged second, so `PATCH` and `OPTIONS` on
    ///   `/api/connections/{connector}` are decided by the `POST` entry's guard today and by the
    ///   open one if the two were reordered. Nothing pins that order. It is not a hole in either
    ///   order — a caller this host cannot resolve gets `401` and no handler runs, whichever guard
    ///   answers — but it is unpinned, and X-61 records it rather than fixing it here.
    /// - **What a route answers once a principal exists.** This axis is *anonymous or not*;
    ///   [`the_kind_gated_surface_is_only_what_was_declared`] is the other one, and it reads
    ///   [`published`] directly rather than probing, so it already sees every declaration —
    ///   confirmed by mutation, in the note on that test.
    #[tokio::test]
    async fn the_anonymous_surface_is_only_what_was_declared_anonymous() {
        /// Every route allowed to answer without a principal. Adding a line here is the decision;
        /// the assertion below is only the enforcement.
        const ANONYMOUS: &[(&str, &str)] = &[
            // Liveness: an operator has to be able to ask whether the process is up before it can
            // tell them anything else.
            ("health", "/health"),
            // The catalogue: what this binary *could* run, never what a caller may run.
            ("catalogue", "/api/catalogue/connectors"),
            ("catalogue", "/api/catalogue/connectors/{id}/operations"),
            // What a connector *declares* — X-46's widening, and the narrowest one on this list.
            // It reads `Provider::auth`, the same `&'static` vendor data the two routes above
            // read, and it publishes names and nothing a value could occupy. Crucially it is the
            // **declaration and never a tenant's state**: whether anyone holds one of these is
            // `GET /api/connections`, which is `Principal` and stays there. Before this route the
            // only place the service stated a declaration was a `422`, so a console read a
            // capability fact out of an error body — `catalogue::view::ConnectorCredentials`
            // carries the long form.
            ("catalogue", "/api/catalogue/connectors/{id}/credentials"),
            // Sign-in, and the callback it returns through. X-04's widening, and the argument the
            // identity design asked this story to make in its own words rather than inherit.
            //
            // Both are anonymous because **a principal is what they exist to produce**, and
            // neither can present one it has not obtained yet: a human arriving at `/api/signin`
            // has nothing to offer, and the callback is a browser mid-redirect from the provider,
            // carrying an authorization code rather than a credential of ours. Requiring a
            // principal would make signing in possible only for callers who were already signed
            // in.
            //
            // What keeps that from being a hole is that neither route *reads* a credential. The
            // callback's authority is a single-use `state` this host drew from the OS and has not
            // yet spent — not a cookie, not a header, nothing a caller could arrive holding — and
            // it answers with a document and a `Set-Cookie`, never a body a script could read a
            // token out of. `crate::routes::signin`'s module documentation carries the long form,
            // and `the_callback_issues_a_session_only_as_a_cookie` is what holds it.
            ("signin", "/api/signin"),
            ("signin", "/api/signin/callback"),
            // Whether this host can sign anyone in. X-43's widening, and the same argument in a
            // weaker form: a caller who has no principal is exactly who the answer is for, since
            // the alternative is a console that renders a *Sign in* link and finds out by being
            // refused. It reads no credential, it takes no input at all, and what it discloses is
            // one boolean about this **service** — never anything about its configuration, which
            // is why the two compositions that cannot sign anyone in answer byte for byte
            // identically. `crate::routes::signin::availability` carries the long form, including
            // why this is not a field on `/api/session`.
            ("signin", "/api/signin/availability"),
            // The agent descriptor. X-42's widening, and the one on this list that had to be
            // argued field by field rather than route by route — `crate::routes::onboarding`
            // carries that argument, and the types there are `deny_unknown_fields` so a field
            // added to the console model cannot reach this surface without somebody writing one.
            //
            // Anonymous because an agent that must already be authenticated to learn how to
            // authenticate is a closed loop, which is `docs/designs/agent-onboarding.md` §1 and
            // the same shape as `/api/signin` above. What it publishes is a compile-time artifact
            // describing **this build**: identical bytes in every deployment of a version, reading
            // nothing from the composition, the store, the catalogue or the request. The single
            // exception is `sign_in_available`, which is the boolean the route two lines up
            // already publishes — embedding it costs a stranger nothing they could not learn with
            // one more request, and withholding it would make the document dishonest on exactly
            // the deployment the story names.
            ("onboarding", "/api/onboarding"),
        ];

        let reachable = anonymously_reachable(MODULES, || app(AppState::without_identity())).await;

        assert_eq!(
            reachable, ANONYMOUS,
            "the set of routes answering a caller with no principal changed; every entry needs an \
             argument written beside it in ANONYMOUS, and these are what answered: {reachable:?}",
        );
    }

    /// X-03's deliverable, stated as an enumeration rather than as a route name: this host
    /// publishes at least one route that actually requires a principal.
    ///
    /// Without it, the anonymous-surface test above is vacuously green — a surface on which
    /// *every* route is anonymous satisfies "the anonymous set is what was declared" as happily as
    /// one where the guard works. This is the assertion that stops the guard from being mechanism
    /// with no user.
    #[test]
    fn the_surface_publishes_a_route_that_requires_a_principal() {
        let guarded: Vec<_> = published()
            .filter(|(_, route)| route.access != Access::Anonymous)
            .map(|(module, route)| (module.name, route.path))
            .collect();

        assert!(
            !guarded.is_empty(),
            "no published route requires a principal, so the guard protects nothing and the \
             enumeration test above compares health against an empty set",
        );
    }

    /// The tenant comes from the resolved principal and from **nothing a caller controls** — so no
    /// published route may take a tenant in its path. Stated over the whole surface rather than
    /// over the routes that exist today, so a module added by a later story is covered on the day
    /// it lands. X-10 ("no route accepts an address") inherits this.
    #[test]
    fn no_published_route_takes_a_tenant_in_its_path() {
        let tenant_addressed: Vec<_> = published()
            .filter(|(_, route)| {
                route
                    .path
                    .split('/')
                    .filter_map(|segment| segment.strip_prefix('{'))
                    .filter_map(|segment| segment.strip_suffix('}'))
                    .any(|parameter| parameter.to_ascii_lowercase().contains("tenant"))
            })
            .map(|(module, route)| (module.name, route.path))
            .collect();

        assert!(
            tenant_addressed.is_empty(),
            "these routes take a tenant in the path, which is a vector a caller controls; the \
             tenant must come from the resolved principal: {tenant_addressed:?}",
        );
    }

    /// **X-36.** This surface publishes a route that mints an agent principal, and it requires one.
    ///
    /// `docs/vision.md`'s second sentence names an agent as the primary caller, and until this
    /// route existed nothing in this binary could create one: an agent became a principal only
    /// through the development identity, a roster of secretless handles that forces a loopback
    /// bind. So this is the gap stated as an enumeration over the published surface rather than as
    /// a claim about one module — a mint route that stopped being published, or that quietly became
    /// [`Access::Anonymous`], is the same regression either way.
    ///
    /// The second half is the Acceptance's own: an anonymous caller is refused, and the refusal
    /// names nothing about what exists. It is checked here rather than in the module because the
    /// thing that decides it is the declared access, which lives in this file's guard.
    #[tokio::test]
    async fn the_surface_mints_an_agent_principal_and_refuses_an_anonymous_caller() {
        let minting: Vec<_> = published()
            .filter(|(module, _)| module.name == "agents")
            .map(|(_, route)| (route.path, route.access))
            .collect();

        assert_eq!(
            minting,
            vec![("/api/agents", Access::PrincipalOfKind(agents::MAY_MINT))],
            "nothing on this surface mints an agent principal, so the primary caller the vision \
             names can become one only through the development identity",
        );

        let (status, body) = anonymous_post(app(AppState::without_identity()), "/api/agents").await;

        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "minting requires an authenticated principal",
        );
        assert!(
            !body.contains("agent"),
            "the refusal must name nothing about what exists: {body}",
        );
    }

    /// **X-42's failing-first test.** This surface serves an agent descriptor, it serves it to a
    /// caller it has not identified, and it is declared [`Access::Anonymous`] to do so.
    ///
    /// Stated as an enumeration over [`published`] rather than as a claim about one module, in the
    /// shape `the_surface_mints_an_agent_principal_and_refuses_an_anonymous_caller` uses: a
    /// descriptor that stopped being published and one that quietly stopped being anonymous are
    /// both this story undone, and the second is the more likely of the two — a route that is
    /// reachable only by a caller who already has a principal is exactly the closed loop
    /// `docs/designs/agent-onboarding.md` §1 rejects.
    ///
    /// The three assertions on the document are the minimum the Acceptance names: it says what
    /// this service is, it says which capabilities are live, and — on a host with **no identity
    /// provider configured**, which is what `AppState::without_identity` is — it says sign-in is
    /// unavailable rather than pretending. Everything else the descriptor promises is held by
    /// `onboarding::tests` and by `console/test/descriptor.test.mjs`.
    #[tokio::test]
    async fn the_surface_serves_an_agent_descriptor_anonymously() {
        let descriptor: Vec<_> = published()
            .filter(|(module, _)| module.name == "onboarding")
            .map(|(_, route)| (route.path, route.access))
            .collect();

        assert_eq!(
            descriptor,
            vec![("/api/onboarding", Access::Anonymous)],
            "nothing on this surface publishes an agent descriptor, so the caller the vision calls \
             primary can learn what this service is only by reading a console page",
        );

        let (status, body) =
            anonymous_get_body(app(AppState::without_identity()), "/api/onboarding").await;

        assert_eq!(
            status,
            StatusCode::OK,
            "an agent arriving with no principal is exactly who this document is for: {body}",
        );

        let document: serde_json::Value =
            serde_json::from_str(&body).expect("the descriptor is a JSON document");

        assert_eq!(
            document["service"]["name"], "flux-exchange",
            "the descriptor must say what this service is: {body}",
        );
        assert!(
            document["capabilities"]
                .as_array()
                .is_some_and(
                    |capabilities| capabilities.iter().any(|entry| entry["live"] == true)
                        && capabilities.iter().any(|entry| entry["live"] == false)
                ),
            "the descriptor must say which capabilities are live, and this build has both kinds — \
             a document where everything is live claims a platform that does not exist, and one \
             where nothing is tells an agent author nothing: {body}",
        );
        assert_eq!(
            document["sign_in_available"], false,
            "this host has no identity provider configured, and the descriptor must say so rather \
             than pretend: {body}",
        );
    }

    /// **X-40.** Every route that admits only some kinds of principal, and the argument for each.
    ///
    /// The companion to `the_anonymous_surface_is_only_what_was_declared_anonymous` on the other
    /// axis, and it has the same two teeth. A route that *stops* being kind-gated fails it, which
    /// is the regression that matters — that is how a leaked agent token gets its mint back. And a
    /// route that *starts* being kind-gated fails it too, so nobody narrows the surface without
    /// writing down why, which is how a `403` for a caller that should have worked gets shipped.
    ///
    /// **The list is four entries, and the rest of the surface is deliberately not on it.** In
    /// particular `DELETE /api/connections/{connector}` is not — and since X-54 that is visible
    /// here rather than merely stated, because the *same path* appears above for its `POST`. The
    /// `DELETE` destroys tenant data inside the tenant the caller already belongs to, an operator
    /// can see it (`GET /api/connections`) and undo it by reconnecting, and **nothing about it
    /// outlives revocation of the token that did it**. Whether an agent may reach a destructive
    /// route is a real question, but it is the **grant-shaped** one — *what may this principal do*
    /// — which is X-13. Neither are the reads, which answer addresses and booleans and no values.
    ///
    /// That last clause is the test the four entries here pass and `DELETE` does not. Minting
    /// leaves a principal behind, so revoking the token that minted it is not a remedy. Writing a
    /// connection setting puts the tenant's credential on a request to an origin the writer chose,
    /// so by the time anybody revokes anything the credential is already on somebody else's server.
    /// Supplying or rotating the credential itself decides which account every later operation
    /// reaches, at an address nothing records the author of. None of them is *what may this
    /// principal do*; all of them are *what does this outlive*.
    /// `crate::routes::agents`, `crate::routes::connections::MAY_SUPPLY_A_CREDENTIAL` and
    /// `crate::routes::connections::MAY_CONFIGURE` carry the long forms.
    ///
    /// # This one sees both declarations, and that was checked rather than reasoned (X-61)
    ///
    /// X-61 found the anonymous guard next door blind to the second declaration at a duplicated
    /// path, so this one was asked the same question — by mutation, because *it reads
    /// [`published`] so it must be fine* is exactly the reasoning that left the other guard green
    /// for a story and a half. It compares a `Vec` built from [`published`], one entry per
    /// **declaration**, so both entries at `/api/connections/{connector}` are in it independently.
    /// Both directions were driven and both turned it red:
    ///
    /// - the `POST` declaration set to [`Access::Anonymous`] — the mutation the anonymous guard
    ///   could not see — drops its entry, and the assertion names the missing line;
    /// - the `GET`/`DELETE` declaration at the same path widened to
    ///   [`Access::PrincipalOfKind`] adds one.
    ///
    /// What the second run also showed is worth knowing before reading a failure here: two
    /// declarations at one path produce two `(module, path, kinds)` tuples that can be **byte
    /// identical**, so the diff says which entry count is wrong without saying which declaration
    /// caused it. The assertion still fails — a `Vec` keeps count and order — but the message is
    /// read alongside the table, not on its own.
    #[test]
    fn the_kind_gated_surface_is_only_what_was_declared() {
        /// Every route that admits fewer than all kinds. Adding a line here is the decision; the
        /// assertion below is only the enforcement.
        const KIND_GATED: &[(&str, &str, &[PrincipalKind])] = &[
            // Creating a connection. Only a signed-in human, because supplying the credential
            // decides which account this tenant's operations run under — and **nothing records
            // who supplied one**, so `GET /api/connections` reads identically either way and
            // revoking the token that did it does not take the value back out. This is the same
            // path as the two entries missing from this list: `GET` and `DELETE` on
            // `/api/connections/{connector}` stay open to every kind, which is why the path is
            // declared twice. See `connections::MAY_SUPPLY_A_CREDENTIAL`.
            (
                "connections",
                "/api/connections/{connector}",
                connections::MAY_SUPPLY_A_CREDENTIAL,
            ),
            // Rotating one credential. The same argument, and the more invisible half of it: a
            // rotation replaces the value in place, with no observable state in which anything is
            // missing, and it exists for revoking a leaked secret — an operator's act.
            (
                "connections",
                "/api/connections/{connector}/credentials/{credential}",
                connections::MAY_SUPPLY_A_CREDENTIAL,
            ),
            // Writing a connection setting. Only a signed-in human, because the value is
            // substituted into the operation's own request — so whoever writes it chooses the
            // origin this host sends that tenant's credential to, and an agent's token grants
            // access to an operation and never to a credential. The `GET` collection beside it is
            // deliberately **not** here: it answers targets and a `set` boolean and no values.
            (
                "connections",
                "/api/connections/{connector}/settings/{service}/{field}",
                connections::MAY_CONFIGURE,
            ),
            // Minting an agent principal. Only a signed-in human, because a principal that can
            // create principals makes revocation (X-38) an incomplete remedy that an operator
            // cannot see — and `Service` is refused for the same reason one level up, since this
            // host mints, verifies and revokes nothing for a service.
            ("agents", "/api/agents", agents::MAY_MINT),
            // Reading and editing what a tenant may run (X-62). Only a signed-in human, and this
            // is the entry that makes the four above worth having: whoever may edit a grant
            // decides which operations run at all, for every principal of the tenant and across
            // every connection it holds, so an agent that could write here would grant itself the
            // rest of the catalogue and every other gate on this list would be advisory.
            //
            // **The read is on this list too**, which is the half that is easy to miss. The
            // `GET /api/connections` collection is open to every kind because it answers addresses
            // and a boolean; this answers a tenant's whole *policy*, and `exchange_host::admit_grant`
            // deliberately withholds it from a refused caller so that an agent cannot enumerate it
            // one call at a time. A read open to every kind would hand it over in one request.
            // See `grants::MAY_GRANT`.
            //
            // Both verbs of `/api/grants` are one declaration, unlike the two entries above for
            // `/api/connections/{connector}`: they admit the same kinds, and X-61 records what a
            // duplicated path costs the anonymous enumeration next door.
            ("grants", "/api/grants", grants::MAY_GRANT),
            // Evaluating a grant before saving it. Gated with the write rather than left open,
            // because a proposed policy is still a policy — and because a surface that let an
            // agent enumerate which selector admits which operation is the same disclosure the
            // read above is closed for, reached one step sideways.
            ("grants", "/api/grants/preview", grants::MAY_GRANT),
        ];

        let gated: Vec<_> = published()
            .filter_map(|(module, route)| match route.access {
                Access::PrincipalOfKind(kinds) => Some((module.name, route.path, kinds)),
                Access::Anonymous | Access::Principal => None,
            })
            .collect();

        assert_eq!(
            gated, KIND_GATED,
            "the set of routes that admit only some kinds of principal changed; every entry needs \
             an argument written beside it in KIND_GATED, and these are what are gated: {gated:?}",
        );
    }

    /// The kind gate's own guard, in the shape `the_declared_access_is_what_decides_the_answer`
    /// uses: run the mechanism against a caller it must refuse and one it must admit, through the
    /// **same handler**, so only the declared access can explain the difference.
    ///
    /// Without this, `the_kind_gated_surface_is_only_what_was_declared` is a test about a field's
    /// value: a `PrincipalOfKind` the guard never consulted would satisfy it exactly as happily.
    #[tokio::test]
    async fn a_declared_kind_is_what_decides_the_answer() {
        fn open() -> MethodRouter<AppState> {
            get(|| async { "reached" })
        }

        const ONLY_A_USER: &[PrincipalKind] = &[PrincipalKind::User];

        const SPY: Module = Module {
            name: "spy",
            routes: &[
                Route {
                    path: "/spy-any-kind",
                    access: Access::Principal,
                    method_router: open,
                },
                Route {
                    path: "/spy-only-a-user",
                    access: Access::PrincipalOfKind(ONLY_A_USER),
                    method_router: open,
                },
            ],
        };

        let state = AppState::with_development_identity(std::sync::Arc::new(
            crate::dev_identity::DevIdentity::from_roster("user:alice@acme,agent:bot@acme")
                .expect("a well-formed roster"),
        ));
        let app = SPY.router(&state).with_state(state);

        for (handle, path, expected) in [
            ("alice", "/spy-any-kind", StatusCode::OK),
            ("bot", "/spy-any-kind", StatusCode::OK),
            ("alice", "/spy-only-a-user", StatusCode::OK),
            ("bot", "/spy-only-a-user", StatusCode::FORBIDDEN),
        ] {
            let (status, body) = authenticated_get(app.clone(), path, handle).await;

            assert_eq!(
                status, expected,
                "`{handle}` at `{path}`: only the declared access differs between these routes, \
                 and `/spy-any-kind` answering both is what stops this passing for a guard that \
                 refuses everyone: {body}",
            );

            if status == StatusCode::FORBIDDEN {
                // `403` and not `401`: the credential was good. And the refusal quotes the rule —
                // which kinds are admitted — and never the caller, its tenant, or what exists.
                assert!(
                    body.contains("user") && !body.contains("bot") && !body.contains("acme"),
                    "the refusal must quote the rule and name nothing else: {body}",
                );
            }
        }
    }

    /// The other half of the Acceptance's first item: the route an operator checks must answer.
    #[tokio::test]
    async fn health_answers() {
        assert_eq!(
            anonymous_get(app(AppState::without_identity()), "/health").await,
            StatusCode::OK,
        );
    }

    /// The guard's own guard, in the shape this repository already uses for the console scanner:
    /// run the mechanism against a surface it must refuse and one it must admit.
    ///
    /// Without this, a `require_principal` wired to nothing would leave the enumeration above green
    /// — every route would be "reachable", but so would health, and the assertion only names the
    /// set. Here a route declared `Principal` is checked to actually refuse.
    #[tokio::test]
    async fn the_declared_access_is_what_decides_the_answer() {
        fn open() -> MethodRouter<AppState> {
            get(|| async { "reached" })
        }

        const SPY: Module = Module {
            name: "spy",
            routes: &[
                Route {
                    path: "/spy-anonymous",
                    access: Access::Anonymous,
                    method_router: open,
                },
                Route {
                    path: "/spy-principal",
                    access: Access::Principal,
                    method_router: open,
                },
            ],
        };

        let state = AppState::without_identity();
        let app = SPY.router(&state).with_state(state);

        assert_eq!(
            anonymous_get(app.clone(), "/spy-anonymous").await,
            StatusCode::OK,
            "a route declared Anonymous must answer without a principal",
        );
        assert_eq!(
            anonymous_get(app, "/spy-principal").await,
            StatusCode::UNAUTHORIZED,
            "a route declared Principal must refuse without one — the same handler, so only the \
             declared access can explain the difference",
        );
    }

    /// A path no module publishes is a 404, not a 401. `route_layer` is what makes that true, and
    /// getting it wrong would tell an anonymous caller which paths exist.
    #[tokio::test]
    async fn an_unpublished_path_is_not_found() {
        assert_eq!(
            anonymous_get(app(AppState::without_identity()), "/nope").await,
            StatusCode::NOT_FOUND,
        );
    }

    #[test]
    fn probe_paths_are_substituted_for_parameters() {
        assert_eq!(probe_path("/health"), "/health");
        assert_eq!(probe_path("/connections/{id}"), "/connections/x");
        assert_eq!(probe_path("/a/{b}/c/{d}"), "/a/x/c/x");
    }

    // -----------------------------------------------------------------------------------------
    // X-62: the surface a grant is edited through.
    // -----------------------------------------------------------------------------------------

    /// The development roster the three tests below sign in through: one human, one agent, one
    /// tenant. Both kinds are needed — the claim is that the *kind* is what decides the answer.
    const EDITORS: &str = "user:alice@acme,agent:bot@acme";

    /// A grant store that lives in the test, and that really stores.
    ///
    /// Hand-rolled rather than re-exported from `exchange_host`, for
    /// `invoke::tests::HeldGrants`' reason: an in-memory store published from the library crate is
    /// a fallback a production composition could bind, and `AGENTS.md` refuses one. This one has to
    /// actually *hold* what it is handed, because the claim below is about what a write through the
    /// surface leaves behind for the gate to read.
    ///
    /// Keyed by tenant rather than a single list, so a store that answered one tenant's grants for
    /// another could not make these tests greener than they should be.
    #[derive(Default)]
    struct StoredGrants(Mutex<HashMap<String, Vec<Grant>>>);

    impl Grants for StoredGrants {
        fn held(&self, tenant: &Tenant) -> Vec<Grant> {
            self.0
                .lock()
                .expect("no test poisons this")
                .get(tenant.as_str())
                .cloned()
                .unwrap_or_default()
        }

        fn set(&self, tenant: &Tenant, grants: &[Grant]) -> Result<(), GrantRefusal> {
            self.0
                .lock()
                .expect("no test poisons this")
                .insert(tenant.as_str().to_owned(), grants.to_vec());
            Ok(())
        }
    }

    /// A bound credential store that holds nothing. Editing a grant reads no credential, and this
    /// refuses rather than answering, so a test that accidentally reached one would say so.
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
    ///
    /// The grant store reaches the surface through the **invoker**, which is the same binding the
    /// gate decides against — see `routes::grants` for why that is the shape rather than a second
    /// port on `AppState`.
    fn editing(grants: Arc<StoredGrants>) -> AppState {
        let invoker = Arc::new(
            crate::execution::invoker(
                Arc::new(NoCredentials),
                Arc::new(exchange_host::MemoryConfig::new()),
                grants,
            )
            .expect("a usable workspace root"),
        );

        AppState::with_development_identity(Arc::new(
            crate::dev_identity::DevIdentity::from_roster(EDITORS).expect("a well-formed roster"),
        ))
        .with_invoker(invoker)
    }

    /// Drive one request through a fully assembled app and hand back what a caller sees.
    async fn driven(
        app: Router,
        method: Method,
        path: &str,
        handle: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let mut service = app.into_service::<Body>();
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

    /// **X-62's first failing-first test.** This surface publishes a route that edits a grant, and
    /// the kinds it admits are no wider than the ones that may supply a credential.
    ///
    /// Stated as an enumeration over [`published`] in the shape
    /// `the_surface_mints_an_agent_principal_and_refuses_an_anonymous_caller` uses, because the two
    /// regressions worth catching are the same two: a route that stops being published, and one
    /// that quietly widens. The comparison is against `connections::MAY_SUPPLY_A_CREDENTIAL`
    /// itself rather than against a list written out here — the Acceptance's wording is *"at least
    /// as narrow as"*, and pinning it to the constant means widening credential supply cannot
    /// silently widen this too.
    ///
    /// **Why editing a grant is at least that authority.** Supplying a credential decides which
    /// account a tenant's operations reach; editing a grant decides *which operations run at all*,
    /// for every principal of the tenant and for every connection it holds. An agent that could
    /// write here would grant itself the rest of the catalogue, which makes every other kind gate
    /// on this surface advisory.
    #[tokio::test]
    async fn the_surface_edits_a_grant_and_the_write_is_no_wider_than_supplying_a_credential() {
        let editable: Vec<_> = published()
            .filter(|(module, _)| module.name == "grants")
            .map(|(_, route)| (route.path, route.access))
            .collect();

        assert_eq!(
            editable,
            vec![
                (
                    "/api/grants",
                    Access::PrincipalOfKind(connections::MAY_SUPPLY_A_CREDENTIAL),
                ),
                (
                    "/api/grants/preview",
                    Access::PrincipalOfKind(connections::MAY_SUPPLY_A_CREDENTIAL),
                ),
            ],
            "nothing on this surface reads or edits a grant, so a deployment runs nothing at all \
             until somebody hand-writes the grant file",
        );

        // A caller this host cannot identify is refused, and told nothing about what exists.
        let (status, body) = driven(
            app(AppState::without_identity()),
            Method::PUT,
            "/api/grants",
            None,
            Some(json!({ "grants": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert!(
            !body.contains("grant") || !body.contains("acme"),
            "the refusal must name nothing about what this host holds: {body}",
        );

        // An agent this host *did* identify is refused for its kind, and the refusal quotes the
        // rule and nothing else. A leaked agent token that could widen its own tenant's grants
        // makes revocation an incomplete remedy in the worst possible direction.
        let store = Arc::new(StoredGrants::default());
        let (status, body) = driven(
            app(editing(store.clone())),
            Method::PUT,
            "/api/grants",
            Some("bot"),
            Some(json!({ "grants": [] })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "an agent must not be able to decide what its own tenant may run: {body}",
        );
        assert!(
            body.contains("user") && !body.contains("bot") && !body.contains("acme"),
            "the refusal must quote the rule and name nothing else: {body}",
        );
        assert!(
            store.0.lock().expect("no test poisons this").is_empty(),
            "a refused write must have stored nothing",
        );
    }

    /// **X-62's second failing-first test, and the Acceptance's central one.** A grant written
    /// through the surface admits exactly what the gate admits.
    ///
    /// Asserted against [`admit_grant`] itself — the function `Invoker::invoke` calls — and not
    /// against a second copy of its rules. What the surface answers is a *preview*, and a preview
    /// an operator cannot trust is worse than none: a grant that reads as narrow in the console and
    /// is wide at the gate is exactly the mistake this whole model exists to prevent.
    ///
    /// The two bracketing assertions are what stop it passing vacuously. A selector that admitted
    /// nothing would agree with a gate that admits nothing, and one that admitted the whole
    /// connector would agree with a gate that never decided anything — so the grant written here
    /// has to select a proper, non-empty subset of what `github` declares.
    #[tokio::test]
    async fn a_grant_written_through_the_surface_admits_exactly_what_the_gate_admits() {
        let store = Arc::new(StoredGrants::default());

        let (status, body) = driven(
            app(editing(store.clone())),
            Method::PUT,
            "/api/grants",
            Some("alice"),
            Some(json!({
                "grants": [{
                    "connector": "github",
                    "selector": { "max_risk": "low", "effects_within": ["network"] },
                }],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");

        // What the surface told the operator it had just granted.
        let document: Value = serde_json::from_str(&body).expect("a JSON document");
        let previewed: BTreeSet<String> = document["grants"][0]["admits"]
            .as_array()
            .unwrap_or_else(|| panic!("the answer carries what the grant admits: {body}"))
            .iter()
            .map(|facts| {
                facts["id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("an admitted operation carries its id: {body}"))
                    .to_owned()
            })
            .collect();

        // And what the gate would decide, over every operation the connector declares, from the
        // grants this write actually left in the store.
        let held = store.held(&Tenant::new("acme").expect("a usable tenant"));
        assert!(
            !held.is_empty(),
            "the write answered 200 and stored nothing, so the preview above describes a grant \
             that does not exist",
        );

        let caller = Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new("acme").expect("a usable tenant"),
        );
        let provider = connector_catalog::provider(connector_catalog::ProviderKey::id("github"))
            .expect("the catalogue carries `github`");

        let admitted: BTreeSet<String> = provider
            .operations
            .iter()
            .filter(|operation| {
                let runtime =
                    admit_runtime(Deployment::MultiTenant, &ConnectorSurface::of(provider))
                        .expect("http is admitted in every deployment");

                admit_grant(
                    runtime,
                    &caller,
                    provider.id,
                    &OperationFacts::of(operation),
                    &held,
                )
                .is_ok()
            })
            .map(|operation| operation.id.to_owned())
            .collect();

        assert_eq!(
            previewed, admitted,
            "the surface and the gate disagree about what this grant admits; the preview is what \
             an operator decides against, and the gate is what runs",
        );
        assert!(
            !admitted.is_empty(),
            "the grant admits nothing, so the comparison above holds between two empty sets",
        );
        assert!(
            admitted.len() < provider.operations.len(),
            "the grant admits everything `github` declares, so the selector selected nothing and \
             the comparison above cannot tell a gate that decides from one that does not",
        );
    }

    /// **X-62's third failing-first test.** A request naming an operation id is refused.
    ///
    /// The story's *what it must not become*, as a test rather than as a review note. X-13's Goal
    /// is that a grant is decided from an operation's declared metadata **and not from a list of
    /// names**, and `Selector` carries `allow_ids` and `deny_ids` — deliberately, as an operator's
    /// last-resort exception — which serialise. A surface that deserialised `Selector` verbatim
    /// would therefore let a console write ids straight back into the model, and the property the
    /// gate was built around would be gone through the one path that edits it.
    ///
    /// So the refusal is asserted together with the store being untouched: *refuse; never repair*
    /// means the ids are not quietly dropped and the rest of the grant written anyway, because a
    /// caller that asked for an exception and got a narrower grant without being told has been
    /// answered with something it did not ask for.
    #[tokio::test]
    async fn the_surface_refuses_a_grant_that_names_an_operation_id() {
        let store = Arc::new(StoredGrants::default());

        let (status, body) = driven(
            app(editing(store.clone())),
            Method::PUT,
            "/api/grants",
            Some("alice"),
            Some(json!({
                "grants": [{
                    "connector": "github",
                    "selector": {
                        "max_risk": "low",
                        "allow_ids": ["github-issue-create"],
                    },
                }],
            })),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "a grant naming an operation id must be refused: {body}",
        );
        assert!(
            body.contains("allow_ids"),
            "the refusal must name the field that was refused, or an operator cannot act on it: \
             {body}",
        );
        assert!(
            store.0.lock().expect("no test poisons this").is_empty(),
            "a refused grant must not have been stored, in any narrowed form: the caller asked for \
             something this surface does not express and is owed a refusal rather than a guess",
        );
    }

    /// A scratch directory holding a console just real enough to serve, removed on drop.
    ///
    /// Hand-rolled, following `credentials::tests::Scratch` and for the same stated reason: a
    /// dependency is too much to pay for four lines of `create_dir_all`. Two files, because a
    /// direct hit and the fallback are different paths through `ServeDir` and a one-file fixture
    /// cannot tell them apart.
    struct BuiltConsole(std::path::PathBuf);

    impl BuiltConsole {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "exchange-console-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            std::fs::write(
                path.join(CONSOLE_ENTRY_POINT),
                "<!doctype html><title>console</title>",
            )
            .expect("writing the entry point");
            std::fs::write(path.join("app.js"), "// the bundle")
                .expect("writing a bundle beside it");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for BuiltConsole {
        fn drop(&mut self) {
            // Best effort: a test that already removed it is not a test failure.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn a_built_console() -> BuiltConsole {
        BuiltConsole::new()
    }

    /// Drive one anonymous request through an app serving `console`, and report status and body.
    async fn anonymous_request_to(
        console: &Path,
        method: Method,
        path: &str,
    ) -> (StatusCode, String) {
        let app = app_with_console(AppState::without_identity(), Some(console));
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("a well-formed request");

        let response = service.call(request).await.expect("the router answers");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a readable body");

        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// **X-87's failing-first header test.** Browser policy is attached at the outer router, so it
    /// applies equally to API answers, refusals and the static console while only sensitive routes
    /// opt out of caching.
    #[tokio::test]
    async fn the_outer_router_hardens_every_response_and_does_not_cache_the_api() {
        let console = a_built_console();

        for (path, no_store) in [("/", false), ("/health", false), ("/api/onboarding", true)] {
            let app = app_with_console(AppState::without_identity(), Some(console.path()));
            let response = app
                .into_service::<Body>()
                .call(
                    HttpRequest::builder()
                        .uri(path)
                        .body(Body::empty())
                        .expect("a well-formed request"),
                )
                .await
                .expect("the router answers");
            let headers = response.headers();

            for required in [
                "content-security-policy",
                "strict-transport-security",
                "x-content-type-options",
                "referrer-policy",
                "permissions-policy",
            ] {
                assert!(
                    headers.contains_key(required),
                    "GET {path} omitted {required}"
                );
            }

            assert_eq!(
                headers
                    .get(header::CACHE_CONTROL)
                    .and_then(|value| value.to_str().ok()),
                no_store.then_some("no-store"),
                "GET {path} has the wrong caching policy"
            );
        }
    }

    /// **X-83's failing-first test.** An `/api` path this host does not serve must refuse, rather
    /// than being answered by the console's own entry point with a `200`.
    ///
    /// This is the defect a single-page fallback introduces and it is completely silent: the
    /// fallback exists so a refreshed deep link resolves, and left to itself it also claims every
    /// unmatched `/api` path. A client asking for a route this build does not have would receive a
    /// page of HTML and a success status — which is worse than a `404`, because every layer above
    /// treats it as an answer. Watched to fail before `API_CATCH_ALL` existed: it returned `200`
    /// and `<!doctype html>`.
    #[tokio::test]
    async fn an_unknown_api_path_refuses_rather_than_serving_the_console() {
        let console = a_built_console();

        for path in [
            "/api/definitely-not-a-route",
            "/api/connections/../../etc/passwd",
            "/api/",
        ] {
            let (status, body) = anonymous_request_to(console.path(), Method::GET, path).await;

            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "GET {path} was answered {status} instead of refused — a client cannot tell a \
                 missing route from a served one, and the body it got was: {body}"
            );
            assert!(
                !body.contains("<!doctype html"),
                "GET {path} was answered with the console's entry point: a caller asking for a \
                 route this build does not serve received a page and a status that reads as success"
            );
        }
    }

    /// The console is served at `/`, and a deep link it owns survives a refresh.
    ///
    /// The second half is what the fallback is *for*: a client-side router owns paths this host
    /// never declared, and a refresh on one of them is a real request that must answer with the
    /// document rather than a `404`.
    #[tokio::test]
    async fn the_console_is_served_and_its_own_paths_survive_a_refresh() {
        let console = a_built_console();

        let (root, body) = anonymous_request_to(console.path(), Method::GET, "/").await;
        assert_eq!(root, StatusCode::OK, "GET / did not serve the console");
        assert!(body.contains("<title>console</title>"), "{body}");

        let (asset, bundle) = anonymous_request_to(console.path(), Method::GET, "/app.js").await;
        assert_eq!(asset, StatusCode::OK, "a real file beside the entry point");
        assert!(bundle.contains("the bundle"), "{bundle}");

        // A path the console's router owns and this host has never heard of.
        let (deep, page) =
            anonymous_request_to(console.path(), Method::GET, "/connections/zendesk").await;
        assert_eq!(
            deep,
            StatusCode::OK,
            "a deep link the console owns must survive a refresh, or every link is one-way"
        );
        assert!(page.contains("<title>console</title>"), "{page}");
    }

    /// Serving a console does not change what the API answers, on any declared route.
    ///
    /// The guard that matters is [`the_anonymous_surface_is_only_what_was_declared_anonymous`],
    /// which walks [`app`] — and `app` is now `app_with_console(state, None)`, so it enumerates the
    /// surface a checkout runs. This is the other half: with a console bound, every declared route
    /// still answers exactly as it did, so the static service cannot have shadowed one.
    #[tokio::test]
    async fn a_bound_console_shadows_no_declared_route() {
        let console = a_built_console();

        for (module, route) in published() {
            let with_console = anonymous_request_to(console.path(), Method::GET, route.path)
                .await
                .0;
            let without = anonymous_get(app(AppState::without_identity()), route.path).await;

            assert_eq!(
                with_console, without,
                "{}'s {} answered {with_console} with a console bound and {without} without one — \
                 the static service is answering a declared route",
                module.name, route.path
            );
        }
    }
}
