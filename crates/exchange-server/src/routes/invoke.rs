//! Running one operation.
//!
//! ```text
//! POST /api/operations/{operation}/invoke
//! body: the operation's declared parameters, verbatim — no envelope
//! → 200 { "operation": …, "content": …, "view": …, "is_error": false }
//! → 4xx { "refusal": …, "operation": …, "sent": "no", "retryable": false, "message": … }
//! ```
//!
//! **This module is an adapter and nothing else.** It reads a principal out of the guard's
//! extension, reads an operation id out of the path, hands both to
//! [`exchange_host::Invoker::invoke`], and turns the answer into a status. Every decision that
//! matters — the catalogue lookup, the deployment gate, the credential address, the request itself
//! — is made in `exchange-host` and, below that, in `connector_pack`. There is deliberately nothing
//! here to get wrong.
//!
//! # What a caller supplies, and what it cannot
//!
//! One noun: `{operation}`, the catalogue's own spelling of an operation id
//! (`zendesk-ticket-show`). The body is the parameter object and nothing else.
//!
//! **There is no envelope**, and that is the shape rather than a validation. An envelope is a place
//! to put a field, and the field that eventually gets added is `endpoint`, or `base_url`, or
//! `credential`; `docs/designs/invoke.md` §1 rejects `POST /api/invoke {"operation":…,"params":…}`
//! for exactly that reason. **There is no tenant segment either**, not even an ignored one — an
//! ignored tenant segment is worse than an honoured one, because it reads as authoritative in every
//! log line and client SDK, and the first person who "fixes" the inconsistency by honouring it
//! breaks the north star in a diff that looks like a cleanup.
//!
//! The tenant comes from [`Extension<Principal>`], which only the guard inserts.
//! `super::tests::no_published_route_takes_a_tenant_in_its_path` walks the whole surface for the
//! path half of that, and this module gives it nothing new to find.
//!
//! # Why the connector is not in the path
//!
//! It is derivable: `catalog::operation` is a global lookup and the entry carries its provider. A
//! redundant name is a name that can *disagree*, which needs a reconciliation rule, and a
//! reconciliation rule is a decision procedure over caller input about which connector to use.
//!
//! # This route is gated by identity alone
//!
//! [`Access::Principal`]: a caller this host cannot identify cannot run anything. **It is not gated
//! by grant** — that is X-13, and `docs/designs/invoke.md` §6 records it as *a stated gap rather
//! than a position*. There is deliberately no interim scheme here, because a half-grant is worse
//! than a stated gap: the next story would have to unpick it.
//!
//! # `sent` and `retryable` are fields, not inferences
//!
//! Two questions matter to an agent and the HTTP status space expresses neither: *was the request
//! sent?* and *will retrying help?* A `502` says nothing about whether the effect happened, so both
//! answers are in the body. See [`exchange_host::Sent`] and
//! [`exchange_host::InvokeRefusal::retryable`] for how each is decided.
//!
//! # A vendor's `4xx` is an answer, not a failure
//!
//! It comes back as `200` with the vendor's own response in `content`, unshaped. Flattening a
//! Zendesk `404` into a host error would destroy the distinction between "the vendor said no" and
//! "we could not ask", which is the distinction this whole surface exists to keep.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{post, MethodRouter};
use axum::{Extension, Json};
use exchange_host::{InvokeRefusal, Principal, Sent};
use serde_json::{json, Value};
use tracing::warn;

use super::{Access, Module, Route};
use crate::state::AppState;

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "invoke",
    routes: &[Route {
        // `{operation}` is a catalogue key, never an address and never a destination. It selects
        // *what* runs; the tenant, the host and the credential are all derived from things the
        // caller did not supply.
        path: "/api/operations/{operation}/invoke",
        access: Access::Principal,
        method_router: invoke_route,
    }],
};

fn invoke_route() -> MethodRouter<AppState> {
    post(run)
}

/// Run one operation for the caller's tenant.
async fn run(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(operation): Path<String>,
    Json(params): Json<Value>,
) -> Response {
    let Some(invoker) = state.invoker() else {
        return no_invoker();
    };

    match invoker.invoke(&principal, &operation, params).await {
        Ok(invocation) => (StatusCode::OK, Json(invocation)).into_response(),
        Err(refusal) => {
            // To the log at `warn` when this host could not be sure the request stayed home. That
            // is the one class an operator has to be able to find afterwards, because it is the one
            // where an effect may exist that nobody has a record of.
            if refusal.sent() == Sent::Maybe {
                warn!(%principal, operation, "an invocation may have reached the vendor and failed");
            }
            refuse(refusal)
        }
    }
}

