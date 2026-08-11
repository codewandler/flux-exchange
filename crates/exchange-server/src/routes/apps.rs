//! Tenant-installed Flux App management, chat and Event Delivery routes.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, MethodRouter};
use axum::{Extension, Json};
use exchange_host::{
    AppRefusal, AvailableConnection, ConnectionLabel, Datasource, InstallRequest, ModelProfile,
    Principal,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{Access, Module, Route};
use crate::managed_apps::ManagedAppRefusal;
use crate::state::AppState;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

pub(super) const MODULE: Module = Module {
    name: "apps",
    routes: &[
        Route {
            path: "/api/app-packages",
            access: Access::Operator,
            method_router: packages_route,
        },
        Route {
            path: "/api/model-profiles",
            access: Access::Operator,
            method_router: profiles_route,
        },
        Route {
            path: "/api/datasources",
            access: Access::Operator,
            method_router: datasources_route,
        },
        Route {
            path: "/api/apps",
            access: Access::Operator,
            method_router: apps_route,
        },
        Route {
            path: "/api/apps/{app}",
            access: Access::Operator,
            method_router: app_route,
        },
        Route {
            path: "/api/apps/{app}/chat",
            access: Access::Principal,
            method_router: chat_route,
        },
        Route {
            path: "/api/apps/{app}/events/{event}",
            access: Access::Operator,
            method_router: event_route,
        },
        Route {
            path: "/api/apps/{app}/activity",
            access: Access::Operator,
            method_router: activity_route,
        },
        Route {
            path: "/api/apps/{app}/sessions",
            access: Access::Operator,
            method_router: sessions_route,
        },
        Route {
            path: "/api/app-deliveries/{delivery}/retry",
            access: Access::Operator,
            method_router: retry_route,
        },
    ],
};

fn packages_route() -> MethodRouter<AppState> {
    get(packages)
}

fn profiles_route() -> MethodRouter<AppState> {
    get(profiles).post(save_profile)
}

fn datasources_route() -> MethodRouter<AppState> {
    get(datasources).post(save_datasource)
}

fn apps_route() -> MethodRouter<AppState> {
    get(apps).post(install)
}

fn app_route() -> MethodRouter<AppState> {
    get(app)
}

fn chat_route() -> MethodRouter<AppState> {
    post(chat)
}

fn event_route() -> MethodRouter<AppState> {
    post(deliver_event)
}

fn activity_route() -> MethodRouter<AppState> {
    get(activity)
}

fn sessions_route() -> MethodRouter<AppState> {
    get(sessions)
}

fn retry_route() -> MethodRouter<AppState> {
    post(retry)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventRequest {
    #[serde(default = "empty_object")]
    payload: Value,
    #[serde(default)]
    session: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryRequest {
    #[serde(default)]
    session: Option<String>,
}

fn empty_object() -> Value {
    json!({})
}

async fn packages(State(state): State<AppState>) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    (
        StatusCode::OK,
        Json(json!({ "packages": apps.store().packages() })),
    )
        .into_response()
}

async fn profiles(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps.store().model_profiles(principal.tenant()) {
        Ok(profiles) => (StatusCode::OK, Json(json!({ "profiles": profiles }))).into_response(),
        Err(error) => refusal(error),
    }
}

async fn save_profile(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(profile): Json<ModelProfile>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps
        .store()
        .put_model_profile(principal.tenant(), profile.clone())
    {
        Ok(()) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn datasources(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps.store().datasources(principal.tenant()) {
        Ok(datasources) => {
            (StatusCode::OK, Json(json!({ "datasources": datasources }))).into_response()
        }
        Err(error) => refusal(error),
    }
}

async fn save_datasource(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(datasource): Json<Datasource>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps
        .store()
        .put_datasource(principal.tenant(), datasource.clone())
    {
        Ok(()) => (StatusCode::CREATED, Json(datasource)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn apps(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps.store().list(principal.tenant()) {
        Ok(installed) => (StatusCode::OK, Json(json!({ "apps": installed }))).into_response(),
        Err(error) => refusal(error),
    }
}

async fn app(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(app): Path<String>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps.store().get(principal.tenant(), &app) {
        Ok(installed) => (StatusCode::OK, Json(installed)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn install(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(request): Json<InstallRequest>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    let package = match apps.store().package(&request.package, &request.version) {
        Ok(package) => package,
        Err(error) => return refusal(error),
    };
    let mut available = Vec::new();
    for requirement in &package.requirements.connections {
        let Some(label) = request.connections.get(&requirement.name) else {
            return refusal(AppRefusal::MissingConnection {
                slot: requirement.name.clone(),
                connector: requirement.connector.clone(),
            });
        };
        let Some(provider) =
            connector_catalog::provider(connector_catalog::ProviderKey::id(&requirement.connector))
        else {
            return refusal(AppRefusal::MissingConnection {
                slot: requirement.name.clone(),
                connector: requirement.connector.clone(),
            });
        };
        let instance =
            match super::connections::channel_instance(&state, &principal, provider, Some(label))
                .await
            {
                Ok(instance) => instance,
                Err(response) => return response,
            };
        let label = match ConnectionLabel::new(label.clone()) {
            Ok(label) => label,
            Err(error) => return refusal(AppRefusal::Unavailable(error.to_string())),
        };
        available.push(AvailableConnection::new(
            requirement.connector.clone(),
            label,
            instance,
        ));
    }
    match apps
        .store()
        .install(principal.tenant(), request, &available)
    {
        Ok(installed) => (StatusCode::CREATED, Json(installed)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn chat(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(app): Path<String>,
    Json(request): Json<ChatRequest>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    let session = request.session.unwrap_or_else(new_session);
    match apps
        .deliver(
            principal.tenant(),
            &app,
            "chat",
            json!({ "text": request.message, "conversation": session }),
            &session,
        )
        .await
    {
        Ok(reply) => (StatusCode::OK, Json(reply)).into_response(),
        Err(error) => runtime_refusal(error),
    }
}

async fn deliver_event(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((app, event)): Path<(String, String)>,
    Json(request): Json<EventRequest>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    let session = request.session.unwrap_or_else(new_session);
    match apps
        .deliver(principal.tenant(), &app, &event, request.payload, &session)
        .await
    {
        Ok(reply) => (StatusCode::OK, Json(reply)).into_response(),
        Err(error) => runtime_refusal(error),
    }
}

async fn activity(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(app): Path<String>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps.activity(principal.tenant(), &app) {
        Ok(activity) => (StatusCode::OK, Json(json!({ "activity": activity }))).into_response(),
        Err(error) => runtime_refusal(error),
    }
}

async fn sessions(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(app): Path<String>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    match apps.sessions(principal.tenant(), &app) {
        Ok(sessions) => (StatusCode::OK, Json(json!({ "sessions": sessions }))).into_response(),
        Err(error) => runtime_refusal(error),
    }
}

async fn retry(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(delivery): Path<String>,
    Json(request): Json<RetryRequest>,
) -> Response {
    let Some(apps) = state.apps() else {
        return unavailable();
    };
    let session = request.session.unwrap_or_else(new_session);
    match apps.retry(principal.tenant(), &delivery, &session).await {
        Ok(reply) => (StatusCode::OK, Json(reply)).into_response(),
        Err(error) => runtime_refusal(error),
    }
}

fn new_session() -> String {
    format!(
        "managed-session-{}",
        NEXT_SESSION.fetch_add(1, Ordering::Relaxed)
    )
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": format!(
                "this host has no installed App store; configure {}",
                exchange_host::APP_STORE_SETTING,
            ),
        })),
    )
        .into_response()
}

fn runtime_refusal(error: ManagedAppRefusal) -> Response {
    match error {
        ManagedAppRefusal::Store(error) => refusal(error),
        ManagedAppRefusal::Provider(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": error.to_string(), "code": "model_provider_unavailable" })),
        )
            .into_response(),
        ManagedAppRefusal::Execution(_)
        | ManagedAppRefusal::Events(_)
        | ManagedAppRefusal::RuntimeState => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "the installed App runtime is unavailable" })),
        )
            .into_response(),
    }
}

fn refusal(error: AppRefusal) -> Response {
    let status = match &error {
        AppRefusal::NoSuchApp(_)
        | AppRefusal::NoSuchManagedAgent(_)
        | AppRefusal::NoSuchEventType(_)
        | AppRefusal::NoSuchDelivery(_)
        | AppRefusal::MissingPackage { .. } => StatusCode::NOT_FOUND,
        AppRefusal::NeedsReview { .. }
        | AppRefusal::DeliveryState(_)
        | AppRefusal::UnsafeRetry(_)
        | AppRefusal::StaleRuntimeToken
        | AppRefusal::OperationContractChanged(_) => StatusCode::CONFLICT,
        AppRefusal::Unconfigured(_) | AppRefusal::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    let required_review = match &error {
        AppRefusal::NeedsReview { fingerprint } => Some(fingerprint),
        _ => None,
    };
    (
        status,
        Json(json!({
            "error": error.to_string(),
            "required_review": required_review,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request};
    use axum::Router;
    use exchange_host::{
        AppStore, ConnectionRegistry, CredentialRef, CredentialScope, InstanceId,
        MemoryConnectionRegistry, PackageRegistry, Secret, SecretStore, StoreError, Tenant,
    };
    use tower::ServiceExt;

    use crate::dev_identity::DevIdentity;
    use crate::managed_apps::ManagedAppSupervisor;

    use super::*;

    const ROSTER: &str = "user:alice@acme,user:bob@globex";

    #[derive(Default)]
    struct AddressStore {
        references: Mutex<Vec<CredentialRef>>,
    }

    #[async_trait]
    impl SecretStore for AddressStore {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            if self
                .references
                .lock()
                .expect("test store")
                .contains(reference)
            {
                Ok(Secret::new("fixture"))
            } else {
                Err(StoreError::NotFound {
                    path: "fixture credential".into(),
                })
            }
        }

        async fn put(&self, reference: &CredentialRef, _secret: &Secret) -> Result<(), StoreError> {
            self.references
                .lock()
                .expect("test store")
                .push(reference.clone());
            Ok(())
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            self.references
                .lock()
                .expect("test store")
                .retain(|candidate| candidate != reference);
            Ok(())
        }

        async fn references(
            &self,
            scope: &CredentialScope,
        ) -> Result<Vec<CredentialRef>, StoreError> {
            Ok(self
                .references
                .lock()
                .expect("test store")
                .iter()
                .filter(|reference| scope.contains(reference))
                .cloned()
                .collect())
        }
    }

    fn app_router() -> Router {
        let tenant = Tenant::new("acme").expect("tenant");
        let instance = InstanceId::parse("11111111-1111-4111-8111-111111111111").expect("instance");
        let reference = CredentialRef::for_instance(
            tenant.as_str(),
            "com.slack.api",
            instance.as_str(),
            "default",
            "bot_token",
        )
        .expect("Slack credential address");
        let credentials = Arc::new(AddressStore {
            references: Mutex::new(vec![reference]),
        });
        let registry = Arc::new(MemoryConnectionRegistry::default());
        ConnectionRegistry::assign(
            registry.as_ref(),
            &tenant,
            "slack",
            &ConnectionLabel::new("team").expect("label"),
            &instance,
        )
        .expect("registry binding");
        let store = Arc::new(AppStore::in_memory(PackageRegistry::curated()));
        let supervisor =
            Arc::new(ManagedAppSupervisor::new(store, None, None).expect("App supervisor"));
        crate::routes::app(
            AppState::with_development_identity(Arc::new(
                DevIdentity::from_roster(ROSTER).expect("roster"),
            ))
            .with_credentials(credentials)
            .with_connection_registry(registry)
            .with_apps(supervisor),
        )
    }

    async fn request(
        app: &Router,
        caller: &str,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {caller}"));
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = app
            .clone()
            .oneshot(request.body(body).expect("request"))
            .await
            .expect("router");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("JSON")
        };
        (status, body)
    }

    #[tokio::test]
    async fn install_chat_and_activity_are_tenant_derived_end_to_end() {
        let app = app_router();
        let (status, _) = request(
            &app,
            "alice",
            Method::POST,
            "/api/model-profiles",
            Some(json!({
                "id": "demo", "provider": "static", "model": "static",
                "revision": 1, "static_reply": "Hello from Flux"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, installation) = request(
            &app,
            "alice",
            Method::POST,
            "/api/apps",
            Some(json!({
                "id": "assistant", "package": "exchange-apps/slack-bot", "version": "1.0.0",
                "connections": { "slack": "team" }, "model_profile": "demo",
                "access_layers": ["reply"], "datasources": {}, "risk_ceiling": "high",
                "scopes": ["chat:reply"], "review": null
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{installation}");
        assert_eq!(installation["activation"], "active");

        let (status, reply) = request(
            &app,
            "alice",
            Method::POST,
            "/api/apps/assistant/chat",
            Some(json!({ "message": "private prompt", "session": "thread-1" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["reply"], "Hello from Flux");
        assert_eq!(reply["activation"], "active");

        let (status, activity) = request(
            &app,
            "alice",
            Method::GET,
            "/api/apps/assistant/activity",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{activity}");
        assert_eq!(activity["activity"][0]["outcome"], "completed");
        assert!(!activity.to_string().contains("private prompt"));

        let (status, refused) = request(
            &app,
            "bob",
            Method::POST,
            "/api/apps/assistant/chat",
            Some(json!({ "message": "cross tenant" })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
        assert!(!refused.to_string().contains("acme"));
        assert!(!refused.to_string().contains("Hello from Flux"));
    }
}
