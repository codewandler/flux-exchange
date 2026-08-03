//! How a credential was **obtained**, and the named weaknesses in obtaining it that way.
//!
//! Every credential this service holds today arrived the same way: a human pasted it in. That is
//! `connector_catalog::Acquisition::Static` — *the stored secret, unchanged* — and it is the reason
//! nothing in this crate has ever had to describe an acquisition. A connector that mints its
//! credential by presenting the resource owner's own password does have to be described, because
//! the presenting is a weakness a deployment must be able to refuse.
//!
//! This module is the vocabulary for that refusal, and — for now — nothing else. [`AuthHazard`] is
//! the word the filter is written against; the filter itself, the opt-in that arms it and the
//! acquisition that triggers it are X-74 and X-75. `docs/designs/credential-acquisition.md` is the
//! argument, and §1 is this file.
//!
//! # Why a deployment refuses a property and not a connector
//!
//! `AGENTS.md` § Invariants: **grants select by declared metadata, not by name.** An operator who
//! wants no password-grant authentication anywhere should say that once, about a declared property,
//! and have every connector carrying it refuse — including the fifty-fifth provider, added next
//! month by somebody who never read their policy. The alternative is a list of connector names,
//! which is correct on the day it is written and silently wrong afterwards.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A named weakness in **how a credential is obtained**, declared by the connector.
///
/// A closed set, and the closedness is the feature. The obvious alternative — a free-form
/// `hazard = "..."` string carrying its own citations — is more expressive and strictly worse,
/// because it makes the filter a string match: a near-miss spelling matches no allow-list entry,
/// reads as *no hazard declared*, and is admitted by the very deployment that refused the thing it
/// names. An unknown value here is a refusal at deserialisation instead, which
/// `tests/auth_hazard.rs` is the whole of. The cost is that a new hazard is a deliberate edit to
/// this file, and that cost is the point.
///
/// # This is a *kind*. [`Risk`](crate::Risk) is a *level*
///
/// The two are not interchangeable and the difference is load-bearing, because [`Risk`](crate::Risk)
/// is **ordered** and [`Selector::at_most`](crate::Selector::at_most) compares against that
/// ordering. A hazard has no position on that ladder: a password grant that buys a **read-only**
/// token is `Risk::Low` *and* hazardous. A fifth rung is therefore wrong in one direction or the
/// other — high enough to catch it and every destructive operation inherits a weakness it does not
/// have; low enough not to and `at_most(Risk::High)` silently admits password-grant authentication
/// to every grant an operator has already written.
///
/// It is also deliberately **not** a field on [`OperationFacts`](crate::OperationFacts). A hazard is
/// a property of an *acquisition*, which happens once per connection; an operation happens per call.
/// Putting it there would restate one connection's fact on 389 rows and invite a per-call answer to
/// a per-connection question.
///
/// # `#[non_exhaustive]`, from the start
///
/// A second hazard is a matter of when, not whether, and adding one must not be a breaking change
/// for a published crate. Marking it at birth is free; retrofitting it is itself the break it was
/// meant to prevent.
///
/// **That is not licence for a wildcard arm inside this crate.** `#[non_exhaustive]` binds
/// *consumers*; a match written here is still exhaustive with no `_`, exactly as
/// [`OperationFacts::of`](crate::OperationFacts::of) argues for the vocabularies it mirrors. When
/// upstream declares a hazard and this crate maps it, a catch-all would answer a hazard it had never
/// heard of with a plausible wrong one — and the filter would then admit it without anybody having
/// decided to.
///
/// # Ordering
///
/// `Ord` is derived so a set of hazards can be a `BTreeSet`, which is the shape an allow-list wants,
/// and for **no other reason**. It is declaration order, it is not severity, and nothing may read it
/// as a ladder — that is the mistake the section above exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthHazard {
    /// The resource owner's own password is presented to **this host** rather than to the
    /// authorization server.
    ///
    /// The OAuth 2.0 resource owner password credentials grant. The name states the property a
    /// filter is written against and an auditor can check — *the resource owner's secret was
    /// shared* — rather than naming the grant that happens to be the shipped instance of it.
    ///
    /// **RFC 9700 §2.4** (Best Current Practice for OAuth 2.0 Security, 2025) — the grant
    /// **MUST NOT** be used. Three stated reasons, and together they are the whole hazard:
    ///
    /// 1. it exposes the resource owner's credentials to the client;
    /// 2. it widens where those credentials can leak, beyond the authorization server;
    /// 3. it cannot carry two-factor or any other multi-step authentication.
    ///
    /// **RFC 6749 §4.3** — the client MUST discard those credentials once an access token is
    /// obtained. Here the client is this host, so that MUST is ours: the password is never stored,
    /// never logged, and never in an error body.
    ///
    /// **CWE-522**, *Insufficiently Protected Credentials*, is the nearest weakness-catalogue entry,
    /// and is what an auditor's tooling will be looking for.
    ///
    /// OAuth 2.1 drops the grant entirely. It remains worth supporting behind an explicit opt-in
    /// because a vendor that offers nothing else offers this or nothing — which is a decision for a
    /// deployment to make knowingly, and the reason this type exists rather than a refusal being
    /// hard-coded.
    ResourceOwnerSecretShared,
}

