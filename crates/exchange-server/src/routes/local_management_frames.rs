//! Authenticated hosted transport for one exact FXLM operation.

use axum::extract::ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{OriginalUri, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use exchange_host::Principal;
use futures_util::{Sink as _, SinkExt as _};
use serde_json::json;

use super::{Access, Module, Route};
use crate::local_management::{
    expired_reply, DeadlineController, Dispatcher, SessionAdvance, SessionBegin, Transport,
};
use crate::state::AppState;

const PROTOCOL: &str = "exchange.local-management.v1";
const MAX_MESSAGE_BYTES: usize = 65_548;
const TERMINAL_FINALIZATION_BUDGET: std::time::Duration = std::time::Duration::from_secs(1);
const TERMINAL_RESERVATION_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);
// Axum's Tungstenite sink can retain a frame after a cancelled `send` flush. Keep enough
// userspace capacity to enqueue one maximum FXLM frame and its close before either reaches TCP.
const TERMINAL_ATOMIC_BUFFER: usize = MAX_MESSAGE_BYTES + 256;
const TERMINAL_MAX_BUFFER: usize = TERMINAL_ATOMIC_BUFFER * 2;
const _: () = assert!(TERMINAL_ATOMIC_BUFFER > MAX_MESSAGE_BYTES + 16);
const _: () = assert!(TERMINAL_MAX_BUFFER > TERMINAL_ATOMIC_BUFFER + MAX_MESSAGE_BYTES + 16);

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
        .write_buffer_size(TERMINAL_ATOMIC_BUFFER)
        .max_write_buffer_size(TERMINAL_MAX_BUFFER)
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
                    if let Some(session) = active.as_mut() {
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
                        SessionBegin::Active {
                            response,
                            mut session,
                        } => {
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
                    if let Some(session) = active.as_mut() {
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
                    if let Some(session) = active.as_mut() {
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
                    if let Some(session) = active.as_mut() {
                        session.abort().await;
                    }
                }
                return;
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(Some(Err(_))) => {
                if deadline.may_abort() {
                    if let Some(session) = active.as_mut() {
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
/// The canonical FXLM frame and empty-reason close are reserved atomically in a bounded userspace
/// buffer before the one flush begins. This is deliberately separate from the logical-operation
/// clock: at expiry that clock has no time left, but protocol finalization remains best effort.
async fn finalize_terminal(socket: &mut WebSocket, response: Option<Vec<u8>>, code: u16) {
    let final_by = tokio::time::Instant::now() + TERMINAL_FINALIZATION_BUDGET;
    let reserve_by = tokio::time::Instant::now() + TERMINAL_RESERVATION_BUDGET;
    let Some(_reservation) = reserve_terminal(socket, response, code, reserve_by).await else {
        return;
    };

    let flushing = socket.flush();
    tokio::pin!(flushing);
    let flushed = tokio::select! {
        biased;
        result = &mut flushing => result.is_ok(),
        _ = tokio::time::sleep_until(final_by) => false,
    };
    if !flushed {
        return;
    }
    let confirming = socket.recv();
    tokio::pin!(confirming);
    tokio::select! {
        biased;
        _ = &mut confirming => {}
        _ = tokio::time::sleep_until(final_by) => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalReservation {
    FrameAndClose,
    CloseOnly,
}

async fn reserve_terminal(
    socket: &mut WebSocket,
    response: Option<Vec<u8>>,
    code: u16,
    reserve_by: tokio::time::Instant,
) -> Option<TerminalReservation> {
    if !sink_ready(socket, reserve_by).await {
        return None;
    }
    let frame_reserved = if let Some(response) = response {
        // `start_send` is synchronous. With the production upgrade's buffer contract, every
        // admitted FXLM frame queues completely without I/O. An oversized adversarial frame can be
        // rejected atomically and must leave the sink ready for close.
        std::pin::Pin::new(&mut *socket)
            .start_send(Message::Binary(response.into()))
            .is_ok()
    } else {
        false
    };
    // Sink::start_send requires readiness before every item. For a reserved admitted frame this is
    // immediately ready because no transport write has begun; after WriteBufferFull it proves the
    // rejected frame left the bounded sink usable.
    if !sink_ready(socket, reserve_by).await {
        return None;
    }
    std::pin::Pin::new(&mut *socket)
        .start_send(Message::Close(Some(CloseFrame {
            code,
            reason: "".into(),
        })))
        .ok()?;
    Some(if frame_reserved {
        TerminalReservation::FrameAndClose
    } else {
        TerminalReservation::CloseOnly
    })
}

async fn sink_ready(socket: &mut WebSocket, reserve_by: tokio::time::Instant) -> bool {
    let ready =
        std::future::poll_fn(|context| std::pin::Pin::new(&mut *socket).poll_ready(context));
    tokio::pin!(ready);
    tokio::select! {
        biased;
        result = &mut ready => result.is_ok(),
        _ = tokio::time::sleep_until(reserve_by) => false,
    }
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

    use exchange_host::{
        GrantReceiptId, GrantSelector, GrantStore, GrantTransactions as _, PrincipalKind,
    };
    use futures_util::{FutureExt as _, SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    use tokio_tungstenite::tungstenite::http::{header as ws_header, HeaderValue};
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    use super::*;
    use crate::audit::AuditJournal;
    use crate::dev_identity::DevIdentity;
    use crate::local_management::TransactionCoordinator;
    use crate::local_management::{ReceiptIdentity, Unresolved};

    #[derive(Clone)]
    struct DeadlineRouteState {
        app: AppState,
        receipt: ReceiptIdentity,
        decision_anchor: Arc<tokio::sync::Notify>,
    }

    async fn postdecision_upgrade(
        State(state): State<DeadlineRouteState>,
        upgrade: WebSocketUpgrade,
    ) -> Response {
        let dispatcher = Dispatcher::from_state(state.app.clone()).expect("test dispatcher");
        let tenant = exchange_host::Tenant::new("local").expect("tenant");
        let principal = Principal::new(PrincipalKind::User, "local-owner", tenant.clone());
        let claim = state
            .app
            .begin_local_management(&principal)
            .expect("hosted slot");
        let deadline = DeadlineController::start();
        let decision_anchor = state.decision_anchor.clone();
        let receipt = state.receipt;
        upgrade
            .protocols([PROTOCOL])
            .on_upgrade(move |socket| async move {
                decision_anchor.notified().await;
                deadline
                    .decided(receipt, Unresolved::Store)
                    .expect("pre-existing durable decision");
                serve(socket, dispatcher, tenant, claim, deadline).await;
            })
    }

    async fn replay_upgrade(
        State(state): State<DeadlineRouteState>,
        upgrade: WebSocketUpgrade,
    ) -> Response {
        let dispatcher = Dispatcher::from_state(state.app.clone()).expect("test dispatcher");
        let tenant = exchange_host::Tenant::new("local").expect("tenant");
        let principal = Principal::new(PrincipalKind::User, "local-owner", tenant.clone());
        let claim = state
            .app
            .begin_local_management(&principal)
            .expect("hosted slot");
        let deadline = DeadlineController::start();
        upgrade
            .protocols([PROTOCOL])
            .on_upgrade(move |socket| serve(socket, dispatcher, tenant, claim, deadline))
    }

    async fn backpressured_terminal_upgrade(
        State(code): State<u16>,
        upgrade: WebSocketUpgrade,
    ) -> Response {
        upgrade
            // The real Tungstenite sink can hold a close but not this test's data frame. Its
            // WriteBufferFull result is frame-atomic and leaves the close reservation untouched.
            .write_buffer_size(1)
            .max_write_buffer_size(128)
            .protocols([PROTOCOL])
            .on_upgrade(move |mut socket| async move {
                finalize_terminal(&mut socket, Some(vec![0x5a; 4096]), code).await;
            })
    }

    async fn test_socket(
        address: std::net::SocketAddr,
        path: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let mut request = format!("ws://{address}{path}")
            .into_client_request()
            .expect("WebSocket request");
        request.headers_mut().insert(
            ws_header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static(PROTOCOL),
        );
        let (socket, response) = tokio_tungstenite::connect_async(request)
            .await
            .expect("test WebSocket upgrade");
        assert_eq!(
            response.headers().get(ws_header::SEC_WEBSOCKET_PROTOCOL),
            Some(&HeaderValue::from_static(PROTOCOL))
        );
        socket
    }

    fn client_control(opcode: u16, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(12 + payload.len());
        frame.extend_from_slice(b"FXLM");
        frame.extend_from_slice(&[1, 1]);
        frame.extend_from_slice(&opcode.to_be_bytes());
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

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

        let post_root = root.join("post-decision");
        std::fs::create_dir(&post_root).expect("post-decision root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&post_root, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only post-decision root");
        }
        let post_credentials =
            exchange_host::CredentialStore::bind(post_root.join("credentials/store"))
                .expect("post-decision credential store");
        let post_coordinator = Arc::new(
            TransactionCoordinator::bind(
                post_root.join("transactions/journal.sqlite3"),
                post_credentials.prepared_secrets(),
            )
            .expect("post-decision coordinator"),
        );
        let post_audit = Arc::new(
            AuditJournal::bind(post_root.join("audit/events.jsonl")).expect("post-decision audit"),
        );
        let grants = Arc::new(GrantStore::bind(post_root.join("grants.json")).expect("grants"));
        let tenant = exchange_host::Tenant::new("local").expect("tenant");
        let selector: GrantSelector = serde_json::from_slice(
            br#"{"effects_within":null,"idempotency":null,"max_risk":"low"}"#,
        )
        .expect("selector");
        let candidate = grants
            .preview(&tenant, "github", selector)
            .expect("grant candidate");
        let receipt = GrantReceiptId::from_protocol_bytes([0x42; 32]).expect("receipt");
        grants
            .apply(
                &tenant,
                &candidate.candidate,
                candidate.revision,
                candidate.proposal_digest,
                receipt,
            )
            .expect("durable grant decision");
        let receipt_identity =
            ReceiptIdentity::from_protocol_bytes(receipt.protocol_bytes()).expect("receipt");
        let post_state = AppState::without_identity()
            .with_transaction_coordinator(post_coordinator)
            .with_audit(post_audit)
            .with_grant_transactions(grants);
        let decision_anchor = Arc::new(tokio::sync::Notify::new());
        let deadline_state = DeadlineRouteState {
            app: post_state,
            receipt: receipt_identity,
            decision_anchor: decision_anchor.clone(),
        };
        let app = axum::Router::new()
            .route("/post", axum::routing::get(postdecision_upgrade))
            .route("/replay", axum::routing::get(replay_upgrade))
            .with_state(deadline_state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("post-decision listener");
        let address = listener.local_addr().expect("listener address");
        let post_server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });

        tokio::time::pause();
        let mut socket = test_socket(address, "/post").await;
        decision_anchor.notify_one();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(29)).await;
        socket
            .send(ClientMessage::Ping(Vec::new().into()))
            .await
            .expect("post-decision traffic at 29 seconds");
        tokio::task::yield_now().await;
        match socket.next().now_or_never() {
            None => {}
            Some(Some(Ok(ClientMessage::Pong(_)))) => assert!(
                socket.next().now_or_never().is_none(),
                "a pong cannot hide an early terminal response"
            ),
            Some(other) => panic!("unexpected value-free traffic at 29 seconds: {other:?}"),
        }
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        let terminal = socket
            .next()
            .await
            .expect("post-decision terminal message")
            .expect("post-decision terminal frame");
        let ClientMessage::Binary(terminal) = terminal else {
            panic!("expected post-decision FXLM refusal");
        };
        let terminal = String::from_utf8_lossy(&terminal);
        assert!(terminal.contains("\"code\":\"store_unavailable\""));
        assert!(terminal.contains(&receipt_identity.encoded()));
        let mut close = socket
            .next()
            .await
            .expect("normal close")
            .expect("normal close frame");
        while matches!(close, ClientMessage::Ping(_) | ClientMessage::Pong(_)) {
            close = socket
                .next()
                .await
                .expect("normal close after control traffic")
                .expect("normal close frame after control traffic");
        }
        let ClientMessage::Close(Some(close)) = close else {
            panic!("expected a close frame after post-decision expiry");
        };
        assert_eq!(close.code, CloseCode::Normal);
        assert!(close.reason.is_empty());

        let mut replay = test_socket(address, "/replay").await;
        let query = format!(r#"{{"receipt_id":"{}"}}"#, receipt_identity.encoded());
        replay
            .send(ClientMessage::Binary(
                client_control(0x0013, query.as_bytes()).into(),
            ))
            .await
            .expect("receipt QUERY");
        let replayed = replay
            .next()
            .await
            .expect("replay response")
            .expect("replay frame");
        let ClientMessage::Binary(replayed) = replayed else {
            panic!("expected receipt replay frame");
        };
        let replayed = String::from_utf8_lossy(&replayed);
        assert!(replayed.contains(&receipt_identity.encoded()));
        assert!(replayed.contains("\"replayed\":true"));
        let close = replay
            .next()
            .await
            .expect("replay close")
            .expect("replay close frame");
        let ClientMessage::Close(Some(close)) = close else {
            panic!("expected normal close after replay");
        };
        assert_eq!(close.code, CloseCode::Normal);
        assert!(close.reason.is_empty());

        post_server.abort();
        drop(credentials);
        drop(post_credentials);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(start_paused = true)]
    async fn hosted_backpressured_terminal_frame_reserves_the_required_close() {
        for (path, code, expected) in [
            ("/pre", close_code::POLICY, CloseCode::Policy),
            ("/post", close_code::NORMAL, CloseCode::Normal),
        ] {
            let app = axum::Router::new()
                .route(path, axum::routing::get(backpressured_terminal_upgrade))
                .with_state(code);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("backpressure listener");
            let address = listener.local_addr().expect("listener address");
            let server = tokio::spawn(async move {
                axum::serve(listener, app)
                    .await
                    .expect("backpressure server");
            });
            let mut socket = test_socket(address, path).await;
            tokio::task::yield_now().await;

            // Do not poll the client until the atomic reservation has run. The bounded sink must
            // reject the data frame while retaining capacity for the close.
            tokio::task::yield_now().await;
            tokio::time::resume();
            let terminal = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
                .await
                .expect("bounded hosted terminal read")
                .expect("hosted close item")
                .expect("hosted close frame");
            let ClientMessage::Close(Some(close)) = terminal else {
                panic!("the abandoned terminal frame must not precede the reserved close: {terminal:?}");
            };
            assert_eq!(close.code, expected);
            assert!(close.reason.is_empty());
            server.abort();
            tokio::time::pause();
        }
    }
}
