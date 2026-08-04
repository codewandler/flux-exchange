//! Closed, value-free inputs for connection and credential proposals.
//!
//! Parsing proves the canonical control-object grammar. [`TargetFact`] is the deliberately small
//! seam through which the caller supplies the already-snapshotted X-125 target universe; validating
//! against it closes target membership, revision, partition and order before a coordinator is
//! allowed to allocate anything.

use std::collections::BTreeSet;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const CONNECT_DOMAIN: &[u8] = b"exchange.local-management.v1.connect-proposal";
const CREDENTIAL_DOMAIN: &[u8] = b"exchange.local-management.v1.credential-proposal";
const MAX_TARGETS: usize = 64;
const MAX_CONNECTOR_BYTES: usize = 128;
const MAX_LABEL_BYTES: usize = 64;
const MAX_TARGET_BYTES: usize = 512;
const MAX_SETTING_BYTES: usize = 1024;

/// A refusal produced before any coordinator, store or audit mutation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProposalError {
    /// The payload is not the closed JSON type for its opcode.
    #[error("invalid proposal control object")]
    InvalidJson,
    /// The payload parses, but its bytes are not its one RFC 8785 representation.
    #[error("proposal control object is not canonical JSON")]
    NonCanonical,
    /// One bounded scalar or collection is outside the published grammar.
    #[error("invalid proposal member: {0}")]
    InvalidMember(&'static str),
    /// Target membership, revision, order or partition disagrees with the plan snapshot.
    #[error("invalid proposal target closure: {0}")]
    InvalidTargetClosure(&'static str),
}

macro_rules! hex_identity {
    ($name:ident, $member:literal, $nonzero:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Parse the exact lowercase-hex wire representation.
            pub fn parse(value: impl Into<String>) -> Result<Self, ProposalError> {
                let value = value.into();
                validate_hex_64(&value, $nonzero)
                    .map_err(|_| ProposalError::InvalidMember($member))?;
                Ok(Self(value))
            }

            /// Return the opaque wire identity without parsing or ordering its value.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(D::Error::custom)
            }
        }
    };
}

hex_identity!(PlanRevision, "plan_revision", false);
hex_identity!(TargetRevision, "target revision", false);
hex_identity!(CredentialRevision, "credential_revision", true);
hex_identity!(ProposalDigest, "proposal_digest", false);
// Receipt wire validation is a released codec contract exercised by the integration codec suite.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReceiptId(String);

#[allow(dead_code)]
impl ReceiptId {
    /// Parse the exact nonzero lowercase-hex wire representation.
    pub fn parse(value: impl Into<String>) -> Result<Self, ProposalError> {
        let value = value.into();
        validate_hex_64(&value, true).map_err(|_| ProposalError::InvalidMember("receipt_id"))?;
        Ok(Self(value))
    }

    /// Return the opaque wire identity without parsing or ordering its value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ReceiptId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

/// One value-free target class from the authoritative plan snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetPartition {
    /// The synthetic `connection.name` target, first in every target universe.
    ConnectionName,
    /// An ordinary non-secret setting.
    Setting,
    /// A typed custom-origin setting with authority lifecycle state.
    Authority,
    /// A credential address whose bytes are supplied only in later raw frames.
    Credential,
}

/// One ordered target fact supplied by the parent plan snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetFact<'a> {
    /// The exact public target id.
    pub target: &'a str,
    /// The exact static target revision.
    pub revision: &'a str,
    /// Whether connect must select this routable target.
    pub required: bool,
    /// The target's one X-125-derived partition.
    pub partition: TargetPartition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Connector(String);

