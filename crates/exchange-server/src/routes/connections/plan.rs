//! The declaration-driven labelled connection plan.
//!
//! This is deliberately a projection and an orchestrator, not another store. Field rows come from
//! the connector catalogue, values are handed immediately to the existing connection/settings
//! handlers, and the response is rebuilt from persisted state after every attempt.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{ConfigField, Provider};
use exchange_host::{
    AuthorityState, AuthorityStatus, ConnectionLabel, DeclaredSetting, HostPinning, InstanceId,
    TenantInstances,
};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::*;

const VERSION: &str = "exchange.connection-plan.v1";

pub(super) fn route() -> MethodRouter<AppState> {
    get(show).post(apply)
}

pub(super) fn authority_route() -> MethodRouter<AppState> {
    put(approve_authority).delete(revoke_authority)
}

#[derive(Default, Deserialize)]
struct Selection {
    name: Option<String>,
    version: Option<String>,
}

/// Secret values exist only in this request type and the existing credential request types.
///
/// No `Debug`, deliberately: adding `debug!(?body)` must remain a compile error.
#[derive(Deserialize)]
struct Submission {
    version: String,
    name: String,
    #[serde(default)]
    current_name: Option<String>,
    #[serde(default)]
    values: SubmittedValues,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoritySubmission {
    version: String,
    revision: String,
}

#[derive(Serialize)]
struct AuthorityResponse {
    version: &'static str,
    connector: String,
    label: String,
    service: String,
    field: String,
    authority: AuthorityTransition,
}

#[derive(Serialize)]
struct AuthorityTransition {
    state: AuthorityViewState,
    revision: String,
}

/// A map that refuses duplicate JSON keys instead of accepting the last secret silently.
#[derive(Default)]
struct SubmittedValues(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for SubmittedValues {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ValuesVisitor;

        impl<'de> Visitor<'de> for ValuesVisitor {
            type Value = SubmittedValues;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map from unique published target ids to values")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((target, value)) = map.next_entry::<String, String>()? {
                    if values.insert(target.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "target `{target}` occurs more than once"
                        )));
                    }
                }
                Ok(SubmittedValues(values))
            }
        }

        deserializer.deserialize_map(ValuesVisitor)
    }
}

#[derive(Clone, Serialize)]
struct Plan {
    version: &'static str,
    connector: &'static str,
    vendor: &'static str,
    labels: Vec<String>,
    selection: Option<String>,
    state: PlanState,
    fields: Vec<FieldView>,
    apply: ApplyView,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum PlanState {
    Complete,
    Incomplete,
}

#[derive(Clone, Serialize)]
struct FieldView {
    identity: String,
    name: String,
    service: Option<String>,
    label: String,
    help: String,
    required: bool,
    secret: bool,
    input: String,
    aliases: Vec<String>,
    binds: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    also_binds: Vec<String>,
    provenance: &'static str,
    routable: bool,
    set: bool,
    target: Option<TargetView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<ChoiceView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authority: Option<AuthorityView>,
}

#[derive(Clone, Serialize)]
struct AuthorityView {
    state: AuthorityViewState,
    revision: Option<String>,
    actions: Option<AuthorityActions>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum AuthorityViewState {
    Unset,
    Proposed,
    Approved,
    Revoked,
}

#[derive(Clone, Serialize)]
struct AuthorityActions {
    approve: AuthorityAction,
    revoke: AuthorityAction,
}

#[derive(Clone, Serialize)]
struct AuthorityAction {
    method: &'static str,
    target: String,
}

#[derive(Clone, Serialize)]
struct TargetView {
    id: String,
}

#[derive(Clone, Serialize)]
struct ChoiceView {
    value: String,
    label: String,
}

#[derive(Clone, Serialize)]
struct ApplyView {
    method: &'static str,
    target: String,
    retry: &'static str,
    compensation: [&'static str; 2],
}

#[derive(Serialize)]
struct ApplyResponse {
    outcome: ApplyOutcome,
    steps: Vec<StepView>,
    plan: Plan,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ApplyOutcome {
    Complete,
    Incomplete,
    Refused,
    Partial,
}

#[derive(Serialize)]
struct StepView {
    target: String,
    outcome: StepOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum StepOutcome {
    Applied,
    Unchanged,
    Refused,
    Skipped,
}

#[derive(Clone)]
enum Destination {
    Credential(String),
    Settings(Vec<DeclaredSetting>),
}

#[derive(Clone)]
struct TargetSpec {
    id: String,
    destination: Destination,
    choices: Option<Vec<String>>,
}

struct DescribedField {
    view: FieldView,
    target: Option<TargetSpec>,
    custom_origin: bool,
}

struct ConnectionContext {
    labels: Vec<String>,
    selected_instance: Option<InstanceId>,
    held_instances: Vec<InstanceId>,
}

async fn show(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
    Query(query): Query<Selection>,
) -> Response {
    if let Some(version) = query
        .version
        .as_deref()
        .filter(|version| *version != VERSION)
    {
        return unsupported_version(version);
    }
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    match project(&state, &principal, provider, query.name.as_deref()).await {
        Ok(plan) => Json(plan).into_response(),
        Err(response) => response,
    }
}

async fn apply(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(request_id): Extension<RequestId>,
    Path(connector): Path<String>,
    Json(body): Json<Submission>,
) -> Response {
    if body.version != VERSION {
        return unsupported_version(&body.version);
    }
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    // Every semantic refusal below embeds this persisted projection. A caller never has to infer
    // whether a preflight failure wrote anything from a generic error body.
    let unselected = match project(&state, &principal, provider, None).await {
        Ok(plan) => plan,
        Err(response) => return response,
    };
    let name = match ConnectionLabel::new(&body.name) {
        Ok(name) => name,
        Err(refusal) => {
            return preflight_refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                refusal.to_string(),
                &unselected,
            )
        }
    };
    let current_name = match body.current_name.as_deref() {
        Some(current) => match ConnectionLabel::new(current) {
            Ok(current) => Some(current),
            Err(refusal) => {
                return preflight_refused(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    refusal.to_string(),
                    &unselected,
                )
            }
        },
        None => None,
    };

    // This read is the preflight boundary. No target, choice or rename is allowed to fail after a
    // write merely because the whole request was not checked first.
    let existing = current_name
        .as_ref()
        .map(ConnectionLabel::as_str)
        .unwrap_or(name.as_str());
    let exists = unselected.labels.iter().any(|label| label == existing);
    if current_name.is_some() && !exists {
        return preflight_refused(
            StatusCode::NOT_FOUND,
            format!(
                "connection label `{existing}` is not held for `{}`",
                provider.id
            ),
            &unselected,
        );
    }
    if current_name
        .as_ref()
        .is_some_and(|current| current != &name)
        && unselected.labels.iter().any(|label| label == name.as_str())
    {
        return preflight_refused(
            StatusCode::CONFLICT,
            format!("connection label `{}` already exists", name.as_str()),
            &unselected,
        );
    }

    let Some(settings_store) = state.settings() else {
        return no_settings_store();
    };
    let descriptions = match describe_for(provider, settings_store.as_ref()) {
        Ok(descriptions) => descriptions,
        Err(refusal) => return settings_refused(&refusal),
    };
    let targets = match submission_targets(provider, &descriptions) {
        Ok(targets) => targets,
        Err(response) => return *response,
    };
    for (target, value) in &body.values.0 {
        let Some(spec) = targets.iter().find(|candidate| candidate.id == *target) else {
            return preflight_refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("`{target}` is not a routable target in this connection plan"),
                &unselected,
            );
        };
        if spec
            .choices
            .as_ref()
            .is_some_and(|choices| !choices.iter().any(|choice| choice == value))
        {
            return preflight_refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("`{target}` must be one of its published choices"),
                &unselected,
            );
        }
    }

    let credential_targets: Vec<&TargetSpec> = targets
        .iter()
        .filter(|target| matches!(target.destination, Destination::Credential(_)))
        .collect();
    let setting_targets: Vec<&TargetSpec> = targets
        .iter()
        .filter(|target| matches!(target.destination, Destination::Settings(_)))
        .collect();
    if !exists
        && credential_targets
            .iter()
            .all(|target| !body.values.0.contains_key(&target.id))
    {
        return preflight_refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "a new `{}` connection requires at least one published credential target",
                provider.id
            ),
            &unselected,
        );
    }

    let mut steps = Vec::new();
    let mut applied_any = false;
    let mut stopped = false;
    let mut refusal_status = None;
    let mut new_connection_applied = false;
    let mut rename_applied = false;
    let working_label = existing.to_owned();

