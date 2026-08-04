//! Authenticated hosted transport for one exact FXLM operation.

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
    expired_reply, DeadlineController, Dispatcher, SessionAdvance, SessionBegin, Transport,
};
use crate::state::AppState;

const PROTOCOL: &str = "exchange.local-management.v1";
const MAX_MESSAGE_BYTES: usize = 65_548;
const TERMINAL_FINALIZATION_BUDGET: std::time::Duration = std::time::Duration::from_secs(1);
const TERMINAL_FRAME_ATTEMPT: std::time::Duration = std::time::Duration::from_millis(250);

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
    let deadline = DeadlineController::start();
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
    deadline: DeadlineController,
) {
    let mut active: Option<Box<crate::local_management::ActiveSession>> = None;
    loop {
        match deadline.race(socket.recv()).await {
            Err(expired) => {
                if deadline.may_abort() {
                    if let Some(session) = &active {
                        session.abort().await;
                    }
                }
                let reply = expired_reply(expired);
                let (response, code) = reply.into_parts();
                finalize_terminal(&mut socket, Some(response), code).await;
                return;
            }
            Ok(Some(Ok(Message::Binary(bytes)))) if bytes.len() <= MAX_MESSAGE_BYTES => {
                if let Some(session) = active.as_mut() {
                    match session.accept_message(&bytes).await {
                        SessionAdvance::Awaiting => {}
                        SessionAdvance::Terminal(reply) => {
                            let (response, code) = reply.into_parts();
                            send_terminal(&mut socket, response, code, &deadline).await;
                            return;
                        }
                    }
                } else {
                    let begun = deadline
                        .race(dispatcher.begin_message(
                            Transport::Hosted,
                            &tenant,
                            &bytes,
                            &deadline,
                        ))
                        .await;
                    let begun = match begun {
                        Ok(begun) => begun,
                        Err(expired) => {
                            let (response, code) = expired_reply(expired).into_parts();
                            finalize_terminal(&mut socket, Some(response), code).await;
                            return;
                        }
                    };
                    match begun {
                        SessionBegin::Terminal(reply) => {
                            let (response, code) = reply.into_parts();
                            send_terminal(&mut socket, response, code, &deadline).await;
                            return;
                        }
                        SessionBegin::Active { response, session } => {
                            if deadline
                                .race_response(socket.send(Message::Binary(response.into())))
                                .await
                                .is_err()
                            {
                                if deadline.may_abort() {
                                    session.abort().await;
                                }
                                return;
                            }
                            active = Some(session);
                        }
                    }
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => {
                if deadline.may_abort() {
                    if let Some(session) = &active {
                        session.abort().await;
                    }
                }
                let _ = deadline
                    .race_response(close(&mut socket, close_code::SIZE))
                    .await;
                return;
            }
            Ok(Some(Ok(Message::Text(_)))) => {
                if deadline.may_abort() {
                    if let Some(session) = &active {
                        session.abort().await;
                    }
                }
                let _ = deadline
                    .race_response(close(&mut socket, close_code::UNSUPPORTED))
                    .await;
                return;
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                if deadline.may_abort() {
                    if let Some(session) = &active {
                        session.abort().await;
                    }
                }
                return;
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(Some(Err(_))) => {
                if deadline.may_abort() {
                    if let Some(session) = &active {
                        session.abort().await;
                    }
                }
                let _ = deadline
                    .race_response(close(&mut socket, close_code::PROTOCOL))
                    .await;
                return;
            }
        }
    }
}

async fn close(socket: &mut WebSocket, code: u16) {
    if socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: "".into(),
        })))
        .await
        .is_ok()
    {
        // Retain the upgraded stream for the peer's close response. The caller always bounds this
        // wait by either the logical deadline or the one terminal-finalization deadline.
        let _ = socket.recv().await;
    }
}

async fn send_terminal(
    socket: &mut WebSocket,
    response: Vec<u8>,
    code: u16,
    deadline: &DeadlineController,
) {
    if deadline
        .race_response(socket.send(Message::Binary(response.into())))
        .await
        .is_err()
    {
        finalize_terminal(socket, None, code).await;
        return;
    }
    if deadline.race_response(close(socket, code)).await.is_err() {
        finalize_terminal(socket, None, code).await;
    }
}

