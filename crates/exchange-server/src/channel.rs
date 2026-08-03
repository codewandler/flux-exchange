//! Persistent connector-channel supervision and live fan-out.
//!
//! The supervisor owns lifetimes and queues, not protocol details. A [`ChannelRunner`] supplied by
//! the composition binds the released connector plan to Flux's selected guarded substrate.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use exchange_host::{
    async_trait, ChannelId, ChannelRecord, Channels, ConnectorChannelPlanner, Deployment,
    PreparedChannelPlan, Tenant,
};
use flux_channels::{
    Channel as _, ChannelContext, ConnectorChannel, ConnectorSocketPlan, ConnectorValueSelector,
    ConnectorValueSource, Deliverer,
};
use flux_system::net::PrivateNetAllow;
use flux_system::port::ExecutionSystem;
use flux_system::websocket::WebSocketConnect;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Catalogue view needed before a channel record may be written.
pub trait ChannelDeclarations: Send + Sync {
    /// The closed event set for a connector binding, or `None` when either name is undeclared.
    fn events(&self, connector: &str, binding: &str) -> Option<BTreeSet<String>>;
}

/// Channel/event declarations from the same compiled connector catalogue invocation uses.
pub struct CatalogueChannelDeclarations;

impl ChannelDeclarations for CatalogueChannelDeclarations {
    fn events(&self, connector: &str, binding: &str) -> Option<BTreeSet<String>> {
        connector_catalog::provider(connector_catalog::ProviderKey::id(connector))?
            .channel(binding)
            .map(|channel| {
                channel
                    .events
                    .iter()
                    .map(|event| (*event).to_owned())
                    .collect()
            })
    }
}

/// Placement selected by operator configuration, never by an API request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelPlacement {
    /// Direct execution in a single-tenant/local profile.
    Local,
    /// An operator-provisioned endpoint reference.
    EndpointReference(String),
    /// A trusted selected remote execution system.
    SelectedRemote(String),
}

/// Operator-owned placement decision. It runs before the runner, which is the only port permitted
/// to resolve credentials.
pub trait ChannelPlacementResolver: Send + Sync {
    /// Resolve admissible placement for stored channel identity.
    fn resolve(&self, record: &ChannelRecord) -> Result<ChannelPlacement, ChannelRunError>;
}

/// The built-in placement policy: local execution exists only in the explicitly single-tenant
/// composition. A multi-tenant process needs an operator-provisioned remote selector, which this
/// binary does not invent from request data or ambient defaults.
pub struct DeploymentChannelPlacement {
    deployment: Deployment,
}

impl DeploymentChannelPlacement {
    /// Bind the startup deployment class selected before any request is served.
    pub const fn new(deployment: Deployment) -> Self {
        Self { deployment }
    }
}

impl ChannelPlacementResolver for DeploymentChannelPlacement {
    fn resolve(&self, _: &ChannelRecord) -> Result<ChannelPlacement, ChannelRunError> {
        match self.deployment {
            Deployment::SingleTenant => Ok(ChannelPlacement::Local),
            Deployment::MultiTenant => Err(ChannelRunError::NoPlacement),
        }
    }
}

/// One declared event received from a vendor connection.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelEvent {
    /// Connector catalogue id.
    pub connector: String,
    /// Declared binding name.
    pub binding: String,
    /// Declared local event name.
    pub event: String,
    /// Milliseconds since the Unix epoch at receipt.
    pub received_at_ms: u64,
    /// Full typed vendor payload.
    pub payload: Value,
}

/// Event destination a runner calls without knowing about subscribers.
pub trait ChannelEventSink: Send + Sync {
    /// Deliver one event without blocking the vendor read loop.
    fn deliver(&self, event: ChannelEvent);
}

/// Protocol runner bound by the composing binary after compatible Flux/connector releases exist.
#[async_trait]
pub trait ChannelRunner: Send + Sync {
    /// Run until cancellation, a reconnectable failure or a terminal failure.
    async fn run(
        &self,
        record: ChannelRecord,
        placement: ChannelPlacement,
        sink: Arc<dyn ChannelEventSink>,
        cancel: CancellationToken,
    ) -> Result<(), ChannelRunError>;
}

/// Production binding from connector-pack's zero-I/O plan to Flux's guarded WebSocket channel.
/// It is constructed once by the composition; neither the request body nor a persisted channel can
/// select the execution substrate or its private-network policy.
pub struct GeneratedChannelRunner {
    planner: Arc<ConnectorChannelPlanner>,
    execution_system: Arc<dyn ExecutionSystem>,
    private_network: PrivateNetAllow,
}

