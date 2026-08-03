//! Running one operation.
//!
//! ```text
//! POST /api/operations/{operation}/invoke[?connection=<label>]
//! body: the operation's declared parameters, verbatim — no envelope
//! → 200 { "operation": …, "content": …, "view": …, "is_error": false }
//! → 4xx { "refusal": …, "operation": …, "sent": "no", "retryable": false, "message": … }
//! ```
//!
//! **This module is an adapter and nothing else.** It reads a principal out of the guard's
//! extension, reads an operation id out of the path, hands both to
//! [`exchange_host::Invoker::invoke_for_instance`], and turns the answer into a status. Every decision that
//! matters — the catalogue lookup, the deployment gate, the credential address, the request itself
//! — is made in `exchange-host` and, below that, in `connector_pack`. There is deliberately nothing
//! here to get wrong.
//!
//! # What a caller supplies, and what it cannot
//!
//! `{operation}` is the catalogue's own spelling of an operation id (`zendesk-ticket-show`). The
//! optional `connection` query names an operator label within the principal's tenant; Exchange
//! resolves it to a held host-minted UUID. With one connection it may be omitted. With several it
//! is required, and no default or first match exists. The body remains the parameter object and
//! nothing else.
//!
//! **There is no envelope**, and that is the shape rather than a validation. An envelope is a place
//! to put a field, and the field that eventually gets added is `endpoint`, or `base_url`, or
//! `credential`; `docs/designs/invoke.md` §1 rejects `POST /api/invoke {"operation":…,"params":…}`
//! for exactly that reason. **There is no tenant segment either**, not even an ignored one — an
//! ignored tenant segment is worse than an honoured one, because it reads as authoritative in every
//! log line and client SDK, and the first person who "fixes" the inconsistency by honouring it
//! breaks the north star in a diff that looks like a cleanup. Unknown query axes are denied too: a
//! caller cannot supply a UUID, authority, host or credential address by moving it out of the body.
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
//! # This route is gated by identity **and by grant** (X-13)
//!
//! [`Access::Principal`] is the first half: a caller this host cannot identify cannot run anything.
//! The second half is `exchange_host`'s, and it is not this module's to apply — an operation runs
//! only if one of the caller's tenant's grants admits it, decided from the operation's own declared
//! `risk`, `effects` and `idempotency` rather than from a list of ids. A refusal arrives here as
//! [`InvokeRefusal::NotGranted`] and leaves as `403`.
//!
//! `403` rather than `404`, deliberately. Hiding the existence of an operation a caller may not run
//! would contradict the surface next door: the catalogue is anonymous and publishes every operation
//! in the build, so a `404` here would be a fiction any stranger can disprove — and an agent that
//! cannot tell "you may not" from "there is no such thing" reports the wrong one to whoever has to
//! fix it.
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

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{post, MethodRouter};
use axum::{Extension, Json};
use exchange_host::{InvokeRefusal, Principal, Sent};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

use super::{rate_limited, Access, Module, Route};
use crate::state::AppState;

/// The setting that names the grant store, quoted when no invoker is bound.
///
/// Spelled through the host's own constant, and cfg-gated for the reason
/// [`connections::STORE_SETTING`](super::connections) is: only the *file* binding of
/// `exchange_host::Grants` is `#[cfg(unix)]`, because a planted grant decides what this host will
/// run and the file mode is what keeps that to this process's user. The port is not gated, so a
/// composition on another platform binds its own store and still needs a name to quote.
///
/// `pub(super)` since X-62: [`grants`](super::grants) refuses in the same terms when no store is
/// bound, and one setting quoted from two places would be two strings to keep in step.
#[cfg(unix)]
pub(super) const GRANT_SETTING: &str = exchange_host::GRANT_STORE_SETTING;
/// The same, where the file store does not exist.
#[cfg(not(unix))]
pub(super) const GRANT_SETTING: &str = "FLUX_EXCHANGE_GRANTS";

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvocationQuery {
    connection: Option<String>,
}

