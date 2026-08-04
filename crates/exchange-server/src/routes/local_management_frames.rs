//! Authenticated hosted transport for one exact FXLM operation.

use std::time::Duration;

use axum::extract::ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{OriginalUri, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use exchange_host::Principal;
use serde_json::json;

use super::{Access, Module, Route};
use crate::local_management::{
    deadline_frame, Dispatcher, SessionAdvance, SessionBegin, Transport,
};
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
    // The occupancy claim already exists and the 101 has not been constructed yet. Starting the
    // absolute budget here includes handshake response scheduling exactly as the hosted contract
    // requires; traffic in `serve` never replaces this deadline.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let mut response = upgrade
        .protocols([PROTOCOL])
        .on_upgrade(move |socket| serve(socket, dispatcher, tenant, claim, deadline));
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
    deadline: tokio::time::Instant,
) {
    let mut active: Option<Box<crate::local_management::ActiveSession>> = None;
    loop {
        match tokio::time::timeout_at(deadline, socket.recv()).await {
            Err(_) => {
                if let Some(session) = &active {
                    session.abort().await;
                }
                let _ = socket.send(Message::Binary(deadline_frame().into())).await;
                close(&mut socket, close_code::POLICY).await;
                return;
            }
            Ok(Some(Ok(Message::Binary(bytes)))) if bytes.len() <= MAX_MESSAGE_BYTES => {
                if let Some(session) = active.as_mut() {
                    match session.accept_message(&bytes).await {
                        SessionAdvance::Awaiting => {}
                        SessionAdvance::Terminal(reply) => {
                            let (response, code) = reply.into_parts();
                            let _ = socket.send(Message::Binary(response.into())).await;
                            close(&mut socket, code).await;
                            return;
                        }
                    }
                } else {
                    match dispatcher
                        .begin_message(Transport::Hosted, &tenant, &bytes)
                        .await
                    {
                        SessionBegin::Terminal(reply) => {
                            let (response, code) = reply.into_parts();
                            let _ = socket.send(Message::Binary(response.into())).await;
                            close(&mut socket, code).await;
                            return;
                        }
                        SessionBegin::Active { response, session } => {
                            let _ = socket.send(Message::Binary(response.into())).await;
                            active = Some(session);
                        }
                    }
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => {
                if let Some(session) = &active {
                    session.abort().await;
                }
                close(&mut socket, close_code::SIZE).await;
                return;
            }
            Ok(Some(Ok(Message::Text(_)))) => {
                if let Some(session) = &active {
                    session.abort().await;
                }
                close(&mut socket, close_code::UNSUPPORTED).await;
                return;
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                if let Some(session) = &active {
                    session.abort().await;
                }
                return;
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(Some(Err(_))) => {
                if let Some(session) = &active {
                    session.abort().await;
                }
                close(&mut socket, close_code::PROTOCOL).await;
                return;
            }
        }
    }
}

async fn close(socket: &mut WebSocket, code: u16) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: "".into(),
        })))
        .await;
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
