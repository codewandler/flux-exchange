//! An agent principal, minted for the caller's tenant, and its token shown exactly once.
//!
//! ```text
//! POST /api/agents   mint an agent principal for this tenant
//! ```
//!
//! # Where the tenant comes from, and what a caller may say
//!
//! [`Extension<Principal>`] and nowhere else, exactly as in [`identity`](super::identity) and
//! [`connections`](super::connections). What a caller supplies is a **name** for the agent and the
//! **expiry** it wants; it never supplies a tenant, and there is no argument on
//! [`AgentStore::mint`] one could reach. The three vector tests at the bottom are this module's
//! copy of the ones `routes::identity` wrote for sessions — path segment, body field, header — and
//! they are here rather than inherited because this is a new way to obtain a principal, which is
//! precisely the kind of route that rule exists for.
//!
//! [`Access::Principal`]: super::Access::Principal
//!
//! # Minting requires a principal, and the refusal names nothing
//!
//! The route is [`Access::Principal`], so an anonymous caller is refused by the guard before this
//! module runs, with the one fixed phrase every guarded route answers with. That is deliberate:
//! a refusal that said "no such tenant" or "you may not mint for that tenant" would answer a caller
//! this host has not identified with a fact about what exists.
//! `super::tests::the_surface_mints_an_agent_principal_and_refuses_an_anonymous_caller` is that
//! claim over the published surface.
//!
//! # A cookie-carried caller **does** get a token here, unlike at `/api/session`
//!
//! [`identity::sign_in`](super::identity) refuses to hand a readable token to a caller that
//! authenticated by cookie, because a session cookie *is* the session and minting a readable copy
//! of it would be pure escalation. That reasoning does not transfer, and pretending it did would
//! break the feature rather than protect it: a human wiring up an agent is signed in to the console,
//! which authenticates by cookie, and nothing they could do would be a "readable credential" until
//! they already had one.
//!
//! So what is actually being traded is worth stating plainly rather than leaving to be discovered:
//!
//! - **Cross-site is closed.** The session cookie is `SameSite=Strict`, so a request whose
//!   navigation chain began at another site does not carry it and cannot reach this route at all.
//! - **Same-origin script is not, and cannot be.** Script running on this origin — an XSS — can
//!   `POST` here and read the token out of its own `fetch` response. There is no arrangement in
//!   which a human is shown a token once and script running as that human is not; the token is on
//!   the page. The remedy is revocation (X-38), and that is the story this one leaves a debt to.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{post, MethodRouter};
use axum::{Extension, Json};
use exchange_host::Principal;
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use super::{Access, Module, Route};
use crate::agent::{AgentError, Expiry, AGENT_STORE_SETTING};
use crate::session;
use crate::state::AppState;

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "agents",
    routes: &[Route {
        // Under `/api` for the reason every other route here is: `vite dev` owns the origin and
        // proxies `/api` to this host, so anything outside that prefix is answered by the SPA
        // fallback instead.
        //
        // A collection path with **no parameter**. There is nothing about *which* agent a mint
        // could take, and in particular nothing a tenant could be spelled into —
        // `super::tests::no_published_route_takes_a_tenant_in_its_path` walks the whole surface,
        // and this route gives it nothing to find.
        path: "/api/agents",
        access: Access::Principal,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    post(mint)
}

/// What a caller supplies when it mints an agent.
///
/// Unknown fields are **not** denied, following
/// [`NewConnection`](super::connections) — a body carrying `tenant` is not refused, it is ignored,
/// and [`tests::a_tenant_in_a_body_field_does_not_influence_the_tenant_minted_for`] asserts the
/// stronger property that the principal still comes back in the resolved tenant. Refusing the field
/// would be the weaker claim: it would say this host noticed, rather than that it could not have
/// been influenced.
///
/// **No `Debug`.** Nothing here is a credential today, but this is the body type of the one route
/// that mints one, and a derived `Debug` is one `debug!(?body)` away from being the place a future
/// field gets logged.
#[derive(Deserialize)]
struct NewAgent {
    /// What to call the agent within this tenant.
    id: String,
    /// When its token stops resolving, as seconds since the Unix epoch.
    ///
    /// Stated by the caller and never defaulted. An agent token always carries an expiry — see
    /// `crate::agent::Expiry` — so a body that omits this is a `422` from the extractor rather than
    /// a token this host chose a lifetime for.
    expires_at: i64,
}

/// Mint an agent principal for the caller's tenant, and answer with its token once.
async fn mint(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<NewAgent>,
) -> Response {
    let Some(agents) = state.agents() else {
        // Refuse; never repair, and in the shape `connections::no_store` uses: this host is not
        // serving the request from somewhere else, it is saying it cannot hold the record.
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "this host has no agent store configured, so an agent token could be minted and \
                 never revoked. Set `{AGENT_STORE_SETTING}` to a path",
            ),
        );
    };

    // One reading of the wall clock, taken here and threaded down, so whether the expiry is in the
    // past and how long the token lives are decided against the same instant. X-24 recorded what it
    // costs when they are two readings.
    let expiry = Expiry {
        expires_at: body.expires_at,
        as_of: session::now(),
    };

    match agents.mint(&principal, &body.id, expiry) {
        Ok(minted) => (
            StatusCode::CREATED,
            Json(json!({
                "principal": minted.principal,
                "expires_at": minted.expires_at,
                // The one disclosure, and the whole point of the route. It is not recoverable from
                // this host afterwards: `crate::agent`'s module documentation says what the store
                // holds instead, and `an_attacker_who_reads_the_store_obtains_no_usable_token`
                // pins it.
                "token": minted.token.as_str(),
                "shown": "once",
            })),
        )
            .into_response(),
        Err(error) => refuse_mint(error),
    }
}