impl GeneratedChannelRunner {
    /// Bind an operator-selected execution system and egress posture.
    pub fn new(
        planner: Arc<ConnectorChannelPlanner>,
        execution_system: Arc<dyn ExecutionSystem>,
        private_network: PrivateNetAllow,
    ) -> Self {
        Self {
            planner,
            execution_system,
            private_network,
        }
    }
}

#[async_trait]
impl ChannelRunner for GeneratedChannelRunner {
    async fn run(
        &self,
        record: ChannelRecord,
        placement: ChannelPlacement,
        sink: Arc<dyn ChannelEventSink>,
        cancel: CancellationToken,
    ) -> Result<(), ChannelRunError> {
        if placement != ChannelPlacement::Local {
            return Err(ChannelRunError::NoPlacement);
        }

        // Load-bearing order: `ChannelSupervisor` resolved placement before calling this method;
        // only now may connector-pack consult tenant-bound configuration and credentials.
        let prepared = self
            .planner
            .prepare(&record)
            .await
            .map_err(|_| ChannelRunError::Terminal)?;
        let plan = socket_plan(prepared, record.events(), self.private_network.clone())?;
        let channel = ConnectorChannel::from_socket_plan(record.binding(), plan)
            .map_err(|_| ChannelRunError::Terminal)?;
        let deliverer: Arc<dyn Deliverer> = Arc::new(ExchangeDeliverer {
            connector: record.connector().to_owned(),
            binding: record.binding().to_owned(),
            sink,
        });

        channel
            .start_with_context(ChannelContext {
                deliverer,
                cancel,
                execution_system: Arc::clone(&self.execution_system),
            })
            .await
            // The Flux connector adapter owns transient reconnects and returns only cancellation or
            // a terminal protocol/configuration refusal.
            .map_err(|_| ChannelRunError::Terminal)
    }
}

fn socket_plan(
    prepared: PreparedChannelPlan,
    selected: &BTreeSet<String>,
    private_network: PrivateNetAllow,
) -> Result<ConnectorSocketPlan, ChannelRunError> {
    let mut connect = WebSocketConnect::new(prepared.url.expose_secret().to_owned());
    connect.headers = prepared
        .headers
        .into_iter()
        .map(|(name, value)| (name, value.expose_secret().to_owned()))
        .collect();
    connect.subprotocols = prepared
        .subprotocols
        .into_iter()
        .map(str::to_owned)
        .collect();

    let wire_events = prepared
        .wire_events
        .into_iter()
        .filter(|(_, local)| selected.contains(*local))
        .map(|(wire, local)| (wire.to_owned(), local.to_owned()))
        .collect();
    let discriminator = prepared.discriminator.map(selector).transpose()?;
    let delivery_id = prepared.delivery_id.map(selector).transpose()?;
    let payload = prepared
        .payload
        .iter()
        .map(|pair| (pair.name.to_owned(), pair.value.to_owned()))
        .collect();

    Ok(ConnectorSocketPlan {
        connect,
        private_network,
        wire_events,
        discriminator,
        delivery_id,
        payload,
        payload_root: prepared.payload_root,
    })
}

fn selector(
    selector: connector_catalog::Selector,
) -> Result<ConnectorValueSelector, ChannelRunError> {
    let source = match selector.source {
        "header" => ConnectorValueSource::Header,
        "body" => ConnectorValueSource::Body,
        _ => return Err(ChannelRunError::Terminal),
    };
    Ok(ConnectorValueSelector {
        source,
        name: selector.name.to_owned(),
    })
}

struct ExchangeDeliverer {
    connector: String,
    binding: String,
    sink: Arc<dyn ChannelEventSink>,
}

#[async_trait]
impl Deliverer for ExchangeDeliverer {
    async fn deliver(
        &self,
        label: &str,
        payload: Value,
    ) -> anyhow::Result<Vec<flux_app::JourneyRun>> {
        let event = label
            .strip_prefix(&self.binding)
            .and_then(|rest| rest.strip_prefix('.'))
            .filter(|event| !event.is_empty())
            .ok_or_else(|| anyhow::anyhow!("connector channel produced an undeclared label"))?;
        self.sink.deliver(ChannelEvent {
            connector: self.connector.clone(),
            binding: self.binding.clone(),
            event: event.to_owned(),
            received_at_ms: now_ms(),
            payload,
        });
        Ok(Vec::new())
    }
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

/// Runner outcome classification. Only transient failures reconnect.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelRunError {
    /// Network/5xx failure safe to reconnect.
    #[error("channel transport is temporarily unavailable")]
    Transient,
    /// Declaration/config/auth/4xx failure that requires operator action.
    #[error("channel configuration was refused")]
    Terminal,
    /// No operator-admissible execution placement exists.
    #[error("no admissible channel placement is configured")]
    NoPlacement,
}

