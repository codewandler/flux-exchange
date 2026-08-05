//! Platform-neutral validation of one helper BEGIN against its owner endpoint's v2 plan.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Vendor ceremony selected by the initiating FXLM opcode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VendorOperation {
    Connect,
    Credential,
}

/// Value-free initiating facts retained between the helper's two native connections.
pub(crate) struct VendorBegin {
    authorities: Vec<BeginAuthority>,
    connector: String,
    credential_revision: Option<String>,
    label: String,
    operation: VendorOperation,
    plan_revision: String,
    settings: Vec<BeginSetting>,
    targets: Vec<BeginTarget>,
}

impl VendorBegin {
    /// Parse the complete canonical BEGIN before contacting the owner endpoint.
    pub(crate) fn parse(payload: &[u8], operation: VendorOperation) -> Option<Self> {
        match operation {
            VendorOperation::Connect => {
                let begin: ConnectBegin = serde_json::from_slice(payload).ok()?;
                if serde_json::to_vec(&begin).ok()?.as_slice() != payload
                    || !valid_begin_identity(&begin.connector, &begin.label, &begin.plan_revision)
                    || !valid_begin_targets(&begin.targets)
                    || !unique(begin.settings.iter().map(|setting| setting.target.as_str()))
                    || !unique(
                        begin
                            .authorities
                            .iter()
                            .map(|authority| authority.target.as_str()),
                    )
                    || begin
                        .authorities
                        .iter()
                        .any(|authority| authority.revision.is_some())
                {
                    return None;
                }
                Some(Self {
                    authorities: begin.authorities,
                    connector: begin.connector,
                    credential_revision: None,
                    label: begin.label,
                    operation,
                    plan_revision: begin.plan_revision,
                    settings: begin.settings,
                    targets: begin.targets,
                })
            }
            VendorOperation::Credential => {
                let begin: CredentialBegin = serde_json::from_slice(payload).ok()?;
                if serde_json::to_vec(&begin).ok()?.as_slice() != payload
                    || !valid_begin_identity(&begin.connector, &begin.label, &begin.plan_revision)
                    || !is_nonzero_lowerhex_32(&begin.credential_revision)
                    || !valid_begin_targets(&begin.targets)
                {
                    return None;
                }
                Some(Self {
                    authorities: Vec::new(),
                    connector: begin.connector,
                    credential_revision: Some(begin.credential_revision),
                    label: begin.label,
                    operation,
                    plan_revision: begin.plan_revision,
                    settings: Vec::new(),
                    targets: begin.targets,
                })
            }
        }
    }