/// A refusal as the caller sees it, per kind of failure.
///
/// The split is the repository's usual one: what the caller can act on comes back, and what names
/// this host's own machinery goes to the log. `TooManyLive` is on the log side deliberately even
/// though a caller could in principle act on it, because the bound is **host-wide** — telling one
/// tenant how many agents this host holds would answer them with the sum of everybody else's.
fn refuse_mint(error: AgentError) -> Response {
    match error {
        AgentError::UnusableId { .. }
        | AgentError::AlreadyExpired { .. }
        | AgentError::ImplausibleLifetime { .. } => {
            // The caller's own input, refused rather than repaired, in the caller's own terms.
            refuse(StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
        }
        AgentError::AlreadyMinted { .. } => {
            // Scoped to the caller's tenant at the store, so this can only ever be about an agent
            // the caller's own tenant holds.
            refuse(StatusCode::CONFLICT, error.to_string())
        }
        AgentError::TooManyLive { .. }
        | AgentError::NoEntropy { .. }
        | AgentError::Unwritable { .. } => {
            error!(%error, "cannot mint an agent token");
            refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "cannot mint an agent token right now".to_string(),
            )
        }
    }
}

/// A refusal as the caller sees it: a status and a reason, never a credential.
fn refuse(status: StatusCode, reason: String) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