/// Redaction-safe lifecycle state exposed to the operator console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelStatus {
    /// Persisted and waiting for its task to enter the runner.
    Starting,
    /// The guarded runner task is active (including its own transport reconnect loop).
    Running,
    /// Exchange's outer runner reported a transient failure and is backing off.
    Retrying,
    /// Placement, declaration, configuration or authentication needs operator action.
    Refused,
    /// Explicitly stopped or cleanly ended.
    Stopped,
}

type SubscriberSet = BTreeMap<u64, mpsc::Sender<ChannelEvent>>;
type Subscribers = Arc<Mutex<BTreeMap<ChannelId, SubscriberSet>>>;

struct Fanout {
    record: ChannelRecord,
    subscribers: Subscribers,
    dropped: Arc<AtomicU64>,
}

impl ChannelEventSink for Fanout {
    fn deliver(&self, event: ChannelEvent) {
        if event.connector != self.record.connector()
            || event.binding != self.record.binding()
            || !self.record.events().contains(&event.event)
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut all = self
            .subscribers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(subscribers) = all.get_mut(self.record.id()) {
            subscribers.retain(|_, subscriber| match subscriber.try_send(event.clone()) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                    false
                }
            });
        }
    }
}

/// One live subscriber queue. Dropping it cannot stop the vendor channel.
pub struct ChannelSubscription {
    /// Receiver bounded to 32 events.
    pub receiver: mpsc::Receiver<ChannelEvent>,
}

/// Restores and supervises one runner per persistent channel.
pub struct ChannelSupervisor {
    store: Arc<dyn Channels>,
    declarations: Arc<dyn ChannelDeclarations>,
    placements: Arc<dyn ChannelPlacementResolver>,
    runner: Arc<dyn ChannelRunner>,
    active: Mutex<BTreeMap<ChannelId, CancellationToken>>,
    statuses: Mutex<BTreeMap<ChannelId, ChannelStatus>>,
    subscribers: Subscribers,
    next_subscriber: AtomicU64,
    next_channel: AtomicU64,
    dropped: Arc<AtomicU64>,
}

