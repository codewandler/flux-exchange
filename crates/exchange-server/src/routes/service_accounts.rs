//! Service Account resources for the caller's tenant, with a token shown exactly once.
//!
//! ```text
//! POST   /api/service-accounts       mint a Service Account principal for this tenant
//! GET    /api/service-accounts       list this tenant's Service Accounts
//! DELETE /api/service-accounts/{id}  revoke one Service Account
//! ```
//!
//! # Where the tenant comes from, and what a caller may say
//!
//! [`Extension<Principal>`] and nowhere else, exactly as in [`identity`](super::identity) and
//! [`connections`](super::connections). What a caller supplies is a **name** for the Service Account and the
//! **expiry** it wants; it never supplies a tenant, and there is no argument on
//! [`ServiceAccountStore::mint`] one could reach. The three vector tests at the bottom are this module's
//! copy of the ones `routes::identity` wrote for sessions — path segment, body field, header — and
//! they are here rather than inherited because this is a new way to obtain a principal, which is
//! precisely the kind of route that rule exists for.
//!
//! [`Access::Principal`]: super::Access::Principal
//!
//! # Minting requires a principal, and the refusal names nothing
//!
//! The route requires a principal, so an anonymous caller is refused by the guard before this
//! module runs, with the one fixed phrase every guarded route answers with. That is deliberate:
//! a refusal that said "no such tenant" or "you may not mint for that tenant" would answer a caller
//! this host has not identified with a fact about what exists.
//! `super::tests::the_surface_manages_service_accounts_and_refuses_an_anonymous_caller` is that
//! claim over the published surface.
//!
//! # Who may mint: a `User`, and nothing else (X-40)
//!
//! The route is [`Access::PrincipalOfKind`]`(`[`MAY_MINT`]`)`, and [`MAY_MINT`] holds one kind.
//!
//! **Why a `ServiceAccount` may not.** X-36 built the original route and reported the hole in it:
//! nothing gated minting by kind, so a leaked Service Account token could mint
//! successor Service Accounts. The damage is not one extra account — it is that **revocation stops being a
//! remedy, invisibly.** X-38 exists so a leaked token has an answer, and a token that mints
//! successors makes that answer incomplete in a way an operator cannot see: the descendants are
//! ordinary Service Accounts with no recorded relationship to the one that was revoked, so an operator who
//! revokes the leaked token, sees it stop resolving, and closes the incident is wrong and has no
//! way to find out. Nothing here relates a minted account to whatever minted it, which is exactly why
//! the gate is on *creating* one rather than on *cleaning up after* one.
//!
//! **Why a `Service` may not, which is a decision and not an omission.** A `Service` is another
//! backend acting on behalf of one of its own accounts and actors, and it is the caller a
//! programmatic provisioning story would reach for — so refusing it costs something real, and the
//! argument has to be worth that.
//!
//! It is. The property this gate defends is that *revoking a token ends the access it gave*, and
//! that holds only if every minter is itself revocable **by this host's operator** and every mint
//! is attributable to something that can be ended. A `User` is: sign-in is federated, so the
//! account behind it is disabled at the identity provider and X-16 is what makes this host notice.
//! A `Service` is not: nothing in this repository mints a service credential, verifies one, lists
//! one or revokes one — `PrincipalKind::Service` is a kind the identity port may return and nothing
//! else. Letting it mint would put the *same* defect one level up, and one level further out of
//! sight, since there would be no revoke route to be incomplete in the first place.
//!
//! There is a second reason, weaker but pointing the same way: a `Service` acts **on behalf of** an
//! account and an actor. Its authority is delegated, and a Service Account principal is not a thing done on
//! someone's behalf — it is a durable, independently-authenticating identity in the tenant with
//! its own expiry and revocation lifecycle.
//!
//! **The other answer, stated and rejected.** It runs: a service is a backend, backends are
//! trusted, an operator who wired one up meant it to act — so let it mint. That mistakes *being a
//! backend* for *being accountable*, which is the property actually at stake. And the two mistakes
//! are not symmetric: refusing a `Service` that should mint is a `403` an operator meets on their
//! first attempt and files a story about, while admitting one that should not is a hole nobody
//! meets until a credential leaks. One of those is reversible in a patch release and the other is
//! reversible in nobody's incident review. **Refuse; never repair** — this repository's own rule —
//! points the same way, so `Service` is refused now and the story that wants it is the story that
//! gives it a revocation path.
//!
//! **How far the argument reaches, and where it stops.** ⚠ *This said "only here" and was true when
//! X-40 wrote it. It is not now: X-47 gated the connection-settings write to `User`, and X-54 gated
//! `POST /api/connections/{connector}` and `PUT /api/connections/{connector}/credentials/{credential}`
//! the same way. **Several routes on this surface are kind-gated**, and `routes::KIND_GATED` is the list
//! — read that rather than this sentence, because it is enforced and this is prose.* What follows is
//! still accurate and is the part worth keeping. In particular
//! `DELETE /api/connections/{connector}` stays [`Access::Principal`] for every kind, and that too
//! is decided rather than left: destroying a connection destroys *tenant data the tenant owns*,
//! and a Service Account doing it is a Service Account acting inside the tenant it already belongs to, doing
//! something an operator can see (`GET /api/connections`) and undo by reconnecting. Nothing about
//! it survives revocation of the token that did it. Whether a Service Account *should* be able to reach a
//! destructive route at all is a real question, but it is the grant-shaped one — *what may this
//! principal do* — which is X-13 and needs the grant model. This story is the
//! authentication-shaped one — *what kind of principal is this* — and answering the other with it
//! would be inventing a policy model in a route table.
//!
//! # A cookie-carried caller **does** get a token here, unlike at `/api/session`
//!
//! [`identity::sign_in`](super::identity) refuses to hand a readable token to a caller that
//! authenticated by cookie, because a session cookie *is* the session and minting a readable copy
//! of it would be pure escalation. That reasoning does not transfer, and pretending it did would
//! break the feature rather than protect it: a human wiring up a Service Account is signed in to the console,
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
//!   the page. The remedy is the revocation route on this same resource.

