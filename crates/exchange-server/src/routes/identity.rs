//! The session a caller carries, and the principal it resolves to.
//!
//! # Every route here requires a principal, including sign-in
//!
//! There is no anonymous sign-in route, and that is a decision rather than an omission. A caller
//! presents whatever its identity port understands — for the development port, a roster handle;
//! for a future one, a federated token — the guard resolves it to a [`Principal`], and `POST`
//! *exchanges* that for a session a browser can carry in a cookie. So minting a session requires
//! already being resolvable, and this module adds **nothing** to the set of routes that answer a
//! caller with no principal. That set is enumerated and argued for in `super::tests`; a session
//! route is the last thing that should be widening it silently.
//!
//! # Where the tenant comes from
//!
//! [`Extension<Principal>`] and nowhere else. The guard is the only thing that inserts it, so a
//! handler cannot obtain a tenant without one having been resolved — and none of the handlers
//! below takes a `Path`, a body or a header. That is the story's invariant expressed as a
//! signature; the three tests at the bottom are what keep it true.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use exchange_host::Principal;
use serde_json::json;
use tracing::error;

use super::{Access, Carrier, Module, Route};
use crate::session;
use crate::state::AppState;

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "identity",
    routes: &[Route {
        // Under `/api` so the console's dev server proxies it: `vite dev` owns the origin and
        // forwards `/api` to the service, so a session route outside that prefix would be answered
        // by the SPA fallback rather than by this host.
        path: "/api/session",
        // The first guarded route in this binary, and what removed the `#[allow(dead_code)]` from
        // `Access::Principal`.
        access: Access::Principal,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    get(whoami).post(sign_in).delete(sign_out)
}

/// Who this host resolved the caller to be.
///
/// The one route that answers the question the whole story is about, so it is also the one the
/// vector tests interrogate.
async fn whoami(Extension(principal): Extension<Principal>) -> Response {
    Json(json!({ "principal": principal })).into_response()
}

/// Exchange a resolved principal for a session a browser can carry.
///
/// Takes **no** body extractor. There is nothing a request could say about who it is that this
/// route would read: the principal argument was resolved by the guard before the handler ran, and
/// the session is opened for that principal.
///
/// # A caller that authenticated by cookie gets no token
///
/// This is the rule that makes `HttpOnly` mean what it says, and it is worth stating as an
/// invariant rather than as a branch:
///
/// > **A session token is returned in the body only to a caller that presented a *readable*
/// > credential. This route can never turn an unreadable credential into a readable one.**
///
/// Without it, `HttpOnly` is decorative. Script cannot read the cookie, but it does not need to:
/// same-origin `fetch` has the browser attach the cookie ambiently, and `SameSite=Strict` does not
/// apply to a same-origin request. So script could `POST` here with a credential it cannot read and
/// receive one it can — a token that outlives the page, survives sign-out elsewhere and travels off
/// the machine. That is precisely the exfiltration `HttpOnly` exists to prevent, reintroduced by
/// the route that hands out credentials.
///
/// So a cookie-carried caller mints **nothing**: there is nothing to exchange, because a session
/// cookie is already the session. It is answered with its principal, and no new credential enters
/// the world.
async fn sign_in(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(carrier): Extension<Carrier>,
) -> Response {
    let Some(dev) = state.development_identity() else {
        // Only the development port mints its own sessions. A federated provider's session is
        // X-04's, and answering "created" here without having created anything would be worse
        // than saying the route is not implemented for this composition.
        return refuse(
            StatusCode::NOT_IMPLEMENTED,
            "this composition's identity provider does not mint sessions",
        );
    };

    // The caller already holds a session and cannot read it. Minting a second one it *could* read
    // would be an escalation, so this answers without creating anything.
    if carrier == Carrier::Cookie {
        return Json(json!({ "principal": principal })).into_response();
    }

    let token = match dev.open_session(principal.clone()) {
        Ok(token) => token,
        Err(error) => {
            // Refuse; never repair. The reason names this host's own machinery, so it goes to the
            // log rather than to the caller.
            error!(%error, "cannot mint a session");
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "cannot mint a session right now",
            );
        }
    };
    (
        [(header::SET_COOKIE, session::planted(&token))],
        Json(json!({ "token": token.as_str(), "principal": principal })),
    )
        .into_response()
}

