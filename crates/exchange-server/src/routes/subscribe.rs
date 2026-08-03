//! One authenticated agent WebSocket multiplexing live channel subscriptions.

use std::collections::BTreeMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::Extension;
use exchange_host::{admit_inbound, ChannelId, Principal};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{Access, Module, Route};
use crate::channel::ChannelEvent;
use crate::state::AppState;

pub(super) const MODULE: Module = Module {
    name: "subscribe",
    routes: &[Route {
        path: "/api/subscribe",
        access: Access::Principal,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    get(upgrade)
}

async fn upgrade(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if state.channels().is_none() || state.invoker().is_none() {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({"error": "this host has no connector-channel runtime configured"})),
        )
            .into_response();
    }
    upgrade.on_upgrade(move |socket| serve(socket, state, principal))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Subscribe {
        request_id: String,
        channel_id: String,
    },
    Unsubscribe {
        request_id: String,
        channel_id: String,
    },
}

async fn serve(mut socket: WebSocket, state: AppState, principal: Principal) {
    let Some(supervisor) = state.channels().cloned() else {
        return;
    };
    let Some(invoker) = state.invoker().cloned() else {
        return;
    };
    let (outgoing, mut events) = mpsc::channel::<ChannelEvent>(32);
    let slow = CancellationToken::new();
    let mut subscriptions: BTreeMap<ChannelId, CancellationToken> = BTreeMap::new();

    loop {
        tokio::select! {
            _ = slow.cancelled() => break,
            event = events.recv() => {
                let Some(event) = event else { break; };
                let document = json!({"type": "event", "event": event});
                if socket.send(Message::Text(document.to_string().into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(Message::Text(text))) = incoming else { break; };
                let command = match serde_json::from_str::<Command>(&text) {
                    Ok(command) => command,
                    Err(_) => {
                        if send(&mut socket, json!({
                            "type": "refusal",
                            "request_id": Value::Null,
                            "code": "invalid_command"
                        })).await.is_err() { break; }
                        continue;
                    }
                };
                let response = match command {
                    Command::Subscribe { request_id, channel_id } => {
                        subscribe(
                            &supervisor,
                            &invoker,
                            &principal,
                            &mut subscriptions,
                            outgoing.clone(),
                            slow.clone(),
                            request_id,
                            channel_id,
                        )
                    }
                    Command::Unsubscribe { request_id, channel_id } => {
                        unsubscribe(&mut subscriptions, request_id, channel_id)
                    }
                };
                if send(&mut socket, response).await.is_err() { break; }
            }
        }
    }
    for (_, cancel) in subscriptions {
        cancel.cancel();
    }
}

#[allow(clippy::too_many_arguments)]
fn subscribe(
    supervisor: &std::sync::Arc<crate::channel::ChannelSupervisor>,
    invoker: &std::sync::Arc<exchange_host::Invoker>,
    principal: &Principal,
    subscriptions: &mut BTreeMap<ChannelId, CancellationToken>,
    outgoing: mpsc::Sender<ChannelEvent>,
    slow: CancellationToken,
    request_id: String,
    channel_id: String,
) -> Value {
    let Ok(id) = ChannelId::new(channel_id) else {
        return refusal(request_id, "no_such_channel");
    };
    let Some(record) = supervisor.store().get(principal.tenant(), &id) else {
        return refusal(request_id, "no_such_channel");
    };
    if admit_inbound(
        principal,
        record.connector(),
        record.binding(),
        record.events(),
        &invoker.grants().held(principal.tenant()),
    )
    .is_err()
    {
        return refusal(request_id, "not_granted");
    }
    let Some(mut subscription) = supervisor.subscribe(principal.tenant(), &id) else {
        return refusal(request_id, "no_such_channel");
    };
    if let Some(previous) = subscriptions.remove(&id) {
        previous.cancel();
    }
    let cancel = CancellationToken::new();
    subscriptions.insert(id.clone(), cancel.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                event = subscription.receiver.recv() => {
                    let Some(event) = event else { return; };
                    if outgoing.try_send(event).is_err() {
                        slow.cancel();
                        return;
                    }
                }
            }
        }
    });
    json!({"type": "ack", "request_id": request_id, "channel_id": id.as_str(), "subscribed": true})
}

fn unsubscribe(
    subscriptions: &mut BTreeMap<ChannelId, CancellationToken>,
    request_id: String,
    channel_id: String,
) -> Value {
    let Ok(id) = ChannelId::new(channel_id) else {
        return refusal(request_id, "no_such_subscription");
    };
    let Some(cancel) = subscriptions.remove(&id) else {
        return refusal(request_id, "no_such_subscription");
    };
    cancel.cancel();
    json!({"type": "ack", "request_id": request_id, "channel_id": id.as_str(), "subscribed": false})
}

fn refusal(request_id: String, code: &str) -> Value {
    json!({"type": "refusal", "request_id": request_id, "code": code})
}

async fn send(socket: &mut WebSocket, document: Value) -> Result<(), ()> {
    socket
        .send(Message::Text(document.to_string().into()))
        .await
        .map_err(|_| ())
}
