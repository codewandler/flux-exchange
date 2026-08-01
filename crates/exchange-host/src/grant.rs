//! Grants: what a principal may call, decided from declared metadata.
//!
//! The rule this file exists to enforce: **a grant selects operations by what they declare, not by
//! what they are called.** A grant written as a list of operation ids is a list somebody has to
//! maintain, and it silently stops covering a connector the moment that connector gains an
//! operation. A grant written as `risk <= low` covers the new one correctly on the day it lands.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// How dangerous an operation is, as the catalogue declares it.
///
/// Ordered, and the ordering is load-bearing — [`Selector::at_most`] compares against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// A read, or a write with no lasting consequence.
    Low,
    /// A write a person would want to know about.
    Medium,
    /// A write that moves money, credentials, or live state.
    High,
    /// Irreversible.
    Destructive,
}

/// Whether repeating a call repeats its effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    /// Repeating is safe.
    Idempotent,
    /// Repeating is safe only under a stated condition.
    Conditional,
    /// Repeating repeats the effect.
    NotIdempotent,
}

/// What an operation touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Effect {
    /// Reaches the network.
    Network,
    /// Writes to a workspace.
    WorkspaceWrite,
    /// Starts a process.
    Process,
}

/// The declared facts about one operation that a [`Selector`] reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationFacts {
    /// The operation id, as the catalogue spells it.
    pub id: String,
    /// Its declared risk.
    pub risk: Risk,
    /// Its declared idempotency.
    pub idempotency: Idempotency,
    /// Its declared effects.
    pub effects: BTreeSet<Effect>,
}

/// A predicate over declared metadata, plus explicit exceptions.
///
/// Exceptions are last and they are deliberately asymmetric: `deny` beats `allow`, and both beat
/// the predicate. An operator who has explicitly denied one operation means it, and no metadata
/// change should quietly re-admit it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selector {
    /// Admit only operations at or below this risk.
    pub max_risk: Option<Risk>,
    /// Admit only operations whose effects are a subset of this set.
    pub effects_within: Option<BTreeSet<Effect>>,
    /// Admit only operations with this idempotency.
    pub idempotency: Option<Idempotency>,
    /// Always admit these ids, whatever the predicate says.
    pub allow_ids: BTreeSet<String>,
    /// Never admit these ids, whatever anything else says.
    pub deny_ids: BTreeSet<String>,
}

impl Selector {
    /// Admit everything. Useful for an operator's own interactive session; rarely right for an agent.
    pub fn any() -> Self {
        Self::default()
    }

    /// Admit operations at or below `risk`.
    pub fn at_most(risk: Risk) -> Self {
        Self {
            max_risk: Some(risk),
            ..Self::default()
        }
    }

    /// Additionally require that every effect is in `effects`.
    pub fn with_effects_within(mut self, effects: impl IntoIterator<Item = Effect>) -> Self {
        self.effects_within = Some(effects.into_iter().collect());
        self
    }

    /// Never admit this operation, whatever the predicate says.
    pub fn deny(mut self, id: impl Into<String>) -> Self {
        self.deny_ids.insert(id.into());
        self
    }

    /// Admit this operation even if the predicate would not.
    pub fn allow(mut self, id: impl Into<String>) -> Self {
        self.allow_ids.insert(id.into());
        self
    }

    /// Does this selector admit `op`?
    pub fn admits(&self, op: &OperationFacts) -> bool {
        if self.deny_ids.contains(&op.id) {
            return false;
        }
        if self.allow_ids.contains(&op.id) {
            return true;
        }
        if self.max_risk.is_some_and(|max| op.risk > max) {
            return false;
        }
        if self
            .effects_within
            .as_ref()
            .is_some_and(|allowed| !op.effects.is_subset(allowed))
        {
            return false;
        }
        if self.idempotency.is_some_and(|want| op.idempotency != want) {
            return false;
        }
        true
    }
}

/// One grant: a connector, and which of its operations are reachable.
///
/// A grant never names a credential. That is the property that makes an agent token safe to hand
/// out — resolving the credential is the host's job, from the connection the grant points at, and a
/// stolen token therefore yields a bounded set of *operations* rather than a vendor secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// The connector this grant reaches. Never a wildcard: a grant that reached every connector
    /// would re-admit whatever the next connection added, without anyone deciding to.
    pub connector: String,
    /// Which of its operations.
    pub selector: Selector,
}

impl Grant {
    /// A grant over one connector.
    pub fn for_connector(connector: impl Into<String>, selector: Selector) -> Self {
        Self {
            connector: connector.into(),
            selector,
        }
    }

    /// Does this grant admit `op` of `connector`?
    pub fn admits(&self, connector: &str, op: &OperationFacts) -> bool {
        self.connector == connector && self.selector.admits(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(id: &str, risk: Risk) -> OperationFacts {
        OperationFacts {
            id: id.into(),
            risk,
            idempotency: Idempotency::Idempotent,
            effects: BTreeSet::from([Effect::Network]),
        }
    }

    #[test]
    fn at_most_is_inclusive_of_its_own_level() {
        let selector = Selector::at_most(Risk::Medium);
        assert!(selector.admits(&facts("a", Risk::Low)));
        assert!(selector.admits(&facts("b", Risk::Medium)));
        assert!(!selector.admits(&facts("c", Risk::High)));
        assert!(!selector.admits(&facts("d", Risk::Destructive)));
    }

    /// The asymmetry that matters: an explicit deny survives an explicit allow.
    #[test]
    fn deny_beats_allow_and_both_beat_the_predicate() {
        let selector = Selector::at_most(Risk::Low)
            .allow("escalated")
            .deny("forbidden")
            .allow("forbidden");

        assert!(selector.admits(&facts("escalated", Risk::Destructive)));
        assert!(!selector.admits(&facts("forbidden", Risk::Low)));
    }

    #[test]
    fn effects_must_be_a_subset_not_an_intersection() {
        let selector = Selector::any().with_effects_within([Effect::Network]);

        let network_only = facts("a", Risk::Low);
        let mut also_spawns = facts("b", Risk::Low);
        also_spawns.effects.insert(Effect::Process);

        assert!(selector.admits(&network_only));
        assert!(
            !selector.admits(&also_spawns),
            "an operation with an extra effect must be refused, not admitted on the overlap",
        );
    }

    /// A grant with no constraints still does not leak across connectors.
    #[test]
    fn a_permissive_selector_is_still_connector_scoped() {
        let grant = Grant::for_connector("zendesk", Selector::any());
        assert!(grant.admits("zendesk", &facts("x", Risk::Destructive)));
        assert!(!grant.admits("slack", &facts("x", Risk::Low)));
    }
}
