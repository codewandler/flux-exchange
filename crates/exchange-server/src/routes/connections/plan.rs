//! The declaration-driven labelled connection plan.
//!
//! This is deliberately a value-free projection, not another store. Field rows come from the
//! connector catalogue; owner-bound FXLM owns every connection and credential mutation.

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{ConfigField, Provider};
use exchange_host::{
    AuthorityState, AuthorityStatus, ConnectionLabel, DeclaredSetting, HostPinning, InstanceId,
    Tenant,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;

const VERSION: &str = crate::protocol::CONNECTION_PLAN_V2.as_str();

pub(super) fn read_route() -> MethodRouter<AppState> {
    get(show)
}

pub(super) fn write_route() -> MethodRouter<AppState> {
    post(reject_secret_json)
}

pub(super) fn authority_route() -> MethodRouter<AppState> {
    get(inspect_authority)
        .put(approve_authority)
        .delete(revoke_authority)
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    name: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSelection {
    connector: String,
    selection: serde_json::Value,
}

/// One value-free refusal from the native plan projection.
pub(crate) struct NativePlanRefusal {
    pub(crate) code: &'static str,
    pub(crate) status: u16,
    pub(crate) retry: &'static str,
}

/// Value-free plan facts consumed by the local-management proposal validator.
pub(crate) struct NativePlanSnapshot {
    pub(crate) canonical: Vec<u8>,
    pub(crate) credential_revision: Option<String>,
    pub(crate) plan_revision: String,
    pub(crate) targets: Vec<NativeTargetFact>,
}

/// One ordered target fact without a stored value or credential-presence bit.
pub(crate) struct NativeTargetFact {
    pub(crate) id: String,
    pub(crate) revision: String,
    pub(crate) required: bool,
    pub(crate) partition: crate::local_management::proposal::TargetPartition,
}

/// Project the same canonical v2 body used by HTTP after a closed native PLAN_QUERY.
pub(crate) fn native_query(
    state: &AppState,
    tenant: &Tenant,
    payload: &[u8],
) -> Result<Vec<u8>, NativePlanRefusal> {
    let request: NativeSelection = serde_json::from_slice(payload)
        .map_err(|_| native_refusal("invalid_request", 400, "never"))?;
    let selection = match request.selection {
        serde_json::Value::Null => None,
        serde_json::Value::String(label) => Some(label),
        _ => return Err(native_refusal("invalid_request", 400, "never")),
    };
    let snapshot = native_snapshot(state, tenant, &request.connector, selection.as_deref())?;
    Ok(snapshot.canonical)
}

/// Resolve one exact plan snapshot for a BEGIN before any transaction allocation.
pub(crate) fn native_snapshot(
    state: &AppState,
    tenant: &Tenant,
    connector: &str,
    selection: Option<&str>,
) -> Result<NativePlanSnapshot, NativePlanRefusal> {
    let provider =
        catalogued(connector).ok_or_else(|| native_refusal("unknown_connector", 404, "refresh"))?;
    match state.connection_publication_pending(tenant, provider.id) {
        Ok(false) => {}
        Ok(true) => return Err(native_refusal("connect_busy", 409, "refresh")),
        Err(()) => return Err(native_refusal("store_unavailable", 503, "operator")),
    }
    if let Some(label) = selection {
        let label = ConnectionLabel::new(label)
            .map_err(|_| native_refusal("invalid_label", 422, "never"))?;
        let registry = state
            .connection_registry()
            .ok_or_else(|| native_refusal("local_management_unavailable", 503, "operator"))?;
        match registry.resolve(tenant, provider.id, &label) {
            Ok(Some(_)) => {}
            Ok(None) => return Err(native_refusal("unknown_label", 404, "refresh")),
            Err(_) => {
                return Err(native_refusal("store_unavailable", 503, "operator"));
            }
        }
    }
    let principal = Principal::new(
        exchange_host::PrincipalKind::User,
        "local-owner",
        tenant.clone(),
    );
    let plan = project(state, &principal, provider, selection)
        .map_err(|_| native_refusal("store_unavailable", 503, "operator"))?;
    let canonical =
        canonical_json(&plan).map_err(|_| native_refusal("internal_refusal", 500, "operator"))?;
    let mut seen_targets = BTreeSet::new();
    let targets = plan
        .fields
        .iter()
        .filter_map(|field| {
            field.target.as_ref().and_then(|target| {
                // One connector address may be shared by several presentation fields. FXLM's
                // target universe carries that address exactly once, in first plan order.
                seen_targets
                    .insert(target.id.as_str())
                    .then(|| NativeTargetFact {
                        id: target.id.clone(),
                        revision: target.revision.clone(),
                        required: field.required,
                        partition: if target.id == "connection.name" {
                            crate::local_management::proposal::TargetPartition::ConnectionName
                        } else if field.secret {
                            crate::local_management::proposal::TargetPartition::Credential
                        } else if field.authority.is_some() {
                            crate::local_management::proposal::TargetPartition::Authority
                        } else {
                            crate::local_management::proposal::TargetPartition::Setting
                        },
                    })
            })
        })
        .collect();
    Ok(NativePlanSnapshot {
        canonical,
        credential_revision: plan.credential_revision,
        plan_revision: plan.plan_revision,
        targets,
    })
}

const fn native_refusal(code: &'static str, status: u16, retry: &'static str) -> NativePlanRefusal {
    NativePlanRefusal {
        code,
        status,
        retry,
    }
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
    action: &'static str,
    authority: AuthorityTransition,
}

#[derive(Serialize)]
struct AuthorityInspectionResponse {
    version: &'static str,
    connector: String,
    label: String,
    service: String,
    field: String,
    authority: AuthorityInspection,
}

#[derive(Serialize)]
struct AuthorityInspection {
    state: AuthorityViewState,
    revision: String,
    origin: String,
}

#[derive(Serialize)]
struct AuthorityPartialResponse {
    version: &'static str,
    connector: String,
    label: String,
    service: String,
    field: String,
    action: &'static str,
    authority: AuthorityTransition,
    outcome: &'static str,
    may_have_happened: bool,
}

#[derive(Serialize)]
struct AuthorityTransition {
    state: AuthorityViewState,
    revision: String,
}

#[derive(Clone, Serialize)]
struct Plan {
    connector: &'static str,
    credential_revision: Option<String>,
    fields: Vec<FieldView>,
    labels: Vec<String>,
    plan_revision: String,
    selection: Option<String>,
    state: PlanState,
    vendor: &'static str,
    version: &'static str,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum PlanState {
    Complete,
    Incomplete,
}

#[derive(Clone, Serialize)]
struct FieldView {
    aliases: Vec<String>,
    also_binds: Vec<String>,
    authority: Option<AuthorityView>,
    binds: Option<String>,
    choices: Option<Vec<ChoiceView>>,
    help: String,
    identity: String,
    input: String,
    label: String,
    name: String,
    provenance: &'static str,
    reason: Option<String>,
    required: bool,
    routable: bool,
    secret: bool,
    service: Option<String>,
    set: Option<bool>,
    target: Option<TargetView>,
}

#[derive(Clone, Serialize)]
struct AuthorityView {
    actions: Vec<&'static str>,
    revision: Option<String>,
    state: AuthorityViewState,
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
struct TargetView {
    id: String,
    revision: String,
}

#[derive(Clone, Serialize)]
struct ChoiceView {
    value: String,
    label: String,
}

#[derive(Clone)]
enum Destination {
    ConnectionLabel,
    Credential(String),
    Settings(Vec<DeclaredSetting>),
}

#[derive(Clone)]
struct TargetSpec {
    id: String,
    destination: Destination,
    choices: Option<Vec<String>>,
    custom_origin: bool,
}

struct DescribedField {
    view: FieldView,
    target: Option<TargetSpec>,
    custom_origin: bool,
}

struct ConnectionContext {
    labels: Vec<String>,
    selected_instance: Option<InstanceId>,
}

async fn show(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
    Query(query): Query<Selection>,
) -> Response {
    if query.version.as_deref() != Some(VERSION) {
        return unsupported_version(query.version.as_deref().unwrap_or("missing"));
    }
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    match project(&state, &principal, provider, query.name.as_deref()) {
        Ok(plan) => canonical_plan_response(&plan),
        Err(response) => *response,
    }
}

async fn reject_secret_json() -> Response {
    (
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        Json(json!({ "code": "secret_json_forbidden" })),
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

async fn inspect_authority(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((connector, label, service, field)): Path<(String, String, String, String)>,
) -> Response {
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
    let instance = match invocation_instance(&state, &principal, provider, Some(&label)).await {
        Ok(instance) => instance,
        Err(response) => return response,
    };
    let status = match settings.authority_status_for_instance(
        principal.tenant(),
        provider.id,
        instance.as_ref(),
        &declared,
    ) {
        Ok(status) => status,
        Err(refusal) => return authority_refused(&refusal),
    };
    let (Some(revision), Some(origin)) = (status.revision, status.origin) else {
        return authority_refused(&SettingsRefusal::AuthorityUnset {
            connector: provider.id.to_owned(),
            setting: declared.binds(),
        });
    };
    Json(AuthorityInspectionResponse {
        version: VERSION,
        connector: provider.id.to_owned(),
        label,
        service: declared.service.clone(),
        field: declared.binds(),
        authority: AuthorityInspection {
            state: status.state.into(),
            revision: revision.to_string(),
            origin,
        },
    })
    .into_response()
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
    let Some(_claim) = state.claim_connection(principal.tenant(), provider.id) else {
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
    let action_name = if approve { "approved" } else { "revoked" };
    let audit = match begin_audit(
        &state,
        &request_id,
        &principal,
        action,
        AuditTarget::SettingAuthority {
            connector: provider.id.to_owned(),
            service: declared.service.clone(),
            field: declared.binds(),
            revision: revision.to_string(),
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
    // Persisted authority invalidates the runtime snapshot immediately. Cancellation alone is not
    // acknowledgment: the response waits until every old projection terminates before any new
    // projection can observe the changed authority.
    let replacement_failed = match state.channels() {
        Some(channels) => channels
            .replace_authority(principal.tenant(), provider.id)
            .await
            .is_err(),
        None => false,
    };
    let revision = transition
        .revision
        .expect("transition has revision")
        .to_string();
    let authority = AuthorityTransition {
        state: transition.state.into(),
        revision,
    };
    let audit_failed = audit
        .finish(&state, &request_id, &principal, StatusCode::OK)
        .is_err();
    if replacement_failed || audit_failed {
        return (
            StatusCode::MULTI_STATUS,
            Json(AuthorityPartialResponse {
                version: VERSION,
                connector: provider.id.to_owned(),
                label,
                service: declared.service.clone(),
                field: declared.binds(),
                action: action_name,
                authority,
                outcome: "partial",
                may_have_happened: true,
            }),
        )
            .into_response();
    }
    Json(AuthorityResponse {
        version: VERSION,
        connector: provider.id.to_owned(),
        label,
        service: declared.service.clone(),
        field: declared.binds(),
        action: action_name,
        authority,
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

fn same_target(left: &TargetSpec, right: &TargetSpec) -> bool {
    let same_destination = match (&left.destination, &right.destination) {
        (Destination::ConnectionLabel, Destination::ConnectionLabel) => true,
        (Destination::Credential(left), Destination::Credential(right)) => left == right,
        (Destination::Settings(left), Destination::Settings(right)) => left == right,
        _ => false,
    };
    same_destination && left.choices == right.choices && left.custom_origin == right.custom_origin
}

fn projected_targets(
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

fn project(
    state: &AppState,
    principal: &Principal,
    provider: &'static Provider,
    selection: Option<&str>,
) -> Result<Plan, Box<Response>> {
    match state.connection_publication_pending(principal.tenant(), provider.id) {
        Ok(false) => {}
        Ok(true) => return Err(Box::new(change_in_flight(provider))),
        Err(()) => return Err(Box::new(no_store())),
    }
    let context = connection_context(state, principal, provider, selection)?;
    let settings = state
        .settings()
        .ok_or_else(|| Box::new(no_settings_store()))?;
    let described = describe_for(provider, settings.as_ref())
        .map_err(|refusal| Box::new(settings_refused(&refusal)))?;
    projected_targets(provider, &described)?;

    let name_target = TargetSpec {
        id: "connection.name".to_owned(),
        destination: Destination::ConnectionLabel,
        choices: None,
        custom_origin: false,
    };
    let mut fields = Vec::with_capacity(described.len() + 1);
    fields.push(FieldView {
        aliases: vec!["--name".to_owned()],
        also_binds: Vec::new(),
        authority: None,
        binds: None,
        choices: None,
        help: "A tenant-scoped label such as company, sandbox, or production.".to_owned(),
        identity: "connection.name".to_owned(),
        input: "text".to_owned(),
        label: "Connection name".to_owned(),
        name: "name".to_owned(),
        provenance: "exchange",
        reason: None,
        required: true,
        routable: true,
        secret: false,
        service: None,
        set: Some(selection.is_some()),
        target: Some(
            target_view(&name_target)
                .map_err(|refusal| Box::new(internal_plan_refusal(refusal)))?,
        ),
    });

    for mut field in described {
        field.view.target = field
            .target
            .as_ref()
            .map(target_view)
            .transpose()
            .map_err(|refusal| Box::new(internal_plan_refusal(refusal)))?;
        if field.custom_origin {
            let status = match (selection, field.target.as_ref()) {
                (
                    Some(_),
                    Some(TargetSpec {
                        destination: Destination::Settings(declared),
                        ..
                    }),
                ) => settings
                    .authority_status_for_instance(
                        principal.tenant(),
                        provider.id,
                        context.selected_instance.as_ref(),
                        &declared[0],
                    )
                    .map_err(|refusal| Box::new(settings_refused(&refusal)))?,
                _ => AuthorityStatus {
                    state: AuthorityState::Unset,
                    revision: None,
                    origin: None,
                },
            };
            field.view.authority = Some(authority_view(status));
        }

        field.view.set = if field.view.secret {
            None
        } else {
            Some(match (&field.target, selection) {
                (_, None) | (None, _) => false,
                (Some(target), Some(_)) => match &target.destination {
                    Destination::ConnectionLabel | Destination::Credential(_) => false,
                    Destination::Settings(_) if field.custom_origin => {
                        field.view.authority.as_ref().is_some_and(|authority| {
                            matches!(authority.state, AuthorityViewState::Approved)
                        })
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
            })
        };
        fields.push(field.view);
    }

    validate_cli_aliases(&fields).map_err(|reason| {
        Box::new(refuse(
            StatusCode::BAD_GATEWAY,
            format!(
                "connector `{}` cannot publish an unambiguous connection-plan alias set: {reason}",
                provider.id
            ),
            json!({ "connector": provider.id }),
        ))
    })?;

    let credential_revision = selection
        .map(|label| {
            let heads = state
                .credential_heads()
                .ok_or_else(|| "the durable credential-head store is unavailable".to_owned())?;
            let key = crate::credential_head::CredentialHeadKey::new(
                principal.tenant().as_str(),
                provider.id,
                label,
            )
            .map_err(|error| error.to_string())?;
            heads
                .current(&key)
                .map(|head| head.as_str().to_owned())
                .map_err(|error| error.to_string())
        })
        .transpose()
        .map_err(|refusal| Box::new(internal_plan_refusal(refusal)))?;
    validate_plan_shape(
        &fields,
        &context.labels,
        selection,
        credential_revision.as_deref(),
    )
    .map_err(|refusal| Box::new(internal_plan_refusal(refusal)))?;
    let complete = fields
        .iter()
        .filter(|field| field.required)
        .all(|field| field.routable && (field.secret || field.set == Some(true)));
    let plan_revision = plan_revision(provider, &fields)
        .map_err(|refusal| Box::new(internal_plan_refusal(refusal)))?;
    Ok(Plan {
        connector: provider.id,
        credential_revision,
        fields,
        labels: context.labels,
        plan_revision,
        selection: selection.map(str::to_owned),
        state: if complete {
            PlanState::Complete
        } else {
            PlanState::Incomplete
        },
        vendor: provider.vendor,
        version: VERSION,
    })
}

fn validate_plan_shape(
    fields: &[FieldView],
    labels: &[String],
    selection: Option<&str>,
    credential_revision: Option<&str>,
) -> Result<(), String> {
    if fields.is_empty() || fields.len() > 128 {
        return Err("connection plan field count is outside 1..=128".to_owned());
    }
    if labels.len() > 256
        || labels.windows(2).any(|pair| pair[0] >= pair[1])
        || labels
            .iter()
            .any(|label| ConnectionLabel::new(label).is_err())
    {
        return Err("connection plan labels are not a sorted unique bounded set".to_owned());
    }
    match (selection, credential_revision) {
        (None, None) => {}
        (Some(selected), Some(revision))
            if labels.iter().any(|label| label == selected) && lower_hex_256_nonzero(revision) => {}
        _ => {
            return Err(
                "connection plan selection and credential revision are inconsistent".to_owned(),
            )
        }
    }

    let mut identities = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    let mut target_revisions = BTreeMap::<&str, &str>::new();
    for field in fields {
        if !plan_atom(&field.identity)
            || !identities.insert(&field.identity)
            || !plan_atom(&field.name)
            || !plan_atom(&field.input)
            || field
                .service
                .as_deref()
                .is_some_and(|value| !plan_atom(value))
            || field
                .binds
                .as_deref()
                .is_some_and(|value| !plan_atom(value))
            || field.help.len() > 2_048
            || field.label.is_empty()
            || field.label.len() > 2_048
            || field
                .reason
                .as_deref()
                .is_some_and(|reason| reason.is_empty() || reason.len() > 2_048)
        {
            return Err(format!(
                "connection plan field `{}` exceeds its string or identity bounds",
                field.identity
            ));
        }
        if field.aliases.len() > 64
            || field
                .aliases
                .iter()
                .any(|alias| alias.len() > 66 || !valid_cli_alias(alias) || !aliases.insert(alias))
            || (field.secret && !field.aliases.is_empty())
        {
            return Err(format!(
                "connection plan field `{}` has an invalid alias set",
                field.identity
            ));
        }
        let also_binds: BTreeSet<_> = field.also_binds.iter().collect();
        if field.also_binds.len() > 64
            || also_binds.len() != field.also_binds.len()
            || field.also_binds.iter().any(|binds| !plan_atom(binds))
        {
            return Err(format!(
                "connection plan field `{}` has invalid also-binds",
                field.identity
            ));
        }
        if let Some(choices) = &field.choices {
            let values: BTreeSet<_> = choices.iter().map(|choice| &choice.value).collect();
            if choices.is_empty()
                || choices.len() > 256
                || values.len() != choices.len()
                || choices.iter().any(|choice| {
                    choice.value.len() > 1_024
                        || choice.label.is_empty()
                        || choice.label.len() > 2_048
                })
            {
                return Err(format!(
                    "connection plan field `{}` has invalid choices",
                    field.identity
                ));
            }
        }
        if field.routable != field.target.is_some()
            || field.routable == field.reason.is_some()
            || field.secret != field.set.is_none()
        {
            return Err(format!(
                "connection plan field `{}` contradicts routing or live-set semantics",
                field.identity
            ));
        }
        if let Some(target) = &field.target {
            if target.id.is_empty() || target.id.len() > 512 || !lower_hex_256(&target.revision) {
                return Err(format!(
                    "connection plan field `{}` has an invalid target",
                    field.identity
                ));
            }
            match target_revisions.insert(&target.id, &target.revision) {
                Some(existing) if existing != target.revision => {
                    return Err(format!(
                        "connection plan target `{}` has conflicting revisions",
                        target.id
                    ));
                }
                _ => {}
            }
        }
        if let Some(authority) = &field.authority {
            let valid = match authority.state {
                AuthorityViewState::Unset => {
                    authority.revision.is_none() && authority.actions.is_empty()
                }
                AuthorityViewState::Proposed => {
                    authority
                        .revision
                        .as_deref()
                        .is_some_and(canonical_revision_string)
                        && authority.actions == ["approve", "revoke"]
                }
                AuthorityViewState::Approved => {
                    authority
                        .revision
                        .as_deref()
                        .is_some_and(canonical_revision_string)
                        && authority.actions == ["revoke"]
                }
                AuthorityViewState::Revoked => {
                    authority
                        .revision
                        .as_deref()
                        .is_some_and(canonical_revision_string)
                        && authority.actions.is_empty()
                }
            };
            if !valid {
                return Err(format!(
                    "connection plan field `{}` has an invalid authority state",
                    field.identity
                ));
            }
        }
    }
    Ok(())
}

fn plan_atom(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512
}

fn lower_hex_256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex_256_nonzero(value: &str) -> bool {
    lower_hex_256(value) && value.bytes().any(|byte| byte != b'0')
}

fn canonical_revision_string(value: &str) -> bool {
    canonical_revision(value).is_some()
}

fn target_view(target: &TargetSpec) -> Result<TargetView, String> {
    Ok(TargetView {
        id: target.id.clone(),
        revision: target_revision(target)?,
    })
}

fn target_revision(target: &TargetSpec) -> Result<String, String> {
    let destination = match &target.destination {
        Destination::ConnectionLabel => json!({"kind": "connection_label"}),
        Destination::Credential(credential) => {
            json!({"credential": credential, "kind": "credential"})
        }
        Destination::Settings(settings) => json!({
            "kind": "settings",
            "settings": settings
                .iter()
                .map(|setting| json!({
                    "binds": setting.binds(),
                    "service": setting.service,
                }))
                .collect::<Vec<_>>(),
        }),
    };
    let input = json!({
        "authority": target.custom_origin.then_some("custom_origin"),
        "choices": target.choices,
        "destination": destination,
        "target": target.id,
    });
    domain_digest(
        b"exchange.connection-plan.v2.target-revision",
        &canonical_json(&input)?,
    )
}

fn plan_revision(provider: &Provider, fields: &[FieldView]) -> Result<String, String> {
    let fields = fields
        .iter()
        .map(|field| {
            json!({
                "aliases": field.aliases,
                "also_binds": field.also_binds,
                "authority": field.authority.as_ref().map(|_| "custom_origin"),
                "binds": field.binds,
                "choices": field.choices,
                "help": field.help,
                "identity": field.identity,
                "input": field.input,
                "label": field.label,
                "name": field.name,
                "provenance": field.provenance,
                "reason": field.reason,
                "required": field.required,
                "secret": field.secret,
                "service": field.service,
                "target": field.target,
            })
        })
        .collect::<Vec<_>>();
    let input = json!({
        "connector": provider.id,
        "fields": fields,
        "schema": VERSION,
        "vendor": provider.vendor,
    });
    domain_digest(
        b"exchange.connection-plan.v2.plan-revision",
        &canonical_json(&input)?,
    )
}

fn domain_digest(domain: &[u8], canonical: &[u8]) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update([0]);
    digest.update(canonical);
    let digest = digest.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| "cannot encode SHA-256 plan revision".to_owned())?;
    }
    Ok(encoded)
}

fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    // Without serde_json's preserve_order feature Value uses a sorted map. These plan schemas use
    // only strings, booleans, nulls and bounded integers, so rendering that tree is RFC 8785.
    let value = serde_json::to_value(value)
        .map_err(|error| format!("cannot construct canonical connection plan: {error}"))?;
    serde_json::to_vec(&value)
        .map_err(|error| format!("cannot serialize canonical connection plan: {error}"))
}

fn canonical_plan_response(plan: &Plan) -> Response {
    match canonical_json(plan) {
        Ok(bytes) if bytes.len() <= 65_536 => {
            ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
        }
        Ok(_) => internal_plan_refusal(
            "connection plan exceeds the exchange.connection-plan.v2 control bound".to_owned(),
        ),
        Err(refusal) => internal_plan_refusal(refusal),
    }
}

fn internal_plan_refusal(reason: String) -> Response {
    refuse(
        StatusCode::INTERNAL_SERVER_ERROR,
        reason,
        json!({"code": "internal_refusal"}),
    )
}

/// The published convenience spelling is derived only from the declaration's stable field name.
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

fn connection_context(
    state: &AppState,
    principal: &Principal,
    provider: &'static Provider,
    selection: Option<&str>,
) -> Result<ConnectionContext, Box<Response>> {
    let _credentials = state.credentials().ok_or_else(|| Box::new(no_store()))?;
    let registry = state
        .connection_registry()
        .ok_or_else(|| Box::new(no_registry()))?;
    let entries = registry
        .entries(principal.tenant(), provider.id)
        .map_err(|refusal| Box::new(registry_refused(&refusal)))?;
    let mut labels: Vec<_> = entries
        .iter()
        .map(|entry| entry.label.as_str().to_owned())
        .collect();
    labels.sort();
    labels.dedup();
    let selected_instance = match selection {
        None => None,
        Some(selected) => {
            let label = ConnectionLabel::new(selected)
                .map_err(|refusal| Box::new(registry_refused(&refusal)))?;
            let Some(entry) = entries.iter().find(|entry| entry.label == label) else {
                return Err(Box::new(unknown_label(provider, &label)));
            };
            // The first named connection still occupies the legacy unqualified settings layout.
            // Creating the second connection migrates that layout to its UUID before publication;
            // from then on every selected label is qualified. Projecting the sole label through
            // its UUID would hide the first connection's already-durable settings after recovery.
            (entries.len() > 1).then(|| entry.instance.clone())
        }
    };
    Ok(ConnectionContext {
        labels,
        selected_instance,
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
                set: None,
                target: Some(TargetView {
                    id: target.clone(),
                    revision: String::new(),
                }),
                choices: None,
                reason: None,
                authority: None,
            },
            target: Some(TargetSpec {
                id: target,
                destination: Destination::Credential(credential.name.to_owned()),
                choices: None,
                custom_origin: false,
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
                    custom_origin: false,
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
                        custom_origin,
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
        revision: String::new(),
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
            set: (!secret).then_some(false),
            target: target_view,
            choices,
            reason,
            authority: None,
        },
        target,
        custom_origin,
    }
}

fn authority_view(status: AuthorityStatus) -> AuthorityView {
    let revision = status.revision.map(|revision| revision.to_string());
    let actions = match status.state {
        AuthorityState::Proposed => vec!["approve", "revoke"],
        AuthorityState::Approved => vec!["revoke"],
        AuthorityState::Unset | AuthorityState::Revoked => Vec::new(),
    };
    AuthorityView {
        actions,
        revision,
        state: status.state.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{Method, Request as HttpRequest};
    use axum::Router;
    use exchange_host::{
        ConnectionRegistry, ConnectionSettings, CredentialStore, MemoryConnectionRegistry, Secret,
        SecretStore, SettingsStore, Tenant, TenantInstances,
    };
    use serde_json::{json, Value};
    use tower::Service;

    use crate::dev_identity::DevIdentity;

    const ROSTER: &str = "user:alice@acme,user:bob@globex,service_account:worker@acme";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-x125-{}",
                crate::entropy::hex::<8>().expect("test entropy")
            ));
            exchange_host::ensure_private_state_directory(&path).expect("test directory");
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

    struct Harness {
        app: Router,
        credentials: Arc<dyn SecretStore>,
        registry: Arc<MemoryConnectionRegistry>,
        heads: Arc<crate::credential_head::CredentialHeadStore>,
        _scratch: Scratch,
    }

    fn harness() -> Harness {
        let scratch = Scratch::new();
        let credentials =
            CredentialStore::bind(scratch.join("credentials")).expect("credential store");
        let settings: Arc<dyn ConnectionSettings> =
            Arc::new(SettingsStore::bind(scratch.join("settings.json")).expect("settings store"));
        let registry = Arc::new(MemoryConnectionRegistry::default());
        let heads = Arc::new(
            crate::credential_head::CredentialHeadStore::migrate_legacy(&scratch.0, &[])
                .expect("empty legacy migration"),
        );
        let ordinary = credentials.secrets();
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("roster"),
        ))
        .with_operator_policy(crate::operator::OperatorPolicy::one("alice"))
        .with_credentials(ordinary.clone())
        .with_settings(settings)
        .with_connection_registry(registry.clone())
        .with_credential_heads(heads.clone());
        Harness {
            app: super::super::super::app(state),
            credentials: ordinary,
            registry,
            heads,
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
        let encoded = body.map(|body| body.to_string().into_bytes());
        let (status, bytes) = call_raw(app, handle, method, path, encoded).await;
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    async fn call_raw(
        app: &Router,
        handle: &str,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> (StatusCode, Vec<u8>) {
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
                .body(Body::from(body)),
            None => request.body(Body::empty()),
        }
        .expect("request");
        let response = service.call(request).await.expect("infallible router");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        (status, bytes.to_vec())
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

    /// The inconsistency under test is **one flag**, so only that flag is authored here.
    ///
    /// This fixture used to be an exhaustive `ConfigField` literal, and every catalogue release
    /// that adds a member to that struct broke it — `also_services` in connector 0.21 (X-146) was
    /// the second time. `ConfigField` is not `#[non_exhaustive]` and should not be: a *consumer*
    /// reading a new member wants to be told it exists. A *fixture* does not, because it is not
    /// making a claim about the shape of the struct. Starting from the provider's own declaration
    /// and overriding the single field the assertions are about says that, and keeps the fixture
    /// honest besides — the rest of it is a real shipped declaration rather than plausible text.
    #[test]
    fn a_noncredential_secret_is_visible_but_never_routed_to_settings() {
        let provider = catalogued("jira").expect("jira");
        let declared = provider
            .config
            .iter()
            .find(|field| field.binds == "endpoint.site")
            .expect("jira declares its site as a non-secret endpoint field");
        // A secret flag on a binding that is not a credential: the deliberate inconsistency.
        let field = ConfigField {
            secret: true,
            ..*declared
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
            custom_origin: false,
        };
        assert!(same_target(&target(&["one"]), &target(&["one"])));
        assert!(!same_target(&target(&["one"]), &target(&["two"])));
    }

    #[test]
    fn every_provider_credential_has_a_projected_target() {
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
    async fn v2_plan_is_closed_and_every_nullable_field_is_explicit() {
        let harness = harness();
        let path = "/api/connections/jira/plan?version=exchange.connection-plan.v2";
        let (status, plan) = call(&harness.app, "alice", Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK, "{plan}");
        assert_eq!(
            plan.as_object()
                .expect("closed plan object")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "connector",
                "credential_revision",
                "fields",
                "labels",
                "plan_revision",
                "selection",
                "state",
                "vendor",
                "version",
            ])
        );
        assert_eq!(plan["credential_revision"], Value::Null);
        assert!(plan["plan_revision"]
            .as_str()
            .is_some_and(|revision| revision.len() == 64));
        for field in plan["fields"].as_array().expect("plan fields") {
            assert_eq!(
                field
                    .as_object()
                    .expect("closed field object")
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([
                    "aliases",
                    "also_binds",
                    "authority",
                    "binds",
                    "choices",
                    "help",
                    "identity",
                    "input",
                    "label",
                    "name",
                    "provenance",
                    "reason",
                    "required",
                    "routable",
                    "secret",
                    "service",
                    "set",
                    "target",
                ]),
                "{field}"
            );
            if field["secret"] == true {
                assert_eq!(field["set"], Value::Null, "{field}");
            } else {
                assert!(field["set"].is_boolean(), "{field}");
            }
            if let Some(target) = field["target"].as_object() {
                assert_eq!(
                    target.keys().map(String::as_str).collect::<BTreeSet<_>>(),
                    BTreeSet::from(["id", "revision"])
                );
            }
        }

        let (status, bytes) = call_raw(&harness.app, "alice", Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, serde_json::to_vec(&plan).expect("canonical plan"));
        assert!(!bytes.ends_with(b"\n"));
    }

    #[tokio::test]
    async fn selected_plan_head_and_state_do_not_encode_secret_presence() {
        let harness = harness();
        let provider = catalogued("jira").expect("jira");
        let tenant = Tenant::new("acme").expect("tenant");
        let label = ConnectionLabel::new("company").expect("label");
        let instance = InstanceId::parse("11111111-1111-4111-8111-111111111111").expect("instance");
        harness
            .registry
            .assign(&tenant, provider.id, &label, &instance)
            .expect("held label");
        let key = crate::credential_head::CredentialHeadKey::new(
            tenant.as_str(),
            provider.id,
            label.as_str(),
        )
        .expect("head key");
        let head = harness.heads.allocate_new(&key).expect("new head");
        harness.heads.insert_new(key, head).expect("publish head");
        let path = "/api/connections/jira/plan?version=exchange.connection-plan.v2&name=company";

        let (status, absent) = call(&harness.app, "alice", Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK, "{absent}");
        let head = absent["credential_revision"]
            .as_str()
            .expect("selected credential head");
        assert_eq!(head.len(), 64);
        assert_ne!(head, "0".repeat(64));
        assert!(absent["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .filter(|field| field["secret"] == true)
            .all(|field| field["set"].is_null()));

        let credentials = declared_credentials(provider);
        let declaration = declaration(provider, &credentials);
        let held = vec![instance.clone()];
        let reference = declaration
            .address_of_for(
                &tenant,
                provider.auth[0].name,
                TenantInstances::held(&held, Some(&instance)),
            )
            .expect("credential address");
        let sentinel = "X134-SECRET-PRESENCE-MUST-NOT-TRAVEL";
        harness
            .credentials
            .put(&reference, &Secret::new(sentinel))
            .await
            .expect("store secret");

        let (status, present) = call(&harness.app, "alice", Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK, "{present}");
        assert_eq!(present, absent);
        assert!(!present.to_string().contains(sentinel));
    }

    #[test]
    fn static_revision_preimages_include_reason_but_exclude_live_facts() {
        let provider = catalogued("jira").expect("jira");
        let target = TargetSpec {
            id: "setting.default.endpoint.site".to_owned(),
            destination: Destination::Settings(vec![DeclaredSetting::parse(
                "default",
                "endpoint.site",
            )
            .expect("setting")]),
            choices: None,
            custom_origin: false,
        };
        let first_target = target_revision(&target).expect("target revision");
        let expected_input = br#"{"authority":null,"choices":null,"destination":{"kind":"settings","settings":[{"binds":"endpoint.site","service":"default"}]},"target":"setting.default.endpoint.site"}"#;
        assert_eq!(
            canonical_json(&json!({
                "authority": Value::Null,
                "choices": Value::Null,
                "destination": {
                    "kind": "settings",
                    "settings": [{"binds":"endpoint.site","service":"default"}]
                },
                "target": "setting.default.endpoint.site",
            }))
            .expect("target preimage input"),
            expected_input
        );
        assert_eq!(
            first_target,
            "45ec877ab7fc252feef8700c75d96d158e6a48353b986eb360ae16594c64d5b9"
        );
        let mut changed_target = target.clone();
        changed_target.choices = Some(vec!["one".to_owned()]);
        assert_ne!(
            target_revision(&changed_target).expect("changed target revision"),
            first_target
        );

        let mut field = FieldView {
            aliases: vec!["--site".to_owned()],
            also_binds: Vec::new(),
            authority: None,
            binds: Some("endpoint.site".to_owned()),
            choices: None,
            help: "Site".to_owned(),
            identity: "config.default.site".to_owned(),
            input: "text".to_owned(),
            label: "Site".to_owned(),
            name: "site".to_owned(),
            provenance: "provider.config",
            reason: Some("static refusal one".to_owned()),
            required: true,
            routable: false,
            secret: false,
            service: Some("default".to_owned()),
            set: Some(false),
            target: None,
        };
        let original = plan_revision(provider, &[field.clone()]).expect("plan revision");
        field.set = Some(true);
        assert_eq!(
            plan_revision(provider, &[field.clone()]).expect("live-set revision"),
            original
        );
        field.reason = Some("static refusal two".to_owned());
        assert_ne!(
            plan_revision(provider, &[field]).expect("changed-reason revision"),
            original
        );
    }

    #[test]
    fn authority_wire_states_are_the_four_closed_value_free_objects() {
        for (state, revision, expected) in [
            (
                AuthorityState::Unset,
                None,
                json!({"actions":[],"revision":null,"state":"unset"}),
            ),
            (
                AuthorityState::Proposed,
                Some(7),
                json!({"actions":["approve","revoke"],"revision":"7","state":"proposed"}),
            ),
            (
                AuthorityState::Approved,
                Some(8),
                json!({"actions":["revoke"],"revision":"8","state":"approved"}),
            ),
            (
                AuthorityState::Revoked,
                Some(9),
                json!({"actions":[],"revision":"9","state":"revoked"}),
            ),
        ] {
            let view = authority_view(AuthorityStatus {
                state,
                revision,
                origin: None,
            });
            assert_eq!(serde_json::to_value(view).expect("authority"), expected);
        }
    }

    #[tokio::test]
    async fn plan_read_requires_v2_and_a_human_principal() {
        let harness = harness();
        for path in [
            "/api/connections/jira/plan",
            "/api/connections/jira/plan?version=exchange.connection-plan.v1",
        ] {
            let (status, _) = call(&harness.app, "alice", Method::GET, path, None).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
        }
        let (status, _) = call(
            &harness.app,
            "alice",
            Method::GET,
            "/api/connections/jira/plan?version=exchange.connection-plan.v2&extra=1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &harness.app,
            "worker",
            Method::GET,
            "/api/connections/jira/plan?version=exchange.connection-plan.v2",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            &harness.app,
            "bob",
            Method::GET,
            "/api/connections/jira/plan?version=exchange.connection-plan.v2",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn legacy_secret_json_is_refused_before_mutation_or_reflection() {
        let harness = harness();
        let sentinel = "X134-LEGACY-SECRET-SENTINEL";
        let (status, refusal) = call(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(json!({
                "version": "exchange.connection-plan.v1",
                "name": "company",
                "values": {"credential.jira.api_token": sentinel}
            })),
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{refusal}");
        assert_eq!(refusal, json!({"code":"secret_json_forbidden"}));
        assert!(!refusal.to_string().contains(sentinel));

        let tenant = Tenant::new("acme").expect("tenant");
        assert!(harness
            .registry
            .entries(&tenant, "jira")
            .expect("registry after refusal")
            .is_empty());

        let (status, bytes) = call_raw(
            &harness.app,
            "alice",
            Method::POST,
            "/api/connections/jira/plan",
            Some(format!("not-json:{sentinel}:%58%31%33%34:WDEzNA==").into_bytes()),
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(bytes, br#"{"code":"secret_json_forbidden"}"#);
        assert!(!String::from_utf8_lossy(&bytes).contains(sentinel));
    }
}