use axum::extract::{Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, MethodRouter};
use axum::{Extension, Json};
use exchange_host::Principal;
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use super::{refuse_operator, Access, Module, Route};
use crate::service_account::{Expiry, ServiceAccountError, SERVICE_ACCOUNT_STORE_SETTING};
use crate::session;
use crate::state::AppState;

/// This module's contribution to the surface.
///
/// The route boundary requires deployment operator authority. The store retains its defensive
/// human-kind check, so a future internal caller cannot bypass the narrower domain invariant.
pub(super) const MODULE: Module = Module {
    name: "service-accounts",
    routes: &[
        Route {
            // Under `/api` for the reason every other route here is: `vite dev` owns the origin and
            // proxies `/api` to this host, so anything outside that prefix is answered by the SPA
            // fallback instead.
            //
            // A collection path with **no parameter**. There is nothing about *which* agent a mint
            // could take, and in particular nothing a tenant could be spelled into —
            // `super::tests::no_published_route_takes_a_tenant_in_its_path` walks the whole surface,
            // and this route gives it nothing to find.
            path: "/api/service-accounts",
            // **Only a `User`.** Minting is not an operation against a connection, it is the creation
            // of a principal in this tenant — so the question this route asks is which *kind* of caller
            // may do that, and [`MAY_MINT`] is the answer.
            access: Access::Operator,
            method_router: collection,
        },
        Route {
            path: "/api/service-accounts/{id}",
            access: Access::Operator,
            method_router: item,
        },
        Route {
            path: "/api/agents",
            access: Access::Operator,
            method_router: legacy_collection,
        },
    ],
};

fn collection() -> MethodRouter<AppState> {
    get(list).post(mint)
}

fn item() -> MethodRouter<AppState> {
    delete(revoke)
}

fn legacy_collection() -> MethodRouter<AppState> {
    post(mint_legacy)
}

/// What a caller supplies when it mints a Service Account.
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
struct NewServiceAccount {
    /// What to call the Service Account within this tenant.
    id: String,
    /// When its token stops resolving, as seconds since the Unix epoch.
    ///
    /// Stated by the caller and never defaulted. A Service Account token always carries an expiry — see
    /// `crate::service_account::Expiry` — so a body that omits this is a `422` from the extractor rather than
    /// a token this host chose a lifetime for.
    expires_at: i64,
}