/// One fixed, non-configurable terminalization budget reserves time for the mandatory close.
///
/// If the canonical FXLM frame backpressures, its short attempt is cancelled and the remainder is
/// spent on the empty-reason close. This is deliberately separate from the logical-operation clock:
/// at expiry that clock has no time left, but the protocol still requires a best-effort close.
async fn finalize_terminal(socket: &mut WebSocket, response: Option<Vec<u8>>, code: u16) {
    let final_by = tokio::time::Instant::now() + TERMINAL_FINALIZATION_BUDGET;
    if let Some(response) = response {
        let frame_by = tokio::time::Instant::now() + TERMINAL_FRAME_ATTEMPT;
        let _ =
            tokio::time::timeout_at(frame_by, socket.send(Message::Binary(response.into()))).await;
    }
    let _ = tokio::time::timeout_at(final_by, close(socket, code)).await;
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    use tokio_tungstenite::tungstenite::http::{header as ws_header, HeaderValue};
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    use super::*;
    use crate::audit::AuditJournal;
    use crate::dev_identity::DevIdentity;
    use crate::local_management::TransactionCoordinator;

    fn private_root() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-hosted-deadline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("private test root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only test root");
        }
        root
    }

    #[tokio::test(start_paused = true)]
    async fn hosted_slot_idle_and_ping_traffic_expire_on_the_admission_clock() {
        let root = private_root();
        let credentials = exchange_host::CredentialStore::bind(root.join("credentials/store"))
            .expect("retained credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("transactions/journal.sqlite3"),
                credentials.prepared_secrets(),
            )
            .expect("transaction coordinator"),
        );
        let audit = Arc::new(AuditJournal::bind(root.join("audit/events.jsonl")).expect("audit"));
        let identity =
            Arc::new(DevIdentity::from_roster("user:owner@local").expect("development identity"));
        let origin = "http://owner.local";
        let state = AppState::with_development_identity(identity)
            .with_transaction_coordinator(coordinator)
            .with_audit(audit)
            .with_hosted_origin(origin);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            axum::serve(listener, crate::routes::app(state))
                .await
                .expect("test server");
        });

        let mut request = format!("ws://{address}/api/onboarding/frames")
            .into_client_request()
            .expect("WebSocket request");
        request.headers_mut().insert(
            ws_header::AUTHORIZATION,
            HeaderValue::from_static("Bearer owner"),
        );
        request
            .headers_mut()
            .insert(ws_header::ORIGIN, HeaderValue::from_static(origin));
        request.headers_mut().insert(
            ws_header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static(PROTOCOL),
        );
        let (mut socket, response) = tokio_tungstenite::connect_async(request)
            .await
            .expect("hosted local-management upgrade");
        assert_eq!(
            response.headers().get(ws_header::SEC_WEBSOCKET_PROTOCOL),
            Some(&HeaderValue::from_static(PROTOCOL))
        );

        tokio::time::advance(std::time::Duration::from_secs(299)).await;
        socket
            .send(ClientMessage::Ping(Vec::new().into()))
            .await
            .expect("traffic at 299 seconds");
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        // The logical boundary was advanced deterministically; resume wall time so the real TCP
        // close handshake, which is not a simulated clock operation, can flush normally.
        tokio::time::resume();

        let terminal = socket
            .next()
            .await
            .expect("deadline terminal message")
            .expect("deadline terminal frame");
        assert_eq!(
            terminal,
            ClientMessage::Binary(crate::local_management::deadline_frame().into())
        );
        let close = socket
            .next()
            .await
            .expect("policy close")
            .expect("policy close frame");
        let ClientMessage::Close(Some(close)) = close else {
            panic!("expected a close frame after the deadline response");
        };
        assert_eq!(close.code, CloseCode::Policy);
        assert!(close.reason.is_empty());

        server.abort();
        drop(credentials);
        let _ = std::fs::remove_dir_all(root);
    }
}