impl ChannelSupervisor {
    /// Bind the four operator-owned ports. No default runner or placement is invented.
    pub fn new(
        store: Arc<dyn Channels>,
        declarations: Arc<dyn ChannelDeclarations>,
        placements: Arc<dyn ChannelPlacementResolver>,
        runner: Arc<dyn ChannelRunner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            declarations,
            placements,
            runner,
            active: Mutex::new(BTreeMap::new()),
            statuses: Mutex::new(BTreeMap::new()),
            subscribers: Arc::new(Mutex::new(BTreeMap::new())),
            next_subscriber: AtomicU64::new(1),
            next_channel: AtomicU64::new(1),
            dropped: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Validate a declared binding/event subset without reading placement or credentials.
    pub fn validates(&self, connector: &str, binding: &str, events: &BTreeSet<String>) -> bool {
        !events.is_empty()
            && self
                .declarations
                .events(connector, binding)
                .is_some_and(|declared| events.is_subset(&declared))
    }

    /// Persistent store behind this supervisor.
    pub fn store(&self) -> &Arc<dyn Channels> {
        &self.store
    }

    /// Mint an opaque process/time-scoped id; no caller input contributes to it.
    pub fn mint_id(&self) -> ChannelId {
        let sequence = self.next_channel.fetch_add(1, Ordering::Relaxed);
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        ChannelId::new(format!("ch_{epoch:x}_{sequence:x}"))
            .expect("host-minted channel ids satisfy their fixed grammar")
    }

    /// Restore every persisted channel independently of subscribers.
    pub fn restore(self: &Arc<Self>) {
        for record in self.store.all() {
            self.start(record);
        }
    }

    /// Start or restart one stored channel.
    pub fn start(self: &Arc<Self>, record: ChannelRecord) {
        self.stop(record.id());
        self.set_status(record.id(), ChannelStatus::Starting);
        let cancel = CancellationToken::new();
        self.active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(record.id().clone(), cancel.clone());
        let supervisor = Arc::clone(self);
        tokio::spawn(async move { supervisor.drive(record, cancel).await });
    }

    /// Restart after credential or connection-setting rotation.
    pub fn restart(self: &Arc<Self>, tenant: &Tenant, connector: &str) {
        for record in self.store.held(tenant) {
            if record.connector() == connector {
                self.start(record);
            }
        }
    }

    /// Stop a channel without deleting its persistent record.
    pub fn stop(&self, id: &ChannelId) {
        if let Some(cancel) = self
            .active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(id)
        {
            cancel.cancel();
        }
        self.set_status(id, ChannelStatus::Stopped);
    }

    /// Forget lifecycle state after the persistent record has been removed.
    pub fn forget(&self, id: &ChannelId) {
        self.statuses
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(id);
    }

    /// Current redaction-safe lifecycle state for an operator view.
    pub fn status(&self, id: &ChannelId) -> ChannelStatus {
        self.statuses
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(id)
            .copied()
            .unwrap_or(ChannelStatus::Stopped)
    }

    /// Subscribe to a tenant-owned id. Unknown and cross-tenant ids are the same refusal.
    pub fn subscribe(&self, tenant: &Tenant, id: &ChannelId) -> Option<ChannelSubscription> {
        self.store.get(tenant, id)?;
        let (sender, receiver) = mpsc::channel(32);
        let subscriber = self.next_subscriber.fetch_add(1, Ordering::Relaxed);
        self.subscribers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .entry(id.clone())
            .or_default()
            .insert(subscriber, sender);
        Some(ChannelSubscription { receiver })
    }

    /// Events dropped for invalid declarations or slow subscribers.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    async fn drive(self: Arc<Self>, record: ChannelRecord, cancel: CancellationToken) {
        let mut backoff = 1u64;
        loop {
            if cancel.is_cancelled() {
                self.set_status(record.id(), ChannelStatus::Stopped);
                return;
            }
            // Load-bearing order: placement refuses before the runner can read a credential.
            let placement = match self.placements.resolve(&record) {
                Ok(placement) => placement,
                Err(_) => {
                    self.set_status(record.id(), ChannelStatus::Refused);
                    return;
                }
            };
            let sink: Arc<dyn ChannelEventSink> = Arc::new(Fanout {
                record: record.clone(),
                subscribers: Arc::clone(&self.subscribers),
                dropped: Arc::clone(&self.dropped),
            });
            self.set_status(record.id(), ChannelStatus::Running);
            match self
                .runner
                .run(record.clone(), placement, sink, cancel.clone())
                .await
            {
                Ok(()) => {
                    self.set_status(record.id(), ChannelStatus::Stopped);
                    return;
                }
                Err(ChannelRunError::Terminal | ChannelRunError::NoPlacement) => {
                    self.set_status(record.id(), ChannelStatus::Refused);
                    return;
                }
                Err(ChannelRunError::Transient) => {
                    self.set_status(record.id(), ChannelStatus::Retrying);
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    self.set_status(record.id(), ChannelStatus::Stopped);
                    return;
                },
                _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
            }
            backoff = (backoff * 2).min(30);
        }
    }

    fn set_status(&self, id: &ChannelId, status: ChannelStatus) {
        self.statuses
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(id.clone(), status);
    }
}

