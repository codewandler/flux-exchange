//! Authenticated hosted transport for one exact FXLM operation.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{OriginalUri, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use exchange_host::Principal;
use futures_util::SinkExt as _;
use serde_json::json;

use super::{Access, Module, Route};
use crate::local_management::{Dispatcher, Transport};
use crate::state::AppState;

const PROTOCOL: &str = "exchange.local-management.v1";
const MAX_MESSAGE_BYTES: usize = 65_548;

pub(super) const MODULE: Module = Module {
    name: "local-management-frames",
    routes: &[Route {
        path: "/api/onboarding/frames",
        access: Access::Operator,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    get(upgrade)
}

async fn upgrade(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if uri.query().is_some() {
        return refusal(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(expected_origin) = state.hosted_origin() else {
        return refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "local_management_unavailable",
        );
    };
    if headers
        .get(header::ORIGIN)
        .is_none_or(|origin| origin.as_bytes() != expected_origin.as_bytes())
    {
        return refusal(StatusCode::FORBIDDEN, "invalid_request");
    }
    let offered = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok());
    if offered != Some(PROTOCOL) {
        return refusal(StatusCode::BAD_REQUEST, "unsupported_version");
    }
    let dispatcher = match Dispatcher::from_state(state) {
        Ok(dispatcher) => dispatcher,
        Err(_) => {
            return refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "local_management_unavailable",
            );
        }
    };
    if dispatcher.state().audit().is_none() {
        return refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "local_management_unavailable",
        );
    }
    let claim = match dispatcher.state().begin_local_management(&principal) {
        Ok(claim) => claim,
        Err(_) => {
            let mut response = refusal(StatusCode::TOO_MANY_REQUESTS, "invalid_request");
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
            return response;
        }
    };
    let tenant = principal.tenant().clone();
    let mut response = upgrade
        .protocols([PROTOCOL])
        .on_upgrade(move |socket| serve(socket, dispatcher, tenant, claim));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn serve(
    mut socket: WebSocket,
    dispatcher: Dispatcher,
    tenant: exchange_host::Tenant,
    _claim: crate::traffic::HostedClaim,
) {
    let incoming = tokio::time::timeout(Duration::from_secs(30), socket.recv()).await;
    let response = match incoming {
        Ok(Some(Ok(Message::Binary(bytes)))) if bytes.len() <= MAX_MESSAGE_BYTES => {
            dispatcher
                .dispatch_message(Transport::Hosted, &tenant, &bytes)
                .await
        }
        _ => invalid_frame(),
    };
    let _ = socket.send(Message::Binary(response.into())).await;
    let _ = socket.close().await;
}

fn invalid_frame() -> Vec<u8> {
    // Fixed canonical FXLM ERROR frame for a transport-level invalid request.
    let payload = br#"{"code":"invalid_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":422}"#;
    let mut frame = Vec::with_capacity(12 + payload.len());
    frame.extend_from_slice(b"FXLM");
    frame.push(1);
    frame.push(2);
    frame.extend_from_slice(&0x7fff_u16.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn refusal(status: StatusCode, code: &'static str) -> Response {
    let mut response = (
        status,
        Json(json!({
            "schema": "exchange.local-management-error.v1",
            "code": code,
            "status": status.as_u16(),
            "retry": if status == StatusCode::SERVICE_UNAVAILABLE { "operator" } else { "never" },
            "commit": "none"
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
