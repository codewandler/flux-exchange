use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Protocols {
    pub exchange_api: String,
    pub effective_catalogue_response: String,
    pub invoke_request: String,
    pub invoke_response: String,
    pub connection_plan: String,
    pub local_management: String,
    pub service_account_handoff: String,
    pub supervisor: String,
}

impl Protocols {
    pub fn v2() -> Self {
        Self {
            exchange_api: "exchange.api.v1".into(),
            effective_catalogue_response: "exchange.effective-catalogue-response.v1".into(),
            invoke_request: "exchange.invoke-request.v1".into(),
            invoke_response: "exchange.invoke-response.v1".into(),
            connection_plan: "exchange.connection-plan.v2".into(),
            local_management: "exchange.local-management.v1".into(),
            service_account_handoff: "exchange.service-account-handoff.v1".into(),
            supervisor: "exchange.supervisor-ready.v2".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustDocument {
    pub schema: String,
    pub origin: String,
    pub version: u64,
    pub issued_at: String,
    pub expires_at: String,
    pub root_signing_key_ids: Vec<String>,
    pub roles: Roles,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Roles {
    pub channel: Role,
    pub release: Role,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub threshold: u64,
    pub keys: Vec<DelegatedKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DelegatedKey {
    pub key_id: String,
    pub minisign_public_key: String,
    pub not_before: String,
    pub not_after: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub schema: String,
    pub channel: String,
    pub origin: String,
    pub generation: u64,
    pub issued_at: String,
    pub expires_at: String,
    pub signing_key_ids: Vec<String>,
    pub releases: Vec<ReleaseEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseEntry {
    pub tag: String,
    pub version: String,
    pub source_commit: String,
    pub build_id: String,
    pub manifest_sha256: String,
    pub release_key_ids: Vec<String>,
    pub protocols: Protocols,
}

impl ReleaseEntry {
    #[doc(hidden)]
    pub fn test(version: &str, protocols: Protocols) -> Self {
        Self {
            tag: format!("refs/tags/v{version}"),
            version: version.into(),
            source_commit: "0000000000000000000000000000000000000000".into(),
            build_id: "test".into(),
            manifest_sha256: "0".repeat(64),
            release_key_ids: vec!["test-release".into()],
            protocols,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub origin: String,
    pub tag: String,
    pub version: String,
    pub source_commit: String,
    pub build_id: String,
    pub protocols: Protocols,
    pub signing_key_ids: Vec<String>,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub target: String,
    pub archive: String,
    pub format: String,
    pub archive_bytes: u64,
    pub archive_sha256: String,
    pub executable: Member,
    pub other_members: Vec<OtherMember>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OtherMember {
    pub path: String,
    pub kind: MemberKind,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberKind {
    Documentation,
    License,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RootPolicy {
    pub schema: String,
    pub threshold: u64,
    pub test_only: bool,
    pub keys: Vec<RootKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RootKey {
    pub key_id: String,
    pub minisign_public_key: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackState {
    pub trust: Option<Floor>,
    pub channel: Option<Floor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    pub number: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub schema: String,
    pub release: CompatibilityRelease,
    pub protocols: Protocols,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityRelease {
    pub tag: String,
    pub version: String,
    pub source_commit: String,
    pub build_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureSet {
    pub schema: String,
    pub exchange_commit: String,
    pub files: BTreeMap<String, String>,
    pub cases: Vec<FixtureCase>,
    /// Native process cases bound to exact tests on the five release runners.
    pub native_cases: Vec<NativeFixtureCase>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFixtureCase {
    pub id: String,
    pub evidence: Vec<NativeFixtureEvidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFixtureEvidence {
    pub targets: Vec<String>,
    pub test_target: String,
    pub exact_test: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureCase {
    pub id: String,
    pub operation: String,
    pub input: String,
    pub clock: String,
    pub platform: String,
    pub prior_state: RollbackState,
    pub prior_install: Option<InstalledIdentity>,
    pub expected_result: String,
    pub expected_state: RollbackState,
    pub expected_install: Option<InstalledIdentity>,
    pub expected_stage: String,
    pub expected_error_contains: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledIdentity {
    pub version: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub executable_sha256: String,
}
