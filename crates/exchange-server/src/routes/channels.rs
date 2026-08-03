//! Operator-owned persistent connector channels.

use std::collections::BTreeSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put, MethodRouter};
use axum::{Extension, Json};
use exchange_host::{ChannelId, ChannelRecord, Principal};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Access, Module, Route};
use crate::state::AppState;

pub(super) const MODULE: Module = Module {
    name: "channels",
    routes: &[
        Route {
            path: "/api/channels",
            access: Access::Operator,
            method_router: collection,
        },
        Route {
            path: "/api/channels/{id}",
            access: Access::Operator,
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
    connection: Option<String>,
    binding: String,
    events: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateChannel {
    connection: Option<String>,
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

fn view(
    state: &AppState,
    supervisor: &crate::channel::ChannelSupervisor,
    record: ChannelRecord,
) -> Result<ChannelView, Box<Response>> {
    let connection = super::connections::channel_label(
        state,
        record.tenant(),
        record.connector(),
        record.connection(),
    )?;
    Ok(ChannelView {
        id: record.id().to_string(),
        connector: record.connector().to_owned(),
        connection,
        binding: record.binding().to_owned(),
        events: record.events().clone(),
        status: supervisor.status(record.id()),
    })
}

async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(supervisor) = state.channels() else {
        return unavailable();
    };
    let channels = supervisor
        .store()
        .held(principal.tenant())
        .into_iter()
        .map(|record| view(&state, supervisor, record))
        .collect::<Result<Vec<_>, _>>();
    match channels {
        Ok(channels) => Json(channels).into_response(),
        Err(response) => *response,
    }
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
    let Some(_claim) = state
        .connections()
        .claim(principal.tenant(), &proposed.connector)
    else {
        return refuse(
            StatusCode::CONFLICT,
            "a connection change is already in flight",
        );
    };
    let Some(provider) =
        connector_catalog::provider(connector_catalog::ProviderKey::id(&proposed.connector))
    else {
        return refuse(StatusCode::BAD_REQUEST, "connector is not declared");
    };
    let connection = match super::connections::channel_instance(
        &state,
        &principal,
        provider,
        proposed.connection.as_deref(),
    )
    .await
    {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let record = match ChannelRecord::new(
        supervisor.mint_id(),
        principal.tenant().clone(),
        proposed.connector.clone(),
        connection,
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
    let projected = match view(&state, supervisor, record) {
        Ok(projected) => projected,
        Err(response) => return *response,
    };
    (StatusCode::CREATED, Json(projected)).into_response()
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
    let Some(_claim) = state
        .connections()
        .claim(principal.tenant(), record.connector())
    else {
        return refuse(
            StatusCode::CONFLICT,
            "a connection change is already in flight",
        );
    };
    let Some(provider) =
        connector_catalog::provider(connector_catalog::ProviderKey::id(record.connector()))
    else {
        return refuse(StatusCode::BAD_REQUEST, "connector is not declared");
    };
    let connection = match super::connections::channel_instance(
        &state,
        &principal,
        provider,
        proposed.connection.as_deref(),
    )
    .await
    {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    if !supervisor.validates(record.connector(), record.binding(), &proposed.events) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "selected events are not declared by this binding",
        );
    }
    let Ok(record) = record.with_connection_and_events(connection, proposed.events) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "channel event selection is invalid",
        );
    };
    if supervisor.store().set(record.clone()).is_err() {
        return unavailable();
    }
    supervisor.start(record.clone());
    let projected = match view(&state, supervisor, record) {
        Ok(projected) => projected,
        Err(response) => return *response,
    };
    Json(projected).into_response()
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

    use exchange_host::{
        async_trait, ChannelRecord, Channels, ConnectionLabel, ConnectionRegistry, CredentialRef,
        CredentialScope, MemoryChannels, MemoryConnectionRegistry, PrincipalKind, Secret,
        SecretStore, StoreError, Tenant,
    };
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

    struct Inventory {
        references: Vec<CredentialRef>,
    }

    #[async_trait]
    impl SecretStore for Inventory {
        async fn get(&self, _: &CredentialRef) -> Result<Secret, StoreError> {
            unreachable!("channel management enumerates addresses but reads no value")
        }

        async fn put(&self, _: &CredentialRef, _: &Secret) -> Result<(), StoreError> {
            unreachable!("channel management writes no credential")
        }

        async fn delete(&self, _: &CredentialRef) -> Result<(), StoreError> {
            unreachable!("channel management deletes no credential")
        }

        async fn references(&self, _: &CredentialScope) -> Result<Vec<CredentialRef>, StoreError> {
            Ok(self.references.clone())
        }
    }

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

    fn fixture_with(
        references: Vec<CredentialRef>,
        rows: &[(&str, &str, &str)],
    ) -> (AppState, Arc<MemoryChannels>) {
        let store = Arc::new(MemoryChannels::default());
        let registry = Arc::new(MemoryConnectionRegistry::default());
        for (tenant, label, instance) in rows {
            ConnectionRegistry::assign(
                registry.as_ref(),
                &Tenant::new(*tenant).expect("tenant"),
                "asterisk",
                &ConnectionLabel::new(*label).expect("label"),
                &exchange_host::InstanceId::parse(instance).expect("instance"),
            )
            .expect("registry row");
        }
        let credentials = Arc::new(Inventory { references });
        let supervisor = ChannelSupervisor::new(
            store.clone(),
            Arc::new(Declarations),
            Arc::new(NoPlacement),
            Arc::new(NeverRuns),
        );
        (
            AppState::without_identity()
                .with_credentials(credentials)
                .with_connection_registry(registry)
                .with_channels(supervisor),
            store,
        )
    }

    fn fixture() -> (AppState, Arc<MemoryChannels>) {
        fixture_with(
            vec![
                CredentialRef::new("alpha", "org.asterisk.ari", "default", "password")
                    .expect("reference"),
            ],
            &[("alpha", "primary", "11111111-1111-4111-8111-111111111111")],
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
                connection: None,
                binding: "ari-events".into(),
                events: ["channel-created".to_owned()].into_iter().collect(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let held = store.held(owner.tenant());
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].tenant(), owner.tenant());
        assert_eq!(
            held[0].connection().as_str(),
            "11111111-1111-4111-8111-111111111111"
        );

        let refused = create(
            State(state),
            Extension(owner.clone()),
            Json(CreateChannel {
                connector: "asterisk".into(),
                connection: None,
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
                    exchange_host::InstanceId::parse("11111111-1111-4111-8111-111111111111")
                        .expect("instance"),
                    "ari-events",
                    ["channel-created".to_owned()].into_iter().collect(),
                )
                .expect("record"),
            )
            .expect("store");
        let proposed = || UpdateChannel {
            connection: None,
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

    #[tokio::test]
    async fn tenant_a_cannot_bind_a_channel_to_tenant_bs_label_or_uuid() {
        let first = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        let (state, channels) = fixture_with(
            vec![CredentialRef::for_instance(
                "alpha",
                "org.asterisk.ari",
                first,
                "default",
                "password",
            )
            .expect("reference")],
            &[("alpha", "primary", first), ("beta", "private", second)],
        );
        let proposed = |connection: &str| CreateChannel {
            connector: "asterisk".into(),
            connection: Some(connection.to_owned()),
            binding: "ari-events".into(),
            events: ["channel-created".to_owned()].into_iter().collect(),
        };

        let by_label = create(
            State(state.clone()),
            Extension(principal("alpha")),
            Json(proposed("private")),
        )
        .await;
        let by_uuid = create(
            State(state),
            Extension(principal("alpha")),
            Json(proposed(second)),
        )
        .await;

        assert_eq!(by_label.status(), StatusCode::NOT_FOUND);
        assert_eq!(by_uuid.status(), StatusCode::NOT_FOUND);
        assert!(channels
            .held(&Tenant::new("alpha").expect("tenant"))
            .is_empty());
    }

    #[tokio::test]
    async fn omitting_a_connection_refuses_when_two_instances_are_held() {
        let first = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        let reference = |instance: &str| {
            CredentialRef::for_instance(
                "alpha",
                "org.asterisk.ari",
                instance,
                "default",
                "password",
            )
            .expect("reference")
        };
        let (state, channels) = fixture_with(
            vec![reference(first), reference(second)],
            &[("alpha", "primary", first), ("alpha", "secondary", second)],
        );
        let response = create(
            State(state),
            Extension(principal("alpha")),
            Json(CreateChannel {
                connector: "asterisk".into(),
                connection: None,
                binding: "ari-events".into(),
                events: ["channel-created".to_owned()].into_iter().collect(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(channels
            .held(&Tenant::new("alpha").expect("tenant"))
            .is_empty());
    }

    #[tokio::test]
    async fn update_rebinds_by_a_label_and_persists_only_its_host_minted_uuid() {
        let first = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        let reference = |instance: &str| {
            CredentialRef::for_instance(
                "alpha",
                "org.asterisk.ari",
                instance,
                "default",
                "password",
            )
            .expect("reference")
        };
        let (state, channels) = fixture_with(
            vec![reference(first), reference(second)],
            &[("alpha", "primary", first), ("alpha", "secondary", second)],
        );
        let owner = principal("alpha");
        let response = create(
            State(state.clone()),
            Extension(owner.clone()),
            Json(CreateChannel {
                connector: "asterisk".into(),
                connection: Some("primary".into()),
                binding: "ari-events".into(),
                events: ["channel-created".to_owned()].into_iter().collect(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let record = channels
            .held(owner.tenant())
            .into_iter()
            .next()
            .expect("created channel");

        let response = update(
            State(state),
            Extension(owner.clone()),
            Path(record.id().to_string()),
            Json(UpdateChannel {
                connection: Some("secondary".into()),
                events: ["channel-destroyed".to_owned()].into_iter().collect(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let rebound = channels
            .get(owner.tenant(), record.id())
            .expect("rebound channel");
        assert_eq!(rebound.connection().as_str(), second);
        assert_eq!(
            rebound.events(),
            &["channel-destroyed".to_owned()].into_iter().collect()
        );
    }

    #[test]
    fn management_bodies_cannot_name_tenant_connection_endpoint_credential_or_placement() {
        for field in [
            "tenant",
            "instance",
            "connection_uuid",
            "authority",
            "host",
            "endpoint",
            "credential",
            "credential_address",
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
