//! X-113's consumer contract, driven through the assembled HTTP application.
//!
//! These tests deliberately authenticate with a token minted by the canonical Service Account
//! store rather than the development roster. They are the executable fixture Flux C-503 consumes:
//! discover at a turn boundary, retain the generation while content is equal, bind the returned
//! connection label, and invoke without ever receiving the credential behind it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use exchange_host::{
    address_path, ConnectionLabel, ConnectionRegistry, Contexts, CredentialRef, CredentialScope,
    Deployment, Egress, Grant, GrantRefusal, Grants, InstanceId, Invoker, MemoryConfig,
    MemoryConnectionRegistry, Principal, PrincipalKind, Risk, Secret, SecretStore, Selector,
    StoreError, Tenant, ToolContext,
};
use flux_runtime::Tool;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::service_account::{Expiry, ServiceAccountStore};
use crate::state::AppState;

const SENTINEL: &str = "quiggle-marrow-plimth-42";
const INSTANCE: &str = "0d3f79ae-b6df-4f77-8f77-438436c3b2ef";

/// A mutable credential port whose inventory and values change together.
#[derive(Default)]
struct Credentials(Mutex<BTreeMap<CredentialRef, String>>);

impl Credentials {
    fn hold(&self, reference: CredentialRef, value: &str) {
        self.0
            .lock()
            .expect("no test poisons the credential store")
            .insert(reference, value.to_owned());
    }

    fn clear(&self) {
        self.0
            .lock()
            .expect("no test poisons the credential store")
            .clear();
    }
}

#[exchange_host::async_trait]
impl SecretStore for Credentials {
    async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
        self.0
            .lock()
            .expect("no test poisons the credential store")
            .get(reference)
            .map(Secret::new)
            .ok_or_else(|| StoreError::NotFound {
                path: address_path(reference),
            })
    }

    async fn put(&self, _: &CredentialRef, _: &Secret) -> Result<(), StoreError> {
        unreachable!("the protocol fixture seeds credentials without exercising management")
    }

    async fn delete(&self, _: &CredentialRef) -> Result<(), StoreError> {
        unreachable!("the protocol fixture changes inventory through its private seam")
    }

    async fn references(&self, scope: &CredentialScope) -> Result<Vec<CredentialRef>, StoreError> {
        Ok(self
            .0
            .lock()
            .expect("no test poisons the credential store")
            .keys()
            .filter(|reference| scope.contains(reference))
            .cloned()
            .collect())
    }
}

/// Mutable because changing the held grant must change discovery without rebuilding the app.
struct HeldGrants(Mutex<Vec<Grant>>);

impl HeldGrants {
    fn new(grants: Vec<Grant>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(grants)))
    }
}

impl Grants for HeldGrants {
    fn held(&self, _: &Tenant) -> Vec<Grant> {
        self.0
            .lock()
            .expect("no test poisons the grant store")
            .clone()
    }

    fn set(&self, _: &Tenant, grants: &[Grant]) -> Result<(), GrantRefusal> {
        *self.0.lock().expect("no test poisons the grant store") = grants.to_vec();
        Ok(())
    }
}