    if exists {
        for target in &credential_targets {
            let Some(value) = body.values.0.get(&target.id) else {
                steps.push(step(
                    &target.id,
                    StepOutcome::Unchanged,
                    Some("the target was omitted and its existing value remains"),
                ));
                continue;
            };
            if stopped {
                steps.push(step(
                    &target.id,
                    StepOutcome::Skipped,
                    Some("an earlier step was refused"),
                ));
                continue;
            }
            let Destination::Credential(credential) = &target.destination else {
                unreachable!("credential targets were filtered above")
            };
            let audit = match begin_audit(
                &state,
                &request_id,
                &principal,
                AuditAction::CredentialRotated,
                AuditTarget::InstanceCredential {
                    connector: provider.id.to_owned(),
                    label: working_label.clone(),
                    credential: credential.clone(),
                },
            ) {
                Ok(audit) => audit,
                Err(response) => {
                    refusal_status = Some(response.status());
                    stopped = true;
                    steps.push(step(
                        &target.id,
                        StepOutcome::Refused,
                        Some("audit preparation refused the credential write"),
                    ));
                    continue;
                }
            };
            let response = supply_instance_credential(
                &state,
                &principal,
                provider,
                &working_label,
                credential,
                value,
            )
            .await;
            let write_status = response.status();
            let audit_failure = audit
                .finish(&state, &request_id, &principal, write_status)
                .err()
                .map(|response| response.status());
            if write_status.is_success() {
                applied_any = true;
                if let Some(status) = audit_failure {
                    refusal_status = Some(status);
                    stopped = true;
                    steps.push(step(
                        &target.id,
                        StepOutcome::Applied,
                        Some(
                            "the credential write persisted, but audit finalization refused further execution",
                        ),
                    ));
                } else {
                    steps.push(step(&target.id, StepOutcome::Applied, None));
                }
            } else {
                refusal_status = Some(audit_failure.unwrap_or(write_status));
                stopped = true;
                steps.push(step(
                    &target.id,
                    StepOutcome::Refused,
                    Some(if audit_failure.is_some() {
                        "the credential write was refused and its audit could not be finalized"
                    } else {
                        "the existing credential write surface refused this target"
                    }),
                ));
            }
        }
    } else {
        let credentials: BTreeMap<String, String> = credential_targets
            .iter()
            .filter_map(|target| {
                let value = body.values.0.get(&target.id)?;
                let Destination::Credential(credential) = &target.destination else {
                    return None;
                };
                Some((credential.clone(), value.clone()))
            })
            .collect();
        let audit = begin_audit(
            &state,
            &request_id,
            &principal,
            AuditAction::ConnectionCreated,
            AuditTarget::ConnectionInstance {
                connector: provider.id.to_owned(),
                label: name.as_str().to_owned(),
            },
        );
        match audit {
            Err(response) => {
                refusal_status = Some(response.status());
                stopped = true;
                let mut refusal_reported = false;
                for target in &credential_targets {
                    let submitted = body.values.0.contains_key(&target.id);
                    let outcome = if submitted && !refusal_reported {
                        refusal_reported = true;
                        StepOutcome::Refused
                    } else {
                        StepOutcome::Skipped
                    };
                    let reason = if matches!(outcome, StepOutcome::Refused) {
                        "audit preparation refused the atomic credential creation"
                    } else if submitted {
                        "an earlier submitted credential could not begin audited creation"
                    } else {
                        "the credential target was omitted from creation"
                    };
                    steps.push(step(&target.id, outcome, Some(reason)));
                }
            }
            Ok(audit) => {
                let response = create_instance(
                    State(state.clone()),
                    Extension(principal.clone()),
                    Path((provider.id.to_owned(), name.as_str().to_owned())),
                    Query(AcquisitionQuery::default()),
                    Json(NewConnection {
                        credentials,
                        acquisition: None,
                    }),
                )
                .await;
                let write_status = response.status();
                let audit_failure = audit
                    .finish(&state, &request_id, &principal, write_status)
                    .err()
                    .map(|response| response.status());
                if write_status.is_success() {
                    applied_any = true;
                    new_connection_applied = true;
                    if let Some(status) = audit_failure {
                        refusal_status = Some(status);
                        stopped = true;
                    }
                    let mut audit_reason_reported = false;
                    for target in &credential_targets {
                        let submitted = body.values.0.contains_key(&target.id);
                        let reason = if submitted
                            && audit_failure.is_some()
                            && !audit_reason_reported
                        {
                            audit_reason_reported = true;
                            Some(
                                "the credential set persisted, but audit finalization refused further execution",
                            )
                        } else if submitted {
                            None
                        } else {
                            Some("the credential target was omitted from atomic creation")
                        };
                        steps.push(step(
                            &target.id,
                            if submitted {
                                StepOutcome::Applied
                            } else {
                                StepOutcome::Skipped
                            },
                            reason,
                        ));
                    }
                } else {
                    refusal_status = Some(audit_failure.unwrap_or(write_status));
                    stopped = true;
                    let mut refusal_reported = false;
                    for target in &credential_targets {
                        let submitted = body.values.0.contains_key(&target.id);
                        let outcome = if submitted && !refusal_reported {
                            refusal_reported = true;
                            StepOutcome::Refused
                        } else {
                            StepOutcome::Skipped
                        };
                        let reason = if matches!(outcome, StepOutcome::Refused) {
                            if audit_failure.is_some() {
                                "atomic credential creation was refused and its audit could not be finalized"
                            } else {
                                "the atomic credential creation was refused"
                            }
                        } else if submitted {
                            "an earlier submitted credential was refused atomically"
                        } else {
                            "the credential target was omitted from creation"
                        };
                        steps.push(step(&target.id, outcome, Some(reason)));
                    }
                }
            }
        }
    }

    for target in &setting_targets {
        let Some(value) = body.values.0.get(&target.id) else {
            steps.push(step(
                &target.id,
                if exists {
                    StepOutcome::Unchanged
                } else {
                    StepOutcome::Skipped
                },
                Some(if exists {
                    "the target was omitted and its existing value remains"
                } else {
                    "the settings target was omitted from creation"
                }),
            ));
            continue;
        };
        if stopped {
            steps.push(step(
                &target.id,
                StepOutcome::Skipped,
                Some("an earlier step was refused"),
            ));
            continue;
        }
        let Destination::Settings(settings) = &target.destination else {
            unreachable!("setting targets were filtered above")
        };
        let mut target_applied = false;
        let mut audit_refused = false;
        for setting in settings {
            let audit = match begin_audit(
                &state,
                &request_id,
                &principal,
                AuditAction::SettingSet,
                AuditTarget::InstanceSetting {
                    connector: provider.id.to_owned(),
                    label: working_label.clone(),
                    service: setting.service.clone(),
                    field: setting.binds(),
                },
            ) {
                Ok(audit) => audit,
                Err(response) => {
                    refusal_status = Some(response.status());
                    stopped = true;
                    audit_refused = true;
                    break;
                }
            };
            let response = set_instance_setting(
                State(state.clone()),
                Extension(principal.clone()),
                Path((
                    provider.id.to_owned(),
                    working_label.clone(),
                    setting.service.clone(),
                    setting.binds(),
                )),
                Json(SuppliedSetting {
                    value: value.clone(),
                }),
            )
            .await;
            let write_status = response.status();
            let audit_failure = audit
                .finish(&state, &request_id, &principal, write_status)
                .err()
                .map(|response| response.status());
            if write_status.is_success() {
                applied_any = true;
                target_applied = true;
                if let Some(status) = audit_failure {
                    refusal_status = Some(status);
                    stopped = true;
                    audit_refused = true;
                    break;
                }
            } else {
                refusal_status = Some(audit_failure.unwrap_or(write_status));
                stopped = true;
                audit_refused = audit_failure.is_some();
                break;
            }
        }
        if stopped {
            steps.push(step(
                &target.id,
                StepOutcome::Refused,
                Some(if audit_refused && target_applied {
                    "one or more setting writes persisted before audit refused further execution"
                } else if audit_refused {
                    "audit preparation or finalization refused this settings target"
                } else if target_applied {
                    "one declared destination was applied before another destination refused"
                } else {
                    "the existing settings write surface refused this target"
                }),
            ));
        } else {
            let reason = descriptions
                .iter()
                .find(|description| description.target.as_ref().is_some_and(|spec| spec.id == target.id))
                .is_some_and(|description| description.custom_origin)
                .then_some("proposal persisted; explicit authority approval is required before runtime use");
            steps.push(step(&target.id, StepOutcome::Applied, reason));
        }
    }

    let rename_requested = exists && working_label != name.as_str();
    if stopped && !exists && new_connection_applied {
        steps.push(step(
            "connection.name",
            StepOutcome::Applied,
            Some("the labelled connection persisted before a later step was refused"),
        ));
    } else if stopped && rename_requested {
        steps.push(step(
            "connection.name",
            StepOutcome::Skipped,
            Some("rename runs last and an earlier step was refused"),
        ));
    } else if stopped {
        steps.push(step(
            "connection.name",
            if exists {
                StepOutcome::Unchanged
            } else {
                StepOutcome::Skipped
            },
            Some(if exists {
                "the submitted label already names this connection"
            } else {
                "atomic credential creation was refused before the label became durable"
            }),
        ));
    } else if rename_requested {
        let audit = begin_audit(
            &state,
            &request_id,
            &principal,
            AuditAction::ConnectionLabeled,
            AuditTarget::ConnectionInstance {
                connector: provider.id.to_owned(),
                label: working_label.clone(),
            },
        );
        match audit {
            Err(response) => {
                refusal_status = Some(response.status());
                stopped = true;
                steps.push(step(
                    "connection.name",
                    StepOutcome::Refused,
                    Some("audit preparation refused the label rename"),
                ));
            }
            Ok(audit) => {
                let response = rename_instance(
                    State(state.clone()),
                    Extension(principal.clone()),
                    Path((provider.id.to_owned(), working_label.clone())),
                    Json(LabelBody {
                        label: name.as_str().to_owned(),
                    }),
                )
                .await;
                let write_status = response.status();
                let audit_failure = audit
                    .finish(&state, &request_id, &principal, write_status)
                    .err()
                    .map(|response| response.status());
                if write_status.is_success() {
                    applied_any = true;
                    rename_applied = true;
                    if let Some(status) = audit_failure {
                        refusal_status = Some(status);
                        stopped = true;
                        steps.push(step(
                            "connection.name",
                            StepOutcome::Applied,
                            Some("the rename persisted, but audit finalization refused completion"),
                        ));
                    } else {
                        steps.push(step("connection.name", StepOutcome::Applied, None));
                    }
                } else {
                    refusal_status = Some(audit_failure.unwrap_or(write_status));
                    stopped = true;
                    steps.push(step(
                        "connection.name",
                        StepOutcome::Refused,
                        Some(if audit_failure.is_some() {
                            "the label rename was refused and its audit could not be finalized"
                        } else {
                            "the existing label registry refused the rename"
                        }),
                    ));
                }
            }
        }
    } else {
        steps.push(step(
            "connection.name",
            if exists {
                StepOutcome::Unchanged
            } else {
                StepOutcome::Applied
            },
            exists.then_some("the submitted label already names this connection"),
        ));
    }

    let persisted_selection = if !exists && !new_connection_applied {
        None
    } else if stopped && rename_requested && !rename_applied {
        Some(working_label.as_str())
    } else {
        Some(name.as_str())
    };
    let plan = match project(&state, &principal, provider, persisted_selection).await {
        Ok(plan) => plan,
        Err(response) => return response,
    };
    let outcome = if stopped && applied_any {
        ApplyOutcome::Partial
    } else if stopped {
        ApplyOutcome::Refused
    } else if matches!(plan.state, PlanState::Complete) {
        ApplyOutcome::Complete
    } else {
        ApplyOutcome::Incomplete
    };
    let status = if matches!(outcome, ApplyOutcome::Partial) {
        StatusCode::MULTI_STATUS
    } else if matches!(outcome, ApplyOutcome::Refused) {
        refusal_status.unwrap_or(StatusCode::UNPROCESSABLE_ENTITY)
    } else {
        StatusCode::OK
    };
    (
        status,
        Json(ApplyResponse {
            outcome,
            steps,
            plan,
        }),
    )
        .into_response()
}

async fn approve_authority(
    state: State<AppState>,
    principal: Extension<Principal>,
    request_id: Extension<RequestId>,
    path: Path<(String, String, String, String)>,
    Json(body): Json<AuthoritySubmission>,
) -> Response {
    transition_authority(state, principal, request_id, path, body, true).await
}

async fn revoke_authority(
    state: State<AppState>,
    principal: Extension<Principal>,
    request_id: Extension<RequestId>,
    path: Path<(String, String, String, String)>,
    Json(body): Json<AuthoritySubmission>,
) -> Response {
    transition_authority(state, principal, request_id, path, body, false).await
}