/// Mint a Service Account principal for the caller's tenant, and answer with its token once.
async fn mint(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<NewServiceAccount>,
) -> Response {
    let Some(service_accounts) = state.service_accounts() else {
        // Refuse; never repair, and in the shape `connections::no_store` uses: this host is not
        // serving the request from somewhere else, it is saying it cannot hold the record.
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "this host has no Service Account store configured, so a Service Account token could be minted and \
                 never revoked. Set `{SERVICE_ACCOUNT_STORE_SETTING}` to a path",
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

    match service_accounts.mint(&principal, &body.id, expiry) {
        Ok(minted) => {
            (
                StatusCode::CREATED,
                Json(json!({
                    "principal": minted.principal,
                    "expires_at": minted.expires_at,
                    // The one disclosure, and the whole point of the route. It is not recoverable from
                    // this host afterwards: `crate::service_account`'s module documentation says what the store
                    // holds instead, and `an_attacker_who_reads_the_store_obtains_no_usable_token`
                    // pins it.
                    "token": minted.token.as_str(),
                    "shown": "once",
                })),
            )
                .into_response()
        }
        Err(error) => refuse_mint(error),
    }
}

/// The v0.16 create alias. It produces the canonical principal and advertises its replacement on
/// every response, including refusals.
async fn mint_legacy(
    state: State<AppState>,
    principal: Extension<Principal>,
    body: Json<NewServiceAccount>,
) -> Response {
    deprecated(mint(state, principal, body).await)
}

async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(accounts) = state.service_accounts() else {
        return no_store();
    };
    match accounts.list(&principal, session::now()) {
        Ok(accounts) => (
            StatusCode::OK,
            Json(json!({ "service_accounts": accounts })),
        )
            .into_response(),
        Err(ServiceAccountError::MayNotManage { .. }) => refuse_operator(),
        Err(error) => refuse_management(error),
    }
}

async fn revoke(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    let Some(accounts) = state.service_accounts() else {
        return no_store();
    };
    match accounts.revoke(&principal, &id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ServiceAccountError::MayNotManage { .. }) => refuse_operator(),
        Err(ServiceAccountError::NotFound { .. }) => {
            refuse(StatusCode::NOT_FOUND, "no such Service Account".to_owned())
        }
        Err(error) => refuse_management(error),
    }
}

fn no_store() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "this host has no Service Account store configured. Set `{SERVICE_ACCOUNT_STORE_SETTING}` to a path"
        ),
    )
}

fn refuse_management(error: ServiceAccountError) -> Response {
    match error {
        ServiceAccountError::UnusableId { .. } => {
            refuse(StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
        }
        ServiceAccountError::Unwritable { .. } => {
            error!(%error, "cannot update Service Accounts");
            refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "cannot update Service Accounts right now".to_owned(),
            )
        }
        _ => {
            error!(%error, "unexpected Service Account management refusal");
            refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "request refused".to_owned(),
            )
        }
    }
}

fn deprecated(mut response: Response) -> Response {
    response.headers_mut().insert(
        HeaderName::from_static("deprecation"),
        HeaderValue::from_static("true"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("link"),
        HeaderValue::from_static("</api/service-accounts>; rel=\"successor-version\""),
    );
    response.headers_mut().insert(
        HeaderName::from_static("warning"),
        HeaderValue::from_static(
            "299 flux-exchange \"/api/agents is deprecated and will be removed in v0.17\"",
        ),
    );
    response
}

/// A refusal as the caller sees it, per kind of failure.
///
/// The split is the repository's usual one: what the caller can act on comes back, and what names
/// this host's own machinery goes to the log. `TooManyLive` is on the log side deliberately even
/// though a caller could in principle act on it, because the bound is **host-wide** — telling one
/// tenant how many service_accounts this host holds would answer them with the sum of everybody else's.
fn refuse_mint(error: ServiceAccountError) -> Response {
    match error {
        ServiceAccountError::UnusableId { .. }
        | ServiceAccountError::AlreadyExpired { .. }
        | ServiceAccountError::ImplausibleLifetime { .. } => {
            // The caller's own input, refused rather than repaired, in the caller's own terms.
            refuse(StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
        }
        ServiceAccountError::AlreadyMinted { .. } => {
            // Scoped to the caller's tenant at the store, so this can only ever be about a Service Account
            // the caller's own tenant holds.
            refuse(StatusCode::CONFLICT, error.to_string())
        }
        ServiceAccountError::MayNotMint { .. } => {
            // Unreachable through the published operator route, and answered anyway in the
            // guard's terms. `error.to_string()` names the caller's kind, which belongs in a log
            // line and not in an answer.
            refuse_operator()
        }
        ServiceAccountError::MayNotManage { .. } | ServiceAccountError::NotFound { .. } => {
            error!(%error, "unexpected Service Account mint refusal");
            refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "request refused".to_owned(),
            )
        }
        ServiceAccountError::TooManyLive { .. }
        | ServiceAccountError::NoEntropy { .. }
        | ServiceAccountError::Unwritable { .. } => {
            error!(%error, "cannot mint a Service Account token");
            refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "cannot mint a Service Account token right now".to_string(),
            )
        }
    }
}