    pub(crate) fn connector(&self) -> &str {
        &self.connector
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    /// Validate the complete current v2 response before opening mutation connection two.
    pub(crate) fn admits_plan(&self, payload: &[u8]) -> bool {
        let Ok(plan) = serde_json::from_slice::<Plan>(payload) else {
            return false;
        };
        if serde_json::to_vec(&plan).ok().as_deref() != Some(payload)
            || plan.version != "exchange.connection-plan.v2"
            || plan.connector != self.connector
            || plan.plan_revision != self.plan_revision
            || !is_nonzero_lowerhex_32(&plan.plan_revision)
            || plan.vendor.is_empty()
            || !valid_labels(&plan.labels)
        {
            return false;
        }
        match self.operation {
            VendorOperation::Connect
                if plan.selection.is_some() || plan.credential_revision.is_some() =>
            {
                return false;
            }
            VendorOperation::Credential
                if plan.selection.as_deref() != Some(self.label.as_str())
                    || !plan.labels.iter().any(|label| label == &self.label)
                    || !plan
                        .credential_revision
                        .as_deref()
                        .is_some_and(is_nonzero_lowerhex_32)
                    || self.credential_revision.is_none() =>
            {
                return false;
            }
            _ => {}
        }
        let Some(facts) = validate_fields(&plan.fields, plan.state) else {
            return false;
        };
        match self.operation {
            VendorOperation::Connect => self.admits_connect(&facts),
            VendorOperation::Credential => self.admits_credential(&facts),
        }
    }

    fn admits_connect(&self, facts: &[TargetFact]) -> bool {
        let chosen = self
            .targets
            .iter()
            .map(|target| target.target.as_str())
            .collect::<BTreeSet<_>>();
        if chosen.len() != self.targets.len() {
            return false;
        }
        let expected = facts
            .iter()
            .filter(|fact| {
                fact.partition == TargetPartition::ConnectionName
                    || fact.required
                    || chosen.contains(fact.id.as_str())
            })
            .collect::<Vec<_>>();
        exact_targets(&self.targets, &expected)
            && exact_settings(
                &self.settings,
                expected.iter().copied().filter(|fact| {
                    matches!(
                        fact.partition,
                        TargetPartition::Setting | TargetPartition::Authority
                    )
                }),
            )
            && exact_authorities(
                &self.authorities,
                expected
                    .iter()
                    .copied()
                    .filter(|fact| fact.partition == TargetPartition::Authority),
            )
    }

    fn admits_credential(&self, facts: &[TargetFact]) -> bool {
        let expected = facts
            .iter()
            .filter(|fact| fact.partition == TargetPartition::Credential)
            .collect::<Vec<_>>();
        !expected.is_empty() && exact_targets(&self.targets, &expected)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConnectBegin {
    authorities: Vec<BeginAuthority>,
    connector: String,
    label: String,
    plan_revision: String,
    settings: Vec<BeginSetting>,
    targets: Vec<BeginTarget>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialBegin {
    action: CredentialAction,
    connector: String,
    credential_revision: String,
    label: String,
    plan_revision: String,
    targets: Vec<BeginTarget>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CredentialAction {
    Acquire,
    Rotate,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BeginAuthority {
    revision: Option<String>,
    target: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BeginSetting {
    target: String,
    value: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BeginTarget {
    revision: String,
    target: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    connector: String,
    credential_revision: Option<String>,
    fields: Vec<PlanField>,
    labels: Vec<String>,
    plan_revision: String,
    selection: Option<String>,
    state: PlanState,
    vendor: String,
    version: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanField {
    aliases: Vec<String>,
    also_binds: Vec<String>,
    authority: Option<PlanAuthority>,
    binds: Option<String>,
    choices: Option<Vec<PlanChoice>>,
    help: String,
    identity: String,
    input: String,
    label: String,
    name: String,
    provenance: String,
    reason: Option<String>,
    required: bool,
    routable: bool,
    secret: bool,
    service: Option<String>,
    set: Option<bool>,
    target: Option<PlanTarget>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanAuthority {
    actions: Vec<String>,
    revision: Option<String>,
    state: AuthorityState,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum AuthorityState {
    Unset,
    Proposed,
    Approved,
    Revoked,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanChoice {
    label: String,
    value: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanTarget {
    id: String,
    revision: String,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum PlanState {
    Complete,
    Incomplete,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetPartition {
    ConnectionName,
    Setting,
    Authority,
    Credential,
}

struct TargetFact {
    id: String,
    partition: TargetPartition,
    required: bool,
    revision: String,
}

fn validate_fields(fields: &[PlanField], state: PlanState) -> Option<Vec<TargetFact>> {
    if fields.is_empty() || fields.len() > 128 {
        return None;
    }
    let mut facts = Vec::<TargetFact>::new();
    for field in fields {
        if field.identity.is_empty()
            || field.name.is_empty()
            || field.input.is_empty()
            || field.label.is_empty()
            || field.provenance.is_empty()
            || field.secret == field.set.is_some()
        {
            return None;
        }
        if let Some(authority) = &field.authority {
            if field.secret || field.target.is_none() || !valid_authority(authority) {
                return None;
            }
        }
        let Some(target) = &field.target else {
            continue;
        };
        if target.id.is_empty() || !is_lowerhex_32(&target.revision) {
            return None;
        }
        let partition = if target.id == "connection.name" {
            if field.secret || field.authority.is_some() {
                return None;
            }
            TargetPartition::ConnectionName
        } else if field.secret {
            TargetPartition::Credential
        } else if field.authority.is_some() {
            TargetPartition::Authority
        } else {
            TargetPartition::Setting
        };
        if let Some(existing) = facts.iter().find(|fact| fact.id == target.id) {
            if existing.revision != target.revision || existing.partition != partition {
                return None;
            }
        } else {
            facts.push(TargetFact {
                id: target.id.clone(),
                partition,
                required: field.required,
                revision: target.revision.clone(),
            });
        }
    }
    let complete = fields
        .iter()
        .filter(|field| field.required)
        .all(|field| field.routable && (field.secret || field.set == Some(true)));
    if (state == PlanState::Complete) != complete {
        return None;
    }
    Some(facts)
}

fn valid_authority(authority: &PlanAuthority) -> bool {
    match authority.state {
        AuthorityState::Unset => authority.revision.is_none() && authority.actions.is_empty(),
        AuthorityState::Proposed => {
            authority
                .revision
                .as_deref()
                .is_some_and(canonical_positive_u64)
                && authority.actions == ["approve", "revoke"]
        }
        AuthorityState::Approved => {
            authority
                .revision
                .as_deref()
                .is_some_and(canonical_positive_u64)
                && authority.actions == ["revoke"]
        }
        AuthorityState::Revoked => {
            authority
                .revision
                .as_deref()
                .is_some_and(canonical_positive_u64)
                && authority.actions.is_empty()
        }
    }
}

fn exact_targets(begin: &[BeginTarget], expected: &[&TargetFact]) -> bool {
    begin.len() == expected.len()
        && begin.iter().zip(expected).all(|(actual, expected)| {
            actual.target == expected.id && actual.revision == expected.revision
        })
}

fn exact_settings<'a>(
    begin: &[BeginSetting],
    expected: impl Iterator<Item = &'a TargetFact>,
) -> bool {
    begin
        .iter()
        .map(|setting| setting.target.as_str())
        .eq(expected.map(|fact| fact.id.as_str()))
}

fn exact_authorities<'a>(
    begin: &[BeginAuthority],
    expected: impl Iterator<Item = &'a TargetFact>,
) -> bool {
    begin
        .iter()
        .filter(|authority| authority.revision.is_none())
        .map(|authority| authority.target.as_str())
        .eq(expected.map(|fact| fact.id.as_str()))
        && begin.iter().all(|authority| authority.revision.is_none())
}

fn valid_begin_identity(connector: &str, label: &str, plan_revision: &str) -> bool {
    !connector.is_empty() && !label.is_empty() && is_nonzero_lowerhex_32(plan_revision)
}

fn valid_begin_targets(targets: &[BeginTarget]) -> bool {
    !targets.is_empty()
        && unique(targets.iter().map(|target| target.target.as_str()))
        && targets
            .iter()
            .all(|target| !target.target.is_empty() && is_lowerhex_32(&target.revision))
}

fn valid_labels(labels: &[String]) -> bool {
    labels.len() <= 256
        && labels.windows(2).all(|pair| pair[0] < pair[1])
        && labels.iter().all(|label| !label.is_empty())
}

fn unique<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let values = values.collect::<Vec<_>>();
    values.iter().copied().collect::<BTreeSet<_>>().len() == values.len()
}

fn canonical_positive_u64(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('0')
        && value.parse::<u64>().is_ok_and(|revision| revision != 0)
}

fn is_lowerhex_32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_nonzero_lowerhex_32(value: &str) -> bool {
    is_lowerhex_32(value) && value.bytes().any(|byte| byte != b'0')
}