async fn transition_authority(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(request_id): Extension<RequestId>,
    Path((connector, label, service, field)): Path<(String, String, String, String)>,
    body: AuthoritySubmission,
    approve: bool,
) -> Response {
    if body.version != VERSION {
        return unsupported_version(&body.version);
    }
    let revision =
        match canonical_revision(&body.revision) {
            Some(revision) => revision,
            None => return refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                "authority revision must be a canonical decimal integer from 1 through u64::MAX",
                json!({ "revision": body.revision }),
            ),
        };
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(settings) = state.settings() else {
        return no_settings_store();
    };
    let Some(declared) = DeclaredSetting::parse(&service, &field) else {
        return unreadable_field(provider, &service, &field);
    };
    let declarations = match declared_settings(provider) {
        Ok(declarations) => declarations,
        Err(refusal) => return settings_refused(&refusal),
    };
    if !declarations.contains(&declared) || !settings.is_custom_origin(provider.id, &declared) {
        return settings_refused(&SettingsRefusal::AuthorityUnsupported {
            connector: provider.id.to_owned(),
            setting: declared.binds(),
        });
    }
    let Some(_claim) = state.connections().claim(principal.tenant(), provider.id) else {
        return change_in_flight(provider);
    };
    let instance = match invocation_instance(&state, &principal, provider, Some(&label)).await {
        Ok(instance) => instance,
        Err(response) => return response,
    };
    let action = if approve {
        AuditAction::SettingAuthorityApproved
    } else {
        AuditAction::SettingAuthorityRevoked
    };
    let audit = match begin_audit(
        &state,
        &request_id,
        &principal,
        action,
        AuditTarget::InstanceSetting {
            connector: provider.id.to_owned(),
            label: label.clone(),
            service: declared.service.clone(),
            field: declared.binds(),
        },
    ) {
        Ok(audit) => audit,
        Err(response) => return *response,
    };
    let result = if approve {
        settings.approve_authority_for_instance(
            principal.tenant(),
            provider.id,
            instance.as_ref(),
            &declared,
            revision,
        )
    } else {
        settings.revoke_authority_for_instance(
            principal.tenant(),
            provider.id,
            instance.as_ref(),
            &declared,
            revision,
        )
    };
    let transition = match result {
        Ok(status) => status,
        Err(refusal) => {
            let response = authority_refused(&refusal);
            if let Err(audit_response) =
                audit.finish(&state, &request_id, &principal, response.status())
            {
                return *audit_response;
            }
            return response;
        }
    };
    // Persisted authority invalidates the runtime snapshot immediately. Audit finalization can
    // refuse independently, but it must not leave a pre-revocation channel alive.
    if let Some(channels) = state.channels() {
        channels.restart(principal.tenant(), provider.id);
    }
    if let Err(response) = audit.finish(&state, &request_id, &principal, StatusCode::OK) {
        return *response;
    };
    Json(AuthorityResponse {
        version: VERSION,
        connector: provider.id.to_owned(),
        label,
        service: declared.service.clone(),
        field: declared.binds(),
        authority: AuthorityTransition {
            state: transition.state.into(),
            revision: transition
                .revision
                .expect("transition has revision")
                .to_string(),
        },
    })
    .into_response()
}

fn canonical_revision(revision: &str) -> Option<u64> {
    if revision.is_empty()
        || revision.starts_with('0')
        || !revision.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    revision.parse().ok().filter(|revision| *revision > 0)
}

impl From<AuthorityState> for AuthorityViewState {
    fn from(state: AuthorityState) -> Self {
        match state {
            AuthorityState::Unset => Self::Unset,
            AuthorityState::Proposed => Self::Proposed,
            AuthorityState::Approved => Self::Approved,
            AuthorityState::Revoked => Self::Revoked,
        }
    }
}

fn unsupported_version(requested: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "code": "unsupported_connection_plan_version",
            "requested": requested,
            "supported": VERSION,
        })),
    )
        .into_response()
}

fn authority_refused(refusal: &SettingsRefusal) -> Response {
    match refusal {
        SettingsRefusal::AuthorityUnset { .. } => refuse(
            StatusCode::CONFLICT,
            refusal.to_string(),
            json!({ "code": "origin_not_proposed" }),
        ),
        SettingsRefusal::AuthorityRevisionConflict {
            expected, current, ..
        } => refuse(
            StatusCode::CONFLICT,
            refusal.to_string(),
            json!({
                "code": "stale_origin_revision",
                "requested": expected.to_string(),
                "current": current.to_string(),
            }),
        ),
        other => settings_refused(other),
    }
}

/// Add or replace one declared credential on an already-held labelled connection.
///
/// The public rotation route deliberately remains replace-only. The composite plan needs this
/// internal upsert because initial creation may omit a credential, while retry still has to fill
/// that declared instance address.
async fn supply_instance_credential(
    state: &AppState,
    principal: &Principal,
    provider: &'static Provider,
    label: &str,
    credential: &str,
    value: &str,
) -> Response {
    let Some(store) = state.credentials() else {
        return no_store();
    };
    let Some(_claim) = state.connections().claim(principal.tenant(), provider.id) else {
        return change_in_flight(provider);
    };
    let selected = match invocation_instance(state, principal, provider, Some(label)).await {
        Ok(selected) => selected,
        Err(response) => return response,
    };
    let declared = declared_credentials(provider);
    let declaration = declaration(provider, &declared);
    let inventory = match inventory(store, principal.tenant(), &declaration).await {
        Ok(inventory) => inventory,
        Err(response) => return response,
    };
    let held_instances = inventory.ids();
    let instances = TenantInstances::held(&held_instances, selected.as_ref());
    let (reference, secret) =
        match declaration.write_of_for(principal.tenant(), credential, value, instances) {
            Ok(write) => write,
            Err(refusal) => return connection_refused(&refusal),
        };
    if let Some(refusal) = managed_rotation_refusal(store, provider, &reference).await {
        return refusal;
    }
    let replacing = match store.get(&reference).await {
        Ok(current) => stored_bytes(&current),
        Err(error) if error.is_not_found() => 0,
        Err(error) => return store_failed(&error),
    };
    let Some(_allowance) = state.connections().claim_tenant(principal.tenant()) else {
        return allowance_change_in_flight(provider);
    };
    let held_bytes = match occupied(store, principal.tenant()).await {
        Ok(bytes) => bytes,
        Err(error) => return store_failed(&error),
    };
    if let Err(refusal) =
        admit_tenant_occupancy(held_bytes.saturating_sub(replacing), stored_bytes(&secret))
    {
        return connection_refused(&refusal);
    }
    if let Err(error) = store.put(&reference, &secret).await {
        return rotation_failed(provider, credential, &reference, &error);
    }
    if let Some(channels) = state.channels() {
        channels.restart(principal.tenant(), provider.id);
    }
    Json(json!({
        "connector": provider.id,
        "label": label,
        "credential": credential,
        "address": address_path(&reference),
    }))
    .into_response()
}

enum StepAudit {
    Durable(crate::audit::Attempt),
    Ephemeral {
        action: AuditAction,
        target: AuditTarget,
    },
}

impl StepAudit {
    fn finish(
        self,
        state: &AppState,
        request_id: &RequestId,
        principal: &Principal,
        status: StatusCode,
    ) -> Result<(), Box<Response>> {
        let outcome = if status.is_success() {
            AuditOutcome::Succeeded
        } else {
            AuditOutcome::Refused
        };
        let result = match self {
            Self::Durable(attempt) => attempt.finish(outcome).map(|_| ()),
            Self::Ephemeral { action, target } => super::super::record_audit(
                state,
                request_id,
                action,
                outcome,
                Some(principal),
                target,
            ),
        };
        result.map_err(|error| Box::new(super::super::audit_unavailable(error)))
    }
}

fn begin_audit(
    state: &AppState,
    request_id: &RequestId,
    principal: &Principal,
    action: AuditAction,
    target: AuditTarget,
) -> Result<StepAudit, Box<Response>> {
    match state.audit() {
        Some(journal) => journal
            .begin(request_id, action, principal, target)
            .map(StepAudit::Durable)
            .map_err(|error| Box::new(super::super::audit_unavailable(error))),
        None => Ok(StepAudit::Ephemeral { action, target }),
    }
}

fn step(target: &str, outcome: StepOutcome, reason: Option<&str>) -> StepView {
    StepView {
        target: target.to_owned(),
        outcome,
        reason: reason.map(str::to_owned),
    }
}

fn same_target(left: &TargetSpec, right: &TargetSpec) -> bool {
    let same_destination = match (&left.destination, &right.destination) {
        (Destination::Credential(left), Destination::Credential(right)) => left == right,
        (Destination::Settings(left), Destination::Settings(right)) => left == right,
        _ => false,
    };
    same_destination && left.choices == right.choices
}

fn submission_targets(
    provider: &Provider,
    described: &[DescribedField],
) -> Result<Vec<TargetSpec>, Box<Response>> {
    let mut targets = Vec::<TargetSpec>::new();
    for described in described {
        let Some(target) = &described.target else {
            continue;
        };
        match targets.iter().find(|candidate| candidate.id == target.id) {
            Some(existing) if !same_target(existing, target) => {
                return Err(Box::new(refuse(
                    StatusCode::BAD_GATEWAY,
                    format!(
                        "connector `{}` maps target `{}` to inconsistent declarations",
                        provider.id, target.id
                    ),
                    json!({ "connector": provider.id }),
                )));
            }
            Some(_) => {}
            None => targets.push(target.clone()),
        }
    }
    Ok(targets)
}

async fn project(
    state: &AppState,
    principal: &Principal,
    provider: &'static Provider,
    selection: Option<&str>,
) -> Result<Plan, Response> {
    let context = connection_context(state, principal, provider, selection).await?;
    let settings = state.settings().ok_or_else(no_settings_store)?;
    let described =
        describe_for(provider, settings.as_ref()).map_err(|refusal| settings_refused(&refusal))?;
    // A shared target is one browser control. Refuse an internally ambiguous declaration on GET
    // as well as POST rather than publishing a plan the write side cannot honor.
    submission_targets(provider, &described).map_err(|response| *response)?;
    let credentials = state.credentials().ok_or_else(no_store)?;
    let declared = declared_credentials(provider);
    let declaration = declaration(provider, &declared);
    let instance_selection =
        TenantInstances::held(&context.held_instances, context.selected_instance.as_ref());

    let mut fields = Vec::with_capacity(described.len() + 1);
    fields.push(FieldView {
        identity: "connection.name".to_owned(),
        name: "name".to_owned(),
        service: None,
        label: "Connection name".to_owned(),
        help: "A tenant-scoped label such as company, sandbox, or production.".to_owned(),
        required: true,
        secret: false,
        input: "text".to_owned(),
        aliases: vec!["--name".to_owned()],
        binds: None,
        also_binds: Vec::new(),
        provenance: "exchange",
        routable: true,
        set: selection.is_some(),
        target: Some(TargetView {
            id: "connection.name".to_owned(),
        }),
        choices: None,
        reason: None,
        authority: None,
    });

    for mut field in described {
        field.view.set = match (&field.target, selection) {
            (_, None) | (None, _) => false,
            (Some(target), Some(_)) => match &target.destination {
                Destination::Credential(name) => {
                    let reference = declaration
                        .address_of_for(principal.tenant(), name, instance_selection)
                        .map_err(|refusal| connection_refused(&refusal))?;
                    match credentials.get(&reference).await {
                        Ok(_) => true,
                        Err(error) if error.is_not_found() => false,
                        Err(error) => return Err(store_failed(&error)),
                    }
                }
                Destination::Settings(declared) if field.custom_origin => {
                    let status = settings
                        .authority_status_for_instance(
                            principal.tenant(),
                            provider.id,
                            context.selected_instance.as_ref(),
                            &declared[0],
                        )
                        .map_err(|refusal| settings_refused(&refusal))?;
                    field.view.authority = Some(authority_view(
                        provider.id,
                        selection.expect("selected above"),
                        &declared[0],
                        status,
                    ));
                    status.state == AuthorityState::Approved
                }
                Destination::Settings(declared) => declared.iter().all(|setting| {
                    settings.is_set_for_instance(
                        principal.tenant(),
                        provider.id,
                        context.selected_instance.as_ref(),
                        setting,
                    )
                }),
            },
        };
        if field.custom_origin && selection.is_none() {
            field.view.authority = Some(AuthorityView {
                state: AuthorityViewState::Unset,
                revision: None,
                actions: None,
            });
        }
        fields.push(field.view);
    }

    validate_cli_aliases(&fields).map_err(|reason| {
        refuse(
            StatusCode::BAD_GATEWAY,
            format!(
                "connector `{}` cannot publish an unambiguous connection-plan alias set: {reason}",
                provider.id
            ),
            json!({ "connector": provider.id }),
        )
    })?;

    let complete = fields
        .iter()
        .filter(|field| field.required)
        .all(|field| field.routable && field.set);
    Ok(Plan {
        version: VERSION,
        connector: provider.id,
        vendor: provider.vendor,
        labels: context.labels,
        selection: selection.map(str::to_owned),
        state: if complete {
            PlanState::Complete
        } else {
            PlanState::Incomplete
        },
        fields,
        apply: ApplyView {
            method: "POST",
            target: format!("/api/connections/{}/plan", provider.id),
            retry: "Retry the same name and submitted targets; an existing label is edited and already-persisted targets are not recreated.",
            compensation: [
                "Unset settings through the existing labelled connection settings routes.",
                "Remove the labelled connection through the existing instance route; Exchange never claims to roll back a committed store write.",
            ],
        },
    })
}

