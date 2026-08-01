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
mod health;
mod identity;
mod invoke;
mod signin;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use axum::{Json, Router};
use exchange_host::{IdentityError, PrincipalKind};
use serde_json::json;
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

/// The assembled application.
pub fn app(state: AppState) -> Router {
    MODULES
        .iter()
        .fold(Router::new(), |app, module| {
            app.merge(module.router(&state))
        })
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

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
                warn!(%principal, "a principal of a kind this route does not admit was refused");
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

    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use tower::Service;

    /// Drive one anonymous `GET` through a fully assembled app and report what it answered.
    async fn anonymous_get(app: Router, path: &str) -> StatusCode {
        let mut service = app.into_service::<Body>();
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
        ];

        let mut reachable = Vec::new();

        for (module, route) in published() {
            let status =
                anonymous_get(app(AppState::without_identity()), &probe_path(route.path)).await;

            if status != StatusCode::UNAUTHORIZED {
                reachable.push((module.name, route.path));
            }
        }

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

    /// **X-40.** Every route that admits only some kinds of principal, and the argument for each.
    ///
    /// The companion to `the_anonymous_surface_is_only_what_was_declared_anonymous` on the other
    /// axis, and it has the same two teeth. A route that *stops* being kind-gated fails it, which
    /// is the regression that matters — that is how a leaked agent token gets its mint back. And a
    /// route that *starts* being kind-gated fails it too, so nobody narrows the surface without
    /// writing down why, which is how a `403` for a caller that should have worked gets shipped.
    ///
    /// **The whole list is one entry, and the rest of the surface is deliberately not on it.** In
    /// particular `DELETE /api/connections/{connector}` is not: it destroys tenant data inside the
    /// tenant the caller already belongs to, an operator can see it (`GET /api/connections`) and
    /// undo it by reconnecting, and nothing about it outlives revocation of the token that did it.
    /// Whether an agent may reach a destructive route is a real question, but it is the
    /// **grant-shaped** one — *what may this principal do* — which is X-13. Minting is the
    /// authentication-shaped one — *what kind of principal is this, and may it create another* —
    /// and that is the whole of what this decides. `crate::routes::agents` carries the long form.
    #[test]
    fn the_kind_gated_surface_is_only_what_was_declared() {
        /// Every route that admits fewer than all kinds. Adding a line here is the decision; the
        /// assertion below is only the enforcement.
        const KIND_GATED: &[(&str, &str, &[PrincipalKind])] = &[
            // Minting an agent principal. Only a signed-in human, because a principal that can
            // create principals makes revocation (X-38) an incomplete remedy that an operator
            // cannot see — and `Service` is refused for the same reason one level up, since this
            // host mints, verifies and revokes nothing for a service.
            ("agents", "/api/agents", agents::MAY_MINT),
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
}