impl Connector {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Connector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() || value.len() > MAX_CONNECTOR_BYTES {
            return Err(D::Error::custom(
                "connector must contain 1..=128 UTF-8 bytes",
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Label(String);

impl Label {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let valid = !value.is_empty()
            && value.len() <= MAX_LABEL_BYTES
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !valid {
            return Err(D::Error::custom("label has an invalid grammar"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct Target(String);

impl Target {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() || value.len() > MAX_TARGET_BYTES {
            return Err(D::Error::custom("target must contain 1..=512 UTF-8 bytes"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct SettingValue(String);

impl SettingValue {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SettingValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() > MAX_SETTING_BYTES {
            return Err(D::Error::custom("setting value exceeds 1024 UTF-8 bytes"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct StoreRevision(String);

impl StoreRevision {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for StoreRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let valid_digits = !value.is_empty()
            && value.len() <= 20
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && (value == "0" || !value.starts_with('0'));
        let parsed = valid_digits
            .then(|| value.parse::<u64>().ok())
            .flatten()
            .filter(|revision| *revision != 0);
        if parsed.is_none() {
            return Err(D::Error::custom(
                "store revision is not canonical nonzero u64",
            ));
        }
        Ok(Self(value))
    }
}

/// One exact `{revision,target}` pair in a BEGIN target array.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanTarget {
    revision: TargetRevision,
    target: Target,
}

impl PlanTarget {
    /// The target revision from the plan snapshot.
    pub fn revision(&self) -> &str {
        self.revision.as_str()
    }

    /// The public target id.
    pub fn target(&self) -> &str {
        self.target.as_str()
    }
}

/// One exact non-secret setting projection in a connect proposal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    target: Target,
    value: SettingValue,
}

impl Setting {
    /// The selected setting target.
    pub fn target(&self) -> &str {
        self.target.as_str()
    }

    /// The non-secret value, still subject to the parent's target-specific validator.
    pub fn value(&self) -> &str {
        self.value.as_str()
    }
}

/// One exact authority CAS projection in a connect proposal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRevision {
    revision: Option<StoreRevision>,
    target: Target,
}

impl AuthorityRevision {
    /// The proposed authority target.
    pub fn target(&self) -> &str {
        self.target.as_str()
    }

    /// The decimal authority revision, or `None` for connection creation.
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_ref().map(StoreRevision::as_str)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConnectControl {
    authorities: Vec<AuthorityRevision>,
    connector: Connector,
    label: Label,
    plan_revision: PlanRevision,
    settings: Vec<Setting>,
    targets: Vec<PlanTarget>,
}

/// A canonical connection-create BEGIN and its verified RFC 8785 bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectBegin {
    control: ConnectControl,
    canonical: Vec<u8>,
}

impl ConnectBegin {
    /// Parse and close the target universe in one pre-mutation step.
    #[allow(dead_code)] // The integration codec suite exercises the combined parse/closure seam.
    pub fn parse_and_validate(
        bytes: &[u8],
        universe: &[TargetFact<'_>],
    ) -> Result<Self, ProposalError> {
        let begin = Self::parse_canonical(bytes)?;
        begin.validate_target_closure(universe)?;
        Ok(begin)
    }

    /// Parse one exact, duplicate-free, deny-unknown canonical BEGIN object.
    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, ProposalError> {
        let (control, canonical) = parse_canonical(bytes)?;
        validate_connect_intrinsic(&control)?;
        Ok(Self { control, canonical })
    }

    /// Validate membership, revisions, selection, partitions and order against the plan snapshot.
    pub fn validate_target_closure(
        &self,
        universe: &[TargetFact<'_>],
    ) -> Result<(), ProposalError> {
        validate_universe(universe)?;

        let actual_ids = self
            .control
            .targets
            .iter()
            .map(PlanTarget::target)
            .collect::<BTreeSet<_>>();
        let selected = universe
            .iter()
            .filter(|fact| {
                fact.partition == TargetPartition::ConnectionName
                    || fact.required
                    || actual_ids.contains(fact.target)
            })
            .collect::<Vec<_>>();
        validate_exact_targets(&self.control.targets, &selected)?;

        let expected_settings = selected
            .iter()
            .filter(|fact| {
                matches!(
                    fact.partition,
                    TargetPartition::Setting | TargetPartition::Authority
                )
            })
            .map(|fact| fact.target)
            .collect::<Vec<_>>();
        if self
            .control
            .settings
            .iter()
            .map(Setting::target)
            .ne(expected_settings.iter().copied())
        {
            return Err(ProposalError::InvalidTargetClosure(
                "settings are not the selected setting/authority projection in plan order",
            ));
        }

        let expected_authorities = selected
            .iter()
            .filter(|fact| fact.partition == TargetPartition::Authority)
            .map(|fact| fact.target)
            .collect::<Vec<_>>();
        if self
            .control
            .authorities
            .iter()
            .map(AuthorityRevision::target)
            .ne(expected_authorities.iter().copied())
        {
            return Err(ProposalError::InvalidTargetClosure(
                "authorities are not the selected authority projection in plan order",
            ));
        }
        if self
            .control
            .authorities
            .iter()
            .any(|authority| authority.revision().is_some())
        {
            return Err(ProposalError::InvalidTargetClosure(
                "connection-create authority revisions must be null",
            ));
        }
        Ok(())
    }

    /// The released connector id, whose catalogue membership the parent must resolve.
    pub fn connector(&self) -> &str {
        self.control.connector.as_str()
    }

    /// The proposed connection label.
    pub fn label(&self) -> &str {
        self.control.label.as_str()
    }

    /// The exact static plan identity.
    pub fn plan_revision(&self) -> &str {
        self.control.plan_revision.as_str()
    }

    /// Ordered selected targets.
    pub fn targets(&self) -> &[PlanTarget] {
        &self.control.targets
    }

    /// Ordered non-secret setting projections.
    pub fn settings(&self) -> &[Setting] {
        &self.control.settings
    }

    /// Ordered authority CAS projections.
    pub fn authorities(&self) -> &[AuthorityRevision] {
        &self.control.authorities
    }

    /// The byte-exact canonical control object.
    #[allow(dead_code)] // Exposed to the integration codec suite for byte identity evidence.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// The exact domain, zero separator and RFC 8785 control bytes.
    #[allow(dead_code)] // Exposed to the integration codec suite for domain-separation evidence.
    pub fn proposal_preimage(&self) -> Vec<u8> {
        proposal_preimage(CONNECT_DOMAIN, &self.canonical)
    }

    /// The lowercase SHA-256 identity of [`Self::proposal_preimage`].
    pub fn proposal_digest(&self) -> ProposalDigest {
        digest(CONNECT_DOMAIN, &self.canonical)
    }
}

/// The only two credential mutation actions in protocol v1.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAction {
    /// Populate a complete absent credential partition.
    Acquire,
    /// Replace a complete present credential partition.
    Rotate,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialControl {
    action: CredentialAction,
    connector: Connector,
    credential_revision: CredentialRevision,
    label: Label,
    plan_revision: PlanRevision,
    targets: Vec<PlanTarget>,
}

/// A canonical credential acquire/rotate BEGIN and its verified RFC 8785 bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialBegin {
    control: CredentialControl,
    canonical: Vec<u8>,
}

impl CredentialBegin {
    /// Parse and close the complete credential partition in one pre-mutation step.
    #[allow(dead_code)] // The integration codec suite exercises the combined parse/closure seam.
    pub fn parse_and_validate(
        bytes: &[u8],
        universe: &[TargetFact<'_>],
    ) -> Result<Self, ProposalError> {
        let begin = Self::parse_canonical(bytes)?;
        begin.validate_target_closure(universe)?;
        Ok(begin)
    }

    /// Parse one exact, duplicate-free, deny-unknown canonical BEGIN object.
    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, ProposalError> {
        let (control, canonical) = parse_canonical(bytes)?;
        validate_credential_intrinsic(&control)?;
        Ok(Self { control, canonical })
    }

    /// Require the complete nonempty credential partition in plan order.
    pub fn validate_target_closure(
        &self,
        universe: &[TargetFact<'_>],
    ) -> Result<(), ProposalError> {
        validate_universe(universe)?;
        let credentials = universe
            .iter()
            .filter(|fact| fact.partition == TargetPartition::Credential)
            .collect::<Vec<_>>();
        if credentials.is_empty() {
            return Err(ProposalError::InvalidTargetClosure(
                "credential partition is empty",
            ));
        }
        validate_exact_targets(&self.control.targets, &credentials)
    }

    /// The requested complete-partition action.
    pub fn action(&self) -> CredentialAction {
        self.control.action
    }

    /// The released connector id, whose catalogue membership the parent must resolve.
    pub fn connector(&self) -> &str {
        self.control.connector.as_str()
    }

    /// The held connection label.
    pub fn label(&self) -> &str {
        self.control.label.as_str()
    }

    /// The exact static plan identity.
    pub fn plan_revision(&self) -> &str {
        self.control.plan_revision.as_str()
    }

    /// The exact opaque selected-label credential head.
    pub fn credential_revision(&self) -> &str {
        self.control.credential_revision.as_str()
    }

    /// The complete credential partition in plan order.
    pub fn targets(&self) -> &[PlanTarget] {
        &self.control.targets
    }

    /// The byte-exact canonical control object.
    #[allow(dead_code)] // Exposed to the integration codec suite for byte identity evidence.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// The exact domain, zero separator and RFC 8785 control bytes.
    #[allow(dead_code)] // Exposed to the integration codec suite for domain-separation evidence.
    pub fn proposal_preimage(&self) -> Vec<u8> {
        proposal_preimage(CREDENTIAL_DOMAIN, &self.canonical)
    }

    /// The lowercase SHA-256 identity of [`Self::proposal_preimage`].
    pub fn proposal_digest(&self) -> ProposalDigest {
        digest(CREDENTIAL_DOMAIN, &self.canonical)
    }
}

fn parse_canonical<T>(bytes: &[u8]) -> Result<(T, Vec<u8>), ProposalError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    // Serde's diagnostic may quote an attacker-selected enum or member. Collapse it here so an
    // attempted secret-bearing JSON value can never be reflected by the caller's refusal path.
    let value = serde_json::from_slice::<T>(bytes).map_err(|_| ProposalError::InvalidJson)?;
    // This closed vocabulary has only objects, arrays, strings and null. Declaring every struct in
    // UTF-16 lexical member order makes serde_json's compact spelling its RFC 8785 spelling.
    let canonical = serde_json::to_vec(&value).map_err(|_| ProposalError::InvalidJson)?;
    if canonical != bytes {
        return Err(ProposalError::NonCanonical);
    }
    Ok((value, canonical))
}

fn validate_connect_intrinsic(control: &ConnectControl) -> Result<(), ProposalError> {
    validate_target_array(&control.targets)?;
    validate_projection_order(
        control.settings.iter().map(Setting::target),
        &control.targets,
        "settings",
    )?;
    validate_projection_order(
        control.authorities.iter().map(AuthorityRevision::target),
        &control.targets,
        "authorities",
    )?;
    if control.settings.len() > MAX_TARGETS || control.authorities.len() > MAX_TARGETS {
        return Err(ProposalError::InvalidMember(
            "settings/authorities exceeds 64 entries",
        ));
    }
    Ok(())
}

fn validate_credential_intrinsic(control: &CredentialControl) -> Result<(), ProposalError> {
    validate_target_array(&control.targets)
}

fn validate_target_array(targets: &[PlanTarget]) -> Result<(), ProposalError> {
    if targets.is_empty() || targets.len() > MAX_TARGETS {
        return Err(ProposalError::InvalidMember(
            "targets must contain 1..=64 entries",
        ));
    }
    let mut unique = BTreeSet::new();
    if targets.iter().any(|target| !unique.insert(target.target())) {
        return Err(ProposalError::InvalidMember("duplicate target"));
    }
    Ok(())
}

fn validate_projection_order<'a>(
    projected: impl Iterator<Item = &'a str>,
    targets: &[PlanTarget],
    member: &'static str,
) -> Result<(), ProposalError> {
    let mut prior = None;
    let mut unique = BTreeSet::new();
    for target in projected {
        if !unique.insert(target) {
            return Err(ProposalError::InvalidMember(match member {
                "settings" => "duplicate setting target",
                _ => "duplicate authority target",
            }));
        }
        let position = targets
            .iter()
            .position(|candidate| candidate.target() == target)
            .ok_or(ProposalError::InvalidMember(match member {
                "settings" => "setting target is absent from targets",
                _ => "authority target is absent from targets",
            }))?;
        if prior.is_some_and(|prior| position <= prior) {
            return Err(ProposalError::InvalidMember(match member {
                "settings" => "settings are not in target order",
                _ => "authorities are not in target order",
            }));
        }
        prior = Some(position);
    }
    Ok(())
}

fn validate_universe(universe: &[TargetFact<'_>]) -> Result<(), ProposalError> {
    if universe.is_empty() || universe.len() > MAX_TARGETS {
        return Err(ProposalError::InvalidTargetClosure(
            "target universe must contain 1..=64 entries",
        ));
    }
    if universe[0].partition != TargetPartition::ConnectionName
        || universe[0].target != "connection.name"
        || universe
            .iter()
            .skip(1)
            .any(|fact| fact.partition == TargetPartition::ConnectionName)
    {
        return Err(ProposalError::InvalidTargetClosure(
            "connection.name must be the sole first connection-name target",
        ));
    }
    let mut unique = BTreeSet::new();
    for fact in universe {
        if fact.target.is_empty()
            || fact.target.len() > MAX_TARGET_BYTES
            || !unique.insert(fact.target)
            || validate_hex_64(fact.revision, false).is_err()
        {
            return Err(ProposalError::InvalidTargetClosure(
                "target universe contains an invalid or duplicate fact",
            ));
        }
    }
    Ok(())
}

fn validate_exact_targets(
    actual: &[PlanTarget],
    expected: &[&TargetFact<'_>],
) -> Result<(), ProposalError> {
    if actual.len() != expected.len()
        || actual.iter().zip(expected).any(|(actual, expected)| {
            actual.target() != expected.target || actual.revision() != expected.revision
        })
    {
        return Err(ProposalError::InvalidTargetClosure(
            "targets differ in membership, revision, partition or plan order",
        ));
    }
    Ok(())
}

fn validate_hex_64(value: &str, nonzero: bool) -> Result<(), ()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        || (nonzero && value.bytes().all(|byte| byte == b'0'))
    {
        return Err(());
    }
    Ok(())
}

fn proposal_preimage(domain: &[u8], canonical: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(domain.len() + 1 + canonical.len());
    bytes.extend_from_slice(domain);
    bytes.push(0);
    bytes.extend_from_slice(canonical);
    bytes
}

fn digest(domain: &[u8], canonical: &[u8]) -> ProposalDigest {
    let bytes = Sha256::digest(proposal_preimage(domain, canonical));
    let mut encoded = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    ProposalDigest(encoded)
}