/// A scratch Service Account verifier store, retained for the fixture's lifetime.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-effective-catalogue-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    fn store(&self) -> Arc<ServiceAccountStore> {
        Arc::new(
            ServiceAccountStore::open(self.0.join("state/service_accounts.json"))
                .expect("a fresh Service Account store"),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Wire(Mutex<Vec<Value>>);

fn recording_egress(wire: Arc<Wire>) -> Egress {
    let spec = flux_web::http::HttpRequestTool::new(&flux_web::WebOptions::default()).spec();
    let tool = flux_runtime::tool_fn(spec, move |params: Value| {
        let wire = wire.clone();
        async move {
            wire.0
                .lock()
                .expect("no test poisons the wire")
                .push(params);
            Ok(json!({ "status": 200, "headers": {}, "body": {"ok": true} }))
        }
    });
    Egress::new(tool)
}

fn contexts() -> Arc<dyn Contexts> {
    Arc::new(|| {
        let workspace = flux_system::Workspace::new(std::env::temp_dir())
            .expect("the temp directory is a workspace");
        ToolContext::new(Arc::new(flux_system::System::new(workspace)))
    })
}

struct Fixture {
    _scratch: Scratch,
    state: AppState,
    token: String,
    reference: CredentialRef,
    credentials: Arc<Credentials>,
    grants: Arc<HeldGrants>,
    wire: Arc<Wire>,
}

impl Fixture {
    fn new(selector: Selector) -> Self {
        let credentials = Arc::new(Credentials::default());
        let reference = CredentialRef::new("acme", "com.github.api", "default", "token")
            .expect("github's credential address");
        credentials.hold(reference.clone(), SENTINEL);

        let registry = Arc::new(MemoryConnectionRegistry::default());
        let tenant = Tenant::new("acme").expect("a literal tenant");
        ConnectionRegistry::assign(
            registry.as_ref(),
            &tenant,
            "github",
            &ConnectionLabel::new("prod").expect("a label"),
            &InstanceId::parse(INSTANCE).expect("an instance UUID"),
        )
        .expect("name the sole legacy connection");

        let grants = HeldGrants::new(vec![Grant::for_connector("github", selector)]);
        let wire = Arc::new(Wire::default());
        let invoker = Arc::new(Invoker::new(
            Deployment::MultiTenant,
            recording_egress(wire.clone()),
            credentials.clone(),
            Arc::new(MemoryConfig::new()),
            grants.clone(),
            contexts(),
        ));

        let scratch = Scratch::new();
        let service_accounts = scratch.store();
        let now = crate::session::now();
        let minted = service_accounts
            .mint(
                &Principal::new(PrincipalKind::User, "alice", tenant),
                "flux",
                Expiry {
                    expires_at: now + 60 * 60,
                    as_of: now,
                },
            )
            .expect("a canonical Service Account token");
        let token = minted.token.as_str().to_owned();

        let state = AppState::without_identity()
            .with_service_accounts(service_accounts)
            .with_credentials(credentials.clone())
            .with_connection_registry(registry)
            .with_invoker(invoker);

        Self {
            _scratch: scratch,
            state,
            token,
            reference,
            credentials,
            grants,
            wire,
        }
    }

    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        call(self.state.clone(), Some(&self.token), method, uri, body).await
    }

    async fn catalogue(&self) -> Value {
        let (status, body) = self
            .call(Method::GET, "/api/catalogue/effective", None)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

async fn call(
    state: AppState,
    token: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = crate::routes::app(state)
        .oneshot(request.body(body).expect("a well-formed request"))
        .await
        .expect("a router is infallible");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("a bounded response body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// Authentication, exact filtering, and the absence of caller-selected authority in one wire
/// sample. This is the response shape C-503 can deserialize without touching an operator route.
#[tokio::test]
async fn canonical_service_account_discovers_only_connected_and_granted_bindings() {
    let fixture = Fixture::new(Selector::at_most(Risk::Low));
    let catalogue = fixture.catalogue().await;

    assert!(catalogue["generation"]
        .as_str()
        .is_some_and(|generation| generation.starts_with("sha256:")));
    let operations = catalogue["operations"]
        .as_array()
        .expect("an operation array");
    assert!(!operations.is_empty());
    assert!(operations
        .iter()
        .any(|operation| operation["id"] == "github-repo-get"));
    assert!(!operations
        .iter()
        .any(|operation| operation["id"] == "github-issue-create"));
    assert!(operations
        .iter()
        .all(|operation| { operation["admitted"] == true && operation["connection"] == "prod" }));

    let encoded = catalogue.to_string();
    for forbidden in [
        SENTINEL,
        "\"tenant\"",
        "\"authority\"",
        "\"endpoint\"",
        INSTANCE,
    ] {
        assert!(
            !encoded.contains(forbidden),
            "leaked `{forbidden}`: {encoded}"
        );
    }

    let (status, _) = call(
        fixture.state.clone(),
        None,
        Method::GET,
        "/api/catalogue/effective",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for axis in ["tenant", "credential", "endpoint", "runtime", "instance"] {
        let (status, _) = fixture
            .call(
                Method::GET,
                &format!("/api/catalogue/effective?{axis}=attacker"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "query axis `{axis}`");
    }
}

/// Content identity changes for effective grants and connections, and returns to its earlier value
/// when the exact projection returns. No process-local counter is involved.
#[tokio::test]
async fn generation_is_stable_and_content_addressed_across_grant_and_connection_changes() {
    let fixture = Fixture::new(Selector::at_most(Risk::Low));
    let read_only = fixture.catalogue().await;
    assert_eq!(
        fixture.catalogue().await["generation"],
        read_only["generation"]
    );

    fixture
        .grants
        .set(
            &Tenant::new("acme").expect("tenant"),
            &[Grant::for_connector("github", Selector::any())],
        )
        .expect("change a held grant");
    let all = fixture.catalogue().await;
    assert_ne!(all["generation"], read_only["generation"]);
    assert!(all["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .any(|operation| operation["id"] == "github-issue-create"));
    assert_eq!(fixture.catalogue().await["generation"], all["generation"]);

    fixture.credentials.clear();
    let disconnected = fixture.catalogue().await;
    assert_ne!(disconnected["generation"], all["generation"]);
    assert_eq!(disconnected["operations"], json!([]));

    fixture
        .credentials
        .hold(fixture.reference.clone(), SENTINEL);
    assert_eq!(fixture.catalogue().await["generation"], all["generation"]);
}

/// Both read and explicitly granted write go through the pre-existing invoke path with only the
/// returned label and operation arguments. The Service Account never receives the held token.
#[tokio::test]
async fn discovered_read_and_approved_write_invoke_over_the_existing_http_contract() {
    let fixture = Fixture::new(Selector::any());
    let catalogue = fixture.catalogue().await;
    let ids: Vec<&str> = catalogue["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .filter_map(|operation| operation["id"].as_str())
        .collect();
    assert!(ids.contains(&"github-repo-get"));
    assert!(ids.contains(&"github-issue-create"));

    let (status, read) = fixture
        .call(
            Method::POST,
            "/api/operations/github-repo-get/invoke?connection=prod",
            Some(json!({ "owner": "codewandler", "repo": "flux-exchange" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{read}");

    let (status, write) = fixture
        .call(
            Method::POST,
            "/api/operations/github-issue-create/invoke?connection=prod",
            Some(json!({
                "owner": "codewandler",
                "repo": "flux-exchange",
                "title": "remote protocol contract",
                "body": "approved",
                "labels": [],
                "assignees": [],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{write}");
    assert_eq!(
        fixture
            .wire
            .0
            .lock()
            .expect("no test poisons the wire")
            .len(),
        2
    );
    assert!(!read.to_string().contains(SENTINEL));
    assert!(!write.to_string().contains(SENTINEL));
}

/// The provider-owned request inventory drives the production router, identity guard, connection
/// resolver and connector pack. It is deliberately data rather than a second list in this test.
#[tokio::test]
async fn exchange_http_v1_request_outcomes_match_the_checked_provider_fixture() {
    #[derive(serde::Deserialize)]
    struct RequestFixtures {
        cases: Vec<RequestCase>,
    }

    #[derive(serde::Deserialize)]
    struct RequestCase {
        id: String,
        method: String,
        path: String,
        authenticated: bool,
        body: Option<Value>,
        expected_status: u16,
        expected_wire_delta: usize,
    }

    let fixture = Fixture::new(Selector::any());
    let cases: RequestFixtures = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/exchange-http-v1/requests.json"
    )))
    .expect("typed request fixture");

    for case in cases.cases {
        let method = match case.method.as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            other => panic!("fixture `{}` has unsupported method `{other}`", case.id),
        };
        let before = fixture
            .wire
            .0
            .lock()
            .expect("no test poisons the wire")
            .len();
        let token = case.authenticated.then_some(fixture.token.as_str());
        let (status, response) =
            call(fixture.state.clone(), token, method, &case.path, case.body).await;
        assert_eq!(
            status.as_u16(),
            case.expected_status,
            "{}: {response}",
            case.id
        );
        if status == StatusCode::OK && case.id == "effective-authenticated" {
            crate::protocol::decode_effective_catalogue(response.to_string().as_bytes())
                .expect("the production response satisfies the provider decoder");
        }
        if status == StatusCode::OK && case.id.starts_with("invoke-") {
            crate::protocol::decode_invoke_response(
                status.as_u16(),
                response.to_string().as_bytes(),
            )
            .expect("the production response satisfies the provider decoder");
        }
        let after = fixture
            .wire
            .0
            .lock()
            .expect("no test poisons the wire")
            .len();
        assert_eq!(after - before, case.expected_wire_delta, "{}", case.id);
        let response = response.to_string();
        for forbidden in [SENTINEL, "attacker.example", INSTANCE] {
            assert!(
                !response.contains(forbidden),
                "{} leaked `{forbidden}`",
                case.id
            );
        }
    }
}

/// Malformed authority axes and the principal/refusal classes a Milestone 1 client must preserve
/// all fail closed as bounded JSON responses.
#[tokio::test]
async fn malformed_unauthorized_unknown_disconnected_and_not_granted_are_distinct() {
    let fixture = Fixture::new(Selector::any());

    let (unknown_status, unknown) = fixture
        .call(
            Method::POST,
            "/api/operations/no-such-operation/invoke",
            Some(json!({})),
        )
        .await;
    assert_eq!(unknown_status, StatusCode::NOT_FOUND);
    assert_eq!(unknown["refusal"], "unknown_operation");

    let (refused_status, refused) = fixture
        .call(
            Method::POST,
            "/api/operations/github-repo-get/invoke?connection=prod",
            Some(json!({})),
        )
        .await;
    assert_eq!(refused_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["refusal"], "refused");

    fixture
        .grants
        .set(&Tenant::new("acme").expect("tenant"), &[])
        .expect("remove grants");
    let (grant_status, not_granted) = fixture
        .call(
            Method::POST,
            "/api/operations/github-repo-get/invoke?connection=prod",
            Some(json!({ "owner": "codewandler", "repo": "flux-exchange" })),
        )
        .await;
    assert_eq!(grant_status, StatusCode::FORBIDDEN);
    assert_eq!(not_granted["refusal"], "not_granted");

    fixture
        .grants
        .set(
            &Tenant::new("acme").expect("tenant"),
            &[Grant::for_connector("github", Selector::any())],
        )
        .expect("restore the grant so connection state is the deciding refusal");
    fixture.credentials.clear();
    let (disconnected_status, disconnected) = fixture
        .call(
            Method::POST,
            "/api/operations/github-repo-get/invoke?connection=prod",
            Some(json!({ "owner": "codewandler", "repo": "flux-exchange" })),
        )
        .await;
    assert_eq!(disconnected_status, StatusCode::CONFLICT);
    assert_eq!(disconnected["code"], "disconnected");

    let (malformed_status, _) = fixture
        .call(
            Method::POST,
            "/api/operations/github-repo-get/invoke?authority=attacker",
            Some(json!({ "owner": "codewandler", "repo": "flux-exchange" })),
        )
        .await;
    assert_eq!(malformed_status, StatusCode::BAD_REQUEST);

    let (unauthorized_status, _) = call(
        fixture.state.clone(),
        None,
        Method::POST,
        "/api/operations/github-repo-get/invoke?connection=prod",
        Some(json!({ "owner": "codewandler", "repo": "flux-exchange" })),
    )
    .await;
    assert_eq!(unauthorized_status, StatusCode::UNAUTHORIZED);

    for body in [unknown, refused, not_granted, disconnected] {
        assert!(body.to_string().len() < 64 * 1024);
    }
}

/// A declaration change changes the same content generation even when the effective binding set
/// and connection selector are otherwise equal.
#[test]
fn generation_covers_connector_declarations() {
    let operation =
        connector_catalog::operation(connector_catalog::OperationKey::id("github-repo-get"))
            .expect("the catalogue carries github-repo-get");
    let first = super::view::effective_operation(operation, Some("prod".to_owned()))
        .expect("a projected declaration");
    let mut changed = first.clone();
    changed.operation.description.push_str(" changed");

    let first = super::view::EffectiveCatalogue::new(vec![first]).expect("generation");
    let changed = super::view::EffectiveCatalogue::new(vec![changed]).expect("generation");
    assert_ne!(first.generation, changed.generation);
}

/// Connection configuration is part of effective usability even though no setting value appears
/// in the catalogue. A configured sibling service must not make this operation look configured.
#[test]
fn effective_discovery_requires_the_operations_own_non_secret_settings() {
    let provider = connector_catalog::provider(connector_catalog::ProviderKey::id("zendesk"))
        .expect("the catalogue carries zendesk");
    let operation =
        connector_catalog::operation(connector_catalog::OperationKey::id("zendesk-ticket-show"))
            .expect("the catalogue carries zendesk-ticket-show");
    let principal = Principal::new(
        PrincipalKind::ServiceAccount,
        "flux",
        Tenant::new("acme").expect("tenant"),
    );
    let connection = super::super::connections::EffectiveConnection {
        label: Some("prod".to_owned()),
        held_credentials: vec!["zendesk.api_token".to_owned()],
        instance: None,
    };
    let invoker = |settings: MemoryConfig| {
        Invoker::new(
            Deployment::MultiTenant,
            recording_egress(Arc::new(Wire::default())),
            Arc::new(Credentials::default()),
            Arc::new(settings),
            HeldGrants::new(vec![Grant::for_connector("zendesk", Selector::any())]),
            contexts(),
        )
    };

    assert!(!super::operation_is_configured(
        &invoker(MemoryConfig::new()),
        &principal,
        provider,
        operation,
        &connection,
    ));
    let configured = MemoryConfig::new()
        .with_endpoint("acme", "zendesk", "default", "subdomain", "acme")
        .with_username(
            "acme",
            "zendesk",
            "default",
            "zendesk.api_token",
            "agent@acme.example",
        );
    assert!(super::operation_is_configured(
        &invoker(configured),
        &principal,
        provider,
        operation,
        &connection,
    ));
}