/// The v1 convenience spelling is derived only from the declaration's stable field name.
///
/// Connector declarations validate lower snake case before reaching the catalogue. Keeping this
/// transformation beside the serialized field means clients consume aliases instead of inventing
/// them independently from vendor-facing labels or bindings.
fn canonical_cli_alias(name: &str) -> String {
    format!("--{}", name.replace('_', "-"))
}

fn valid_cli_alias(alias: &str) -> bool {
    let Some(name) = alias.strip_prefix("--") else {
        return false;
    };
    !name.is_empty()
        && name.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
}

fn validate_cli_aliases(fields: &[FieldView]) -> Result<(), String> {
    let mut owners = BTreeMap::<&str, &str>::new();
    for field in fields {
        if field.secret && !field.aliases.is_empty() {
            return Err(format!(
                "secret field `{}` publishes a command-line alias",
                field.identity
            ));
        }
        for alias in &field.aliases {
            if !valid_cli_alias(alias) {
                return Err(format!(
                    "field `{}` publishes malformed alias `{alias}`",
                    field.identity
                ));
            }
            if let Some(owner) = owners.insert(alias, &field.identity) {
                return Err(format!(
                    "alias `{alias}` belongs to both `{owner}` and `{}`",
                    field.identity
                ));
            }
        }
    }
    Ok(())
}

async fn connection_context(
    state: &AppState,
    principal: &Principal,
    provider: &'static Provider,
    selection: Option<&str>,
) -> Result<ConnectionContext, Response> {
    let store = state.credentials().ok_or_else(no_store)?;
    let registry = state.connection_registry().ok_or_else(no_registry)?;
    let declared = declared_credentials(provider);
    let declaration = declaration(provider, &declared);
    let inventory = inventory(store, principal.tenant(), &declaration).await?;
    let entries = registry
        .entries(principal.tenant(), provider.id)
        .map_err(|refusal| registry_refused(&refusal))?;
    let sole_legacy = inventory.count() == 1 && !inventory.legacy.is_empty() && entries.len() == 1;
    let valid: Vec<_> = entries
        .into_iter()
        .filter(|entry| inventory.holds(&entry.instance) || sole_legacy)
        .collect();
    let labels = valid
        .iter()
        .map(|entry| entry.label.as_str().to_owned())
        .collect();
    let selected_instance = match selection {
        None => None,
        Some(selected) => {
            let label =
                ConnectionLabel::new(selected).map_err(|refusal| registry_refused(&refusal))?;
            let Some(entry) = valid.iter().find(|entry| entry.label == label) else {
                return Err(unknown_label(provider, &label));
            };
            inventory
                .holds(&entry.instance)
                .then(|| entry.instance.clone())
        }
    };
    Ok(ConnectionContext {
        labels,
        selected_instance,
        held_instances: inventory.ids(),
    })
}

#[cfg(test)]
fn describe(provider: &'static Provider) -> Result<Vec<DescribedField>, SettingsRefusal> {
    describe_with_policy(provider, |_, _| false)
}

fn describe_for(
    provider: &'static Provider,
    settings: &dyn exchange_host::ConnectionSettings,
) -> Result<Vec<DescribedField>, SettingsRefusal> {
    describe_with_policy(provider, |connector, declared| {
        settings.is_custom_origin(connector, declared)
    })
}

fn describe_with_policy(
    provider: &'static Provider,
    custom_origin: impl Fn(&str, &DeclaredSetting) -> bool,
) -> Result<Vec<DescribedField>, SettingsRefusal> {
    let declared_settings = declared_settings(provider)?;
    let mut described: Vec<DescribedField> = provider
        .config
        .iter()
        .map(|field| {
            let parsed = DeclaredSetting::parse(field.service, field.binds);
            let custom = parsed
                .as_ref()
                .is_some_and(|declared| custom_origin(provider.id, declared));
            describe_config(provider, field, &declared_settings, custom)
        })
        .collect();
    let bound_credentials: BTreeSet<String> = described
        .iter()
        .filter_map(
            |field| match field.target.as_ref().map(|target| &target.destination) {
                Some(Destination::Credential(name)) => Some(name.clone()),
                _ => None,
            },
        )
        .collect();
    for credential in provider
        .auth
        .iter()
        .filter(|credential| !bound_credentials.contains(credential.name))
    {
        let target = format!("credential.{}", credential.name);
        described.push(DescribedField {
            view: FieldView {
                identity: target.clone(),
                name: credential
                    .name
                    .rsplit_once('.')
                    .map_or(credential.name, |(_, name)| name)
                    .to_owned(),
                service: None,
                label: credential.name.to_owned(),
                help: "Credential metadata is declared by the provider without a richer form row; supply it through this provider-derived target.".to_owned(),
                required: credential_required(provider, credential.name),
                secret: true,
                input: "secret".to_owned(),
                aliases: Vec::new(),
                binds: Some(target.clone()),
                also_binds: Vec::new(),
                provenance: "provider.auth",
                routable: true,
                set: false,
                target: Some(TargetView { id: target.clone() }),
                choices: None,
                reason: None,
                authority: None,
            },
            target: Some(TargetSpec {
                id: target,
                destination: Destination::Credential(credential.name.to_owned()),
                choices: None,
            }),
            custom_origin: false,
        });
    }
    Ok(described)
}

fn describe_config(
    provider: &'static Provider,
    field: &ConfigField,
    declared_settings: &[DeclaredSetting],
    custom_origin: bool,
) -> DescribedField {
    let parsed = DeclaredSetting::parse(field.service, field.binds);
    let choices = choices_for(provider, field, parsed.as_ref());
    let credential = field.binds.strip_prefix("credential.");
    // Binding to a credential makes an input secret even if older catalogue metadata omitted the
    // flag; an explicit secret flag remains visible even when its binding is unroutable.
    let secret = field.secret || credential.is_some();
    let (target, reason) = if let Some(credential) = credential {
        if provider.credential(credential).is_some() {
            let id = field.binds.to_owned();
            (
                Some(TargetSpec {
                    id: id.clone(),
                    destination: Destination::Credential(credential.to_owned()),
                    choices: choice_values(&choices),
                }),
                None,
            )
        } else {
            (
                None,
                Some(format!(
                    "`{}` does not name a provider-declared credential",
                    field.binds
                )),
            )
        }
    } else if field.secret {
        (
            None,
            Some("a field marked secret does not bind a provider credential and is refused rather than entering the settings store".to_owned()),
        )
    } else {
        match parsed {
            Some(primary)
                if declared_settings.contains(&primary)
                    && (host_pinning(provider, &primary).tenant_may_supply() || custom_origin) =>
            {
                let id = format!("setting.{}.{}", field.service, field.binds);
                (
                    Some(TargetSpec {
                        id: id.clone(),
                        destination: Destination::Settings(vec![primary]),
                        choices: choice_values(&choices),
                    }),
                    None,
                )
            }
            Some(primary) if matches!(host_pinning(provider, &primary), HostPinning::WholeAuthority(_)) => (
                None,
                Some("the declared setting would choose the whole request authority and deployment policy refuses it".to_owned()),
            ),
            _ => (
                None,
                Some(format!(
                    "binding `{}` is not accepted by the existing settings surface",
                    field.binds
                )),
            ),
        }
    };
    let input = if secret {
        "secret".to_owned()
    } else if choices.is_some() {
        "select".to_owned()
    } else if field.format.is_empty() {
        "text".to_owned()
    } else {
        field.format.to_owned()
    };
    let target_view = target.as_ref().map(|target| TargetView {
        id: target.id.clone(),
    });
    DescribedField {
        view: FieldView {
            identity: format!("config.{}.{}", field.service, field.name),
            name: field.name.to_owned(),
            service: Some(field.service.to_owned()),
            label: field.label.to_owned(),
            help: field.help.to_owned(),
            required: field.required,
            secret,
            input,
            aliases: (!secret)
                .then(|| canonical_cli_alias(field.name))
                .into_iter()
                .collect(),
            binds: Some(field.binds.to_owned()),
            also_binds: field
                .also_binds
                .iter()
                .map(|binds| (*binds).to_owned())
                .collect(),
            provenance: "provider.config",
            routable: target.is_some(),
            set: false,
            target: target_view,
            choices,
            reason,
            authority: None,
        },
        target,
        custom_origin,
    }
}

fn authority_view(
    connector: &str,
    label: &str,
    declared: &DeclaredSetting,
    status: AuthorityStatus,
) -> AuthorityView {
    let revision = status.revision.map(|revision| revision.to_string());
    let actions = match status.state {
        AuthorityState::Proposed | AuthorityState::Approved => revision.as_ref().map(|_| {
            let target = format!(
                "/api/connections/{}/instances/{}/settings/{}/{}/authority",
                encode_segment(connector),
                encode_segment(label),
                encode_segment(&declared.service),
                encode_segment(&declared.binds()),
            );
            AuthorityActions {
                approve: AuthorityAction {
                    method: "PUT",
                    target: target.clone(),
                },
                revoke: AuthorityAction {
                    method: "DELETE",
                    target,
                },
            }
        }),
        AuthorityState::Unset | AuthorityState::Revoked => None,
    };
    AuthorityView {
        state: status.state.into(),
        revision,
        actions,
    }
}

fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0xf) as usize]));
        }
    }
    encoded
}

fn choices_for(
    provider: &'static Provider,
    field: &ConfigField,
    parsed: Option<&DeclaredSetting>,
) -> Option<Vec<ChoiceView>> {
    let choice = parsed
        .and_then(|setting| {
            provider.choices_for(field.service, setting.kind.as_str(), &setting.name)
        })
        .or_else(|| {
            let (kind, name) = field.binds.split_once('.')?;
            provider.choices_for(field.service, kind, name)
        })?;
    let choices: Vec<_> = choice
        .choices
        .iter()
        .map(|choice| ChoiceView {
            value: choice.value.to_owned(),
            label: choice.label.to_owned(),
        })
        .collect();
    (!choices.is_empty()).then_some(choices)
}

fn credential_required(provider: &Provider, credential: &str) -> bool {
    let operation_requires = provider.operations.iter().any(|operation| {
        !operation.credentials.is_empty()
            && operation
                .credentials
                .iter()
                .all(|alternative| alternative.contains(&credential))
    });
    let channel_requires = provider.channels.iter().any(|channel| {
        channel.connect.is_some_and(|connect| {
            !connect.auth.is_empty()
                && connect
                    .auth
                    .iter()
                    .all(|alternative| alternative.contains(&credential))
        })
    });
    operation_requires || channel_requires
}