impl AuthHazard {
    /// The stable configuration spelling of this hazard.
    ///
    /// Matched exhaustively here rather than obtained by serialising the enum. Deployment policy
    /// is configuration an operator audits, and changing a serde helper must not silently change
    /// the string that grants a weaker authentication path.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourceOwnerSecretShared => "resource_owner_secret_shared",
        }
    }
}

impl std::fmt::Display for AuthHazard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A deployment's startup-selected policy over declared authentication hazards.
///
/// The default refuses every hazard. A composition may explicitly construct a posture from typed
/// hazards it read at startup; there is deliberately no method that takes a request, header, body,
/// connector id or other caller-controlled policy input. The connector id reaches only
/// [`admit`](Self::admit), where it makes a refusal actionable and never changes the decision.
///
/// Admission is evaluated when an acquisition is attempted, not when a catalogue is loaded. That
/// is what keeps the rule true after a running deployment learns a connector declaration it did
/// not have at boot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthPosture {
    allowed: BTreeSet<AuthHazard>,
}

impl AuthPosture {
    /// The production default: no declared authentication hazard is allowed.
    pub fn fail_closed() -> Self {
        Self::default()
    }

    /// Construct the policy an operator explicitly selected at startup.
    ///
    /// This takes only closed, typed [`AuthHazard`] values. Parsing deployment configuration and
    /// refusing unknown spellings belongs to the composition, before it constructs this value.
    pub fn allowing(hazards: impl IntoIterator<Item = AuthHazard>) -> Self {
        Self {
            allowed: hazards.into_iter().collect(),
        }
    }

    /// Decide one connector acquisition from the hazard the connector declares.
    ///
    /// `connector` is carried only into the refusal. A deployment opts in to a property once,
    /// never to a list of connector names, so a newly catalogued connector inherits the same rule.
    ///
    /// # Errors
    ///
    /// [`AuthPostureRefusal::HazardNotAllowed`] when this deployment did not explicitly allow the
    /// declared hazard.
    pub fn admit(&self, connector: &str, hazard: AuthHazard) -> Result<(), AuthPostureRefusal> {
        if self.allowed.contains(&hazard) {
            return Ok(());
        }

        Err(AuthPostureRefusal::HazardNotAllowed {
            connector: connector.to_owned(),
            hazard,
        })
    }
}

/// Why a credential acquisition was refused by deployment policy.
///
/// Separate from a vendor rejection: this host made the decision before any request left the
/// process, so telling an operator to re-enter a correct password would be the wrong remedy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthPostureRefusal {
    /// The connector declared a hazard this deployment did not opt into.
    #[error(
        "connector `{connector}` declares authentication hazard `{hazard}`, and this deployment did \
         not allow it; no acquisition request was sent"
    )]
    HazardNotAllowed {
        /// The connector whose acquisition was attempted.
        connector: String,
        /// The declared hazard that was not allowed.
        hazard: AuthHazard,
    },
}

impl AuthPostureRefusal {
    /// The HTTP status this refusal carries when a composition exposes acquisition over HTTP.
    ///
    /// Kept on the refusal so two acquisition surfaces cannot answer the same policy decision
    /// differently. `403` says the deployment forbade a declared path. A vendor rejecting
    /// otherwise admitted credentials is a later, separate refusal and must not use this status.
    pub const fn status(&self) -> u16 {
        match self {
            Self::HazardNotAllowed { .. } => 403,
        }
    }
}
