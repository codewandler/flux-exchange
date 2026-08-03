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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use exchange_host::{
        async_trait, ChannelRecord, Channels, CredentialRef, Deployment, Grant, Grants,
        InboundGrant, MemoryChannels, MemoryConfig, Secret, SecretStore, Selector, StoreError,
        Tenant,
    };
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{header, HeaderValue};
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    use super::*;
    use crate::channel::{
        ChannelDeclarations, ChannelEventSink, ChannelPlacement, ChannelPlacementResolver,
        ChannelRunError, ChannelRunner, ChannelSupervisor,
    };
    use crate::dev_identity::DevIdentity;

    struct Declarations;

    impl ChannelDeclarations for Declarations {
        fn events(&self, connector: &str, binding: &str) -> Option<BTreeSet<String>> {
            (connector == "asterisk" && binding == "ari-events")
                .then(|| ["channel-created".to_owned()].into_iter().collect())
        }
    }

    struct LocalPlacement;

    impl ChannelPlacementResolver for LocalPlacement {
        fn resolve(&self, _: &ChannelRecord) -> Result<ChannelPlacement, ChannelRunError> {
            Ok(ChannelPlacement::Local)
        }
    }

    #[derive(Default)]
    struct CapturingRunner {
        sink: Mutex<Option<Arc<dyn ChannelEventSink>>>,
    }

    #[async_trait]
    impl ChannelRunner for CapturingRunner {
        async fn run(
            &self,
            _: ChannelRecord,
            _: ChannelPlacement,
            sink: Arc<dyn ChannelEventSink>,
            cancel: CancellationToken,
        ) -> Result<(), ChannelRunError> {
            *self.sink.lock().expect("runner sink") = Some(sink);
            cancel.cancelled().await;
            Ok(())
        }
    }

    struct NoCredentials;

    #[async_trait]
    impl SecretStore for NoCredentials {
        async fn get(&self, _: &CredentialRef) -> Result<Secret, StoreError> {
            unreachable!("subscribing reads no credential")
        }

        async fn put(&self, _: &CredentialRef, _: &Secret) -> Result<(), StoreError> {
            unreachable!("subscribing writes no credential")
        }

        async fn delete(&self, _: &CredentialRef) -> Result<(), StoreError> {
            unreachable!("subscribing deletes no credential")
        }
    }

    struct HeldGrants(Vec<Grant>);

    impl Grants for HeldGrants {
        fn held(&self, _: &Tenant) -> Vec<Grant> {
            self.0.clone()
        }

        fn set(&self, _: &Tenant, _: &[Grant]) -> Result<(), exchange_host::GrantRefusal> {
            unreachable!("subscribing edits no grant")
        }
    }

    async fn wait_for_sink(runner: &CapturingRunner) -> Arc<dyn ChannelEventSink> {
        for _ in 0..250 {
            if let Some(sink) = runner.sink.lock().expect("runner sink").clone() {
                return sink;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("channel runner did not start")
    }

    async fn receive_json(
        socket: &mut (impl StreamExt<Item = Result<ClientMessage, tokio_tungstenite::tungstenite::Error>>
                  + Unpin),
    ) -> Value {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("websocket response before timeout")
            .expect("websocket remains open")
            .expect("valid websocket message");
        serde_json::from_str(message.to_text().expect("text response")).expect("JSON response")
    }

    #[tokio::test]
    async fn authenticated_get_multiplexes_request_correlated_subscriptions_and_live_events() {
        let tenant = Tenant::new("acme").expect("tenant");
        let id = ChannelId::new("ch_live").expect("id");
        let record = ChannelRecord::new(
            id.clone(),
            tenant,
            "asterisk",
            "asterisk",
            "ari-events",
            ["channel-created".to_owned()].into_iter().collect(),
        )
        .expect("record");
        let store = Arc::new(MemoryChannels::default());
        store.set(record.clone()).expect("stored channel");
        let runner = Arc::new(CapturingRunner::default());
        let supervisor = ChannelSupervisor::new(
            store,
            Arc::new(Declarations),
            Arc::new(LocalPlacement),
            runner.clone(),
        );
        supervisor.start(record);
        let sink = wait_for_sink(&runner).await;

        let mut grant = Grant::for_connector("asterisk", Selector::any());
        grant.inbound.push(InboundGrant {
            connector: "asterisk".into(),
            binding: "ari-events".into(),
            events: ["channel-created".to_owned()].into_iter().collect(),
        });
        let invoker = Arc::new(
            crate::execution::invoker(
                Deployment::SingleTenant,
                Arc::new(NoCredentials),
                Arc::new(MemoryConfig::new()),
                Arc::new(HeldGrants(vec![grant])),
            )
            .expect("invoker"),
        );
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster("agent:bot@acme").expect("development identity"),
        ))
        .with_invoker(invoker)
        .with_channels(supervisor.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            axum::serve(listener, super::super::app(state))
                .await
                .expect("test server")
        });
        let mut request = format!("ws://{address}/api/subscribe")
            .into_client_request()
            .expect("websocket request");
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer bot"),
        );
        let (mut socket, response) = tokio_tungstenite::connect_async(request)
            .await
            .expect("authenticated websocket upgrade");
        assert_eq!(response.status(), 101);

        socket
            .send(ClientMessage::Text(
                json!({"action":"subscribe", "request_id":"subscribe-1", "channel_id":"ch_live"})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("subscribe command");
        assert_eq!(
            receive_json(&mut socket).await,
            json!({"type":"ack", "request_id":"subscribe-1", "channel_id":"ch_live", "subscribed":true})
        );

        sink.deliver(ChannelEvent {
            connector: "asterisk".into(),
            binding: "ari-events".into(),
            event: "channel-created".into(),
            received_at_ms: 42,
            payload: json!({"channel":{"id":"vendor-7"}}),
        });
        assert_eq!(
            receive_json(&mut socket).await,
            json!({
                "type":"event",
                "event": {
                    "connector":"asterisk",
                    "binding":"ari-events",
                    "event":"channel-created",
                    "received_at_ms":42,
                    "payload":{"channel":{"id":"vendor-7"}}
                }
            })
        );

        socket
            .send(ClientMessage::Text(
                json!({"action":"unsubscribe", "request_id":"unsubscribe-1", "channel_id":"ch_live"})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("unsubscribe command");
        assert_eq!(
            receive_json(&mut socket).await,
            json!({"type":"ack", "request_id":"unsubscribe-1", "channel_id":"ch_live", "subscribed":false})
        );

        socket.close(None).await.expect("close websocket");
        supervisor.stop(&id);
        server.abort();
    }
}
