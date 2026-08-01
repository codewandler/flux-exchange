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

mod health;
mod identity;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use axum::{Json, Router};
use exchange_host::IdentityError;
use serde_json::json;
use tower_http::trace::TraceLayer;
use tracing::warn;

use crate::session;
use crate::state::AppState;

/// The feature modules this app is assembled from. **This is the merge site.**
const MODULES: &[Module] = &[health::MODULE, identity::MODULE];

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
    /// Whether a principal is required. This field is what wires the guard — a route is not
    /// guarded by its handler remembering to ask.
    pub access: Access,
    /// How it answers. A function rather than a value so a module's table can be a `const`.
    pub method_router: fn() -> MethodRouter<AppState>,
}

impl Route {
    fn router(&self, state: &AppState) -> Router<AppState> {
        let route = Router::new().route(self.path, (self.method_router)());

        match self.access {
            Access::Anonymous => route,
            // `route_layer` and not `layer`: the guard must run for this route and leave an
            // unmatched path a plain 404, rather than answering 401 for paths that do not exist.
            Access::Principal => route.route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_principal,
            )),
        }
    }
}

/// Whether a route answers a caller the host could not resolve to a principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Answers without a principal. Health is the only one, and a test enforces that.
    Anonymous,
    /// Refused unless the identity port resolves a principal.
    ///
    /// Everything but health. `identity::MODULE` is the first route declared this way, and
    /// `tests::the_surface_publishes_a_route_that_requires_a_principal` keeps at least one so the
    /// enumeration above never becomes a comparison against an empty set.
    Principal,
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

/// Refuse anything this host cannot attribute to a principal.
async fn require_principal(
    State(state): State<AppState>,
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

    let presented = presented(&request).unwrap_or_default();
    // Bound before the match so the borrow of `request` ends here and the resolved principal can be
    // attached below.
    let resolved = identity.resolve(presented).await;

    match resolved {
        Ok(Some(principal)) => {
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
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

/// The credential material a request presents, if it presents any at all.
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
fn presented(request: &Request) -> Option<&str> {
    let headers = request.headers();

    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    bearer.or_else(|| {
        headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| session::from_cookie_header(value, session::SESSION_COOKIE))
    })
}

/// A refusal as the caller sees it: a status and a reason, never a value.
fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
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

    /// The Acceptance's enumeration. Ask the **assembled app**, route by route, which ones answer a
    /// caller it cannot identify — and check the answer is health and nothing else.
    ///
    /// This walks [`published`] rather than a list written out here, so a module added by a later
    /// story is covered the day it lands, including one that publishes an unguarded route.
    #[tokio::test]
    async fn health_is_the_only_route_reachable_without_a_principal() {
        let mut reachable = Vec::new();

        for (module, route) in published() {
            let status =
                anonymous_get(app(AppState::without_identity()), &probe_path(route.path)).await;

            if status != StatusCode::UNAUTHORIZED {
                reachable.push((module.name, route.path));
            }
        }

        assert_eq!(
            reachable,
            [("health", "/health")],
            "every route but /health must refuse a caller with no principal; \
             these answered one: {reachable:?}",
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
            .filter(|(_, route)| route.access == Access::Principal)
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
