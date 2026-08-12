//! Canonical native-release evidence authority and its derived projections.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::{NativeFixtureCase, NativeFixtureEvidence};
use crate::{canonical, digest_hex, Error, Platform, Result, SUPPORTED_TARGETS};

pub const SCHEMA: &str = "exchange.native-evidence.v1";
pub const REPORT_SCHEMA: &str = "exchange.native-evidence-report.v1";
pub const MAX_BYTES: usize = 256 * 1024;
pub const BUNDLED_BYTES: &[u8] = include_bytes!("../native-evidence-v1.json");

/// The exact Exchange-owned process proofs publication consumes for the retained provider store.
///
/// X-139 owns the final authority document; these two names are what it may never lose. One proves
/// production connect crash recovery, restart QUERY and byte-identical same-proposal replay; the
/// other proves the one retained `FileStore` lease excludes a second process through recovery and
/// readiness and releases after an abrupt exit. Renaming either is a repair in two places, never a
/// silent drop that still reports a complete native inventory.
const RETAINED_PROVIDER_TESTS: [&str; 2] = [
    "real_server_retains_the_c515_lease_through_recovery_and_readiness",
    "unix_connect_crashes_recover_before_readiness_and_replay_one_receipt",
];

/// The obligations whose Cargo bindings are exactly those two Exchange-owned proofs.
const RETAINED_PROVIDER_OBLIGATIONS: [&str; 2] =
    ["x134-c515-retained-lease", "x134-connect-crash-replay"];

/// The complete inherited X-128 supervision obligations this Linux-only tree retains.
///
/// The frozen fixture digest only catches drift that was *not* regenerated. Naming the retained
/// inventory here instead makes the document its own authority: a family renamed, dropped or
/// invented alongside a regenerated projection refuses, and the non-Linux obligations X-137 removed
/// stay absent rather than surviving as a negative or optional list. The counts are the array
/// lengths the compiler derives, never a number publication compares against.
const RETAINED_INHERITED_OBLIGATIONS: [&str; 6] = [
    "x128-expiry-live",
    "x128-supervisor-sigkill-responsive",
    "x128-supervisor-sigkill-wedged",
    "x128-supervisor-unix-normal",
    "x128-supervisor-unix-wedged",
    "x128-unix-inherited-abi",
];

/// The complete literal X-134 obligation families this authority certifies.
const RETAINED_LITERAL_FAMILIES: [&str; 12] = [
    "x134-c515-retained-lease",
    "x134-connect-crash-replay",
    "x134-four-form-sentinel",
    "x134-grant-cas",
    "x134-helper-deadlines",
    "x134-hosted-origin-and-message-state",
    "x134-local-management-deadlines",
    "x134-native-owner-endpoint",
    "x134-native-private-input",
    "x134-native-service-account-handoff",
    "x134-native-stream-framing",
    "x134-production-root-safety",
];