/// Close the caller's session and clear the browser's copy of it.
async fn sign_out(State(state): State<AppState>, request: axum::extract::Request) -> Response {
    // Whatever the caller presented is what gets closed. A caller can only ever close its own
    // session, because closing is keyed on the token it just proved it holds.
    let presented = super::presented(&request).map_or("", |(material, _)| material);
    state.close_session(presented);
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session::cleared())],
    )
        .into_response()
}

/// A refusal as the caller sees it: a status and a reason, never a value.
fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::super::{app, Access, Module, Route};
    use crate::dev_identity::DevIdentity;
    use crate::session::SESSION_COOKIE;
    use crate::state::AppState;

    use std::sync::Arc;

    use axum::body::Body;
    use axum::extract::Path;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE};
    use axum::http::{HeaderMap, Method, Request as HttpRequest, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::{get, MethodRouter};
    use axum::{Extension, Json, Router};
    use exchange_host::{async_trait, Identity, IdentityError, Principal, PrincipalKind, Tenant};
    use serde_json::{json, Value};
    use tower::Service;

    /// The path under test, read from the declaration rather than written out again, so moving
    /// the route cannot leave these tests exercising a path nothing serves.
    const SESSION: &str = super::MODULE.routes[0].path;

    /// The roster every test below is armed with: one development user, in tenant `acme`.
    const ROSTER: &str = "user:alice@acme";

    /// What a hostile caller claims, down every vector. It is never a tenant that exists.
    const CLAIMED: &str = "attacker";

    /// The tenant `alice` is armed with, and therefore the only answer any of these may produce.
    const RESOLVED: &str = "acme";

    /// An app composed with the development identity armed from `roster`.
    fn dev_app(roster: &str) -> Router {
        app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(roster).expect("a well-formed roster"),
        )))
    }

    /// Drive one request through a fully assembled app and hand back everything a caller sees.
    async fn call(app: Router, request: HttpRequest<Body>) -> (StatusCode, HeaderMap, Value) {
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let response = service
            .call(request)
            .await
            .expect("a router is infallible")
            .into_response();

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a response body");

        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A request builder carrying `alice`'s development credential.
    fn as_alice(method: Method, path: &str) -> axum::http::request::Builder {
        HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(AUTHORIZATION, "Bearer alice")
    }

    /// Whether anything in this text could be a session token.
    ///
    /// Looks for the *shape* — 64 hex characters — rather than for a key named `token`, so a
    /// refactor that renames the field, nests it, or adds a second credential somewhere else
    /// cannot quietly reopen the escalation this guards.
    fn carries_a_token(text: &str) -> bool {
        text.as_bytes()
            .windows(64)
            .any(|window| window.iter().all(u8::is_ascii_hexdigit))
    }

    // ---------------------------------------------------------------------------------------
    // The story's whole point, asserted three times — once per vector a caller controls.
    //
    // Each of these authenticates as `alice`, who is armed into tenant `acme`, while claiming
    // tenant `attacker` through one vector. The tenant that comes back must be `acme` every time.
    // ---------------------------------------------------------------------------------------

    /// Vector 1 — a **path segment**.
    ///
    /// Asserted against a route that genuinely takes `{tenant}` in its path, so the claim is
    /// delivered, matched and readable by the handler. That is the sharp version of the question:
    /// not "was the segment dropped" but "with the segment in hand, which tenant does the host
    /// use". The companion structural assertion — that no *published* route takes such a segment
    /// at all — is `super::tests::no_published_route_takes_a_tenant_in_its_path`.
    #[tokio::test]
    async fn a_tenant_in_a_path_segment_does_not_influence_the_tenant_used() {
        async fn spy(
            Path(claimed): Path<String>,
            Extension(principal): Extension<Principal>,
        ) -> Json<Value> {
            Json(json!({ "claimed": claimed, "tenant": principal.tenant() }))
        }

        fn spy_route() -> MethodRouter<AppState> {
            get(spy)
        }

        const SPY: Module = Module {
            name: "spy",
            routes: &[Route {
                path: "/spy/{tenant}",
                access: Access::Principal,
                method_router: spy_route,
            }],
        };

        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ));
        let app = SPY.router(&state).with_state(state);

        let (status, _, body) = call(
            app,
            as_alice(Method::GET, &format!("/spy/{CLAIMED}"))
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["claimed"], CLAIMED,
            "the handler must actually have received the claimed segment, or this test would \
             pass for the wrong reason",
        );
        assert_eq!(
            body["tenant"], RESOLVED,
            "the tenant must come from the resolved principal, not from the path segment",
        );
    }

    /// Vector 2 — a **body field**.
    ///
    /// `POST /session` is the route that mints a session, and therefore the one a caller would
    /// most plausibly try to name a tenant on. It takes no body extractor at all: the tenant comes
    /// from the roster the host was armed with, so there is nothing for the field to reach.
    #[tokio::test]
    async fn a_tenant_in_a_body_field_does_not_influence_the_tenant_used() {
        let (status, _, body) = call(
            dev_app(ROSTER),
            as_alice(Method::POST, SESSION)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "tenant": CLAIMED, "as": CLAIMED }).to_string(),
                ))
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["principal"]["tenant"], RESOLVED,
            "the tenant must come from the resolved principal, not from the body field",
        );
        assert!(
            !body.to_string().contains(CLAIMED),
            "nothing the body claimed may survive into the answer: {body}",
        );
    }

    /// Vector 3 — a **header**.
    ///
    /// Several spellings, because the rule is about the class of vector and not about one name a
    /// future reverse proxy might introduce.
    #[tokio::test]
    async fn a_tenant_in_a_header_does_not_influence_the_tenant_used() {
        let (status, _, body) = call(
            dev_app(ROSTER),
            as_alice(Method::GET, SESSION)
                .header("X-Tenant", CLAIMED)
                .header("X-Flux-Tenant", CLAIMED)
                .header("X-Forwarded-Tenant", CLAIMED)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["principal"]["tenant"], RESOLVED,
            "the tenant must come from the resolved principal, not from a header",
        );
        assert!(
            !body.to_string().contains(CLAIMED),
            "nothing a header claimed may survive into the answer: {body}",
        );
    }

    // ---------------------------------------------------------------------------------------
    // The session itself.
    // ---------------------------------------------------------------------------------------

    /// The Acceptance's cookie item, asserted in **one** test rather than spread across the code.
    #[tokio::test]
    async fn the_session_cookie_is_secure_httponly_and_samesite() {
        let (status, headers, _) = call(
            dev_app(ROSTER),
            as_alice(Method::POST, SESSION)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);

        let planted = headers
            .get(SET_COOKIE)
            .expect("minting a session plants a cookie")
            .to_str()
            .expect("a cookie is ASCII");

        assert!(
            planted.starts_with(&format!("{SESSION_COOKIE}=")),
            "{planted}"
        );
        assert!(planted.contains("Secure"), "{planted}");
        assert!(planted.contains("HttpOnly"), "{planted}");
        assert!(planted.contains("SameSite="), "{planted}");
        assert!(planted.contains("Path=/"), "{planted}");
    }

    /// A browser carries the session in a cookie; an agent carries the same token as a bearer.
    /// Both must resolve to the same principal, or "a session a browser or an agent can carry" is
    /// two mechanisms wearing one name.
    #[tokio::test]
    async fn a_session_is_carried_by_a_cookie_or_by_a_bearer_token() {
        let app = dev_app(ROSTER);

        let (_, headers, body) = call(
            app.clone(),
            as_alice(Method::POST, SESSION)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        let token = body["token"].as_str().expect("a minted session token");
        let planted = headers
            .get(SET_COOKIE)
            .expect("a planted cookie")
            .to_str()
            .expect("a cookie is ASCII");

        // The browser's half: the cookie exactly as it was planted, minus the attributes.
        let (_, _, by_cookie) = call(
            app.clone(),
            HttpRequest::builder()
                .uri(SESSION)
                .header(COOKIE, format!("{SESSION_COOKIE}={token}"))
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        // The agent's half: the same token as a bearer.
        let (_, _, by_bearer) = call(
            app,
            HttpRequest::builder()
                .uri(SESSION)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert!(
            planted.contains(token),
            "the planted cookie carries the minted token"
        );
        assert_eq!(by_cookie["principal"]["tenant"], RESOLVED);
        assert_eq!(by_cookie, by_bearer, "one session, two ways to carry it");
    }

    /// The escalation `HttpOnly` exists to prevent, asserted rather than assumed.
    ///
    /// The threat is same-origin script, which is the case `HttpOnly` is *for*: it cannot read the
    /// cookie, but `fetch` has the browser attach it ambiently and `SameSite=Strict` does not apply
    /// to a same-origin request. So script can reach every route here holding a credential it
    /// cannot read. What it must never be able to do is come away holding one it *can* — a token
    /// that outlives the page and travels off the machine.
    ///
    /// This drives exactly that: hold only the cookie, work every method, and check nothing
    /// token-shaped comes back from any of them.
    #[tokio::test]
    async fn a_cookie_session_cannot_be_exchanged_for_a_readable_token() {
        let app = dev_app(ROSTER);

        // Establish a cookie session the way a browser gets one, then forget the token — from here
        // on the attacker holds only what script could hold.
        let (_, _, minted) = call(
            app.clone(),
            as_alice(Method::POST, SESSION)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;
        let cookie = format!(
            "{SESSION_COOKIE}={}",
            minted["token"].as_str().expect("a minted session token"),
        );

        // Everything script can do with an ambient cookie.
        for method in [Method::GET, Method::POST] {
            let (status, headers, body) = call(
                app.clone(),
                HttpRequest::builder()
                    .method(method.clone())
                    .uri(SESSION)
                    .header(COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("a well-formed request"),
            )
            .await;

            assert_eq!(status, StatusCode::OK, "{method} answered {status}");
            assert!(
                body["token"].is_null(),
                "{method} handed a readable token to a caller that authenticated by cookie: {body}",
            );
            assert!(
                !carries_a_token(&body.to_string()),
                "{method} returned something token-shaped, which is the escalation HttpOnly \
                 exists to prevent: {body}",
            );

            // A token planted in a fresh cookie would be just as readable to script, which can
            // read the response headers of its own `fetch`.
            let planted = headers
                .get(SET_COOKIE)
                .map(|value| value.to_str().expect("a cookie is ASCII").to_string())
                .unwrap_or_default();
            assert!(
                !carries_a_token(&planted),
                "{method} planted a new token a script could read off the response: {planted}",
            );
        }
    }

    /// The other half of the rule: a caller that presented a *readable* credential does get a
    /// token, because it already held one and nothing is escalated by handing it another.
    ///
    /// Without this, the assertion above could be satisfied by never minting a token at all, which
    /// would break every agent.
    #[tokio::test]
    async fn a_readable_credential_still_mints_a_readable_token() {
        let (status, headers, body) = call(
            dev_app(ROSTER),
            as_alice(Method::POST, SESSION)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            body["token"]
                .as_str()
                .is_some_and(|token| token.len() == 64),
            "an agent authenticating by header must still get a session it can carry: {body}",
        );
        assert!(
            headers.get(SET_COOKIE).is_some(),
            "and a browser its cookie"
        );
    }

    /// Signing out closes the session, so the token that worked a moment ago no longer does.
    #[tokio::test]
    async fn signing_out_closes_the_session() {
        let app = dev_app(ROSTER);

        let (_, _, body) = call(
            app.clone(),
            as_alice(Method::POST, SESSION)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;
        let token = body["token"].as_str().expect("a minted session token");

        let carried = |method: Method| {
            HttpRequest::builder()
                .method(method)
                .uri(SESSION)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("a well-formed request")
        };

        let (status, headers, _) = call(app.clone(), carried(Method::DELETE)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(
            headers
                .get(SET_COOKIE)
                .expect("signing out clears the cookie")
                .to_str()
                .expect("a cookie is ASCII")
                .contains("Max-Age=0"),
            "signing out must clear the browser's copy too",
        );

        let (status, _, _) = call(app, carried(Method::GET)).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "a closed session must stop resolving",
        );
    }

    /// X-87's route-level regression: federated logout closes the same server-side store the guard
    /// resolves, so replaying a copied cookie after the browser clears its copy does not work.
    #[tokio::test]
    async fn signing_out_closes_an_oidc_session_server_side() {
        use crate::oidc::config::OidcConfig;
        use crate::oidc::exchange::{ExchangeError, Redemption, SignedClaims, TokenExchange};
        use crate::oidc::Oidc;

        struct UnusedExchange;
        #[async_trait]
        impl TokenExchange for UnusedExchange {
            async fn redeem(&self, _: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
                unreachable!("the test begins after OIDC completion")
            }
        }

        let oidc = Arc::new(Oidc::new(
            OidcConfig::for_test("https://accounts.example.com", "flux-exchange", RESOLVED),
            Arc::new(UnusedExchange),
        ));
        let token = oidc.open_session_for_test(Principal::new(
            PrincipalKind::User,
            "248289761001",
            Tenant::new(RESOLVED).expect("a literal tenant"),
        ));
        let app = app(AppState::with_oidc(oidc));
        let carried = |method: Method| {
            HttpRequest::builder()
                .method(method)
                .uri(SESSION)
                .header(AUTHORIZATION, format!("Bearer {}", token.as_str()))
                .body(Body::empty())
                .expect("a well-formed request")
        };

        assert_eq!(
            call(app.clone(), carried(Method::DELETE)).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(app, carried(Method::GET)).await.0,
            StatusCode::UNAUTHORIZED,
            "a copied OIDC session token must not survive logout"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Rejected and Unreachable, distinguished all the way out to the caller.
    // ---------------------------------------------------------------------------------------

    /// The Acceptance's third item. A caller that collapses these cannot tell "not signed in" from
    /// "the IdP is down", so the two must differ in the **status** — the one part of the answer a
    /// client reliably branches on.
    #[tokio::test]
    async fn a_rejected_credential_and_an_unreachable_provider_are_distinguishable() {
        /// An identity port whose provider is down. Nothing else about it matters.
        struct Unreachable;

        #[async_trait]
        impl Identity for Unreachable {
            async fn resolve(&self, _presented: &str) -> Result<Option<Principal>, IdentityError> {
                Err(IdentityError::Unreachable(
                    "dial tcp 10.0.0.7:443: connection refused".into(),
                ))
            }
        }

        let bad_token = HttpRequest::builder()
            .uri(SESSION)
            .header(AUTHORIZATION, "Bearer not-a-session")
            .body(Body::empty())
            .expect("a well-formed request");

        let (rejected, _, rejected_body) = call(dev_app(ROSTER), bad_token).await;

        let (unreachable, _, unreachable_body) = call(
            app(AppState::with_identity(Arc::new(Unreachable))),
            HttpRequest::builder()
                .uri(SESSION)
                .header(AUTHORIZATION, "Bearer anything")
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(
            rejected,
            StatusCode::UNAUTHORIZED,
            "a credential that was presented and is bad is the caller's problem",
        );
        assert_eq!(
            unreachable,
            StatusCode::SERVICE_UNAVAILABLE,
            "a provider that cannot be reached is this host's problem, and an operator answers \
             the two in opposite ways",
        );
        assert_ne!(
            rejected_body, unreachable_body,
            "and they read differently too"
        );

        // The reason names this host's own dependencies, so it goes to the log and not to the
        // caller. A 503 that leaks an internal address is a map of the deployment.
        let leaked = unreachable_body.to_string();
        assert!(!leaked.contains("10.0.0.7"), "{leaked}");
        assert!(!leaked.contains("connection refused"), "{leaked}");
    }

    /// Anonymous is not an error. The trait says `Ok(None)` and an error are different answers;
    /// this is the end-to-end half of that, and it is what keeps a 401 for "no credential" from
    /// being reported as a rejected one.
    #[tokio::test]
    async fn presenting_nothing_is_anonymous_rather_than_rejected() {
        let (status, _, body) = call(
            dev_app(ROSTER),
            HttpRequest::builder()
                .uri(SESSION)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_ne!(
            body["error"], "credential rejected",
            "presenting nothing is anonymous, not a rejected credential",
        );
    }
}