/// Render a refusal: a status, a stable label, and the two facts a caller cannot derive.
///
/// The message is upstream's own and is already redacted through the invocation's own redactor —
/// it names the **address** an operator has to go and put a value at, and never the value, and
/// never its length, because a length is a fingerprint.
///
/// It is not split into a separate `address` field. The design's sketch has one; producing it would
/// mean parsing a string this host did not compose, and a field that is right four times in five is
/// worse than a message that is always whole. When `connector-pack` publishes the address as data,
/// this is where it lands.
///
/// # `supply_at` (X-47), and why it is a route and not a parsed field
///
/// A `refused` message names the `binds` target that is missing — `endpoint.subdomain` — and until
/// X-47 there was nowhere on this surface to put one, so an operator who read the refusal correctly
/// still had nothing to do about it. `supply_at` is that missing half: the **route** that takes
/// per-connection values for the operation's connector.
///
/// It is derived from the operation the caller named and from nothing in the message, for the
/// reason stated above about the address — this host does not parse a string it did not compose.
/// So it points at the connector's settings **collection**, which answers with every field that
/// connector needs and which of them this tenant has supplied; picking out the one field would mean
/// reading it back out of upstream's prose.
fn refuse(refusal: InvokeRefusal) -> Response {
    let status = match refusal {
        // Nothing in the catalogue spells it.
        InvokeRefusal::UnknownOperation { .. } => StatusCode::NOT_FOUND,
        // This deployment will not serve that runtime, ever, for anyone. `409` rather than `403`:
        // it is not about who is asking.
        InvokeRefusal::Runtime(_) => StatusCode::CONFLICT,
        // The request could not be composed or authenticated as declared. The body was
        // well-formed; what it asks for cannot be built from what this tenant has connected.
        InvokeRefusal::Refused { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        // The transport failed. This host does not know whether the vendor received it.
        InvokeRefusal::Transport { .. } => StatusCode::BAD_GATEWAY,
    };

    let body = json!({
        "refusal": refusal.label(),
        "operation": refusal.operation(),
        "sent": refusal.sent(),
        "retryable": refusal.retryable(),
        "message": refusal.to_string(),
        "supply_at": supply_at(&refusal),
    });

    (status, Json(body)).into_response()
}

/// Where a tenant supplies what a `refused` invocation says is missing.
///
/// Only for [`InvokeRefusal::Refused`], which is the one class whose remedy is a value this tenant
/// has not supplied. A `transport` failure has nothing to supply, an unknown operation has no
/// connector, and a runtime refusal is a property of the deployment that no value changes — each of
/// those answers `null` rather than pointing somewhere that would not help.
///
/// The connector is looked up from the operation the caller named, so this route is derived from
/// the catalogue rather than read out of upstream's message. An operation the catalogue does not
/// spell cannot reach here — `UnknownOperation` is a different variant — but the lookup is written
/// to answer `None` rather than to assume, because a refusal is the wrong place to panic.
fn supply_at(refusal: &InvokeRefusal) -> Option<String> {
    let InvokeRefusal::Refused { operation, .. } = refusal else {
        return None;
    };

    let entry = connector_catalog::operation(connector_catalog::OperationKey::id(operation))?;
    Some(format!("/api/connections/{}/settings", entry.provider))
}

/// No invoker is bound, so nothing can run.
///
/// `503` and the setting's name, in the shape [`connections`](super::connections) already uses:
/// this is a host that cannot serve the request, not a request that was wrong. An invoker exists
/// exactly when a credential store is bound, because an operation without one would send every
/// request unauthenticated.
fn no_invoker() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": format!(
                "this host runs no operations: no credential store is bound ({}), so no \
                 credential could be resolved and nothing would be sent authenticated",
                crate::routes::connections::STORE_SETTING,
            ),
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{header, Request as HttpRequest};
    use tower::Service;

    use crate::dev_identity::DevIdentity;

    /// The development roster this module's tests sign in through.
    const ROSTER: &str = "agent:triage-bot@acme";

    /// A bound credential store that holds nothing.
    ///
    /// Deliberately a local type rather than a memory store re-exported from `exchange-host`: this
    /// host's `AGENTS.md` refuses a store that falls back to memory, and publishing one through the
    /// public crate would make that fallback one line away. What these tests need is a store that
    /// answers `NotFound`, which is exactly what a connector nobody has connected looks like.
    struct EmptyStore;

    #[exchange_host::async_trait]
    impl exchange_host::SecretStore for EmptyStore {
        async fn get(
            &self,
            reference: &exchange_host::CredentialRef,
        ) -> Result<exchange_host::Secret, exchange_host::StoreError> {
            Err(exchange_host::StoreError::NotFound {
                path: exchange_host::address_path(reference),
            })
        }

        async fn put(
            &self,
            _: &exchange_host::CredentialRef,
            _: &exchange_host::Secret,
        ) -> Result<(), exchange_host::StoreError> {
            unreachable!("no test here writes a credential")
        }

        async fn delete(
            &self,
            _: &exchange_host::CredentialRef,
        ) -> Result<(), exchange_host::StoreError> {
            unreachable!("no test here destroys a credential")
        }
    }

    /// Drive one `POST` through a fully assembled app and report the status and the parsed body.
    async fn post_json(state: AppState, path: &str, body: Value) -> (StatusCode, Value) {
        let mut service = super::super::app(state).into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", "triage-bot"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a well-formed request");

        let response = service.call(request).await.expect("a router is infallible");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("a readable body");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A composition with the development identity armed and nothing else bound.
    fn identified() -> AppState {
        AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
    }

    /// A caller this host cannot identify runs nothing, and is told nothing about what exists.
    #[tokio::test]
    async fn an_anonymous_caller_cannot_invoke() {
        let mut service = super::super::app(AppState::without_identity()).into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method("POST")
            .uri("/api/operations/github-repo-get/invoke")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .expect("a well-formed request");

        let status = service
            .call(request)
            .await
            .expect("a router is infallible")
            .status();

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// A composition that bound no credential store refuses rather than running unauthenticated,
    /// and names the setting an operator has to set.
    #[tokio::test]
    async fn a_host_with_no_credential_store_refuses_and_names_the_setting() {
        let (status, body) = post_json(
            identified(),
            "/api/operations/github-repo-get/invoke",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            body["error"]
                .as_str()
                .expect("a refusal carries a reason")
                .contains(crate::routes::connections::STORE_SETTING),
            "{body}",
        );
    }

    /// An id the catalogue does not spell is a `404` — and the refusal carries the two fields a
    /// caller cannot derive from the status.
    #[tokio::test]
    async fn an_unknown_operation_is_a_404_that_says_it_was_never_sent() {
        let state = identified().with_invoker(Arc::new(
            crate::execution::invoker(
                Arc::new(EmptyStore),
                // No connection settings bound: these tests drive connectors that need none.
                Arc::new(exchange_host::MemoryConfig::new()),
            )
            .expect("a usable workspace root"),
        ));

        let (status, body) =
            post_json(state, "/api/operations/no-such-operation/invoke", json!({})).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["refusal"], "unknown_operation");
        assert_eq!(body["sent"], "no");
        assert_eq!(body["retryable"], false);
    }

    /// A tenant this host serves has connected nothing, so the operation refuses **by address** —
    /// terminally, with nothing sent.
    ///
    /// This is the route-level twin of `exchange-host`'s
    /// `a_missing_credential_refuses_by_address_and_is_terminal`, and it is here because the status
    /// and the two fields are this module's decision rather than the host's.
    #[tokio::test]
    async fn a_missing_credential_is_a_422_that_names_the_address_and_is_terminal() {
        let state = identified().with_invoker(Arc::new(
            crate::execution::invoker(
                Arc::new(EmptyStore),
                // No connection settings bound: these tests drive connectors that need none.
                Arc::new(exchange_host::MemoryConfig::new()),
            )
            .expect("a usable workspace root"),
        ));

        let (status, body) = post_json(
            state,
            "/api/operations/github-repo-get/invoke",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["refusal"], "refused");
        assert_eq!(body["sent"], "no");
        assert_eq!(body["retryable"], false);
        assert!(
            body["message"]
                .as_str()
                .expect("a refusal carries a message")
                .contains("tenants/acme/com.github.api/token"),
            "the refusal must name the address an operator has to go and put a value at: {body}",
        );
    }
}