/// Whether anything in this text could be an agent token.
///
/// Looks for the *shape* — 64 hex characters — rather than for a key named `token`, following
/// `routes::identity::tests::carries_a_token`, so a refactor that renames the field or nests it
/// cannot quietly reopen what this guards.
#[cfg(test)]
fn carries_a_token(text: &str) -> bool {
    text.as_bytes()
        .windows(64)
        .any(|window| window.iter().all(u8::is_ascii_hexdigit))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use axum::body::Body;
    use axum::extract::Path;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{HeaderMap, Method, Request as HttpRequest};
    use axum::routing::get;
    use axum::Router;
    use serde_json::Value;
    use tower::Service;

    use crate::agent::AgentStore;
    use crate::dev_identity::DevIdentity;
    use crate::routes::app;

    /// The path under test, read from the declaration rather than written out again, so moving the
    /// route cannot leave these tests exercising a path nothing serves.
    const AGENTS: &str = super::MODULE.routes[0].path;

    /// The roster every test below is armed with: one development user, in tenant `acme`.
    const ROSTER: &str = "user:alice@acme";

    /// What a hostile caller claims, down every vector. It is never a tenant that exists.
    const CLAIMED: &str = "attacker";

    /// The tenant `alice` is armed with, and therefore the only answer any of these may produce.
    const RESOLVED: &str = "acme";

    /// A scratch directory holding one store, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-agent-routes-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Self(path)
        }

        fn store(&self) -> Arc<AgentStore> {
            Arc::new(
                AgentStore::open(self.0.join("state").join("agents.json")).expect("a fresh store"),
            )
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An app composed with the development identity and an agent store.
    fn armed(scratch: &Scratch) -> Router {
        app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_agents(scratch.store()))
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

    /// A `POST /api/agents` carrying `alice`'s development credential and `body`.
    fn as_alice(body: Value) -> HttpRequest<Body> {
        HttpRequest::builder()
            .method(Method::POST)
            .uri(AGENTS)
            .header(AUTHORIZATION, "Bearer alice")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a well-formed request")
    }

    /// Thirty days out, as an operator wiring an agent into a config would state it.
    fn in_thirty_days() -> i64 {
        session::now() + 30 * 24 * 60 * 60
    }

    /// **X-36's headline, end to end.** Minting answers with a token, and the token is not
    /// recoverable from what this host stored.
    ///
    /// The store-level form of this claim — every value in the file presented back to `resolve` —
    /// is `crate::agent::tests::an_attacker_who_reads_the_store_obtains_no_usable_token`. This is
    /// the same property from the wire, which is where a future refactor would reintroduce it: a
    /// handler that kept the token somewhere to render it a second time.
    #[tokio::test]
    async fn minting_answers_with_a_token_that_the_store_does_not_hold() {
        let scratch = Scratch::new("headline");
        let store = scratch.store();
        let app = app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_agents(store.clone()));

        let (status, _, body) = call(
            app,
            as_alice(json!({ "id": "triage-bot", "expires_at": in_thirty_days() })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let token = body["token"].as_str().expect("a minted agent token");
        assert_eq!(token.len(), 64, "256 bits, hex encoded");
        assert_eq!(body["principal"]["kind"], "agent");
        assert_eq!(body["principal"]["id"], "triage-bot");
        assert_eq!(body["principal"]["tenant"], RESOLVED);

        let on_disk = std::fs::read_to_string(store.path()).expect("minting writes the store");
        assert!(
            !on_disk.contains(token),
            "this host stored the token it handed out, so it can show it twice",
        );
        assert!(
            on_disk.contains("triage-bot"),
            "and it must have stored the agent: {on_disk}",
        );
    }

    // ---------------------------------------------------------------------------------------
    // The tenant, asserted three times — once per vector a caller controls.
    //
    // Each authenticates as `alice`, armed into tenant `acme`, while claiming tenant `attacker`
    // through one vector. The tenant the agent is minted into must be `acme` every time. These are
    // `routes::identity`'s three, re-run against the route that creates a *new principal* — which
    // is the one where getting it wrong hands somebody a durable credential in a tenant that is
    // not theirs.
    // ---------------------------------------------------------------------------------------

    /// Vector 1 — a **path segment**.
    ///
    /// Asserted against a spy route that genuinely takes `{tenant}`, so the claim is delivered,
    /// matched and readable by a handler, and the mint still happens for the resolved principal.
    /// The companion structural assertion — that no *published* route takes such a segment — is
    /// `super::super::tests::no_published_route_takes_a_tenant_in_its_path`.
    #[tokio::test]
    async fn a_tenant_in_a_path_segment_does_not_influence_the_tenant_minted_for() {
        async fn spy(
            Path(claimed): Path<String>,
            State(state): State<AppState>,
            Extension(principal): Extension<Principal>,
        ) -> Json<Value> {
            let minted = state
                .agents()
                .expect("a store")
                .mint(
                    &principal,
                    "triage-bot",
                    Expiry {
                        expires_at: session::now() + 3600,
                        as_of: session::now(),
                    },
                )
                .expect("randomness");

            Json(json!({ "claimed": claimed, "tenant": minted.principal.tenant() }))
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

        let scratch = Scratch::new("path-vector");
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_agents(scratch.store());
        let app = SPY.router(&state).with_state(state);

        let (status, _, body) = call(
            app,
            HttpRequest::builder()
                .uri(format!("/spy/{CLAIMED}"))
                .header(AUTHORIZATION, "Bearer alice")
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["claimed"], CLAIMED,
            "the handler must actually have received the claimed segment, or this test would pass \
             for the wrong reason",
        );
        assert_eq!(
            body["tenant"], RESOLVED,
            "the agent must be minted into the resolved principal's tenant, not the path segment's",
        );
    }

    /// Vector 2 — a **body field**.
    ///
    /// The likeliest of the three by far: this route takes a body, so `tenant` is the field a
    /// caller would reach for. It is not refused — it is ignored, and the principal still comes
    /// back in `acme`.
    #[tokio::test]
    async fn a_tenant_in_a_body_field_does_not_influence_the_tenant_minted_for() {
        let scratch = Scratch::new("body-vector");

        let (status, _, body) = call(
            armed(&scratch),
            as_alice(json!({
                "id": "triage-bot",
                "expires_at": in_thirty_days(),
                "tenant": CLAIMED,
                "principal": { "tenant": CLAIMED, "kind": "user" },
                "as": CLAIMED,
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            body["principal"]["tenant"], RESOLVED,
            "the tenant must come from the resolved principal, not from the body field",
        );
        assert_eq!(
            body["principal"]["kind"], "agent",
            "and the kind is this host's, not the body's",
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
    async fn a_tenant_in_a_header_does_not_influence_the_tenant_minted_for() {
        let scratch = Scratch::new("header-vector");

        let (status, _, body) = call(
            armed(&scratch),
            HttpRequest::builder()
                .method(Method::POST)
                .uri(AGENTS)
                .header(AUTHORIZATION, "Bearer alice")
                .header(CONTENT_TYPE, "application/json")
                .header("X-Tenant", CLAIMED)
                .header("X-Flux-Tenant", CLAIMED)
                .header("X-Forwarded-Tenant", CLAIMED)
                .body(Body::from(
                    json!({ "id": "triage-bot", "expires_at": in_thirty_days() }).to_string(),
                ))
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
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
    // Refusals.
    // ---------------------------------------------------------------------------------------

    /// **The Acceptance's fifth item.** An anonymous caller is refused, and the refusal names
    /// nothing about what exists — not a tenant, not an agent, not that this route mints anything.
    ///
    /// The surface-level companion, which also pins that the route is declared `Access::Principal`,
    /// is `super::super::tests::the_surface_mints_an_agent_principal_and_refuses_an_anonymous_caller`.
    #[tokio::test]
    async fn an_anonymous_caller_is_refused_and_told_nothing() {
        let scratch = Scratch::new("anonymous");

        let (status, _, body) = call(
            armed(&scratch),
            HttpRequest::builder()
                .method(Method::POST)
                .uri(AGENTS)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "id": "triage-bot", "expires_at": in_thirty_days() }).to_string(),
                ))
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let rendered = body.to_string();
        for leak in ["triage-bot", "acme", "agent", "tenant"] {
            assert!(
                !rendered.contains(leak),
                "the refusal names `{leak}`, which tells an unidentified caller something about \
                 what exists: {rendered}",
            );
        }
        assert!(!carries_a_token(&rendered), "{rendered}");
    }

    /// **The Acceptance's sixth item, from the wire.** An expiry this host will not honour is
    /// refused rather than clamped, and nothing is minted.
    ///
    /// Both directions, because a clamp in either one would look like success: the caller would be
    /// handed a token whose life is not the life they asked for, and would find out only when it
    /// stopped working — or never, in the other direction.
    #[tokio::test]
    async fn an_expiry_this_host_will_not_honour_is_refused_from_the_wire() {
        let scratch = Scratch::new("expiry");
        let store = scratch.store();
        let app = app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_agents(store.clone()));

        for (label, expires_at) in [
            ("already past", session::now() - 1),
            ("a decade", session::now() + 10 * 365 * 24 * 60 * 60),
            ("milliseconds in a seconds field", i64::MAX),
        ] {
            let (status, _, body) = call(
                app.clone(),
                as_alice(json!({ "id": "triage-bot", "expires_at": expires_at })),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "`{label}` must be refused, not clamped: {body}",
            );
            assert!(
                body["token"].is_null() && !carries_a_token(&body.to_string()),
                "a refusal must mint nothing: {body}",
            );
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("Refusing rather than")),
                "the refusal must say what it declined to repair: {body}",
            );
        }

        assert!(
            !store.path().exists(),
            "nothing was minted, so nothing may have been written",
        );
    }

    /// A composition with no agent store refuses and names the setting that would have bound one.
    ///
    /// Not the in-memory fallback this repository refuses: nothing is served from somewhere else,
    /// the host says it cannot hold the record — and a token it could not record is one nobody
    /// could revoke.
    #[tokio::test]
    async fn a_composition_with_no_agent_store_refuses_and_names_the_setting() {
        let app = app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        )));

        let (status, _, body) = call(
            app,
            as_alice(json!({ "id": "triage-bot", "expires_at": in_thirty_days() })),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|reason| reason.contains(AGENT_STORE_SETTING)),
            "{body}",
        );
        assert!(!carries_a_token(&body.to_string()), "{body}");
    }

    /// Minting twice under one name refuses, and the refusal carries no token.
    #[tokio::test]
    async fn a_name_already_taken_refuses() {
        let scratch = Scratch::new("collision");
        let app = armed(&scratch);
        let body = json!({ "id": "triage-bot", "expires_at": in_thirty_days() });

        let (first, _, _) = call(app.clone(), as_alice(body.clone())).await;
        assert_eq!(first, StatusCode::CREATED);

        let (second, _, answer) = call(app, as_alice(body)).await;
        assert_eq!(second, StatusCode::CONFLICT);
        assert!(
            !carries_a_token(&answer.to_string()),
            "a refusal must mint nothing: {answer}",
        );
    }

    /// An identifier this host will not address is refused, and the refusal quotes the **rule**
    /// rather than the value — so nothing a caller sent is reflected into this host's answers.
    #[tokio::test]
    async fn an_unusable_identifier_is_refused_without_being_echoed() {
        let scratch = Scratch::new("bad-id");
        let hostile = "../../etc/passwd";

        let (status, _, body) = call(
            armed(&scratch),
            as_alice(json!({ "id": hostile, "expires_at": in_thirty_days() })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            !body.to_string().contains(hostile),
            "the refusal echoed what the caller sent: {body}",
        );
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|reason| reason.contains("ASCII alphanumerics")),
            "and it must say what would have worked: {body}",
        );
    }
}
