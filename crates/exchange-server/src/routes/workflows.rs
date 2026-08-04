//! Tenant-derived workflow authoring and immutable publication routes.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, MethodRouter};
use axum::{Extension, Json};
use exchange_host::{
    editor_catalog, editor_schema, validate_workflow, Principal, WorkflowEdit, WorkflowRefusal,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{Access, Module, Route};
use crate::state::AppState;

pub(super) const MODULE: Module = Module {
    name: "workflows",
    routes: &[
        Route {
            path: "/api/workflows",
            access: Access::Operator,
            method_router: collection_route,
        },
        Route {
            path: "/api/workflows/editor-catalog",
            access: Access::Operator,
            method_router: editor_catalog_route,
        },
        Route {
            path: "/api/workflows/{workflow}",
            access: Access::Operator,
            method_router: draft_route,
        },
        Route {
            path: "/api/workflows/{workflow}/validate",
            access: Access::Operator,
            method_router: validate_route,
        },
        Route {
            path: "/api/workflows/{workflow}/publish",
            access: Access::Operator,
            method_router: publish_route,
        },
        Route {
            path: "/api/workflows/{workflow}/versions",
            access: Access::Operator,
            method_router: versions_route,
        },
        Route {
            path: "/api/workflows/{workflow}/versions/{version}",
            access: Access::Operator,
            method_router: version_route,
        },
        Route {
            path: "/api/workflows/{workflow}/runs",
            access: Access::Operator,
            method_router: workflow_runs_route,
        },
        Route {
            path: "/api/workflow-runs",
            access: Access::Operator,
            method_router: activity_route,
        },
        Route {
            path: "/api/workflow-runs/{run}",
            access: Access::Operator,
            method_router: run_route,
        },
        Route {
            path: "/api/workflow-runs/{run}/cancel",
            access: Access::Operator,
            method_router: cancel_route,
        },
    ],
};

fn collection_route() -> MethodRouter<AppState> {
    get(list).post(create)
}

fn editor_catalog_route() -> MethodRouter<AppState> {
    get(catalogue)
}

fn draft_route() -> MethodRouter<AppState> {
    get(read).put(save).delete(remove)
}

fn validate_route() -> MethodRouter<AppState> {
    post(validate)
}

fn publish_route() -> MethodRouter<AppState> {
    post(publish)
}

fn versions_route() -> MethodRouter<AppState> {
    get(versions)
}

fn version_route() -> MethodRouter<AppState> {
    get(version)
}

fn workflow_runs_route() -> MethodRouter<AppState> {
    get(workflow_runs).post(start_run)
}

fn activity_route() -> MethodRouter<AppState> {
    get(activity)
}

fn run_route() -> MethodRouter<AppState> {
    get(run)
}

fn cancel_route() -> MethodRouter<AppState> {
    post(cancel)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateWorkflow {
    id: String,
    title: String,
    edit: WorkflowEdit,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveWorkflow {
    revision: u64,
    title: String,
    edit: WorkflowEdit,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateWorkflow {
    edit: WorkflowEdit,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRun {
    #[serde(default)]
    version: Option<u64>,
    #[serde(default = "empty_object")]
    params: Value,
}

fn empty_object() -> Value {
    json!({})
}

#[derive(Debug, Deserialize)]
struct ActivityQuery {
    workflow: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(store) = state.workflows() else {
        return unavailable();
    };
    match store.list(principal.tenant()) {
        Ok(workflows) => (StatusCode::OK, Json(json!({ "workflows": workflows }))).into_response(),
        Err(error) => refusal(error),
    }
}

async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(request): Json<CreateWorkflow>,
) -> Response {
    let (Some(store), Some(pure)) = (state.workflows(), state.pure_editor_tools()) else {
        return unavailable();
    };
    let validated = match validate_workflow(request.edit, None, pure) {
        Ok(validated) => validated,
        Err(error) => return refusal(error),
    };
    match store.create(principal.tenant(), &request.id, &request.title, validated) {
        Ok(workflow) => (StatusCode::CREATED, Json(workflow)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn catalogue(State(state): State<AppState>) -> Response {
    let Some(pure) = state.pure_editor_tools() else {
        return unavailable();
    };
    match editor_catalog(pure) {
        Ok(operations) => (
            StatusCode::OK,
            Json(json!({
                "schema_version": exchange_host::EDITOR_SCHEMA_VERSION,
                "schema": editor_schema(),
                "operations": operations,
            })),
        )
            .into_response(),
        Err(error) => refusal(error),
    }
}

async fn read(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
) -> Response {
    let Some(store) = state.workflows() else {
        return unavailable();
    };
    match store.get(principal.tenant(), &workflow) {
        Ok(workflow) => (StatusCode::OK, Json(workflow)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn save(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
    Json(request): Json<SaveWorkflow>,
) -> Response {
    let (Some(store), Some(pure)) = (state.workflows(), state.pure_editor_tools()) else {
        return unavailable();
    };
    let previous = store
        .get(principal.tenant(), &workflow)
        .ok()
        .and_then(|draft| draft.graph);
    let validated = match validate_workflow(request.edit, previous.as_ref(), pure) {
        Ok(validated) => validated,
        Err(error) => return refusal(error),
    };
    match store.save(
        principal.tenant(),
        &workflow,
        request.revision,
        &request.title,
        validated,
    ) {
        Ok(workflow) => (StatusCode::OK, Json(workflow)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn validate(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
    Json(request): Json<ValidateWorkflow>,
) -> Response {
    let (Some(store), Some(pure)) = (state.workflows(), state.pure_editor_tools()) else {
        return unavailable();
    };
    let previous = match store.get(principal.tenant(), &workflow) {
        Ok(draft) => draft.graph,
        Err(error) => return refusal(error),
    };
    match validate_workflow(request.edit, previous.as_ref(), pure) {
        Ok(validated) => (StatusCode::OK, Json(validated)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn publish(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
    Json(request): Json<Revision>,
) -> Response {
    let (Some(store), Some(pure)) = (state.workflows(), state.pure_editor_tools()) else {
        return unavailable();
    };
    let draft = match store.get(principal.tenant(), &workflow) {
        Ok(draft) => draft,
        Err(error) => return refusal(error),
    };
    let validated = match validate_workflow(
        WorkflowEdit::Source {
            source: draft.source,
        },
        draft.graph.as_ref(),
        pure,
    ) {
        Ok(validated) => validated,
        Err(error) => return refusal(error),
    };
    match store.publish(principal.tenant(), &workflow, request.revision, validated) {
        Ok(version) => (StatusCode::CREATED, Json(version)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn versions(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
) -> Response {
    let Some(store) = state.workflows() else {
        return unavailable();
    };
    match store.versions(principal.tenant(), &workflow) {
        Ok(versions) => (StatusCode::OK, Json(json!({ "versions": versions }))).into_response(),
        Err(error) => refusal(error),
    }
}

async fn version(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((workflow, version)): Path<(String, u64)>,
) -> Response {
    let Some(store) = state.workflows() else {
        return unavailable();
    };
    match store.version(principal.tenant(), &workflow, version) {
        Ok(version) => (StatusCode::OK, Json(version)).into_response(),
        Err(error) => refusal(error),
    }
}

async fn remove(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
    Query(request): Query<Revision>,
) -> Response {
    let Some(store) = state.workflows() else {
        return unavailable();
    };
    match store.delete(principal.tenant(), &workflow, request.revision) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => refusal(error),
    }
}

async fn workflow_runs(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
) -> Response {
    let Some(runs) = state.workflow_runs() else {
        return unavailable();
    };
    match runs.list(principal.tenant(), Some(&workflow)) {
        Ok(runs) => (StatusCode::OK, Json(json!({ "runs": runs }))).into_response(),
        Err(error) => run_store_refusal(error),
    }
}

async fn activity(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ActivityQuery>,
) -> Response {
    let Some(runs) = state.workflow_runs() else {
        return unavailable();
    };
    match runs.list(principal.tenant(), query.workflow.as_deref()) {
        Ok(runs) => (StatusCode::OK, Json(json!({ "runs": runs }))).into_response(),
        Err(error) => run_store_refusal(error),
    }
}

async fn run(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(run): Path<String>,
) -> Response {
    let Some(runs) = state.workflow_runs() else {
        return unavailable();
    };
    match runs.get(principal.tenant(), &run) {
        Ok(run) => (StatusCode::OK, Json(run)).into_response(),
        Err(error) if error == "no such workflow run" => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": error }))).into_response()
        }
        Err(error) => run_store_refusal(error),
    }
}

async fn cancel(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(run): Path<String>,
) -> Response {
    let Some(runs) = state.workflow_runs() else {
        return unavailable();
    };
    match runs.cancel(principal.tenant(), &run) {
        Ok(true) => StatusCode::ACCEPTED.into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "workflow run is no longer cancellable" })),
        )
            .into_response(),
        Err(error) if error == "no such workflow run" => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": error }))).into_response()
        }
        Err(error) => run_store_refusal(error),
    }
}

async fn start_run(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(workflow): Path<String>,
    Json(request): Json<StartRun>,
) -> Response {
    let (Some(store), Some(invoker), Some(pure), Some(runs)) = (
        state.workflows(),
        state.invoker(),
        state.pure_editor_tools(),
        state.workflow_runs(),
    ) else {
        return unavailable();
    };
    let version = match requested_version(store, &principal, &workflow, request.version) {
        Ok(version) => version,
        Err(response) => return *response,
    };
    let (run, cancellation) = match runs.create(principal.tenant(), &workflow, version.version) {
        Ok(created) => created,
        Err(error) => return run_store_refusal(error),
    };
    let run_id = run.id.clone();
    let invoker = invoker.clone();
    let pure = pure.clone();
    let runs = runs.clone();
    tokio::spawn(async move {
        let observer = runs.observer(&run_id);
        let execution = invoker.invoke_workflow(
            &principal,
            &version,
            request.params,
            &pure,
            &run_id,
            observer.clone(),
        );
        tokio::select! {
            result = execution => match result {
                Ok(invocation) => {
                    if let Some(error) = observer.failure() {
                        let message = format!("workflow trace could not be persisted: {error}");
                        let _ = runs.finish(&run_id, "failed", None, Some(&message));
                    } else {
                        let _ = runs.finish(&run_id, "succeeded", Some(&invocation.content), None);
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    let _ = runs.finish(&run_id, "failed", None, Some(&message));
                }
            },
            _ = cancellation => {
                let _ = runs.finish(&run_id, "cancelled", None, None);
            }
        }
    });
    (StatusCode::ACCEPTED, Json(run)).into_response()
}

fn requested_version(
    store: &exchange_host::WorkflowStore,
    principal: &Principal,
    workflow: &str,
    requested: Option<u64>,
) -> Result<exchange_host::WorkflowVersion, Box<Response>> {
    let version = match requested {
        Some(version) => version,
        None => store
            .get(principal.tenant(), workflow)
            .map_err(|error| Box::new(refusal(error)))?
            .published_version
            .ok_or_else(|| {
                Box::new(
                    (
                        StatusCode::CONFLICT,
                        Json(json!({ "error": "workflow has no published version" })),
                    )
                        .into_response(),
                )
            })?,
    };
    store
        .version(principal.tenant(), workflow, version)
        .map_err(|error| Box::new(refusal(error)))
}

/// Invoke the latest published version through the existing principal operation route.
pub(super) async fn invoke_published(
    state: AppState,
    principal: Principal,
    workflow: String,
    params: Value,
) -> Response {
    let (Some(store), Some(invoker), Some(pure), Some(runs)) = (
        state.workflows(),
        state.invoker(),
        state.pure_editor_tools(),
        state.workflow_runs(),
    ) else {
        return unavailable();
    };
    let version = match requested_version(store, &principal, &workflow, None) {
        Ok(version) => version,
        Err(response) => return *response,
    };
    let (run, mut cancellation) = match runs.create(principal.tenant(), &workflow, version.version)
    {
        Ok(created) => created,
        Err(error) => return run_store_refusal(error),
    };
    let observer = runs.observer(&run.id);
    let execution = invoker.invoke_workflow(
        &principal,
        &version,
        params,
        pure,
        &run.id,
        observer.clone(),
    );
    tokio::select! {
        result = execution => match result {
            Ok(invocation) => {
                if let Some(error) = observer.failure() {
                    let message = format!("workflow trace could not be persisted: {error}");
                    let _ = runs.finish(&run.id, "failed", None, Some(&message));
                    return run_store_refusal(message);
                }
                let _ = runs.finish(&run.id, "succeeded", Some(&invocation.content), None);
                (StatusCode::OK, Json(invocation)).into_response()
            }
            Err(error) => {
                let message = error.to_string();
                let _ = runs.finish(&run.id, "failed", None, Some(&message));
                let status = match error {
                    exchange_host::WorkflowInvokeRefusal::NotGranted(_) => StatusCode::FORBIDDEN,
                    exchange_host::WorkflowInvokeRefusal::ContractChanged(_) => StatusCode::CONFLICT,
                    exchange_host::WorkflowInvokeRefusal::Runtime(_) => StatusCode::CONFLICT,
                    exchange_host::WorkflowInvokeRefusal::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
                    exchange_host::WorkflowInvokeRefusal::Execution(_) => StatusCode::BAD_GATEWAY,
                };
                workflow_run_refusal(status, message, run.id)
            }
        },
        _ = &mut cancellation => {
            let _ = runs.finish(&run.id, "cancelled", None, None);
            workflow_run_refusal(StatusCode::CONFLICT, "workflow run was cancelled", run.id)
        }
    }
}

fn workflow_run_refusal(
    status: StatusCode,
    error: impl Into<String>,
    run: impl Into<String>,
) -> Response {
    (
        status,
        Json(crate::protocol::WorkflowRunRefusalBody::new(error, run)),
    )
        .into_response()
}

fn run_store_refusal(error: String) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(crate::protocol::ErrorBody::new(format!(
            "workflow activity is unavailable: {error}"
        ))),
    )
        .into_response()
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(crate::protocol::ErrorBody::new(format!(
            "this host has no workflow store; configure {} outside every working tree",
            exchange_host::WORKFLOW_STORE_SETTING,
        ))),
    )
        .into_response()
}

fn refusal(error: WorkflowRefusal) -> Response {
    let status = match error {
        WorkflowRefusal::UnknownWorkflow(_) | WorkflowRefusal::UnknownVersion { .. } => {
            StatusCode::NOT_FOUND
        }
        WorkflowRefusal::AlreadyExists(_)
        | WorkflowRefusal::RevisionConflict { .. }
        | WorkflowRefusal::PublishedWorkflowCannotDelete(_)
        | WorkflowRefusal::PublicationSourceChanged => StatusCode::CONFLICT,
        WorkflowRefusal::Unconfigured { .. }
        | WorkflowRefusal::InsideWorkingTree { .. }
        | WorkflowRefusal::Unusable { .. }
        | WorkflowRefusal::Unwritable { .. }
        | WorkflowRefusal::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    let current_revision = match &error {
        WorkflowRefusal::RevisionConflict { current, .. } => Some(*current),
        _ => None,
    };
    (
        status,
        Json(crate::protocol::WorkflowLookupRefusalBody::new(
            error.to_string(),
            current_revision,
        )),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt as _;

    use super::*;
    use crate::dev_identity::DevIdentity;

    static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

    fn state() -> AppState {
        let suffix = NEXT_STORE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-api-workflows-{}-{suffix}",
            std::process::id()
        ));
        let store = exchange_host::WorkflowStore::bind(root).unwrap();
        let runs = Arc::new(
            crate::workflow_runs::WorkflowRunStore::bind(
                &store.path().parent().unwrap().join("runs.sqlite"),
            )
            .unwrap(),
        );
        let mut registry = exchange_host::ToolRegistry::new();
        flux_tools::cognition::try_register_cognition(&mut registry).unwrap();
        let pure = exchange_host::PureEditorTools::new(registry).unwrap();
        AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster("user:alice@acme,user:bob@other").unwrap(),
        ))
        .with_workflows(Arc::new(store), Arc::new(pure), runs)
    }

    async fn request(
        app: axum::Router,
        user: &str,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {user}"));
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&value).unwrap())
            }
            None => Body::empty(),
        };
        let response = app.oneshot(builder.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn assert_invoke_v1(response: Response) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        crate::protocol::decode_invoke_response(status.as_u16(), &bytes)
            .expect("workflow response satisfies exchange.invoke-response.v1");
    }

    #[tokio::test]
    async fn workflow_lookup_and_run_refusal_renderers_are_closed_invoke_v1_shapes() {
        assert_invoke_v1(refusal(exchange_host::WorkflowRefusal::UnknownWorkflow(
            "missing".to_owned(),
        )))
        .await;
        assert_invoke_v1(workflow_run_refusal(
            StatusCode::FORBIDDEN,
            "workflow grant refused",
            "01J00000000000000000000000",
        ))
        .await;
    }

    #[tokio::test]
    async fn the_api_derives_tenant_rejects_stale_saves_and_keeps_versions_immutable() {
        let app = super::super::app(state());
        let source = "flow triage\n  return true\n";
        let (status, created) = request(
            app.clone(),
            "alice",
            Method::POST,
            "/api/workflows",
            Some(json!({
                "id": "triage",
                "title": "Triage",
                "edit": { "mode": "source", "source": source },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");

        let (status, other) =
            request(app.clone(), "bob", Method::GET, "/api/workflows", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(other["workflows"], json!([]));

        let (status, published) = request(
            app.clone(),
            "alice",
            Method::POST,
            "/api/workflows/triage/publish",
            Some(json!({ "revision": 1 })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{published}");

        let (status, saved) = request(
            app.clone(),
            "alice",
            Method::PUT,
            "/api/workflows/triage",
            Some(json!({
                "revision": 1,
                "title": "Triage",
                "edit": { "mode": "source", "source": "flow triage\n  return false\n" },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(saved["revision"], 2);

        let (status, conflict) = request(
            app.clone(),
            "alice",
            Method::PUT,
            "/api/workflows/triage",
            Some(json!({
                "revision": 1,
                "title": "stale",
                "edit": { "mode": "source", "source": source },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
        assert_eq!(conflict["current_revision"], 2);

        let (status, first) = request(
            app,
            "alice",
            Method::GET,
            "/api/workflows/triage/versions/1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{first}");
        assert!(first["source"].as_str().unwrap().contains("return true"));
    }
}
