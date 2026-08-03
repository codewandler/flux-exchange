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
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{ConfigField, Provider};
use exchange_host::{ConnectionLabel, DeclaredSetting, HostPinning, InstanceId, TenantInstances};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::*;

const VERSION: &str = "exchange.connection-plan.v1";

pub(super) fn route() -> MethodRouter<AppState> {
    get(show).post(apply)
}

#[derive(Default, Deserialize)]
struct Selection {
    name: Option<String>,
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
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    // Every semantic refusal below embeds this persisted projection. A caller never has to infer
    // whether a preflight failure wrote anything from a generic error body.
    let unselected = match project(&state, &principal, provider, None).await {
        Ok(plan) => plan,
        Err(response) => return response,
    };
    if body.version != VERSION {
        return preflight_refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("unsupported connection plan version `{}`", body.version),
            &unselected,
        );
    }

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

    let descriptions = match describe(provider) {
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
            steps.push(step(&target.id, StepOutcome::Applied, None));
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
    let described = describe(provider).map_err(|refusal| settings_refused(&refusal))?;
    // A shared target is one browser control. Refuse an internally ambiguous declaration on GET
    // as well as POST rather than publishing a plan the write side cannot honor.
    submission_targets(provider, &described).map_err(|response| *response)?;
    let settings = state.settings().ok_or_else(no_settings_store)?;
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
        fields.push(field.view);
    }

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

fn describe(provider: &'static Provider) -> Result<Vec<DescribedField>, SettingsRefusal> {
    let declared_settings = declared_settings(provider)?;
    let mut described: Vec<DescribedField> = provider
        .config
        .iter()
        .map(|field| describe_config(provider, field, &declared_settings))
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
                binds: Some(target.clone()),
                also_binds: Vec::new(),
                provenance: "provider.auth",
                routable: true,
                set: false,
                target: Some(TargetView { id: target.clone() }),
                choices: None,
                reason: None,
            },
            target: Some(TargetSpec {
                id: target,
                destination: Destination::Credential(credential.name.to_owned()),
                choices: None,
            }),
        });
    }
    Ok(described)
}

fn describe_config(
    provider: &'static Provider,
    field: &ConfigField,
    declared_settings: &[DeclaredSetting],
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
                    && host_pinning(provider, &primary).tenant_may_supply() =>
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
        },
        target,
    }
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{Method, Request as HttpRequest};
    use axum::Router;
    use exchange_host::{
        async_trait, ConfigStore, ConnectionSettings, CredentialRef, CredentialScope,
        CredentialStore, Field, MemoryConnectionRegistry, Secret, SecretBatch, SecretStore,
        SettingsRefusal, SettingsStore, StoreError, Tenant,
    };
    use serde_json::{json, Value};
    use tower::Service;

    use crate::audit::{Action, AuditJournal};
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
        let described = describe_config(provider, &field, &[]);
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