/// Timestamp one received event without carrying a clock dependency into the runner port.
pub fn received_now(
    connector: String,
    binding: String,
    event: String,
    payload: Value,
) -> ChannelEvent {
    let received_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    ChannelEvent {
        connector,
        binding,
        event,
        received_at_ms,
        payload,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use exchange_host::{
        CredentialRef, CredentialScope, MemoryChannels, MemoryConfig, Secret, SecretStore,
        StoreError,
    };

    use super::*;

    struct OneSecret {
        value: Secret,
    }

    #[async_trait]
    impl SecretStore for OneSecret {
        async fn get(&self, _: &CredentialRef) -> Result<Secret, StoreError> {
            Ok(self.value.clone())
        }

        async fn put(&self, _: &CredentialRef, _: &Secret) -> Result<(), StoreError> {
            Ok(())
        }

        async fn delete(&self, _: &CredentialRef) -> Result<(), StoreError> {
            Ok(())
        }

        async fn references(
            &self,
            scope: &CredentialScope,
        ) -> Result<Vec<CredentialRef>, StoreError> {
            Ok(vec![CredentialRef::new(
                scope.tenant(),
                scope.authority(),
                "default",
                "password",
            )
            .expect("declared Asterisk reference")])
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<ChannelEvent>>,
    }

    impl ChannelEventSink for RecordingSink {
        fn deliver(&self, event: ChannelEvent) {
            self.events
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(event);
        }
    }

    struct Declarations;

    impl ChannelDeclarations for Declarations {
        fn events(&self, connector: &str, binding: &str) -> Option<BTreeSet<String>> {
            (connector == "asterisk" && binding == "ari-events").then(|| {
                ["channel-created".to_owned(), "channel-destroyed".to_owned()]
                    .into_iter()
                    .collect()
            })
        }
    }

    struct Placement {
        result: Result<ChannelPlacement, ChannelRunError>,
        calls: AtomicU64,
    }

    impl ChannelPlacementResolver for Placement {
        fn resolve(&self, _: &ChannelRecord) -> Result<ChannelPlacement, ChannelRunError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.result.clone()
        }
    }

    struct Runner {
        outcomes: Mutex<VecDeque<Result<(), ChannelRunError>>>,
        calls: AtomicU64,
    }

    #[async_trait]
    impl ChannelRunner for Runner {
        async fn run(
            &self,
            _: ChannelRecord,
            _: ChannelPlacement,
            _: Arc<dyn ChannelEventSink>,
            cancel: CancellationToken,
        ) -> Result<(), ChannelRunError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let outcome = self
                .outcomes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .pop_front();
            match outcome {
                Some(outcome) => outcome,
                None => {
                    cancel.cancelled().await;
                    Ok(())
                }
            }
        }
    }

    fn record(id: &str) -> ChannelRecord {
        ChannelRecord::new(
            ChannelId::new(id).expect("id"),
            Tenant::new("alpha").expect("tenant"),
            "asterisk",
            exchange_host::InstanceId::parse("11111111-1111-4111-8111-111111111111")
                .expect("instance"),
            "ari-events",
            ["channel-created".to_owned()].into_iter().collect(),
        )
        .expect("record")
    }

    fn supervisor(
        store: Arc<dyn Channels>,
        placement: Arc<Placement>,
        runner: Arc<Runner>,
    ) -> Arc<ChannelSupervisor> {
        ChannelSupervisor::new(store, Arc::new(Declarations), placement, runner)
    }

    async fn wait_for_calls(calls: &AtomicU64, expected: u64) {
        for _ in 0..250 {
            if calls.load(Ordering::Relaxed) >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "runner made {} calls, expected {expected}",
            calls.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn fanout_is_live_only_closed_to_declared_events_and_isolates_a_slow_subscriber() {
        let record = record("ch_1");
        let subscribers: Subscribers = Arc::new(Mutex::new(BTreeMap::new()));
        let (slow, mut slow_rx) = mpsc::channel(32);
        let (fast, mut fast_rx) = mpsc::channel(32);
        subscribers.lock().expect("subscribers").insert(
            record.id().clone(),
            [(1, slow), (2, fast)].into_iter().collect(),
        );
        let dropped = Arc::new(AtomicU64::new(0));
        let fanout = Fanout {
            record: record.clone(),
            subscribers: Arc::clone(&subscribers),
            dropped: Arc::clone(&dropped),
        };

        for sequence in 0..33 {
            fanout.deliver(ChannelEvent {
                connector: "asterisk".into(),
                binding: "ari-events".into(),
                event: "channel-created".into(),
                received_at_ms: sequence,
                payload: serde_json::json!({"sequence": sequence}),
            });
            assert_eq!(
                fast_rx
                    .try_recv()
                    .expect("fast subscriber receives live event")
                    .received_at_ms,
                sequence
            );
        }
        assert_eq!(
            slow_rx
                .try_recv()
                .expect("slow queue retained earlier event")
                .received_at_ms,
            0
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert_eq!(
            subscribers
                .lock()
                .expect("subscribers")
                .get(record.id())
                .expect("channel")
                .len(),
            1
        );

        fanout.deliver(ChannelEvent {
            connector: "asterisk".into(),
            binding: "ari-events".into(),
            event: "vendor-invented".into(),
            received_at_ms: 34,
            payload: serde_json::json!({"private": "not logged"}),
        });
        assert_eq!(dropped.load(Ordering::Relaxed), 2);

        let (_late, mut late_rx) = mpsc::channel::<ChannelEvent>(1);
        assert!(
            late_rx.try_recv().is_err(),
            "fanout retains no replay cursor or payload"
        );
    }

    #[tokio::test]
    async fn transient_failures_reconnect_and_terminal_failures_do_not() {
        let store = Arc::new(MemoryChannels::default());
        let record = record("ch_1");
        store.set(record.clone()).expect("store");
        let placement = Arc::new(Placement {
            result: Ok(ChannelPlacement::Local),
            calls: AtomicU64::new(0),
        });
        let runner = Arc::new(Runner {
            outcomes: Mutex::new(
                [
                    Err(ChannelRunError::Transient),
                    Err(ChannelRunError::Terminal),
                ]
                .into_iter()
                .collect(),
            ),
            calls: AtomicU64::new(0),
        });
        let supervisor = supervisor(store, placement, Arc::clone(&runner));
        supervisor.start(record);
        wait_for_calls(&runner.calls, 2).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(runner.calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn restoration_runs_without_subscribers_and_placement_refuses_before_the_runner() {
        let store = Arc::new(MemoryChannels::default());
        store.set(record("ch_restore")).expect("store");
        let placement = Arc::new(Placement {
            result: Err(ChannelRunError::NoPlacement),
            calls: AtomicU64::new(0),
        });
        let runner = Arc::new(Runner {
            outcomes: Mutex::new(VecDeque::new()),
            calls: AtomicU64::new(0),
        });
        let supervisor = supervisor(store, Arc::clone(&placement), Arc::clone(&runner));
        supervisor.restore();
        wait_for_calls(&placement.calls, 1).await;
        assert_eq!(runner.calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            supervisor.status(&ChannelId::new("ch_restore").expect("id")),
            ChannelStatus::Refused
        );
    }

    #[tokio::test]
    async fn generated_plan_reaches_flux_channel_and_routes_only_the_selected_event() {
        const PASSWORD: &str = "SENTINEL-NOT-A-REAL-ARI-PASSWORD";
        let planner = ConnectorChannelPlanner::new(
            Arc::new(OneSecret {
                value: Secret::new(PASSWORD),
            }),
            Arc::new(
                MemoryConfig::new()
                    .with_endpoint("alpha", "asterisk", "default", "host", "pbx.example.com")
                    .with_username("alpha", "asterisk", "default", "asterisk.password", "flux")
                    .with_channel_query(
                        "alpha",
                        "asterisk",
                        "default",
                        "ari-events",
                        "app",
                        "voice-app",
                    ),
            ),
        );
        let record = record("ch_generated");
        let prepared = planner.prepare(&record).await.expect("generated plan");
        let debug = format!("{prepared:?}");
        assert!(!debug.contains(PASSWORD), "{debug}");
        assert!(!debug.contains("voice-app"), "{debug}");

        let plan = socket_plan(prepared, record.events(), PrivateNetAllow::None)
            .expect("Flux socket plan");
        assert_eq!(
            plan.wire_events,
            [("ChannelCreated".to_owned(), "channel-created".to_owned())]
                .into_iter()
                .collect(),
            "the tenant's selected subset, not all 45 vendor events, reaches the live channel"
        );
        ConnectorChannel::from_socket_plan(record.binding(), plan)
            .expect("the released Flux channel accepts connector-pack's plan");

        let sink = Arc::new(RecordingSink::default());
        let deliverer = ExchangeDeliverer {
            connector: record.connector().to_owned(),
            binding: record.binding().to_owned(),
            sink: sink.clone(),
        };
        deliverer
            .deliver(
                "ari-events.channel-created",
                serde_json::json!({"type": "ChannelCreated", "channel": {"id": "42"}}),
            )
            .await
            .expect("declared label");
        let events = sink.events.lock().expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].connector, "asterisk");
        assert_eq!(events[0].binding, "ari-events");
        assert_eq!(events[0].event, "channel-created");
    }

    #[test]
    fn declaration_validation_is_a_closed_subset() {
        let store = Arc::new(MemoryChannels::default());
        let placement = Arc::new(Placement {
            result: Ok(ChannelPlacement::Local),
            calls: AtomicU64::new(0),
        });
        let runner = Arc::new(Runner {
            outcomes: Mutex::new(VecDeque::new()),
            calls: AtomicU64::new(0),
        });
        let supervisor = supervisor(store, placement, runner);
        assert!(supervisor.validates(
            "asterisk",
            "ari-events",
            &["channel-created".to_owned()].into_iter().collect()
        ));
        assert!(!supervisor.validates(
            "asterisk",
            "ari-events",
            &["vendor-invented".to_owned()].into_iter().collect()
        ));
        assert!(!supervisor.validates("asterisk", "missing", &BTreeSet::new()));
    }
}