fn choice_values(choices: &Option<Vec<ChoiceView>>) -> Option<Vec<String>> {
    choices
        .as_ref()
        .map(|choices| choices.iter().map(|choice| choice.value.clone()).collect())
}

fn preflight_refused(status: StatusCode, reason: impl Into<String>, plan: &Plan) -> Response {
    let reason = reason.into();
    (
        status,
        Json(ApplyResponse {
            outcome: ApplyOutcome::Refused,
            steps: vec![step("request", StepOutcome::Refused, Some(&reason))],
            plan: plan.clone(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::DirBuilder;
    use std::os::unix::fs::DirBuilderExt as _;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{Method, Request as HttpRequest};
    use axum::Router;
    use exchange_host::{
        async_trait, ChannelId, ChannelRecord, Channels, ConfigStore, ConnectionSettings,
        CredentialRef, CredentialScope, CredentialStore, Field, MemoryChannels,
        MemoryConnectionRegistry, Secret, SecretBatch, SecretStore, SettingsRefusal, SettingsStore,
        StoreError, Tenant,
    };
    use serde_json::{json, Value};
    use tokio_util::sync::CancellationToken;
    use tower::Service;

    use crate::audit::{Action, AuditJournal};
    use crate::channel::{
        ChannelDeclarations, ChannelEventSink, ChannelPlacement, ChannelPlacementResolver,
        ChannelRunError, ChannelRunner, ChannelStatus, ChannelSupervisor,
    };
    use crate::dev_identity::DevIdentity;

    const ROSTER: &str = "user:alice@acme,service_account:worker@acme";
    const SENTINEL: &str = "X125-SENTINEL-NOT-A-REAL-SECRET";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-x125-{}",
                crate::entropy::hex::<8>().expect("test entropy")
            ));
            DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .expect("test directory");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct FailOnceSettings {
        inner: SettingsStore,
        fail_at: usize,
        calls: AtomicUsize,
        failed: AtomicBool,
    }

    struct FailFirstCredentialBatch {
        inner: Arc<dyn SecretStore>,
        failed: AtomicBool,
    }

    #[derive(Clone)]
    struct OriginCandidate {
        provider: &'static Provider,
        declared: DeclaredSetting,
    }

    fn origin_candidate() -> OriginCandidate {
        connector_catalog::providers()
            .iter()
            .copied()
            .find_map(|provider| {
                (!provider.auth.is_empty()).then_some(())?;
                declared_settings(provider)
                    .ok()?
                    .into_iter()
                    .find_map(|declared| {
                        matches!(
                            host_pinning(provider, &declared),
                            HostPinning::WholeAuthority(_)
                        )
                        .then_some(OriginCandidate { provider, declared })
                    })
            })
            .expect("catalogue has an authenticated whole-authority declaration")
    }

    struct AuthoritySettings {
        inner: SettingsStore,
        candidate: OriginCandidate,
        status: Mutex<AuthorityStatus>,
        next_revision: AtomicU64,
        break_audit_on_revoke: Option<Arc<AuditJournal>>,
    }

    impl AuthoritySettings {
        fn new(path: &Path, candidate: OriginCandidate) -> Self {
            Self {
                inner: SettingsStore::bind(path).expect("settings store"),
                candidate,
                status: Mutex::new(AuthorityStatus {
                    state: AuthorityState::Unset,
                    revision: None,
                }),
                next_revision: AtomicU64::new(1),
                break_audit_on_revoke: None,
            }
        }

        fn matches(&self, connector: &str, declared: &DeclaredSetting) -> bool {
            connector == self.candidate.provider.id && declared == &self.candidate.declared
        }
    }

    impl ConfigStore for AuthoritySettings {
        fn get(
            &self,
            tenant: &str,
            provider: &str,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.inner.get(tenant, provider, service, field)
        }
    }

    impl ConnectionSettings for AuthoritySettings {
        fn set(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<(), SettingsRefusal> {
            self.set_for_instance(tenant, connector, None, declared, value)
        }

        fn set_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<(), SettingsRefusal> {
            if self.matches(connector, declared) {
                let revision = self.next_revision.fetch_add(1, Ordering::SeqCst);
                *self.status.lock().expect("authority status") = AuthorityStatus {
                    state: AuthorityState::Proposed,
                    revision: Some(revision),
                };
                Ok(())
            } else {
                self.inner
                    .set_for_instance(tenant, connector, instance, declared, value)
            }
        }

        fn clear(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
        ) -> Result<bool, SettingsRefusal> {
            self.clear_for_instance(tenant, connector, None, declared)
        }

        fn clear_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
        ) -> Result<bool, SettingsRefusal> {
            if self.matches(connector, declared) {
                let mut status = self.status.lock().expect("authority status");
                let existed = status.revision.is_some();
                *status = AuthorityStatus {
                    state: AuthorityState::Unset,
                    revision: None,
                };
                Ok(existed)
            } else {
                self.inner
                    .clear_for_instance(tenant, connector, instance, declared)
            }
        }

        fn is_set(&self, tenant: &Tenant, connector: &str, declared: &DeclaredSetting) -> bool {
            self.is_set_for_instance(tenant, connector, None, declared)
        }

        fn is_set_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
        ) -> bool {
            if self.matches(connector, declared) {
                self.status.lock().expect("authority status").state == AuthorityState::Approved
            } else {
                self.inner
                    .is_set_for_instance(tenant, connector, instance, declared)
            }
        }

        fn held_bytes(&self, tenant: &Tenant) -> usize {
            self.inner.held_bytes(tenant)
        }

        fn is_custom_origin(&self, connector: &str, declared: &DeclaredSetting) -> bool {
            self.matches(connector, declared)
        }

        fn authority_status_for_instance(
            &self,
            _tenant: &Tenant,
            connector: &str,
            _instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            if !self.matches(connector, declared) {
                return Err(SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            }
            Ok(*self.status.lock().expect("authority status"))
        }

        fn approve_authority_for_instance(
            &self,
            _tenant: &Tenant,
            connector: &str,
            _instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            revision: u64,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            self.transition(connector, declared, revision, AuthorityState::Approved)
        }

        fn revoke_authority_for_instance(
            &self,
            _tenant: &Tenant,
            connector: &str,
            _instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            revision: u64,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            let status = self.transition(connector, declared, revision, AuthorityState::Revoked)?;
            if let Some(audit) = &self.break_audit_on_revoke {
                audit.refuse_writes_for_test();
            }
            Ok(status)
        }
    }

    impl AuthoritySettings {
        fn transition(
            &self,
            connector: &str,
            declared: &DeclaredSetting,
            revision: u64,
            state: AuthorityState,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            if !self.matches(connector, declared) {
                return Err(SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            }
            let mut status = self.status.lock().expect("authority status");
            let Some(current) = status.revision else {
                return Err(SettingsRefusal::AuthorityUnset {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            };
            if current != revision {
                return Err(SettingsRefusal::AuthorityRevisionConflict {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                    expected: revision,
                    current,
                });
            }
            status.state = state;
            Ok(*status)
        }
    }

    #[async_trait]
    impl SecretStore for FailFirstCredentialBatch {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            self.inner.get(reference).await
        }

        async fn put(&self, reference: &CredentialRef, secret: &Secret) -> Result<(), StoreError> {
            self.inner.put(reference, secret).await
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            self.inner.delete(reference).await
        }

        async fn references(
            &self,
            scope: &CredentialScope,
        ) -> Result<Vec<CredentialRef>, StoreError> {
            self.inner.references(scope).await
        }

        async fn apply(&self, batch: &SecretBatch) -> Result<(), StoreError> {
            if !self.failed.swap(true, Ordering::SeqCst) {
                return Err(StoreError::Unreachable {
                    path: "test credential batch".to_owned(),
                    reason: "the first batch is refused deliberately".to_owned(),
                });
            }
            self.inner.apply(batch).await
        }
    }

    impl FailOnceSettings {
        fn new(path: &Path, fail_at: usize) -> Self {
            Self {
                inner: SettingsStore::bind(path).expect("settings store"),
                fail_at,
                calls: AtomicUsize::new(0),
                failed: AtomicBool::new(false),
            }
        }
    }

    impl ConfigStore for FailOnceSettings {
        fn get(
            &self,
            tenant: &str,
            provider: &str,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.inner.get(tenant, provider, service, field)
        }

        fn get_for_instance(
            &self,
            tenant: &str,
            provider: &str,
            instance: Option<&InstanceId>,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.inner
                .get_for_instance(tenant, provider, instance, service, field)
        }
    }

    impl ConnectionSettings for FailOnceSettings {
        fn set(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<(), SettingsRefusal> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == self.fail_at && !self.failed.swap(true, Ordering::SeqCst) {
                return Err(SettingsRefusal::Unwritable {
                    path: "test-settings".to_owned(),
                    reason: "the test requested one refusal".to_owned(),
                });
            }
            self.inner.set(tenant, connector, declared, value)
        }

        fn clear(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
        ) -> Result<bool, SettingsRefusal> {
            self.inner.clear(tenant, connector, declared)
        }

        fn is_set(&self, tenant: &Tenant, connector: &str, declared: &DeclaredSetting) -> bool {
            self.inner.is_set(tenant, connector, declared)
        }

        fn held_bytes(&self, tenant: &Tenant) -> usize {
            self.inner.held_bytes(tenant)
        }
    }

    struct Harness {
        app: Router,
        registry: Arc<MemoryConnectionRegistry>,
        audit: Arc<AuditJournal>,
        _scratch: Scratch,
    }

    fn harness(fail_setting_at: usize) -> Harness {
        let scratch = Scratch::new();
        let credentials = CredentialStore::bind(scratch.join("credentials")).expect("credentials");
        harness_with_credentials(scratch, credentials.secrets(), fail_setting_at)
    }

    fn harness_with_credentials(
        scratch: Scratch,
        credentials: Arc<dyn SecretStore>,
        fail_setting_at: usize,
    ) -> Harness {
        let settings: Arc<dyn ConnectionSettings> = Arc::new(FailOnceSettings::new(
            &scratch.join("settings.json"),
            fail_setting_at,
        ));
        let registry = Arc::new(MemoryConnectionRegistry::default());
        let audit =
            Arc::new(AuditJournal::bind(scratch.join("audit/events.sqlite")).expect("audit"));
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("roster"),
        ))
        .with_credentials(credentials)
        .with_settings(settings)
        .with_connection_registry(registry.clone())
        .with_audit(audit.clone());
        Harness {
            app: super::super::super::app(state),
            registry,
            audit,
            _scratch: scratch,
        }
    }

    fn authority_harness(candidate: OriginCandidate) -> Harness {
        let scratch = Scratch::new();
        let credentials = CredentialStore::bind(scratch.join("credentials")).expect("credentials");
        let settings: Arc<dyn ConnectionSettings> = Arc::new(AuthoritySettings::new(
            &scratch.join("settings.json"),
            candidate,
        ));
        let registry = Arc::new(MemoryConnectionRegistry::default());
        let audit =
            Arc::new(AuditJournal::bind(scratch.join("audit/events.sqlite")).expect("audit"));
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("roster"),
        ))
        .with_credentials(credentials.secrets())
        .with_settings(settings)
        .with_connection_registry(registry.clone())
        .with_audit(audit.clone());
        Harness {
            app: super::super::super::app(state),
            registry,
            audit,
            _scratch: scratch,
        }
    }

    struct NoChannelDeclarations;
    impl ChannelDeclarations for NoChannelDeclarations {
        fn events(&self, _: &str, _: &str) -> Option<BTreeSet<String>> {
            None
        }
    }

    struct NoChannelPlacement;
    impl ChannelPlacementResolver for NoChannelPlacement {
        fn resolve(&self, _: &ChannelRecord) -> Result<ChannelPlacement, ChannelRunError> {
            Err(ChannelRunError::NoPlacement)
        }
    }

    struct NoChannelRunner;
    #[async_trait]
    impl ChannelRunner for NoChannelRunner {
        async fn run(
            &self,
            _: ChannelRecord,
            _: ChannelPlacement,
            _: Arc<dyn ChannelEventSink>,
            _: CancellationToken,
        ) -> Result<(), ChannelRunError> {
            unreachable!("placement refusal prevents the runner")
        }
    }

    fn authority_harness_with_channel(
        candidate: OriginCandidate,
    ) -> (
        Harness,
        Arc<ChannelSupervisor>,
        ChannelId,
        Arc<AuthoritySettings>,
    ) {
        let scratch = Scratch::new();
        let credentials = CredentialStore::bind(scratch.join("credentials")).expect("credentials");
        let registry = Arc::new(MemoryConnectionRegistry::default());
        let audit =
            Arc::new(AuditJournal::bind(scratch.join("audit/events.sqlite")).expect("audit"));
        let mut settings =
            AuthoritySettings::new(&scratch.join("settings.json"), candidate.clone());
        settings.break_audit_on_revoke = Some(audit.clone());
        let settings = Arc::new(settings);
        let records = Arc::new(MemoryChannels::default());
        let channel_id = ChannelId::new("ch_origin").expect("channel id");
        records
            .set(
                ChannelRecord::new(
                    channel_id.clone(),
                    Tenant::new("acme").expect("tenant"),
                    candidate.provider.id,
                    InstanceId::parse("11111111-1111-4111-8111-111111111111").expect("instance"),
                    "events",
                    ["changed".to_owned()].into_iter().collect(),
                )
                .expect("channel record"),
            )
            .expect("persist channel");
        let supervisor = ChannelSupervisor::new(
            records,
            Arc::new(NoChannelDeclarations),
            Arc::new(NoChannelPlacement),
            Arc::new(NoChannelRunner),
        );
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("roster"),
        ))
        .with_credentials(credentials.secrets())
        .with_settings(settings.clone())
        .with_connection_registry(registry.clone())
        .with_audit(audit.clone())
        .with_channels(supervisor.clone());
        (
            Harness {
                app: super::super::super::app(state),
                registry,
                audit,
                _scratch: scratch,
            },
            supervisor,
            channel_id,
            settings,
        )
    }

    async fn call(
        app: &Router,
        handle: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut service = app.clone().into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("router ready");
        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(AUTHORIZATION, format!("Bearer {handle}"));
        let request = match body {
            Some(body) => request
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("request");
        let response = service.call(request).await.expect("infallible router");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    fn assert_nonapplied_steps_have_reasons(response: &Value) {
        for step in response["steps"].as_array().expect("step array") {
            if step["outcome"] != "applied" {
                assert!(
                    step["reason"]
                        .as_str()
                        .is_some_and(|reason| !reason.is_empty()),
                    "non-applied step has no reason: {step}"
                );
            }
        }
    }

    #[test]
    fn every_config_row_is_projected_once_in_declaration_order() {
        for provider in connector_catalog::providers() {
            let described = describe(provider).expect("shipped declarations are readable");
            let config: Vec<_> = described
                .iter()
                .filter(|field| field.view.provenance == "provider.config")
                .collect();
            assert_eq!(config.len(), provider.config.len(), "{}", provider.id);
            for (projected, declared) in config.into_iter().zip(provider.config) {
                assert_eq!(projected.view.name, declared.name, "{}", provider.id);
                assert_eq!(projected.view.service.as_deref(), Some(declared.service));
                assert_eq!(projected.view.binds.as_deref(), Some(declared.binds));
            }
        }
    }

    #[test]
    fn jira_and_zendesk_censuses_are_derived_without_a_handwritten_field_list() {
        for provider_id in ["jira", "zendesk"] {
            let provider = catalogued(provider_id).expect("shipped provider");
            let declared: Vec<_> = provider
                .config
                .iter()
                .map(|field| {
                    (
                        field.service.to_owned(),
                        field.name.to_owned(),
                        field.binds.to_owned(),
                    )
                })
                .collect();
            let projected: Vec<_> = describe(provider)
                .expect("shipped declarations are readable")
                .into_iter()
                .filter(|field| field.view.provenance == "provider.config")
                .map(|field| {
                    (
                        field.view.service.expect("config service"),
                        field.view.name,
                        field.view.binds.expect("config binding"),
                    )
                })
                .collect();
            assert_eq!(projected, declared, "{provider_id}");
        }
    }

    #[test]
    fn every_cli_alias_is_derived_from_the_declared_field_identity() {
        for provider in connector_catalog::providers() {
            let described = describe(provider).expect("shipped declarations are readable");
            let mut aliases = BTreeSet::from(["--name".to_owned()]);
            for projected in &described {
                let expected = if projected.view.secret {
                    Vec::new()
                } else {
                    vec![canonical_cli_alias(&projected.view.name)]
                };
                assert_eq!(
                    projected.view.aliases, expected,
                    "{} {}",
                    provider.id, projected.view.identity
                );
                for alias in &projected.view.aliases {
                    assert!(
                        aliases.insert(alias.clone()),
                        "{} repeats canonical alias {alias}",
                        provider.id
                    );
                }
            }
        }

        for (declared_name, expected_alias) in [
            ("site", "--site"),
            ("domain", "--domain"),
            ("subdomain", "--subdomain"),
        ] {
            let projected = connector_catalog::providers()
                .iter()
                .find_map(|provider| {
                    describe(provider)
                        .ok()?
                        .into_iter()
                        .find(|field| !field.view.secret && field.view.name == declared_name)
                })
                .unwrap_or_else(|| panic!("catalogue has no non-secret `{declared_name}` field"));
            assert_eq!(projected.view.aliases, [expected_alias]);
        }
    }

    #[test]
    fn malformed_duplicate_and_secret_cli_aliases_are_refused() {
        let provider = catalogued("jira").expect("jira");
        let mut fields: Vec<_> = describe(provider)
            .expect("shipped declarations are readable")
            .into_iter()
            .map(|field| field.view)
            .collect();
        assert!(validate_cli_aliases(&fields).is_ok());

        let first_aliases = fields[0].aliases.clone();
        fields[0].aliases = vec!["site".to_owned()];
        assert!(validate_cli_aliases(&fields)
            .expect_err("a malformed alias was accepted")
            .contains("malformed"));

        fields[0].aliases = fields[1].aliases.clone();
        assert!(validate_cli_aliases(&fields)
            .expect_err("a duplicate alias was accepted")
            .contains("belongs to both"));

        fields[0].aliases = first_aliases;
        let secret = fields
            .iter_mut()
            .find(|field| field.secret)
            .expect("jira has a secret field");
        secret.aliases = vec!["--api-token".to_owned()];
        assert!(validate_cli_aliases(&fields)
            .expect_err("a secret argv alias was accepted")
            .contains("secret field"));
    }

    #[test]
    fn declaration_choices_are_published_by_the_generic_projection() {
        let mut choice_fields = 0;
        for provider in connector_catalog::providers() {
            let described = describe(provider).expect("shipped declarations are readable");
            for declared in provider.config {
                let parsed = DeclaredSetting::parse(declared.service, declared.binds);
                let Some(choices) = choices_for(provider, declared, parsed.as_ref()) else {
                    continue;
                };
                choice_fields += 1;
                let projected = described
                    .iter()
                    .find(|field| {
                        field.view.identity
                            == format!("config.{}.{}", declared.service, declared.name)
                    })
                    .expect("choice field remains in the plan");
                assert_eq!(projected.view.input, "select", "{}", provider.id);
                assert_eq!(
                    projected
                        .view
                        .choices
                        .as_ref()
                        .expect("published choices")
                        .iter()
                        .map(|choice| (&choice.value, &choice.label))
                        .collect::<Vec<_>>(),
                    choices
                        .iter()
                        .map(|choice| (&choice.value, &choice.label))
                        .collect::<Vec<_>>(),
                    "{}",
                    provider.id
                );
            }
        }
        assert!(
            choice_fields > 0,
            "the shipped catalogue must exercise choices"
        );
    }

    #[test]
    fn authority_revision_is_canonical_decimal_u64() {
        assert_eq!(canonical_revision("1"), Some(1));
        assert_eq!(canonical_revision(&u64::MAX.to_string()), Some(u64::MAX));
        for refused in ["", "0", "01", "+1", "-1", "18446744073709551616"] {
            assert_eq!(canonical_revision(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_noncredential_secret_is_visible_but_never_routed_to_settings() {
        let provider = catalogued("jira").expect("jira");
        let field = ConfigField {
            name: "private_site",
            service: "default",
            label: "Private site",
            help: "A deliberately inconsistent fixture",
            example: None,
            format: "text",
            required: true,
            default: None,
            secret: true,
            docs_url: None,
            binds: "endpoint.site",
            also_binds: &[],
            declaration_json: "{}",
        };
        let described = describe_config(provider, &field, &[], false);
        assert!(described.view.secret);
        assert_eq!(described.view.input, "secret");
        assert!(!described.view.routable);
        assert!(described.view.target.is_none());
        assert!(described.target.is_none());
        assert!(described.view.reason.is_some());
    }

    #[test]
    fn shared_targets_must_publish_the_same_choice_values() {
        let target = |choices: &[&str]| TargetSpec {
            id: "credential.example.token".to_owned(),
            destination: Destination::Credential("example.token".to_owned()),
            choices: Some(choices.iter().map(|choice| (*choice).to_owned()).collect()),
        };
        assert!(same_target(&target(&["one"]), &target(&["one"])));
        assert!(!same_target(&target(&["one"]), &target(&["two"])));
    }

    #[test]
    fn every_provider_credential_has_a_submission_target() {
        for provider in connector_catalog::providers() {
            let described = describe(provider).expect("shipped declarations are readable");
            let targets: BTreeSet<_> = described
                .iter()
                .filter_map(|field| field.target.as_ref().map(|target| target.id.as_str()))
                .collect();
            for credential in provider.auth {
                assert!(
                    targets.contains(format!("credential.{}", credential.name).as_str()),
                    "{} dropped {}",
                    provider.id,
                    credential.name
                );
            }
        }
    }

    #[test]
    fn required_unroutable_fields_stay_visible_and_incomplete() {
        for (provider_id, binding) in [
            ("bitbucket", "path.workspace"),
            ("cloudflare", "path.zone_id"),
            ("vercel", "query.teamId"),
            ("zendesk", "path.appId"),
        ] {
            let provider = catalogued(provider_id).expect("shipped provider");
            let field = describe(provider)
                .expect("shipped declarations are readable")
                .into_iter()
                .find(|field| field.view.binds.as_deref() == Some(binding))
                .expect("declared field remains visible");
            assert!(field.view.required);
            assert!(!field.view.routable);
            assert!(field.view.reason.is_some());
        }
    }

    #[test]
    fn shared_credential_rows_keep_their_identity_and_share_one_target() {
        let provider = catalogued("zendesk").expect("zendesk");
        let rows: Vec<_> = describe(provider)
            .expect("shipped declarations are readable")
            .into_iter()
            .filter(|field| field.view.binds.as_deref() == Some("credential.zendesk.api_token"))
            .collect();
        assert!(rows.len() > 1);
        let identities: BTreeSet<_> = rows.iter().map(|field| &field.view.identity).collect();
        assert_eq!(identities.len(), rows.len());
        assert!(rows.iter().all(|field| {
            field
                .target
                .as_ref()
                .is_some_and(|target| target.id == "credential.zendesk.api_token")
        }));
    }

    #[test]
    fn duplicate_submitted_targets_are_refused_during_deserialization() {
        let body = r#"{"version":"exchange.connection-plan.v1","name":"company","values":{"credential.example.token":"first","credential.example.token":"second"}}"#;
        let refusal = match serde_json::from_str::<Submission>(body) {
            Ok(_) => panic!("a duplicate target was accepted"),
            Err(refusal) => refusal,
        };
        assert!(refusal.to_string().contains("occurs more than once"));
    }

    #[test]
    fn metadata_poor_credentials_use_operation_alternatives_for_requiredness() {
        let slack = catalogued("slack").expect("slack");
        let described = describe(slack).expect("slack declarations");
        let required = |credential: &str| {
            described
                .iter()
                .find(|field| field.view.binds.as_deref() == Some(credential))
                .map(|field| field.view.required)
        };
        assert_eq!(required("credential.slack.bot_token"), Some(true));
        assert_eq!(required("credential.slack.signing_secret"), Some(false));

        for provider in connector_catalog::providers() {
            for credential in provider.auth {
                let expected = provider.operations.iter().any(|operation| {
                    !operation.credentials.is_empty()
                        && operation
                            .credentials
                            .iter()
                            .all(|alternative| alternative.contains(&credential.name))
                });
                if provider.channels.is_empty() {
                    assert_eq!(
                        credential_required(provider, credential.name),
                        expected,
                        "{} {} inverted exists/every",
                        provider.id,
                        credential.name
                    );
                }
            }
        }
    }

    /// The browser fixture and the production serializer are one v1 contract, not two similar
    /// documents maintained in different trees. Values deliberately differ; the wire vocabulary,
    /// optional-member coverage and generic shared-target shape may not.
    #[tokio::test]
    async fn shared_browser_fixture_matches_the_production_plan_wire_shape() {
        fn keys(value: &Value) -> BTreeSet<String> {
            value
                .as_object()
                .expect("a JSON object")
                .keys()
                .cloned()
                .collect()
        }

        fn key_union(values: &[Value]) -> BTreeSet<String> {
            values.iter().flat_map(keys).collect()
        }

        fn key_intersection(values: &[Value]) -> BTreeSet<String> {
            let mut values = values.iter();
            let mut intersection = values.next().map(keys).expect("at least one value");
            for value in values {
                intersection.retain(|key| keys(value).contains(key));
            }
            intersection
        }

        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/fixtures/connection-plan.v1.json"
        )))
        .expect("the committed browser fixture is JSON");
        assert_eq!(fixture["version"], VERSION);
        let fixture_fields = fixture["fields"]
            .as_array()
            .expect("fixture fields")
            .clone();
        for field in &fixture_fields {
            let expected = if field["secret"] == json!(true) {
                json!([])
            } else {
                json!([canonical_cli_alias(
                    field["name"].as_str().expect("fixture field name")
                )])
            };
            assert_eq!(field["aliases"], expected, "fixture field: {field}");
        }

        let harness = harness(usize::MAX);
        let mut live_plans = Vec::new();
        let mut live_fields = Vec::new();
        for provider in connector_catalog::providers() {
            let (status, plan) = call(
                &harness.app,
                "alice",
                Method::GET,
                &format!("/api/connections/{}/plan", provider.id),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{}: {plan}", provider.id);
            live_fields.extend(
                plan["fields"]
                    .as_array()
                    .expect("served fields")
                    .iter()
                    .cloned(),
            );
            live_plans.push(plan);
        }

        // Released 0.18 policies remain dormant. Exercise the same projection with a generic
        // typed policy so the shared fixture's authority member is checked rather than ignored.
        let (authority_provider, authority_declared) = connector_catalog::providers()
            .iter()
            .find_map(|provider| {
                declared_settings(provider)
                    .ok()?
                    .into_iter()
                    .find_map(|declared| {
                        matches!(
                            host_pinning(provider, &declared),
                            HostPinning::WholeAuthority(_)
                        )
                        .then_some((*provider, declared))
                    })
            })
            .expect("catalogue has a whole-authority declaration");
        assert!(describe(authority_provider)
            .expect("production description")
            .iter()
            .all(|field| !field.custom_origin));
        let mut authority_field =
            describe_with_policy(authority_provider, |connector, declared| {
                connector == authority_provider.id && declared == &authority_declared
            })
            .expect("typed-policy description")
            .into_iter()
            .find(|field| field.custom_origin)
            .expect("active custom-origin row")
            .view;
        authority_field.authority = Some(authority_view(
            authority_provider.id,
            "production",
            &authority_declared,
            AuthorityStatus {
                state: AuthorityState::Proposed,
                revision: Some(42),
            },
        ));
        assert_eq!(
            authority_field.aliases,
            [canonical_cli_alias(&authority_field.name)]
        );
        assert_eq!(
            fixture_fields
                .iter()
                .find(|field| field.get("authority").is_some())
                .expect("fixture authority row")["aliases"],
            json!(["--custom-origin"])
        );
        live_fields.push(serde_json::to_value(authority_field).expect("authority wire row"));

        assert_eq!(keys(&fixture), keys(&live_plans[0]));
        assert_eq!(keys(&fixture["apply"]), keys(&live_plans[0]["apply"]));
        assert_eq!(key_union(&fixture_fields), key_union(&live_fields));
        assert_eq!(
            key_intersection(&fixture_fields),
            key_intersection(&live_fields)
        );

        let fixture_targets: Vec<Value> = fixture_fields
            .iter()
            .filter_map(|field| field.get("target"))
            .filter(|target| !target.is_null())
            .cloned()
            .collect();
        let live_targets: Vec<Value> = live_fields
            .iter()
            .filter_map(|field| field.get("target"))
            .filter(|target| !target.is_null())
            .cloned()
            .collect();
        assert!(fixture_targets
            .iter()
            .all(|target| keys(target) == keys(&live_targets[0])));

        let fixture_choices: Vec<Value> = fixture_fields
            .iter()
            .filter_map(|field| field.get("choices").and_then(Value::as_array))
            .flatten()
            .cloned()
            .collect();
        let live_choices: Vec<Value> = live_fields
            .iter()
            .filter_map(|field| field.get("choices").and_then(Value::as_array))
            .flatten()
            .cloned()
            .collect();
        assert!(!fixture_choices.is_empty());
        assert!(!live_choices.is_empty());
        assert!(
            fixture_choices
                .iter()
                .chain(&live_choices)
                .all(|choice| keys(choice)
                    == BTreeSet::from(["label".to_owned(), "value".to_owned()]))
        );

        let provenance = |fields: &[Value]| -> BTreeSet<String> {
            fields
                .iter()
                .filter_map(|field| field["provenance"].as_str().map(str::to_owned))
                .collect()
        };
        let expected = BTreeSet::from([
            "exchange".to_owned(),
            "provider.auth".to_owned(),
            "provider.config".to_owned(),
        ]);
        assert_eq!(provenance(&fixture_fields), expected);
        assert_eq!(provenance(&live_fields), expected);

        let has_shared_target = |fields: &[Value]| {
            let mut seen = BTreeSet::new();
            fields.iter().any(|field| {
                field["target"]["id"]
                    .as_str()
                    .is_some_and(|target| !seen.insert(target))
            })
        };
        assert!(has_shared_target(&fixture_fields));
        assert!(live_plans
            .iter()
            .any(|plan| has_shared_target(plan["fields"].as_array().expect("served fields"))));
    }

    #[tokio::test]
    async fn custom_origin_plan_requires_revisioned_operator_approval_and_revocation() {
        let candidate = origin_candidate();
        let provider = candidate.provider;
        let declared = candidate.declared.clone();
        let credential = provider.auth[0].name;
        let harness = authority_harness(candidate);
        let plan_path = format!("/api/connections/{}/plan", provider.id);

        for (method, path, body) in [
            (Method::GET, format!("{plan_path}?version=future"), None),
            (
                Method::POST,
                "/api/connections/not-a-connector/plan".to_owned(),
                Some(json!({"version":"future","name":"production","values":{}})),
            ),
            (
                Method::PUT,
                "/api/connections/not-a-connector/instances/production/settings/default/endpoint.custom_origin/authority".to_owned(),
                Some(json!({"version":"future","revision":"1"})),
            ),
        ] {
            let (status, response) = call(&harness.app, "alice", method, &path, body).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(response, json!({
                "code":"unsupported_connection_plan_version",
                "requested":"future",
                "supported":VERSION,
            }));
        }

        let setting_target = format!("setting.{}.{}", declared.service, declared.binds());
        let credential_target = format!("credential.{credential}");
        let (status, response) = call(
            &harness.app,
            "alice",
            Method::POST,
            &plan_path,
            Some(json!({
                "version": VERSION,
                "name": "production",
                "values": {
                    credential_target: SENTINEL,
                    setting_target.clone(): "custom.example.test"
                }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{response}");
        let step = response["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .find(|step| step["target"] == setting_target)
            .expect("authority step");
        assert_eq!(
            step["reason"],
            "proposal persisted; explicit authority approval is required before runtime use"
        );

        let (status, plan) = call(
            &harness.app,
            "alice",
            Method::GET,
            &format!("{plan_path}?name=production"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{plan}");
        let field = plan["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .find(|field| field["target"]["id"] == setting_target)
            .expect("authority field");
        assert_eq!(field["set"], false);
        assert_eq!(field["authority"]["state"], "proposed");
        assert_eq!(field["authority"]["revision"], "1");
        let target = format!(
            "/api/connections/{}/instances/production/settings/{}/{}/authority",
            provider.id,
            declared.service,
            declared.binds()
        );
        assert_eq!(
            field["authority"]["actions"]["approve"],
            json!({"method":"PUT","target":target})
        );

        let transition = json!({"version":VERSION,"revision":"1"});
        let (status, _) = call(
            &harness.app,
            "worker",
            Method::PUT,
            &target,
            Some(transition.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, approved) = call(
            &harness.app,
            "alice",
            Method::PUT,
            &target,
            Some(transition.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{approved}");
        assert_eq!(
            approved,
            json!({
                "version":VERSION,
                "connector":provider.id,
                "label":"production",
                "service":declared.service,
                "field":declared.binds(),
                "authority":{"state":"approved","revision":"1"}
            })
        );
        let (_, approved_plan) = call(
            &harness.app,
            "alice",
            Method::GET,
            &format!("{plan_path}?name=production"),
            None,
        )
        .await;
        let approved_field = approved_plan["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .find(|field| field["target"]["id"] == setting_target)
            .expect("authority field");
        assert_eq!(approved_field["set"], true);
        assert_eq!(approved_field["authority"]["state"], "approved");
        let (status, stale) = call(
            &harness.app,
            "alice",
            Method::DELETE,
            &target,
            Some(json!({"version":VERSION,"revision":"2"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(stale["code"], "stale_origin_revision");
        let (status, revoked) = call(
            &harness.app,
            "alice",
            Method::DELETE,
            &target,
            Some(transition),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{revoked}");
        assert_eq!(
            revoked["authority"],
            json!({"state":"revoked","revision":"1"})
        );
        let (_, revoked_plan) = call(
            &harness.app,
            "alice",
            Method::GET,
            &format!("{plan_path}?name=production"),
            None,
        )
        .await;
        let revoked_field = revoked_plan["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .find(|field| field["target"]["id"] == setting_target)
            .expect("authority field");
        assert_eq!(revoked_field["set"], false);
        assert_eq!(revoked_field["authority"]["state"], "revoked");
        assert_eq!(revoked_field["authority"]["actions"], Value::Null);
        let setting_path = target.strip_suffix("/authority").expect("setting route");
        let (status, _) = call(&harness.app, "alice", Method::DELETE, setting_path, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, unset) = call(
            &harness.app,
            "alice",
            Method::PUT,
            &target,
            Some(json!({"version":VERSION,"revision":"1"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(unset["code"], "origin_not_proposed");

        let records = harness
            .audit
            .by_actor("acme", "user", "alice", 100)
            .expect("audit records");
        assert!(records
            .iter()
            .any(|record| record.action == Action::SettingAuthorityApproved));
        assert!(records
            .iter()
            .any(|record| record.action == Action::SettingAuthorityRevoked));
    }

    #[tokio::test]
    async fn persisted_revocation_restarts_channels_before_audit_finalization_can_refuse() {
        let candidate = origin_candidate();
        let provider = candidate.provider;
        let declared = candidate.declared.clone();
        let credential = provider.auth[0].name;
        let (harness, supervisor, channel_id, settings) = authority_harness_with_channel(candidate);
        let target = format!(
            "/api/connections/{}/instances/production/settings/{}/{}/authority",
            provider.id,
            declared.service,
            declared.binds()
        );
        let (status, response) = call(
            &harness.app,
            "alice",
            Method::POST,
            &format!("/api/connections/{}/plan", provider.id),
            Some(json!({
                "version":VERSION,
                "name":"production",
                "values":{
                    (format!("credential.{credential}")):SENTINEL,
                    (format!("setting.{}.{}", declared.service, declared.binds())):"custom.example.test"
                }
            })),
        ).await;
        assert_eq!(status, StatusCode::OK, "{response}");
        let transition = json!({"version":VERSION,"revision":"1"});
        let (status, response) = call(
            &harness.app,
            "alice",
            Method::PUT,
            &target,
            Some(transition.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{response}");

        supervisor.stop(&channel_id);
        assert_eq!(supervisor.status(&channel_id), ChannelStatus::Stopped);
        let (status, _) = call(
            &harness.app,
            "alice",
            Method::DELETE,
            &target,
            Some(transition),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_ne!(
            supervisor.status(&channel_id),
            ChannelStatus::Stopped,
            "persisted revocation must synchronously restart the stored channel before audit completion"
        );
        assert_eq!(
            settings.status.lock().expect("authority status").state,
            AuthorityState::Revoked,
        );
    }

    #[tokio::test]
    async fn operator_plan_reports_partial_persistence_and_retry_resumes_the_same_instance() {
        let harness = harness(2);
        let values = json!({
            "credential.jira.api_token": SENTINEL,
            "setting.default.endpoint.site": "acme",
            "setting.default.username.jira.api_token": "alice@example.test"
        });
        let body = || {
            json!({
                "version": VERSION,
                "name": "company",
                "values": values.clone(),
            })
        };

        let (status, partial) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(body()),
        )
        .await;
        assert_eq!(status, StatusCode::MULTI_STATUS, "{partial}");
        assert_eq!(partial["outcome"], "partial");
        assert_eq!(partial["plan"]["state"], "incomplete");
        assert_nonapplied_steps_have_reasons(&partial);
        assert!(partial["steps"].as_array().is_some_and(|steps| {
            steps.iter().any(|step| {
                step["target"] == "setting.default.endpoint.site" && step["outcome"] == "applied"
            }) && steps.iter().any(|step| {
                step["target"] == "setting.default.username.jira.api_token"
                    && step["outcome"] == "refused"
            })
        }));
        assert!(!partial.to_string().contains(SENTINEL), "{partial}");

        let tenant = Tenant::new("acme").expect("tenant");
        let first = harness
            .registry
            .entries(&tenant, "jira")
            .expect("labels after partial");
        assert_eq!(first.len(), 1);

        let (status, complete) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(body()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{complete}");
        assert_eq!(complete["outcome"], "complete");
        assert_eq!(complete["plan"]["state"], "complete");
        assert_nonapplied_steps_have_reasons(&complete);
        assert!(!complete.to_string().contains(SENTINEL), "{complete}");
        let retried = harness
            .registry
            .entries(&tenant, "jira")
            .expect("labels after retry");
        assert_eq!(retried.len(), 1);
        assert_eq!(retried[0].instance, first[0].instance);

        let supplier = harness
            .audit
            .latest_credential_supplier("acme", "jira", "jira.api_token", Some("company"))
            .expect("supplier query")
            .expect("supplier evidence");
        assert_eq!(supplier.action, Action::CredentialRotated);
        assert!(!serde_json::to_string(&supplier)
            .expect("audit json")
            .contains(SENTINEL));

        let renamed = json!({
            "version": VERSION,
            "name": "production",
            "current_name": "company",
            "values": {}
        });
        let (status, renamed) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(renamed),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{renamed}");
        assert_eq!(renamed["plan"]["selection"], "production");
        assert!(!renamed.to_string().contains(first[0].instance.as_str()));
        let after_rename = harness
            .registry
            .entries(&tenant, "jira")
            .expect("renamed label");
        assert_eq!(after_rename[0].instance, first[0].instance);
    }

    #[tokio::test]
    async fn retry_can_add_an_omitted_credential_without_changing_the_instance() {
        let harness = harness(usize::MAX);
        let create = json!({
            "version": VERSION,
            "name": "company",
            "values": {"credential.slack.bot_token": SENTINEL}
        });
        let (status, created) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/slack/plan",
            Some(create),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let tenant = Tenant::new("acme").expect("tenant");
        let before = harness
            .registry
            .entries(&tenant, "slack")
            .expect("created label");
        assert_eq!(before.len(), 1);

        let add = json!({
            "version": VERSION,
            "name": "company",
            "values": {"credential.slack.signing_secret": "SECOND-SENTINEL"}
        });
        let (status, added) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/slack/plan",
            Some(add),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{added}");
        assert_nonapplied_steps_have_reasons(&added);
        let after = harness
            .registry
            .entries(&tenant, "slack")
            .expect("retained label");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].instance, before[0].instance);
        let signing = added["plan"]["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .find(|field| field["target"]["id"] == "credential.slack.signing_secret")
            .expect("signing-secret row");
        assert_eq!(signing["set"], true);
        assert!(!added.to_string().contains("SECOND-SENTINEL"));
    }

    #[tokio::test]
    async fn refused_new_credential_batch_returns_a_fresh_unselected_plan_and_rolls_back_label() {
        let scratch = Scratch::new();
        let credentials = CredentialStore::bind(scratch.join("credentials")).expect("credentials");
        let failing: Arc<dyn SecretStore> = Arc::new(FailFirstCredentialBatch {
            inner: credentials.secrets(),
            failed: AtomicBool::new(false),
        });
        let harness = harness_with_credentials(scratch, failing, usize::MAX);
        let body = || {
            json!({
                "version": VERSION,
                "name": "company",
                "values": {"credential.jira.api_token": SENTINEL}
            })
        };
        let (status, refused) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(body()),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}");
        assert_eq!(refused["outcome"], "refused");
        assert_eq!(refused["plan"]["selection"], Value::Null);
        assert_eq!(refused["plan"]["labels"], json!([]));
        assert_nonapplied_steps_have_reasons(&refused);
        let tenant = Tenant::new("acme").expect("tenant");
        assert!(harness
            .registry
            .entries(&tenant, "jira")
            .expect("registry after refusal")
            .is_empty());

        let (status, retried) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(body()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{retried}");
        assert_eq!(retried["plan"]["selection"], "company");
    }

    #[tokio::test]
    async fn audit_finish_failure_reports_the_write_that_survived_as_partial() {
        let harness = harness(usize::MAX);
        rusqlite::Connection::open(harness.audit.path())
            .expect("open audit database")
            .execute_batch(
                "CREATE TRIGGER fail_x125_audit_finish BEFORE UPDATE ON audit_records \
                 BEGIN SELECT RAISE(FAIL, 'deliberate audit finish refusal'); END;",
            )
            .expect("install audit finish refusal");
        let body = json!({
            "version": VERSION,
            "name": "company",
            "values": {"credential.jira.api_token": SENTINEL}
        });
        let (status, partial) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::MULTI_STATUS, "{partial}");
        assert_eq!(partial["outcome"], "partial");
        assert_eq!(partial["plan"]["selection"], "company");
        assert_nonapplied_steps_have_reasons(&partial);
        assert!(partial["steps"].as_array().is_some_and(|steps| steps
            .iter()
            .any(|step| step["target"] == "credential.jira.api_token"
                && step["outcome"] == "applied")));
        assert!(!partial.to_string().contains(SENTINEL));
    }

    #[tokio::test]
    async fn audit_begin_failure_is_a_structured_refusal_before_any_write() {
        let harness = harness(usize::MAX);
        rusqlite::Connection::open(harness.audit.path())
            .expect("open audit database")
            .execute_batch(
                "CREATE TRIGGER fail_x125_audit_begin BEFORE INSERT ON audit_records \
                 WHEN NEW.outcome = 'attempted' \
                 BEGIN SELECT RAISE(FAIL, 'deliberate audit begin refusal'); END;",
            )
            .expect("make audit begin unavailable");
        let body = json!({
            "version": VERSION,
            "name": "company",
            "values": {"credential.jira.api_token": SENTINEL}
        });
        let (status, refused) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}");
        assert_eq!(refused["outcome"], "refused");
        assert_eq!(refused["plan"]["selection"], Value::Null);
        assert_nonapplied_steps_have_reasons(&refused);
        let tenant = Tenant::new("acme").expect("tenant");
        assert!(harness
            .registry
            .entries(&tenant, "jira")
            .expect("registry after audit refusal")
            .is_empty());
    }

    #[tokio::test]
    async fn plan_get_and_post_are_operator_only() {
        let harness = harness(usize::MAX);
        for method in [Method::GET, Method::POST] {
            let body = (method == Method::POST).then(|| {
                json!({
                    "version": VERSION,
                    "name": "company",
                    "values": {"credential.jira.api_token": SENTINEL}
                })
            });
            let (status, refusal) = call(
                &harness.app,
                "worker",
                method,
                "/api/connections/jira/plan",
                body,
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{refusal}");
            assert!(!refusal.to_string().contains(SENTINEL));
        }
    }
}