/// Run one operation for the caller's tenant.
async fn run(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(operation): Path<String>,
    Query(query): Query<InvocationQuery>,
    Json(params): Json<Value>,
) -> Response {
    if let Some(workflow) = operation
        .strip_prefix("workflow.")
        .and_then(|operation| operation.strip_suffix(".run"))
        .filter(|workflow| !workflow.is_empty() && !workflow.contains('.'))
    {
        if query.connection.is_some() {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "refusal": "invalid_connection_selector",
                    "message": "a stored workflow is not one connector connection; remove `connection`",
                })),
            )
                .into_response();
        }
        let _claim = match state.begin_invocation(&principal) {
            Ok(claim) => claim,
            Err(refusal) => return rate_limited(refusal),
        };
        return super::workflows::invoke_published(state, principal, workflow.to_owned(), params)
            .await;
    }
    let Some(invoker) = state.invoker() else {
        return no_invoker();
    };
    let _claim = match state.begin_invocation(&principal) {
        Ok(claim) => claim,
        Err(refusal) => return rate_limited(refusal),
    };

    let selected =
        match connector_catalog::operation(connector_catalog::OperationKey::id(&operation))
            .and_then(|entry| {
                connector_catalog::provider(connector_catalog::ProviderKey::id(entry.provider))
            }) {
            Some(provider) => match super::connections::invocation_instance(
                &state,
                &principal,
                provider,
                query.connection.as_deref(),
            )
            .await
            {
                Ok(selected) => selected,
                Err(response) => return response,
            },
            None => None,
        };

    let outcome = match selected.as_ref() {
        Some(instance) => {
            invoker
                .invoke_for_instance(&principal, &operation, instance, params)
                .await
        }
        None => invoker.invoke(&principal, &operation, params).await,
    };
    match outcome {
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
        // No grant admits it. `403` rather than `401`: the caller was identified, and presenting a
        // different token is not the remedy — somebody who holds the tenant has to grant it.
        InvokeRefusal::NotGranted { .. } => StatusCode::FORBIDDEN,
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
/// `503` and the settings' names, in the shape [`connections`](super::connections) already uses:
/// this is a host that cannot serve the request, not a request that was wrong.
///
/// An invoker exists exactly when **both** a credential store and a grant store are bound. Without
/// the first, every request would go out unauthenticated; without the second (X-13) there is
/// nowhere for a grant to live, so nothing could admit an operation — and the alternative reading,
/// that an absent grant store admits everything, is the exposure that story closed. Both settings
/// are named because this host does not say which one is missing: that is a fact about the
/// composition, and a caller who is not the operator learns nothing useful from it.
fn no_invoker() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": format!(
                "this host runs no operations: it needs a credential store ({}) to resolve a \
                 credential with and a grant store ({}) to admit an operation from, and it is \
                 missing at least one of them",
                crate::routes::connections::STORE_SETTING,
                GRANT_SETTING,
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
    use crate::traffic::Traffic;

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

    struct TwoGithubInstances {
        references: Vec<exchange_host::CredentialRef>,
    }

    #[exchange_host::async_trait]
    impl exchange_host::SecretStore for TwoGithubInstances {
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

        async fn references(
            &self,
            scope: &exchange_host::CredentialScope,
        ) -> Result<Vec<exchange_host::CredentialRef>, exchange_host::StoreError> {
            Ok(self
                .references
                .iter()
                .filter(|reference| scope.contains(reference))
                .cloned()
                .collect())
        }
    }

    /// A bound grant store holding a fixed set, for [`EmptyStore`]'s reason: a local type rather
    /// than a memory store published from `exchange-host`.
    ///
    /// It takes what to hold rather than admitting everything, because both answers are needed
    /// here — a tenant with a grant and a tenant without one are the two sides of what this route
    /// now decides, and a helper that could only produce one of them would leave the `403` untested.
    struct HeldGrants(Vec<exchange_host::Grant>);

    impl exchange_host::Grants for HeldGrants {
        fn held(&self, _: &exchange_host::Tenant) -> Vec<exchange_host::Grant> {
            self.0.clone()
        }

        fn set(
            &self,
            _: &exchange_host::Tenant,
            _: &[exchange_host::Grant],
        ) -> Result<(), exchange_host::GrantRefusal> {
            unreachable!("no test here edits a grant through the port")
        }
    }

    /// An invoker over a store that holds nothing and the grants a test wants held.
    fn invoker_holding(grants: Vec<exchange_host::Grant>) -> Arc<exchange_host::Invoker> {
        Arc::new(
            crate::execution::invoker(
                exchange_host::Deployment::MultiTenant,
                Arc::new(EmptyStore),
                // No connection settings bound: these tests drive connectors that need none.
                Arc::new(exchange_host::MemoryConfig::new()),
                Arc::new(HeldGrants(grants)),
            )
            .expect("a usable workspace root"),
        )
    }

    fn invoker_over(
        credentials: Arc<dyn exchange_host::SecretStore>,
    ) -> Arc<exchange_host::Invoker> {
        Arc::new(
            crate::execution::invoker(
                exchange_host::Deployment::MultiTenant,
                credentials,
                Arc::new(exchange_host::MemoryConfig::new()),
                Arc::new(HeldGrants(all_of_github())),
            )
            .expect("a usable workspace root"),
        )
    }

    /// Everything github publishes, which is what a test that is not about the grant gate needs.
    fn all_of_github() -> Vec<exchange_host::Grant> {
        vec![exchange_host::Grant::for_connector(
            "github",
            exchange_host::Selector::any(),
        )]
    }

    /// Drive one `POST` through a fully assembled app and report the status and the parsed body.
    async fn post_json(state: AppState, path: &str, body: Value) -> (StatusCode, Value) {
        post_json_with_forwarded_for(state, path, body, None).await
    }

    async fn post_json_with_forwarded_for(
        state: AppState,
        path: &str,
        body: Value,
        forwarded_for: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut service = super::super::app(state).into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let mut request = HttpRequest::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", "triage-bot"))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(forwarded_for) = forwarded_for {
            request = request.header("x-forwarded-for", forwarded_for);
        }
        let request = request
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
        let state = identified().with_invoker(invoker_holding(all_of_github()));

        let (status, body) =
            post_json(state, "/api/operations/no-such-operation/invoke", json!({})).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["refusal"], "unknown_operation");
        assert_eq!(body["sent"], "no");
        assert_eq!(body["retryable"], false);
    }

    /// The invocation rate bound is applied only after identity and an invoker exist, and refuses a
    /// second attempt before operation dispatch.
    #[tokio::test]
    async fn invocations_are_rate_limited_before_dispatch() {
        let state = identified()
            .with_invoker(invoker_holding(all_of_github()))
            .with_traffic(Traffic::for_test(
                1,
                1,
                1,
                std::time::Duration::from_secs(60),
            ));
        let path = "/api/operations/not-in-the-catalogue/invoke";

        assert_eq!(
            post_json(state.clone(), path, json!({})).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            post_json(state, path, json!({})).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn spoofed_forwarding_headers_do_not_select_principal_buckets() {
        let state = identified()
            .with_invoker(invoker_holding(all_of_github()))
            .with_traffic(Traffic::for_test_with_principal(
                1,
                10,
                1,
                2,
                std::time::Duration::from_secs(60),
            ));
        let path = "/api/operations/not-in-the-catalogue/invoke";

        assert_eq!(
            post_json_with_forwarded_for(state.clone(), path, json!({}), Some("192.0.2.1"))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            post_json_with_forwarded_for(state, path, json!({}), Some("198.51.100.9"))
                .await
                .0,
            StatusCode::TOO_MANY_REQUESTS,
            "changing a caller-controlled forwarding header cannot mint another budget"
        );
    }

    #[tokio::test]
    async fn health_remains_responsive_while_invocation_concurrency_is_saturated() {
        let state = identified().with_traffic(Traffic::for_test(
            1,
            10,
            1,
            std::time::Duration::from_secs(60),
        ));
        let principal = Principal::new(
            exchange_host::PrincipalKind::User,
            "holder",
            exchange_host::Tenant::new("acme").expect("tenant"),
        );
        let _held = state.begin_invocation(&principal).expect("hold sole slot");
        let mut service = super::super::app(state).into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("router ready");
        let response = service
            .call(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("router infallible");
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// A tenant this host serves has connected nothing, so the operation refuses **by address** —
    /// terminally, with nothing sent.
    ///
    /// This is the route-level twin of `exchange-host`'s
    /// `a_missing_credential_refuses_by_address_and_is_terminal`, and it is here because the status
    /// and the two fields are this module's decision rather than the host's.
    #[tokio::test]
    async fn a_missing_credential_is_a_422_that_names_the_address_and_is_terminal() {
        // Granted, so what this observes is the credential refusal rather than the grant gate one
        // step earlier — the order is the design's, and this test is about the later step.
        let state = identified().with_invoker(invoker_holding(all_of_github()));

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

    /// **X-13, at the route.** A principal whose tenant holds no grant is refused with `403`, and
    /// the body carries the same two fields every other refusal does.
    ///
    /// `403` rather than `404`: the catalogue is anonymous and publishes this operation to
    /// strangers, so hiding it here would be a fiction the surface next door disproves.
    #[tokio::test]
    async fn an_ungranted_operation_is_a_403_that_says_it_was_never_sent() {
        let state = identified().with_invoker(invoker_holding(Vec::new()));

        let (status, body) = post_json(
            state,
            "/api/operations/github-repo-get/invoke",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["refusal"], "not_granted");
        assert_eq!(body["operation"], "github-repo-get");
        assert_eq!(body["sent"], "no");
        assert_eq!(body["retryable"], false);

        let message = body["message"]
            .as_str()
            .expect("a refusal carries a message");
        assert!(
            message.contains("triage-bot") && message.contains("github-repo-get"),
            "the refusal must name the principal and the operation: {body}",
        );
        assert!(
            !message.contains("com.github.api"),
            "a caller with no grant learns nothing about the connection behind it: {body}",
        );
    }

    /// The gate reads the operation's declared risk, not its name.
    ///
    /// One grant, `risk <= low`, naming no operation: github's read runs as far as the credential
    /// store — a `422` about an address, which is the refusal *after* this gate — and github's
    /// `high`-risk write is refused at it. Two statuses from one grant is what "decided from
    /// declared metadata" looks like from outside.
    #[tokio::test]
    async fn a_read_only_grant_is_read_off_the_catalogue_and_not_off_a_list_of_names() {
        let read_only = || {
            vec![exchange_host::Grant::for_connector(
                "github",
                exchange_host::Selector::at_most(exchange_host::Risk::Low),
            )]
        };

        let (status, body) = post_json(
            identified().with_invoker(invoker_holding(read_only())),
            "/api/operations/github-repo-get/invoke",
            json!({ "owner": "codewandler", "repo": "flux-exchange" }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "the read is admitted and refuses one step later, for want of a credential: {body}",
        );
        assert_eq!(body["refusal"], "refused");

        let (status, body) = post_json(
            identified().with_invoker(invoker_holding(read_only())),
            "/api/operations/github-issue-create/invoke",
            json!({
                "owner": "codewandler",
                "repo": "flux-exchange",
                "title": "no",
                "body": "no",
                "labels": [],
                "assignees": [],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["refusal"], "not_granted");
    }

    /// Unknown query fields are refused by shape. A caller cannot smuggle a host, authority, raw
    /// instance UUID, or credential address alongside the operation's verbatim parameter body.
    #[tokio::test]
    async fn no_query_parameter_can_name_a_host_authority_uuid_or_credential_address() {
        for field in ["host", "authority", "instance", "credential_address"] {
            let path =
                format!("/api/operations/github-repo-get/invoke?{field}=SENTINEL-NOT-AN-ADDRESS");
            let (status, _) = post_json(
                identified().with_invoker(invoker_holding(all_of_github())),
                &path,
                json!({ "owner": "codewandler", "repo": "flux-exchange" }),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
        }
    }

    /// Omitting a label is sole-only. Naming one selects its UUID internally and the eventual
    /// missing-credential refusal names that derived address, proving the raw parameter body was
    /// not wrapped or repurposed for connection metadata.
    #[tokio::test]
    async fn connection_query_selects_a_label_and_omission_is_ambiguous() {
        let first = exchange_host::InstanceId::parse("0d3f79ae-b6df-4f77-8f77-438436c3b2ef")
            .expect("first id");
        let second = exchange_host::InstanceId::parse("3a4bbf6d-5a20-4cdf-bfd7-18f1831fe2fd")
            .expect("second id");
        let reference = |instance: &exchange_host::InstanceId| {
            exchange_host::CredentialRef::for_instance(
                "acme",
                "com.github.api",
                instance.as_str(),
                "default",
                "token",
            )
            .expect("github reference")
        };
        let credentials: Arc<dyn exchange_host::SecretStore> = Arc::new(TwoGithubInstances {
            references: vec![reference(&first), reference(&second)],
        });
        let registry = Arc::new(exchange_host::MemoryConnectionRegistry::default());
        let tenant = exchange_host::Tenant::new("acme").expect("tenant");
        for (label, instance) in [("prod", &first), ("sandbox", &second)] {
            exchange_host::ConnectionRegistry::assign(
                registry.as_ref(),
                &tenant,
                "github",
                &exchange_host::ConnectionLabel::new(label).expect("label"),
                instance,
            )
            .expect("name instance");
        }
        let state = identified()
            .with_credentials(credentials.clone())
            .with_connection_registry(registry)
            .with_invoker(invoker_over(credentials));

        let params = json!({ "owner": "codewandler", "repo": "flux-exchange" });
        let (status, body) = post_json(
            state.clone(),
            "/api/operations/github-repo-get/invoke",
            params.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], "ambiguous_connection");

        let (status, body) = post_json(
            state,
            "/api/operations/github-repo-get/invoke?connection=prod",
            params,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(
            body["message"]
                .as_str()
                .expect("message")
                .contains(first.as_str()),
            "the host-resolved UUID must be the address used: {body}",
        );
    }
}