/// A refusal as the caller sees it: a status and a reason, never a credential.
fn refuse(status: StatusCode, reason: String) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

/// Whether anything in this text could be a Service Account token.
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
    use exchange_host::PrincipalKind;
    use serde_json::Value;
    use tower::Service;

    use crate::dev_identity::DevIdentity;
    use crate::routes::app;
    use crate::service_account::ServiceAccountStore;

    /// The path under test, read from the declaration rather than written out again, so moving the
    /// route cannot leave these tests exercising a path nothing serves.
    const SERVICE_ACCOUNTS: &str = super::MODULE.routes[0].path;

    /// The roster every test below is armed with: one development user, in tenant `acme`.
    const ROSTER: &str = "user:alice@acme";

    /// One handle of each kind, all in tenant `acme`, for the tests about **who** may mint.
    ///
    /// The development roster can produce every kind without needing separate credential fixtures,
    /// so this is what lets the human-only lifecycle rule be asserted over the wire.
    const EVERY_KIND_ROSTER: &str = "user:alice@acme,agent:incumbent@acme,service:ingest@acme";

    /// What a hostile caller claims, down every vector. It is never a tenant that exists.
    const CLAIMED: &str = "attacker";

    /// The tenant `alice` is armed with, and therefore the only answer any of these may produce.
    const RESOLVED: &str = "acme";

    /// The Service Account a `User` mints in the who-may-mint test — the leg that must succeed.
    const MINTED_BY_A_USER: &str = "minted-by-a-human";

    /// The successor an `Agent` must not be able to mint. Distinct spellings for the two refused
    /// kinds, so the store assertion says *which* gate leaked rather than that one of them did.
    const SUCCESSOR_OF_AN_AGENT: &str = "successor-of-an-agent";

    /// The successor a `Service` must not be able to mint.
    const SUCCESSOR_OF_A_SERVICE: &str = "successor-of-a-service";

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

        fn store(&self) -> Arc<ServiceAccountStore> {
            Arc::new(
                ServiceAccountStore::open(self.0.join("state").join("service_accounts.json"))
                    .expect("a fresh store"),
            )
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An app composed with the development identity and a Service Account store.
    fn armed(scratch: &Scratch) -> Router {
        app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_service_accounts(scratch.store()))
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

    /// A `POST /api/service-accounts` carrying `alice`'s development credential and `body`.
    fn as_alice(body: Value) -> HttpRequest<Body> {
        as_handle("alice", body)
    }

    /// A `POST /api/service-accounts` carrying one roster handle's development credential and `body`.
    fn as_handle(handle: &str, body: Value) -> HttpRequest<Body> {
        HttpRequest::builder()
            .method(Method::POST)
            .uri(SERVICE_ACCOUNTS)
            .header(AUTHORIZATION, format!("Bearer {handle}"))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a well-formed request")
    }

    /// Thirty days out, as an operator wiring a Service Account into a config would state it.
    fn in_thirty_days() -> i64 {
        session::now() + 30 * 24 * 60 * 60
    }

    /// **X-40's headline.** Only a `User` mints — asserted for all three kinds in one run.
    ///
    /// The three legs are one test on purpose. A refusal for a Service Account proves nothing on its own:
    /// a route that had simply stopped minting would satisfy it, and the way this gate fails in
    /// practice is by being too wide or by taking the feature down with it. So the same app, the
    /// same store and the same wall clock answer all three, and the user's `201` is what stops the
    /// other two from passing vacuously.
    ///
    /// **Asserted against the store, not only the status.** A `403` that had already written the
    /// agent would be the whole defect wearing the right status code, and the store is the thing
    /// `resolve` reads — so what is on disk is what decides whether a successor exists.
    #[tokio::test]
    async fn only_a_user_mints_and_no_other_kind_creates_a_successor() {
        let scratch = Scratch::new("who-may-mint");
        let store = scratch.store();
        let app = app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(EVERY_KIND_ROSTER).expect("a well-formed roster"),
        ))
        .with_service_accounts(store.clone()));

        // Leg one: a human still mints. First, so the two refusals below are refusals of the kind
        // and not of a route that has stopped working.
        let (status, _, minted) = call(
            app.clone(),
            as_alice(json!({ "id": MINTED_BY_A_USER, "expires_at": in_thirty_days() })),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "a user must still mint, or the refusals below pass by having broken minting for \
             everyone: {minted}",
        );
        assert_eq!(minted["principal"]["id"], MINTED_BY_A_USER);

        // Legs two and three: neither of the non-human kinds creates a principal.
        //
        // The Service Account is the story's case — a leaked token minting successors is what makes
        // revocation (X-38) an incomplete remedy, invisibly, because a successor is an ordinary
        // agent with no recorded relationship to the one that was revoked.
        //
        // The service is the same question one level up, and it is decided rather than omitted:
        // see this module's documentation for why refusing is the answer.
        for (handle, successor) in [
            ("incumbent", SUCCESSOR_OF_AN_AGENT),
            ("ingest", SUCCESSOR_OF_A_SERVICE),
        ] {
            let (status, _, refusal) = call(
                app.clone(),
                as_handle(
                    handle,
                    json!({ "id": successor, "expires_at": in_thirty_days() }),
                ),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "`{handle}` minted a successor, so revoking its own token would not end the \
                 access it gave: {refusal}",
            );

            // The refusal names nothing about what exists, following
            // `an_anonymous_caller_is_refused_and_told_nothing`. This caller is identified, so the
            // rule it broke may be quoted — but no id, no tenant, and no count of what this host
            // holds.
            let rendered = refusal.to_string();
            for leak in [
                successor,
                MINTED_BY_A_USER,
                RESOLVED,
                handle,
                "agent",
                "tenant",
            ] {
                assert!(
                    !rendered.contains(leak),
                    "the refusal names `{leak}`, which tells the caller something about what \
                     exists: {rendered}",
                );
            }
            assert!(!carries_a_token(&rendered), "{rendered}");
        }

        // The claim that actually matters: no successor was created. Read from the store rather
        // than inferred from the statuses, because the store is what `resolve` reads.
        let on_disk = std::fs::read_to_string(store.path()).expect("minting writes the store");
        assert!(
            on_disk.contains(MINTED_BY_A_USER),
            "the user's mint must be on disk, or the assertions below hold over an empty file: \
             {on_disk}",
        );
        for successor in [SUCCESSOR_OF_AN_AGENT, SUCCESSOR_OF_A_SERVICE] {
            assert!(
                !on_disk.contains(successor),
                "`{successor}` exists, so the refusal was a status and not a refusal: {on_disk}",
            );
        }
    }

    /// **X-36's headline, end to end.** Minting answers with a token, and the token is not
    /// recoverable from what this host stored.
    ///
    /// The store-level form of this claim — every value in the file presented back to `resolve` —
    /// is `crate::service_account::tests::an_attacker_who_reads_the_store_obtains_no_usable_token`. This is
    /// the same property from the wire, which is where a future refactor would reintroduce it: a
    /// handler that kept the token somewhere to render it a second time.
    #[tokio::test]
    async fn minting_answers_with_a_token_that_the_store_does_not_hold() {
        let scratch = Scratch::new("headline");
        let store = scratch.store();
        let app = app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_service_accounts(store.clone()));

        let (status, _, body) = call(
            app,
            as_alice(json!({ "id": "triage-bot", "expires_at": in_thirty_days() })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let token = body["token"]
            .as_str()
            .expect("a minted Service Account token");
        assert_eq!(token.len(), 69, "fxsa_ plus 256 bits, hex encoded");
        assert!(token.starts_with("fxsa_"));
        assert!(token[5..].bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(body["principal"]["kind"], "service_account");
        assert_eq!(body["principal"]["id"], "triage-bot");
        assert_eq!(body["principal"]["tenant"], RESOLVED);

        let on_disk = std::fs::read_to_string(store.path()).expect("minting writes the store");
        assert!(
            !on_disk.contains(token),
            "this host stored the token it handed out, so it can show it twice",
        );
        assert!(
            on_disk.contains("triage-bot"),
            "and it must have stored the Service Account: {on_disk}",
        );
    }

    #[tokio::test]
    async fn canonical_create_list_authenticate_and_revoke_form_one_resource() {
        let scratch = Scratch::new("canonical-lifecycle");
        let app = armed(&scratch);
        let (status, _, created) = call(
            app.clone(),
            as_alice(json!({ "id": "ci-runner", "expires_at": in_thirty_days() })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["principal"]["kind"], "service_account");
        let token = created["token"].as_str().expect("one-time token");

        let (status, _, session) = call(
            app.clone(),
            HttpRequest::builder()
                .uri("/api/session")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{session}");
        assert_eq!(session["principal"]["id"], "ci-runner");

        let (status, _, listed) = call(
            app.clone(),
            HttpRequest::builder()
                .uri(SERVICE_ACCOUNTS)
                .header(AUTHORIZATION, "Bearer alice")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        assert_eq!(listed["service_accounts"][0]["id"], "ci-runner");
        assert!(listed.to_string().find(token).is_none());

        let (status, _, _) = call(
            app.clone(),
            HttpRequest::builder()
                .method(Method::DELETE)
                .uri("/api/service-accounts/ci-runner")
                .header(AUTHORIZATION, "Bearer alice")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, _, _) = call(
            app,
            HttpRequest::builder()
                .uri("/api/session")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_bearer_resolved_by_two_identity_ports_is_refused_as_ambiguous() {
        struct AlsoResolves {
            token: String,
            principal: Principal,
        }

        #[exchange_host::async_trait]
        impl exchange_host::Identity for AlsoResolves {
            async fn resolve(
                &self,
                presented: &str,
            ) -> Result<Option<Principal>, exchange_host::IdentityError> {
                (presented == self.token)
                    .then(|| self.principal.clone())
                    .map(Some)
                    .ok_or(exchange_host::IdentityError::Rejected)
            }
        }

        let scratch = Scratch::new("ambiguous-bearer");
        let store = scratch.store();
        let actor = Principal::new(
            PrincipalKind::User,
            "alice",
            exchange_host::Tenant::new("acme").expect("tenant"),
        );
        let minted = store
            .mint(
                &actor,
                "ci-runner",
                Expiry {
                    expires_at: in_thirty_days(),
                    as_of: session::now(),
                },
            )
            .expect("Service Account");
        let token = minted.token.as_str().to_owned();
        let identity = Arc::new(AlsoResolves {
            token: token.clone(),
            principal: actor,
        });
        let state = AppState::with_identity(identity).with_service_accounts(store);

        let (status, _, body) = call(
            app(state),
            HttpRequest::builder()
                .uri("/api/session")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert_eq!(body["error"], "credential rejected");
        assert!(body.to_string().find(&token).is_none());
    }

    #[tokio::test]
    async fn the_legacy_create_alias_is_visibly_deprecated_and_mints_the_canonical_kind() {
        let scratch = Scratch::new("legacy-alias");
        let (status, headers, body) = call(
            armed(&scratch),
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/api/agents")
                .header(AUTHORIZATION, "Bearer alice")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "id": "legacy-client", "expires_at": in_thirty_days() }).to_string(),
                ))
                .expect("request"),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["principal"]["kind"], "service_account");
        assert_eq!(headers.get("deprecation").unwrap(), "true");
        assert!(headers["link"]
            .to_str()
            .unwrap()
            .contains("/api/service-accounts"));
        assert!(headers["warning"].to_str().unwrap().contains("v0.17"));
    }

    // ---------------------------------------------------------------------------------------
    // The tenant, asserted three times — once per vector a caller controls.
    //
    // Each authenticates as `alice`, armed into tenant `acme`, while claiming tenant `attacker`
    // through one vector. The tenant the Service Account is minted into must be `acme` every time. These are
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
                .service_accounts()
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
        .with_service_accounts(scratch.store());
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
            "the Service Account must be minted into the resolved principal's tenant, not the path segment's",
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
            body["principal"]["kind"], "service_account",
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
                .uri(SERVICE_ACCOUNTS)
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
    /// nothing about what exists — not a tenant, not a Service Account, not that this route mints anything.
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
                .uri(SERVICE_ACCOUNTS)
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
        .with_service_accounts(store.clone()));

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

    /// A composition with no Service Account store refuses and names the setting that would have bound one.
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
                .is_some_and(|reason| reason.contains(SERVICE_ACCOUNT_STORE_SETTING)),
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