/// The applicable published C-515 tests Exchange inherits rather than re-proves.
///
/// The provider release stays portable: Exchange consumes this evidence by name and never restates,
/// narrows or re-runs it. Dropping one would leave a provider obligation asserted by nothing.
const INHERITED_PROVIDER_TESTS: [&str; 5] = [
    "every_durable_transaction_boundary_recovers_one_complete_state",
    "file_store_holds_a_lifetime_non_blocking_lease",
    "native_upgrade_fixture_proves_legacy_quiescence_and_v2_refusal",
    "two_children_prove_lease_refusal_and_abrupt_release",
    "unix_lease_metadata_is_owner_only_one_link_and_never_repaired",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEvidenceAuthority {
    pub schema: String,
    pub authorities: BTreeMap<String, Authority>,
    pub targets: BTreeMap<String, NativeTarget>,
    pub target_sets: BTreeMap<String, Vec<String>>,
    pub feature_sets: BTreeMap<String, Vec<String>>,
    pub obligations: BTreeMap<String, Obligation>,
    pub bindings: BTreeMap<String, CargoBinding>,
    pub release_evidence: BTreeMap<String, ReleaseEvidence>,
    pub assertions: BTreeMap<String, EvidenceAssertion>,
    pub assertion_joins: Vec<AssertionJoin>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub class: AuthorityClass,
    pub reference: String,
    pub repository: Option<String>,
    pub package: Option<PackageIdentity>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityClass {
    LiteralStory,
    InheritedStory,
    InheritedUpstream,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageIdentity {
    pub name: String,
    pub version: String,
    pub registry_sha256: String,
    pub released_commit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTarget {
    pub runner: String,
    pub platform: NativePlatform,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePlatform {
    Linux,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Obligation {
    pub authority: String,
    pub target_set: String,
    pub description: String,
    pub adversarial_cases: Vec<String>,
    pub bindings: Vec<String>,
    pub release_evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoBinding {
    pub package: String,
    pub cargo_target: CargoTarget,
    pub exact_test: String,
    pub feature_set: String,
    pub target_set: String,
    pub classes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoTarget {
    pub kind: CargoTargetKind,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoTargetKind {
    Lib,
    Test,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseEvidence {
    pub authority: String,
    pub cargo_target: CargoTarget,
    pub exact_test: String,
    pub target_set: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceAssertion {
    pub authority: String,
    pub description: String,
    pub binding: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionJoin {
    pub assertion: String,
    pub binding_class: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkflowTarget {
    pub runner: String,
    pub target: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunnableBinding<'a> {
    pub id: &'a str,
    pub package: &'a str,
    pub cargo_target: &'a CargoTarget,
    pub exact_test: &'a str,
    pub features: &'a [String],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEvidenceReport {
    pub schema: String,
    pub authority_sha256: String,
    pub source_commit: String,
    pub target: String,
    pub runner: String,
    pub bindings: Vec<NativeBindingResult>,
    pub inherited_release_evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBindingResult {
    pub id: String,
    pub result: String,
}

impl NativeEvidenceAuthority {
    pub fn bundled() -> Result<Self> {
        let authority: Self = canonical::parse(BUNDLED_BYTES, MAX_BYTES)?;
        authority.validate()?;
        Ok(authority)
    }

    pub fn identity(&self) -> Result<String> {
        Ok(digest_hex(&canonical::encode(self)?))
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return schema("unknown native-evidence schema");
        }
        validate_map_keys("authority", &self.authorities)?;
        validate_map_keys("target set", &self.target_sets)?;
        validate_map_keys("feature set", &self.feature_sets)?;
        validate_map_keys("obligation", &self.obligations)?;
        validate_map_keys("binding", &self.bindings)?;
        validate_map_keys("release evidence", &self.release_evidence)?;
        validate_map_keys("assertion", &self.assertions)?;

        let required_classes = [
            AuthorityClass::LiteralStory,
            AuthorityClass::InheritedStory,
            AuthorityClass::InheritedUpstream,
        ];
        for class in required_classes {
            if self
                .authorities
                .values()
                .filter(|authority| authority.class == class)
                .count()
                != 1
            {
                return schema("native evidence requires one authority of every class");
            }
        }
        for authority in self.authorities.values() {
            if authority.reference.is_empty() {
                return schema("authority reference is empty");
            }
            match authority.class {
                AuthorityClass::InheritedUpstream => {
                    if authority.repository.as_deref().is_none_or(str::is_empty)
                        || authority.package.is_none()
                    {
                        return schema("upstream authority lacks repository/package identity");
                    }
                }
                AuthorityClass::LiteralStory | AuthorityClass::InheritedStory => {
                    if authority.repository.is_some() || authority.package.is_some() {
                        return schema("story authority carries an upstream package identity");
                    }
                }
            }
        }
        validate_upstream_package(self)?;

        let supported = SUPPORTED_TARGETS.iter().copied().collect::<BTreeSet<_>>();
        let declared = self
            .targets
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if declared != supported {
            return schema("native target authority is not the exact supported target set");
        }
        let mut runners = BTreeSet::new();
        for (target, native) in &self.targets {
            if native.runner.is_empty() || !runners.insert(native.runner.as_str()) {
                return schema("native runners are empty or duplicated");
            }
            let expected = match Platform::from_target(target)? {
                Platform::Linux => NativePlatform::Linux,
            };
            if native.platform != expected {
                return schema("native target platform disagrees with its target triple");
            }
        }
        for (set, targets) in &self.target_sets {
            validate_unique_refs("target", set, targets, &self.targets)?;
        }
        let expected_sets = ["all-native", "linux-native"]
            .into_iter()
            .collect::<BTreeSet<_>>();
        if self
            .target_sets
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != expected_sets
            || self
                .target_sets
                .values()
                .any(|targets| target_values(targets) != supported)
        {
            return schema(
                "target sets are not exactly all-native and linux-native over both Linux targets",
            );
        }

        for features in self.feature_sets.values() {
            if !is_sorted_unique(features) || features.iter().any(|feature| feature.is_empty()) {
                return schema("feature sets must be sorted, unique and nonempty by member");
            }
        }

        let mut cargo_identities = BTreeSet::new();
        for binding in self.bindings.values() {
            validate_binding(binding, self)?;
            let identity = (
                binding.package.as_str(),
                binding.cargo_target.kind,
                binding.cargo_target.name.as_str(),
                binding.exact_test.as_str(),
                binding.feature_set.as_str(),
                binding.target_set.as_str(),
            );
            if !cargo_identities.insert(identity) {
                return schema("native authority duplicates one exact Cargo binding");
            }
        }

        let mut release_identities = BTreeSet::new();
        for evidence in self.release_evidence.values() {
            validate_release_evidence(evidence, self)?;
            if !release_identities.insert((
                evidence.authority.as_str(),
                evidence.cargo_target.kind,
                evidence.cargo_target.name.as_str(),
                evidence.exact_test.as_str(),
                evidence.target_set.as_str(),
            )) {
                return schema("native authority duplicates inherited release evidence");
            }
        }

        let mut cases = BTreeSet::new();
        for obligation in self.obligations.values() {
            validate_obligation(obligation, self, &mut cases)?;
        }
        for assertion in self.assertions.values() {
            if !self.authorities.contains_key(&assertion.authority)
                || !self.bindings.contains_key(&assertion.binding)
                || assertion.description.is_empty()
            {
                return schema("native assertion has an invalid authority, binding or description");
            }
        }
        let mut joins = BTreeSet::new();
        for join in &self.assertion_joins {
            if !self.assertions.contains_key(&join.assertion)
                || join.binding_class.is_empty()
                || !joins.insert((join.assertion.as_str(), join.binding_class.as_str()))
            {
                return schema("native assertion join is invalid or duplicated");
            }
            if !self
                .bindings
                .values()
                .any(|binding| binding.classes.contains(&join.binding_class))
            {
                return schema("native assertion join selects no binding class");
            }
        }
        if joins.len() != 1 {
            return schema("native authority requires one cross-cutting assertion join");
        }
        self.validate_retained_inventory()?;
        self.validate_retained_provider_evidence()
    }

    /// The retained obligation inventory is exactly the literal and inherited story families.
    ///
    /// Obligations are grouped by the class of the authority that owns them, so the contract does
    /// not depend on what this document happens to call its authorities. Inherited upstream
    /// obligations are pinned by their exact published tests instead — see
    /// [`Self::validate_retained_provider_evidence`].
    fn validate_retained_inventory(&self) -> Result<()> {
        let mut literal = BTreeSet::new();
        let mut inherited = BTreeSet::new();
        for (id, obligation) in &self.obligations {
            let authority = self.authorities.get(&obligation.authority).ok_or_else(|| {
                Error::Schema(format!("unknown authority {:?}", obligation.authority))
            })?;
            match authority.class {
                AuthorityClass::LiteralStory => literal.insert(id.as_str()),
                AuthorityClass::InheritedStory => inherited.insert(id.as_str()),
                AuthorityClass::InheritedUpstream => true,
            };
        }
        if literal != RETAINED_LITERAL_FAMILIES.into_iter().collect() {
            return schema("literal story families are not the exact retained X-134 inventory");
        }
        if inherited != RETAINED_INHERITED_OBLIGATIONS.into_iter().collect() {
            return schema(
                "inherited story obligations are not the exact retained X-128 inventory",
            );
        }
        Ok(())
    }

    /// The retained connector-secrets 0.20 recovery, replay and lease evidence, exactly (X-138).
    ///
    /// Every Exchange-owned process binding joins the retained-lease assertion, so a binding cannot
    /// be reported as complete native evidence while the assertion that gives it meaning covers
    /// something else.
    fn validate_retained_provider_evidence(&self) -> Result<()> {
        let supported = SUPPORTED_TARGETS.iter().copied().collect::<BTreeSet<_>>();
        let mut exchange_owned = BTreeSet::new();
        for id in RETAINED_PROVIDER_OBLIGATIONS {
            let obligation = self.obligations.get(id).ok_or_else(|| {
                Error::Schema(format!("missing retained provider obligation {id:?}"))
            })?;
            for binding_id in &obligation.bindings {
                let binding = self
                    .bindings
                    .get(binding_id)
                    .ok_or_else(|| Error::Schema(format!("unknown binding {binding_id:?}")))?;
                if self
                    .target_set(&binding.target_set)?
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
                    != supported
                {
                    return schema(
                        "retained provider evidence does not cover both supported Linux targets",
                    );
                }
                exchange_owned.insert(binding.exact_test.as_str());
            }
        }
        if exchange_owned != RETAINED_PROVIDER_TESTS.into_iter().collect() {
            return schema("retained provider evidence is not the exact Exchange-owned test pair");
        }

        let inherited = self
            .release_evidence
            .values()
            .map(|evidence| evidence.exact_test.rsplit("::").next().unwrap_or_default())
            .collect::<BTreeSet<_>>();
        if inherited != INHERITED_PROVIDER_TESTS.into_iter().collect() {
            return schema("inherited C-515 release evidence is not the exact published test set");
        }

        let joined = self
            .assertion_joins
            .iter()
            .map(|join| join.binding_class.as_str())
            .collect::<BTreeSet<_>>();
        if self.bindings.values().any(|binding| {
            !binding
                .classes
                .iter()
                .any(|class| joined.contains(class.as_str()))
        }) {
            return schema("one Exchange process binding joins no retained provider assertion");
        }
        Ok(())
    }

    pub fn workflow_matrix(&self) -> Vec<WorkflowTarget> {
        self.targets
            .iter()
            .map(|(target, value)| WorkflowTarget {
                runner: value.runner.clone(),
                target: target.clone(),
            })
            .collect()
    }

    pub fn runnable_bindings(&self, target: &str) -> Result<Vec<RunnableBinding<'_>>> {
        if !self.targets.contains_key(target) {
            return schema("requested native target is not authoritative");
        }
        let mut selected = Vec::new();
        for (id, binding) in &self.bindings {
            if self.target_set(&binding.target_set)?.contains(target) {
                selected.push(RunnableBinding {
                    id,
                    package: &binding.package,
                    cargo_target: &binding.cargo_target,
                    exact_test: &binding.exact_test,
                    features: self
                        .feature_sets
                        .get(&binding.feature_set)
                        .expect("validated feature set"),
                });
            }
        }
        Ok(selected)
    }

    pub fn report(&self, target: &str, source_commit: &str) -> Result<NativeEvidenceReport> {
        validate_source_commit(source_commit)?;
        let native_target = self
            .targets
            .get(target)
            .ok_or_else(|| Error::Schema("report target is not authoritative".into()))?;
        let bindings = self
            .runnable_bindings(target)?
            .into_iter()
            .map(|binding| NativeBindingResult {
                id: binding.id.to_owned(),
                result: "one-passed-zero-ignored-zero-filtered".to_owned(),
            })
            .collect();
        let mut inherited_release_evidence = Vec::new();
        for (id, evidence) in &self.release_evidence {
            if self.target_set(&evidence.target_set)?.contains(target) {
                inherited_release_evidence.push(id.clone());
            }
        }
        Ok(NativeEvidenceReport {
            schema: REPORT_SCHEMA.to_owned(),
            authority_sha256: self.identity()?,
            source_commit: source_commit.to_owned(),
            target: target.to_owned(),
            runner: native_target.runner.clone(),
            bindings,
            inherited_release_evidence,
        })
    }

    pub fn validate_report(
        &self,
        report: &NativeEvidenceReport,
        source_commit: &str,
    ) -> Result<()> {
        let expected = self.report(&report.target, source_commit)?;
        if canonical::encode(report)? != canonical::encode(&expected)? {
            return schema("native runner report disagrees with its authority projection");
        }
        Ok(())
    }

    pub fn validate_reports(
        &self,
        reports: &[NativeEvidenceReport],
        source_commit: &str,
    ) -> Result<()> {
        let mut targets = BTreeSet::new();
        for report in reports {
            self.validate_report(report, source_commit)?;
            if !targets.insert(report.target.as_str()) {
                return schema("native runner reports duplicate one target");
            }
        }
        if targets != self.targets.keys().map(String::as_str).collect() {
            return schema("native runner reports are not the complete authoritative target set");
        }
        Ok(())
    }

    pub fn fixture_cases(&self) -> Result<Vec<NativeFixtureCase>> {
        let mut cases = Vec::new();
        for (id, obligation) in &self.obligations {
            let mut evidence = Vec::new();
            for binding_id in &obligation.bindings {
                let binding = self.bindings.get(binding_id).expect("validated binding");
                evidence.push(NativeFixtureEvidence {
                    targets: self
                        .target_set(&binding.target_set)?
                        .iter()
                        .cloned()
                        .collect(),
                    test_target: binding.cargo_target.fixture_name(),
                    exact_test: binding.exact_test.clone(),
                });
            }
            cases.push(NativeFixtureCase {
                id: id.clone(),
                authority: obligation.authority.clone(),
                evidence,
                release_evidence: obligation.release_evidence.clone(),
            });
        }
        Ok(cases)
    }

    pub fn validate_frozen_projection(
        &self,
        expected_identity: &str,
        expected_cases: &[NativeFixtureCase],
    ) -> Result<()> {
        self.validate()?;
        if self.identity()? != expected_identity {
            return schema("native authority identity disagrees with the frozen projection");
        }
        if canonical::encode(&self.fixture_cases()?)? != canonical::encode(&expected_cases)? {
            return schema("native authority cases disagree with the frozen projection");
        }
        Ok(())
    }

    fn target_set(&self, id: &str) -> Result<BTreeSet<String>> {
        self.target_sets
            .get(id)
            .map(|targets| targets.iter().cloned().collect())
            .ok_or_else(|| Error::Schema(format!("unknown target set {id:?}")))
    }
}

impl CargoTarget {
    pub fn fixture_name(&self) -> String {
        match self.kind {
            CargoTargetKind::Lib => "lib".to_owned(),
            CargoTargetKind::Test => self.name.clone(),
        }
    }
}

fn validate_binding(binding: &CargoBinding, authority: &NativeEvidenceAuthority) -> Result<()> {
    if binding.package.is_empty()
        || binding.exact_test.is_empty()
        || !authority.feature_sets.contains_key(&binding.feature_set)
        || !authority.target_sets.contains_key(&binding.target_set)
        || !is_sorted_unique(&binding.classes)
    {
        return schema("native Cargo binding is incomplete or references an unknown set");
    }
    validate_cargo_target(&binding.cargo_target)
}

fn validate_release_evidence(
    evidence: &ReleaseEvidence,
    authority: &NativeEvidenceAuthority,
) -> Result<()> {
    let upstream = authority
        .authorities
        .get(&evidence.authority)
        .is_some_and(|authority| authority.class == AuthorityClass::InheritedUpstream);
    if !upstream
        || evidence.exact_test.is_empty()
        || !authority.target_sets.contains_key(&evidence.target_set)
    {
        return schema("release evidence is not bound to an upstream authority and target set");
    }
    validate_cargo_target(&evidence.cargo_target)
}

fn validate_cargo_target(target: &CargoTarget) -> Result<()> {
    match target.kind {
        CargoTargetKind::Lib if target.name == "lib" => Ok(()),
        CargoTargetKind::Test if !target.name.is_empty() && target.name != "lib" => Ok(()),
        _ => schema("Cargo target kind/name is not canonical"),
    }
}

fn validate_obligation(
    obligation: &Obligation,
    authority: &NativeEvidenceAuthority,
    all_cases: &mut BTreeSet<String>,
) -> Result<()> {
    if obligation.description.is_empty()
        || !authority.authorities.contains_key(&obligation.authority)
        || !authority.target_sets.contains_key(&obligation.target_set)
        || obligation.bindings.is_empty() && obligation.release_evidence.is_empty()
        || !is_sorted_unique(&obligation.bindings)
        || !is_sorted_unique(&obligation.release_evidence)
        || obligation.adversarial_cases.is_empty()
        || !is_sorted_unique(&obligation.adversarial_cases)
    {
        return schema("native obligation is incomplete, duplicated or references an unknown set");
    }
    for case in &obligation.adversarial_cases {
        validate_id("adversarial case", case)?;
        if !all_cases.insert(case.clone()) {
            return schema("adversarial case is repeated across native obligations");
        }
    }
    let required = authority.target_set(&obligation.target_set)?;
    let mut covered = BTreeSet::new();
    for id in &obligation.bindings {
        let binding = authority
            .bindings
            .get(id)
            .ok_or_else(|| Error::Schema(format!("unknown binding {id:?}")))?;
        covered.extend(authority.target_set(&binding.target_set)?);
    }
    for id in &obligation.release_evidence {
        let evidence = authority
            .release_evidence
            .get(id)
            .ok_or_else(|| Error::Schema(format!("unknown release evidence {id:?}")))?;
        if evidence.authority != obligation.authority {
            return schema("obligation crosses its declared release authority");
        }
        covered.extend(authority.target_set(&evidence.target_set)?);
    }
    if covered != required {
        return schema("native obligation evidence does not exactly cover its target set");
    }
    Ok(())
}

fn validate_upstream_package(authority: &NativeEvidenceAuthority) -> Result<()> {
    let package = authority
        .authorities
        .values()
        .find_map(|authority| authority.package.as_ref())
        .ok_or_else(|| Error::Schema("missing inherited upstream package".into()))?;
    if package.name != "codewandler-connector-secrets"
        || package.version != "0.23.0"
        || package.registry_sha256
            != "360225fcfbd3af81248eb4fa449175a4909651612abdf5ffd9a54a09c35a2e14"
        || package.released_commit != "7f6872c94eba050c556625065618a9f360ed4562"
    {
        return schema("inherited connector-secrets release identity changed");
    }
    Ok(())
}

fn validate_unique_refs<T>(
    kind: &str,
    owner: &str,
    values: &[String],
    available: &BTreeMap<String, T>,
) -> Result<()> {
    if values.is_empty() || !is_sorted_unique(values) {
        return schema(&format!("{owner:?} has an empty or duplicated {kind} list"));
    }
    if values.iter().any(|value| !available.contains_key(value)) {
        return schema(&format!("{owner:?} references an unknown {kind}"));
    }
    Ok(())
}

fn validate_map_keys<T>(kind: &str, values: &BTreeMap<String, T>) -> Result<()> {
    values.keys().try_for_each(|key| validate_id(kind, key))
}

fn is_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn target_values(values: &[String]) -> BTreeSet<&str> {
    values.iter().map(String::as_str).collect()
}

fn validate_id(kind: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && !value.contains("--");
    if valid {
        Ok(())
    } else {
        schema(&format!("{kind} id {value:?} is not canonical"))
    }
}

fn validate_source_commit(value: &str) -> Result<()> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        schema("native report source commit is not one lowercase full SHA")
    }
}

fn schema<T>(message: &str) -> Result<T> {
    Err(Error::Schema(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    type AuthorityMutation = (&'static str, Box<dyn Fn(&mut NativeEvidenceAuthority)>);

    #[test]
    fn native_evidence_authority_rejects_each_missing_family_target_and_test() {
        let authority = NativeEvidenceAuthority::bundled().expect("canonical authority");
        let identity = authority.identity().expect("authority identity");
        let cases = authority.fixture_cases().expect("derived cases");

        let mut mutations: Vec<AuthorityMutation> = Vec::new();
        mutations.push((
            "authority class",
            Box::new(|authority| {
                let key = authority.authorities.keys().next().cloned().expect("class");
                authority.authorities.remove(&key);
            }),
        ));
        mutations.push((
            "family",
            Box::new(|authority| {
                let key = authority
                    .obligations
                    .keys()
                    .next()
                    .cloned()
                    .expect("family");
                authority.obligations.remove(&key);
            }),
        ));
        mutations.push((
            "target",
            Box::new(|authority| {
                let key = authority.targets.keys().next().cloned().expect("target");
                authority.targets.remove(&key);
            }),
        ));
        mutations.push((
            "runner",
            Box::new(|authority| {
                authority
                    .targets
                    .values_mut()
                    .next()
                    .expect("runner")
                    .runner
                    .push_str("-substituted");
            }),
        ));
        mutations.push((
            "feature",
            Box::new(|authority| {
                authority
                    .feature_sets
                    .values_mut()
                    .find(|features| !features.is_empty())
                    .expect("feature set")[0]
                    .push_str("-substituted");
            }),
        ));
        mutations.push((
            "exact Cargo test",
            Box::new(|authority| {
                authority
                    .bindings
                    .values_mut()
                    .next()
                    .expect("binding")
                    .exact_test
                    .push_str("_substituted");
            }),
        ));

        for (label, mutate) in mutations {
            let mut changed = authority.clone();
            mutate(&mut changed);
            assert!(
                changed
                    .validate_frozen_projection(&identity, &cases)
                    .is_err(),
                "{label} mutation retained publication authority"
            );
        }

        let source_commit = "0123456789abcdef0123456789abcdef01234567";
        let mut reports = authority
            .targets
            .keys()
            .map(|target| authority.report(target, source_commit).expect("report"))
            .collect::<Vec<_>>();
        authority
            .validate_reports(&reports, source_commit)
            .expect("complete reports");
        reports.pop();
        assert!(authority.validate_reports(&reports, source_commit).is_err());
        reports = authority
            .targets
            .keys()
            .map(|target| authority.report(target, source_commit).expect("report"))
            .collect();
        reports[0].runner.push_str("-substituted");
        assert!(authority.validate_reports(&reports, source_commit).is_err());
    }

    /// Regenerating the frozen projection may not launder a changed family inventory. The frozen
    /// digest only refuses drift that was *not* regenerated, so the retained inherited obligations
    /// and literal families are held as the document's own contract: renaming, dropping or
    /// inventing one refuses before anything is frozen or reported.
    #[test]
    fn native_evidence_authority_rejects_a_renamed_dropped_or_invented_family() {
        let authority = NativeEvidenceAuthority::bundled().expect("canonical authority");
        authority.validate().expect("canonical inventory validates");

        let mut renamed = authority.clone();
        let family = renamed
            .obligations
            .remove("x134-native-stream-framing")
            .expect("literal stream-framing family");
        renamed
            .obligations
            .insert("x134-native-stream-framing-v2".to_owned(), family);
        assert!(
            renamed.validate().is_err(),
            "a renamed literal family retained publication authority"
        );

        let mut dropped = authority.clone();
        dropped
            .obligations
            .remove("x128-unix-inherited-abi")
            .expect("inherited ABI obligation");
        assert!(
            dropped.validate().is_err(),
            "a dropped inherited obligation retained publication authority"
        );

        let mut invented = authority.clone();
        let mut family = invented
            .obligations
            .get("x134-grant-cas")
            .cloned()
            .expect("literal grant CAS family");
        family.adversarial_cases = vec!["invented-adversarial-case".to_owned()];
        invented
            .obligations
            .insert("x134-extra-family".to_owned(), family);
        assert!(
            invented.validate().is_err(),
            "an invented literal family retained publication authority"
        );
    }
}
