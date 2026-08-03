//! Operator-owned persistent connector channels.

use std::collections::BTreeSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put, MethodRouter};
use axum::{Extension, Json};
use exchange_host::{ChannelId, ChannelRecord, Principal, PrincipalKind};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Access, Module, Route};
use crate::state::AppState;

pub(super) const OPERATORS: &[PrincipalKind] = &[PrincipalKind::User];

pub(super) const MODULE: Module = Module {
    name: "channels",
    routes: &[
        Route {
            path: "/api/channels",
            access: Access::PrincipalOfKind(OPERATORS),
            method_router: collection,
        },
        Route {
            path: "/api/channels/{id}",
            access: Access::PrincipalOfKind(OPERATORS),
            method_router: item,
        },
    ],
};

fn collection() -> MethodRouter<AppState> {
    get(list).post(create)
}

fn item() -> MethodRouter<AppState> {
    put(update).delete(remove)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateChannel {
    connector: String,
    binding: String,
    events: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateChannel {
    events: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct ChannelView {
    id: String,
    connector: String,
    connection: String,
    binding: String,
    events: BTreeSet<String>,
    status: crate::channel::ChannelStatus,
}

fn view(supervisor: &crate::channel::ChannelSupervisor, record: ChannelRecord) -> ChannelView {
    ChannelView {
        id: record.id().to_string(),
        connector: record.connector().to_owned(),
        connection: record.connection().to_owned(),
        binding: record.binding().to_owned(),
        events: record.events().clone(),
        status: supervisor.status(record.id()),
    }
}

async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(supervisor) = state.channels() else {
        return unavailable();
    };
    Json(
        supervisor
            .store()
            .held(principal.tenant())
            .into_iter()
            .map(|record| view(supervisor, record))
            .collect::<Vec<_>>(),
    )
    .into_response()
}

async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(proposed): Json<CreateChannel>,
) -> Response {
    let Some(supervisor) = state.channels() else {
        return unavailable();
    };
    if !supervisor.validates(&proposed.connector, &proposed.binding, &proposed.events) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "binding or selected events are not declared",
        );
    }
    let record = match ChannelRecord::new(
        supervisor.mint_id(),
        principal.tenant().clone(),
        proposed.connector.clone(),
        // X-122 owns the durable, rename-safe binding to an X-14 instance. Until then channels are
        // sole-connection-only and this body still cannot name an authority, UUID or address.
        proposed.connector,
        proposed.binding,
        proposed.events,
    ) {
        Ok(record) => record,
        Err(_) => return refuse(StatusCode::BAD_REQUEST, "channel declaration is invalid"),
    };
    if supervisor.store().set(record.clone()).is_err() {
        return unavailable();
    }
    supervisor.start(record.clone());
    (StatusCode::CREATED, Json(view(supervisor, record))).into_response()
}

async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(proposed): Json<UpdateChannel>,
) -> Response {
    let Some(supervisor) = state.channels() else {
        return unavailable();
    };
    let Ok(id) = ChannelId::new(id) else {
        return not_found();
    };
    let Some(record) = supervisor.store().get(principal.tenant(), &id) else {
        return not_found();
    };
    if !supervisor.validates(record.connector(), record.binding(), &proposed.events) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "selected events are not declared by this binding",
        );
    }
    let Ok(record) = record.with_events(proposed.events) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "channel event selection is invalid",
        );
    };
    if supervisor.store().set(record.clone()).is_err() {
        return unavailable();
    }
    supervisor.start(record.clone());
    Json(view(supervisor, record)).into_response()
}

async fn remove(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    let Some(supervisor) = state.channels() else {
        return unavailable();
    };
    let Ok(id) = ChannelId::new(id) else {
        return not_found();
    };
    match supervisor.store().delete(principal.tenant(), &id) {
        Ok(true) => {
            supervisor.stop(&id);
            supervisor.forget(&id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => not_found(),
        Err(_) => unavailable(),
    }
}

fn unavailable() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "this host has no connector-channel runtime configured",
    )
}

fn not_found() -> Response {
    refuse(StatusCode::NOT_FOUND, "no such channel")
}

fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({"error": reason}))).into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use exchange_host::{async_trait, Channels, MemoryChannels, PrincipalKind, Tenant};
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::channel::{
        ChannelDeclarations, ChannelEventSink, ChannelPlacement, ChannelPlacementResolver,
        ChannelRunError, ChannelRunner, ChannelSupervisor,
    };

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

    struct NoPlacement;

    impl ChannelPlacementResolver for NoPlacement {
        fn resolve(&self, _: &ChannelRecord) -> Result<ChannelPlacement, ChannelRunError> {
            Err(ChannelRunError::NoPlacement)
        }
    }

    struct NeverRuns;

    #[async_trait]
    impl ChannelRunner for NeverRuns {
        async fn run(
            &self,
            _: ChannelRecord,
            _: ChannelPlacement,
            _: Arc<dyn ChannelEventSink>,
            _: CancellationToken,
        ) -> Result<(), ChannelRunError> {
            panic!("placement refusal must precede the runner")
        }
    }

    fn fixture() -> (AppState, Arc<MemoryChannels>) {
        let store = Arc::new(MemoryChannels::default());
        let supervisor = ChannelSupervisor::new(
            store.clone(),
            Arc::new(Declarations),
            Arc::new(NoPlacement),
            Arc::new(NeverRuns),
        );
        (
            AppState::without_identity().with_channels(supervisor),
            store,
        )
    }

    fn principal(tenant: &str) -> Principal {
        Principal::new(
            PrincipalKind::User,
            "operator",
            Tenant::new(tenant).expect("tenant"),
        )
    }

    #[tokio::test]
    async fn create_derives_the_tenant_and_accepts_only_a_declared_event_subset() {
        let (state, store) = fixture();
        let owner = principal("alpha");
        let response = create(
            State(state.clone()),
            Extension(owner.clone()),
            Json(CreateChannel {
                connector: "asterisk".into(),
                binding: "ari-events".into(),
                events: ["channel-created".to_owned()].into_iter().collect(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let held = store.held(owner.tenant());
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].tenant(), owner.tenant());
        assert_eq!(held[0].connection(), "asterisk");

        let refused = create(
            State(state),
            Extension(owner.clone()),
            Json(CreateChannel {
                connector: "asterisk".into(),
                binding: "ari-events".into(),
                events: ["vendor-invented".to_owned()].into_iter().collect(),
            }),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert_eq!(store.held(owner.tenant()).len(), 1);
    }

    #[tokio::test]
    async fn a_cross_tenant_channel_id_is_the_same_not_found_as_an_unknown_one() {
        let (state, store) = fixture();
        let owner = principal("alpha");
        let id = ChannelId::new("ch_existing").expect("id");
        store
            .set(
                ChannelRecord::new(
                    id.clone(),
                    owner.tenant().clone(),
                    "asterisk",
                    "asterisk",
                    "ari-events",
                    ["channel-created".to_owned()].into_iter().collect(),
                )
                .expect("record"),
            )
            .expect("store");
        let proposed = || UpdateChannel {
            events: ["channel-destroyed".to_owned()].into_iter().collect(),
        };

        let cross_tenant = update(
            State(state.clone()),
            Extension(principal("beta")),
            Path(id.to_string()),
            Json(proposed()),
        )
        .await;
        let unknown = update(
            State(state),
            Extension(principal("beta")),
            Path("ch_unknown".into()),
            Json(proposed()),
        )
        .await;
        assert_eq!(cross_tenant.status(), StatusCode::NOT_FOUND);
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            store.get(owner.tenant(), &id).expect("record").events(),
            &["channel-created".to_owned()].into_iter().collect()
        );
    }

    #[test]
    fn management_bodies_cannot_name_tenant_connection_endpoint_credential_or_placement() {
        for field in [
            "tenant",
            "connection",
            "endpoint",
            "credential",
            "placement",
        ] {
            let mut document = json!({
                "connector": "asterisk",
                "binding": "ari-events",
                "events": ["channel-created"]
            });
            document[field] = json!("caller-controlled");
            assert!(
                serde_json::from_value::<CreateChannel>(document).is_err(),
                "{field}"
            );
        }
    }
}
