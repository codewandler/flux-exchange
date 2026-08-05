//! Grants: what a principal may call, decided from declared metadata.
//!
//! The rule this file exists to enforce: **a grant selects operations by what they declare, not by
//! what they are called.** A grant written as a list of operation ids is a list somebody has to
//! maintain, and it silently stops covering a connector the moment that connector gains an
//! operation. A grant written as `risk <= low` covers the new one correctly on the day it lands.
//!
//! # Since X-13 this decides, rather than describing
//!
//! [`Selector`] and [`Grant`] were tested types with nothing behind them until this story wired
//! them to the one path that spends a tenant's credentials. Three things arrived with that, and
//! each is here rather than in [`crate::invoke`] because each is a question about a *grant*:
//!
//! - [`OperationFacts::of`] — the catalogue projected into the facts a [`Selector`] reads, so what
//!   the gate decides on is the operation's own declaration and never a list of ids kept beside it;
//! - [`admit_grant`] and [`Granted`] — the gate, and the proof it produces. `Granted` is what
//!   `Invoker::invoke` must hold to reach the pack at all, so *forgetting* the gate does not
//!   compile. [`crate::Admitted`] carries the whole argument for that shape and the measured
//!   failure it answers;
//! - [`Grants`] and [`GrantStore`] — where a tenant's grants live, so they can be stored and
//!   edited rather than compiled in.
//!
//! **A tenant's grants are the tenant's, not one principal's.** Every principal this host resolves
//! for a tenant is decided against the same set. A Service Account token authenticates one
//! principal, then grants independently decide what its tenant may do; lifecycle routes remain
//! restricted to a `User`. Per-principal grants are a
//! narrowing this build does not make, and saying so is cheaper than implying otherwise: the
//! refusal names the principal because that is who was refused, not because the grant was theirs.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Admitted, Error, Principal, Tenant};

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

impl OperationFacts {
    /// **What one catalogue operation declares**, as a [`Selector`] reads it.
    ///
    /// The projection the gate decides on — and, since X-13, the same one
    /// `routes::catalogue::view` publishes, rather than a second copy beside it. Two projections
    /// would be two answers to *"what is this operation's risk"*, and the one a client reads would
    /// be the one that is not deciding: a caller could then predict admission correctly and still be
    /// refused. `the_catalogue_publishes_the_facts_the_gate_decides_on` is what holds them together.
    ///
    /// Both vocabulary mappings below are exhaustive with no wildcard arm, deliberately and in both
    /// directions: `connector_catalog`'s enums and this crate's are independent mirrors of flux's,
    /// they already disagree about one spelling (`NonIdempotent` here is [`Idempotency::NotIdempotent`]),
    /// and a catch-all would answer a level it had never heard of with a plausible wrong one — which
    /// a `Selector::at_most` then admits without anybody having decided to.
    ///
    /// # `effects` is **derived**, and that is a measured gap rather than a detail
    ///
    /// `connector_catalog::Operation` publishes `risk`, `idempotency`, `credentials`, `hosts` and
    /// the operation's Flux — and **no effects field** (re-checked against 0.16.0 in X-106), even
    /// though the operation's own Flux source carries an `effects` declaration. So the rule here is
    /// the most
    /// the *readable* data supports and not one step further: **an operation the catalogue gives a
    /// host to reach touches the network.** [`Effect::WorkspaceWrite`] and [`Effect::Process`] are
    /// never emitted, because nothing this crate can read speaks to either.
    ///
    /// The consequence for a grant, stated plainly because it is the direction that costs: a
    /// selector written `with_effects_within([Effect::Network])` is exact for every connector this
    /// build carries — all declare `http`, re-measured on catalogue 0.16 in X-106 rather than
    /// carried over from an older catalogue — and would silently admit an operation with an
    /// *unreported* effect the day one ships. What keeps that from being live today is the same fact
    /// that makes
    /// the runtime refusal undrivable through `invoke`, and it is asserted rather than assumed:
    /// `the_whole_catalogue_declares_http` in [`crate::invoke`], and
    /// `effects_are_derived_from_hosts_and_never_claim_more_than_that` here. When upstream declares
    /// effects, this function reads them and the paragraph goes.
    pub fn of(operation: &connector_catalog::Operation) -> Self {
        let mut effects = BTreeSet::new();
        if !operation.hosts.is_empty() {
            effects.insert(Effect::Network);
        }

        Self {
            id: operation.id.to_owned(),
            risk: match operation.risk {
                connector_catalog::Risk::Low => Risk::Low,
                connector_catalog::Risk::Medium => Risk::Medium,
                connector_catalog::Risk::High => Risk::High,
                connector_catalog::Risk::Destructive => Risk::Destructive,
            },
            idempotency: match operation.idempotency {
                connector_catalog::Idempotency::Idempotent => Idempotency::Idempotent,
                connector_catalog::Idempotency::NonIdempotent => Idempotency::NotIdempotent,
                connector_catalog::Idempotency::Conditional => Idempotency::Conditional,
            },
            effects,
        }
    }
}

/// A predicate over declared metadata, plus explicit exceptions.
///
/// Exceptions are last and they are deliberately asymmetric: `deny` beats `allow`, and both beat
/// the predicate. An operator who has explicitly denied one operation means it, and no metadata
/// change should quietly re-admit it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
/// A grant never names a credential. That is the property that makes a Service Account token safe to hand
/// out — resolving the credential is the host's job, from the connection the grant points at, and a
/// stolen token therefore yields a bounded set of *operations* rather than a vendor secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The connector this grant reaches. Never a wildcard: a grant that reached every connector
    /// would re-admit whatever the next connection added, without anyone deciding to.
    pub connector: String,
    /// Which of its operations.
    pub selector: Selector,
    /// Explicit inbound connector bindings and declared event subsets. Absent in old documents and
    /// therefore defaulting to no inbound access.
    #[serde(default)]
    pub inbound: Vec<InboundGrant>,
}

impl Grant {
    /// A grant over one connector.
    pub fn for_connector(connector: impl Into<String>, selector: Selector) -> Self {
        Self {
            connector: connector.into(),
            selector,
            inbound: Vec::new(),
        }
    }

    /// Does this grant admit `op` of `connector`?
    pub fn admits(&self, connector: &str, op: &OperationFacts) -> bool {
        self.connector == connector && self.selector.admits(op)
    }
}

/// One explicit inbound grant. Unlike outbound selectors, an inbound grant is a closed event set:
/// a vendor-created discriminator value can never become a trigger label through a wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboundGrant {
    /// Connector catalogue id.
    pub connector: String,
    /// Declared binding name.
    pub binding: String,
    /// Declared events this grant admits.
    pub events: BTreeSet<String>,
}

/// Proof that a tenant grant admits the selected inbound events.
#[derive(Debug)]
pub struct InboundGranted {
    connector: String,
    binding: String,
    events: BTreeSet<String>,
}

impl InboundGranted {
    /// Connector admitted by the proof.
    pub fn connector(&self) -> &str {
        &self.connector
    }

    /// Binding admitted by the proof.
    pub fn binding(&self) -> &str {
        &self.binding
    }

    /// Selected event subset admitted by the proof.
    pub fn events(&self) -> &BTreeSet<String> {
        &self.events
    }
}

/// Admit a selected declared event subset. An empty selection and any event outside one explicit
/// matching entry are refused. The principal supplies the tenant whose grants the caller loaded;
/// it is used only in the non-enumerating refusal.
pub fn admit_inbound(
    principal: &Principal,
    connector: &str,
    binding: &str,
    events: &BTreeSet<String>,
    grants: &[Grant],
) -> Result<InboundGranted, Error> {
    let admitted = !events.is_empty()
        && grants.iter().flat_map(|grant| &grant.inbound).any(|grant| {
            grant.connector == connector
                && grant.binding == binding
                && events.is_subset(&grant.events)
        });
    if !admitted {
        return Err(Error::InboundNotGranted {
            principal: principal.to_string(),
            connector: connector.to_owned(),
            binding: binding.to_owned(),
        });
    }
    Ok(InboundGranted {
        connector: connector.to_owned(),
        binding: binding.to_owned(),
        events: events.clone(),
    })
}

// ---------------------------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------------------------

/// **Proof that a grant admitted an operation**, as a value rather than as a line somebody ran.
///
/// The same shape as [`Admitted`] and for the same measured reason — read that type first; it
/// carries the argument in full, including the three mutations that defeated the source scanner it
/// replaced. The short version: [`operation`](Self::operation) is private, there is no public
/// constructor, no `Default` and no `Clone`, so a value of this type cannot be spelled into
/// existence anywhere. The only producer is [`admit_grant`], on its success arm.
///
/// What it adds is that the two gates are now **one chain rather than two switches**.
/// [`admit_grant`] *consumes* the [`Admitted`] the runtime gate produced and `Granted::resolve` is
/// the only route from this crate to `connector_pack::resolve`, so the dispatch path cannot reach
/// the pack without having gone through the runtime gate *and then* the grant gate, in that order,
/// and `rustc` is what checks it. Skipping either is not a review finding; it is a type error.
///
/// The chain, each link held by a different mechanism:
///
/// 1. `Invoker::invoke` cannot dispatch without a `Granted` — **the compiler**, because
///    `connector_pack::resolve` is reachable from there only through `Granted::resolve`.
/// 2. A `Granted` comes only from [`admit_grant`] — **the compiler**, via the private field.
/// 3. A `Granted` cannot be made without an [`Admitted`] — **the compiler**, via the argument.
/// 4. [`admit_grant`] refuses an operation no grant admits — **tests**, here and in
///    `tests/invoke.rs`, driven end to end over a read-only grant, a grant for another connector,
///    and an explicit deny.
/// 5. The facts it decides on are the operation's own — **tests**, via [`OperationFacts::of`]'s
///    exhaustive mappings and `the_catalogue_publishes_the_facts_the_gate_decides_on`.
///
/// What no link covers, named here rather than left for the next reviewer: a future edit that calls
/// `admit_grant` with a `Grant` list from somewhere other than the caller's tenant, or with a
/// connector id that is not the operation's. Both type-check. The first is bounded by
/// `Invoker::invoke` reading the tenant off the resolved principal in one expression; the second by
/// the connector being derived from the catalogue entry rather than named by a caller.
#[derive(Debug)]
pub struct Granted {
    /// The runtime gate's own proof, consumed rather than dropped.
    ///
    /// Held so the ordering is a value and not a comment: a `Granted` that could be built without
    /// one would make the two gates independent, and independent gates are two things to remember
    /// instead of one thing to satisfy.
    admitted: Admitted,
    /// The operation that was admitted.
    ///
    /// Private, with no constructor beside [`admit_grant`]. That is not encapsulation for its own
    /// sake — it is the entire enforcement, and a `pub` here would delete the argument above.
    operation: String,
}

impl Granted {
    /// The operation this proof was minted for.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// The runtime this deployment admitted on the way here.
    pub fn runtime(&self) -> crate::Runtime {
        self.admitted.runtime()
    }
}

/// **The grant gate**, as the one function `Invoker::invoke` calls.
///
/// It takes the runtime gate's [`Admitted`] and hands back a [`Granted`], which is the only key to
/// the dispatch seam — see [`Granted`] for why that is the shape rather than a `bool`.
///
/// `operation` is the *facts*, not an id: everything this decides is read out of
/// [`OperationFacts`], so there is no argument here through which a name could stand in for a
/// declaration. `connector` is derived from the catalogue entry by the caller and is never a
/// caller's string — a grant that reached every connector would re-admit whatever the next
/// connection added, without anyone deciding to, which is why [`Grant::connector`] is not a
/// wildcard either.
///
/// The order is the design's §2 and §4: **after** the runtime refusal, which is a property of the
/// deployment and the same answer for every principal, and **before** the ports are bound — a
/// refusal that happened after a secret had been read would have moved that secret into this
/// process's memory for a call that was never going to be made.
///
/// # Errors
///
/// [`Error::NotGranted`], naming the principal and the operation. It names neither the grants the
/// tenant does hold nor the axis that refused: an agent learning *which* predicate turned it down
/// can enumerate a tenant's policy one call at a time, and the remedy is the same sentence either
/// way — ask whoever holds the tenant for a grant that admits it.
pub fn admit_grant(
    admitted: Admitted,
    principal: &Principal,
    connector: &str,
    operation: &OperationFacts,
    grants: &[Grant],
) -> Result<Granted, Error> {
    if !grants
        .iter()
        .any(|grant| grant.admits(connector, operation))
    {
        return Err(Error::NotGranted {
            principal: principal.to_string(),
            operation: operation.id.clone(),
        });
    }

    Ok(Granted {
        admitted,
        operation: operation.id.clone(),
    })
}

// ---------------------------------------------------------------------------------------------
// Where a tenant's grants live
// ---------------------------------------------------------------------------------------------

/// **A tenant's grants**, as the port a composition binds.
///
/// Read and write in one trait rather than two, for [`crate::ConnectionSettings`]' reason: a
/// composition that could bind a *reader* to the invoker and a *writer* to a surface would have two
/// stores that disagree about what a tenant is allowed to do, which is the failure this whole gate
/// exists to prevent arriving through composition instead of through mutation.
///
/// Both sides take a [`Tenant`] rather than a `&str`, so a tenant that could traverse cannot be
/// spelled at either. The tenant is read off the resolved principal by the surface that calls this
/// and from nothing a caller controls.
///
/// # This port is a place to keep decisions, never a place to make them
///
/// An implementation stores what it is given and hands it back. Whether a grant admits an operation
/// is [`admit_grant`]'s question and is decided in one place, so a store cannot answer it
/// differently — a second opinion here would be a second spelling of one rule, and the one that
/// disagreed would be the one deciding what may run.
pub trait Grants: Send + Sync {
    /// Every grant `tenant` holds, in the order they were stored.
    ///
    /// **Infallible, and fail-closed when it cannot answer.** An empty answer means "this tenant
    /// holds nothing", and a store that cannot read itself returns the same — which refuses every
    /// invocation rather than admitting one. That is the safe direction and it is the honest one
    /// only because the file binding below reads its file **at bind time** and refuses to start on
    /// a file it cannot parse: by the time this is called there is nothing left to fail but a
    /// poisoned lock. A binding that reads lazily must not use this shape.
    fn held(&self, tenant: &Tenant) -> Vec<Grant>;

    /// Replace the grants `tenant` holds.
    ///
    /// Whole-set rather than add-one, deliberately: a grant is an authorisation decision and the
    /// thing an operator needs to be able to state is *what this tenant may do*, entire. Editing
    /// one is reading the set, changing it and storing it back — which is a diff somebody can look
    /// at, where a `revoke(one)` beside a `grant(one)` is a sequence nobody can see the end state
    /// of.
    ///
    /// # Errors
    ///
    /// [`GrantRefusal`], which refuses and does not repair: a set that could not be persisted was
    /// not applied, because a store whose memory and file disagree would admit calls today that it
    /// refuses after a restart.
    fn set(&self, tenant: &Tenant, grants: &[Grant]) -> Result<(), GrantRefusal>;
}

/// Why a tenant's grants could not be stored. Every variant refuses; none repairs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantRefusal {
    /// The store could not be written.
    ///
    /// Carries the path so an operator knows which file to look at. A grant carries no secret — it
    /// names a connector and a predicate — so there is nothing here to withhold beyond what the
    /// address already says.
    #[error("the grant store at `{path}` could not be written: {reason}")]
    Unwritable {
        /// The store's path, as resolved.
        path: String,
        /// What the filesystem said.
        reason: String,
    },
}

// ---------------------------------------------------------------------------------------------
// The file binding
// ---------------------------------------------------------------------------------------------

pub use file::{GrantStore, GrantStoreError};

/// The file-backed binding of [`Grants`].
///
/// The portable owner-only filesystem boundary has a different stake here: nothing in this file is a secret, and a
/// **planted** grant is an authorisation bypass — somebody who can write here can decide what this
/// host will run with a tenant's credentials. Unix owner/mode checks or Windows process-SID and
/// protected-DACL checks keep that authority to the process identity and refuse unsafe existing
/// metadata without repairing it.
mod file {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::sync::RwLock;

    use serde::{Deserialize, Serialize};

    use super::{Grant, GrantRefusal, Grants, InboundGrant, Selector};
    use crate::grant_cas::{
        GrantApplyReceipt, GrantCandidate, GrantCandidateInbound, GrantDecisionObserver,
        GrantPreview, GrantProposalDigest, GrantReceiptId, GrantSelector, GrantTransactionRefusal,
        GrantTransactions, StoreRevision,
    };
    use crate::paths::{enclosing_working_tree, resolve};
    use crate::{private_fs, Tenant, GRANT_STORE_SETTING};

    /// The setting a composing binary reads the grant store path from.
    ///
    /// The host does not read the environment itself — a binary passes the value it found to
    /// [`GrantStore::bind_configured`]. The *name* lives here so the refusal below and the reader
    /// that produced the value cannot drift apart into two different spellings.
    /// A location that would have worked, quoted in every refusal.
    ///
    /// Written with `$HOME` rather than expanded: nothing here reads the environment, and a refusal
    /// that quoted this machine's home directory would be a refusal that had already made a choice.
    const EXAMPLE_PATH: &str = "$HOME/.local/share/flux-exchange/grants";

    /// One tenant's grants, keyed by the tenant's own identifier.
    ///
    /// Flat rather than nested, unlike the settings store: a grant already carries its connector,
    /// and keying by connector here would let one tenant hold two grants for one connector at one
    /// key — which is a shape [`Selector`](super::Selector) deliberately expresses inside a single
    /// grant instead.
    type Held = BTreeMap<String, Vec<Grant>>;

    const FORMAT: &str = "exchange.grant-store.v1";

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Versioned {
        format: String,
        tenants: BTreeMap<String, TenantRecord>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TenantRecord {
        grants: Vec<Grant>,
        receipts: Vec<ReceiptRecord>,
        revision: StoreRevision,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ReceiptRecord {
        connector: String,
        proposal_digest: GrantProposalDigest,
        receipt_id: GrantReceiptId,
        revision: StoreRevision,
    }

    impl ReceiptRecord {
        fn response(&self, replayed: bool) -> GrantApplyReceipt {
            GrantApplyReceipt {
                connector: self.connector.clone(),
                receipt_id: self.receipt_id,
                replayed,
                revision: self.revision,
            }
        }
    }

    #[derive(Debug, Clone)]
    enum Loaded {
        Legacy(Held),
        Versioned(Versioned),
    }

    /// **What every tenant of this host may run, in one file.**
    ///
    /// Held in memory and written through on every change, for the reason
    /// [`SettingsStore`](crate::SettingsStore) is: [`Grants::held`] is consulted on the dispatch
    /// path, where a read of the disk would put an IO error between a caller and an answer this
    /// host already knows. The file is the durable record; the map is what answers.
    ///
    /// It is read **once, at bind**, and a file that cannot be parsed refuses to bind. *Refuse;
    /// never repair* — a store that started empty on a parse failure would silently withdraw every
    /// tenant's authorisation and report it as a clean start, which reads to an operator as a host
    /// that refuses everything for no reason.
    #[derive(Debug)]
    pub struct GrantStore {
        path: PathBuf,
        held: RwLock<Loaded>,
    }

    impl GrantStore {
        /// Bind the store a configuration value names, or refuse.
        ///
        /// `configured` is what the operator set — `None` when nothing was set at all. Surrounding
        /// whitespace is trimmed and a value that is then empty counts as unset, because
        /// `FLUX_EXCHANGE_GRANTS=` in an environment file is an operator who has not chosen a path,
        /// not one who has chosen the current directory.
        ///
        /// # Errors
        ///
        /// [`GrantStoreError::Unconfigured`] when nothing was named, and otherwise whatever
        /// [`bind`](Self::bind) refuses with.
        pub fn bind_configured(configured: Option<&str>) -> Result<Self, GrantStoreError> {
            match configured.map(str::trim).filter(|value| !value.is_empty()) {
                Some(path) => Self::bind(path),
                None => Err(GrantStoreError::Unconfigured {
                    setting: GRANT_STORE_SETTING,
                }),
            }
        }

        /// Open — or create — the grant file at `path`, or refuse.
        ///
        /// The path is resolved against the current directory and through any symlink that already
        /// exists, so what is checked below is the location the store will really occupy rather
        /// than the spelling it was given — [`crate::paths`] is the same walk the other two file
        /// stores make, shared rather than copied.
        ///
        /// # Errors
        ///
        /// [`GrantStoreError::Unconfigured`] for an empty path, which is not a location;
        /// [`GrantStoreError::InsideWorkingTree`] for a path under a checkout;
        /// [`GrantStoreError::Unresolvable`] when the path cannot be made absolute at all; and
        /// [`GrantStoreError::Unusable`] when the file cannot be created or parsed.
        pub fn bind(path: impl AsRef<Path>) -> Result<Self, GrantStoreError> {
            let requested = path.as_ref();
            if requested.as_os_str().is_empty() {
                return Err(GrantStoreError::Unconfigured {
                    setting: GRANT_STORE_SETTING,
                });
            }

            let resolved = resolve(requested).map_err(|error| GrantStoreError::Unresolvable {
                path: requested.display().to_string(),
                reason: error.to_string(),
            })?;

            // Checked before anything is created, so a refused path is one nothing was written at.
            // A grant is not a secret and this is not about disclosure: a checked-in grant file is
            // an authorisation policy that arrives with a clone and outlives whoever decided it.
            if let Some(root) = enclosing_working_tree(&resolved) {
                return Err(GrantStoreError::InsideWorkingTree {
                    path: resolved.display().to_string(),
                    root: root.display().to_string(),
                });
            }

            // The directory, at startup rather than at the first write, for the settings store's
            // reason: the file itself is created lazily — there is nothing to write until an
            // operator grants something — so without this a bad path surfaces as a failed write
            // long after the moment an operator could connect it to what they configured.
            let directory = resolved.parent().ok_or_else(|| GrantStoreError::Unusable {
                path: resolved.display().to_string(),
                reason: "the store path has no parent directory".to_owned(),
            })?;
            private_fs::ensure_directory(directory).map_err(|error| GrantStoreError::Unusable {
                path: resolved.display().to_string(),
                reason: error.to_string(),
            })?;

            let held = read(&resolved)?;

            Ok(Self {
                path: resolved,
                held: RwLock::new(held),
            })
        }

        /// The file this store is kept in.
        ///
        /// Read back off the bound store, not remembered from the configuration.
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// The line a binary prints at startup, naming the store it is actually holding.
        ///
        /// It says what this file *decides* as well as where it is: an operator who reads three
        /// store paths at startup should be able to tell which one is the list of what may run.
        pub fn banner(&self) -> String {
            format!(
                "grants: {} (platform owner-only file store, what each tenant may run)",
                self.path.display()
            )
        }

        /// Persist the current map, or say why it could not be.
        ///
        /// Whole-file, through a sibling temporary and a `rename(2)`, so a reader never sees a
        /// half-written store and an interrupted write leaves the previous one intact. The same
        /// shape the other two file stores use, and for the same reason: this file is small and
        /// rewriting it is cheaper than being able to corrupt it.
        fn persist_versioned(&self, held: &Versioned) -> Result<(), GrantRefusal> {
            let unwritable = |reason: String| GrantRefusal::Unwritable {
                path: self.path.display().to_string(),
                reason,
            };

            let encoded =
                serde_json::to_vec_pretty(held).map_err(|error| unwritable(error.to_string()))?;
            if encoded.len() > 1024 * 1024 {
                return Err(unwritable(
                    "the complete grant store exceeds its 1 MiB durable bound".to_owned(),
                ));
            }
            private_fs::write_atomic(&self.path, &encoded)
                .map_err(|error| unwritable(error.to_string()))
        }
    }

    /// Read the store at `path`, or start from an empty one if it is not there yet.
    ///
    /// A file that is *there* and unreadable is a refusal. See [`GrantStore`]: there is deliberately
    /// no arm here that starts empty because parsing failed.
    fn read(path: &Path) -> Result<Loaded, GrantStoreError> {
        let Some(raw) =
            private_fs::read(path, 1024 * 1024).map_err(|error| GrantStoreError::Unusable {
                path: path.display().to_string(),
                reason: error.to_string(),
            })?
        else {
            return Ok(Loaded::Legacy(Held::new()));
        };

        if raw.is_empty() {
            return Ok(Loaded::Legacy(Held::new()));
        }

        let value: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|error| GrantStoreError::Unusable {
                path: path.display().to_string(),
                reason: error.to_string(),
            })?;
        if matches!(value.get("format"), Some(serde_json::Value::String(_))) {
            let versioned: Versioned =
                serde_json::from_value(value).map_err(|error| GrantStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                })?;
            validate_versioned(&versioned).map_err(|reason| GrantStoreError::Unusable {
                path: path.display().to_string(),
                reason,
            })?;
            Ok(Loaded::Versioned(versioned))
        } else {
            serde_json::from_value(value)
                .map(Loaded::Legacy)
                .map_err(|error| GrantStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                })
        }
    }

    fn validate_versioned(document: &Versioned) -> Result<(), String> {
        if document.format != FORMAT {
            return Err(format!(
                "unsupported grant store format `{}`",
                document.format
            ));
        }
        for (tenant, record) in &document.tenants {
            let mut digests = BTreeSet::new();
            let mut receipt_ids = BTreeSet::new();
            let mut prior_revision =
                StoreRevision::new(1).expect("one is a nonzero store revision");
            for receipt in &record.receipts {
                if !digests.insert(receipt.proposal_digest) {
                    return Err(format!("tenant `{tenant}` has a duplicate proposal digest"));
                }
                if !receipt_ids.insert(receipt.receipt_id) {
                    return Err(format!(
                        "tenant `{tenant}` has a duplicate receipt identity"
                    ));
                }
                if receipt.revision <= prior_revision || receipt.revision > record.revision {
                    return Err(format!(
                        "tenant `{tenant}` has a receipt outside its monotonic revision history"
                    ));
                }
                prior_revision = receipt.revision;
            }
        }
        Ok(())
    }

    fn initialize(loaded: &Loaded, tenant: &Tenant) -> (Versioned, bool) {
        match loaded {
            Loaded::Legacy(legacy) => {
                let mut tenants = legacy
                    .iter()
                    .map(|(tenant, grants)| {
                        (
                            tenant.clone(),
                            TenantRecord {
                                grants: grants.clone(),
                                receipts: Vec::new(),
                                revision: StoreRevision::new(1)
                                    .expect("one is a nonzero store revision"),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                tenants
                    .entry(tenant.as_str().to_owned())
                    .or_insert_with(empty_record);
                (
                    Versioned {
                        format: FORMAT.to_owned(),
                        tenants,
                    },
                    true,
                )
            }
            Loaded::Versioned(versioned) => {
                let mut versioned = versioned.clone();
                let changed = !versioned.tenants.contains_key(tenant.as_str());
                versioned
                    .tenants
                    .entry(tenant.as_str().to_owned())
                    .or_insert_with(empty_record);
                (versioned, changed)
            }
        }
    }

    fn empty_record() -> TenantRecord {
        TenantRecord {
            grants: Vec::new(),
            receipts: Vec::new(),
            revision: StoreRevision::new(1).expect("one is a nonzero store revision"),
        }
    }

    fn project(
        grants: &[Grant],
        connector: &str,
        selector: GrantSelector,
    ) -> Result<(GrantCandidate, Option<usize>), GrantTransactionRefusal> {
        let provider = connector_catalog::provider(connector_catalog::ProviderKey::id(connector))
            .ok_or(GrantTransactionRefusal::Unexpressible)?;
        let selected = grants
            .iter()
            .enumerate()
            .filter(|(_, grant)| grant.connector == connector)
            .collect::<Vec<_>>();
        if selected.len() > 1 {
            return Err(GrantTransactionRefusal::Unexpressible);
        }

        let (inbound, position) = match selected.first() {
            None => (Vec::new(), None),
            Some((position, grant)) => {
                if !grant.selector.allow_ids.is_empty() || !grant.selector.deny_ids.is_empty() {
                    return Err(GrantTransactionRefusal::Unexpressible);
                }
                (
                    project_inbound(provider, connector, &grant.inbound)?,
                    Some(*position),
                )
            }
        };
        Ok((
            GrantCandidate {
                connector: connector.to_owned(),
                inbound,
                selector,
            },
            position,
        ))
    }

    fn project_inbound(
        provider: &connector_catalog::Provider,
        connector: &str,
        inbound: &[InboundGrant],
    ) -> Result<Vec<GrantCandidateInbound>, GrantTransactionRefusal> {
        if inbound.len() > 64 {
            return Err(GrantTransactionRefusal::Unexpressible);
        }
        let mut bindings = BTreeSet::new();
        let mut projected = Vec::with_capacity(inbound.len());
        for entry in inbound {
            if entry.connector != connector
                || !bindings.insert(entry.binding.as_str())
                || entry.events.is_empty()
                || entry.events.len() > 256
            {
                return Err(GrantTransactionRefusal::Unexpressible);
            }
            let channel = provider
                .channel(&entry.binding)
                .ok_or(GrantTransactionRefusal::Unexpressible)?;
            if entry
                .events
                .iter()
                .any(|event| !channel.events.contains(&event.as_str()))
            {
                return Err(GrantTransactionRefusal::Unexpressible);
            }
            projected.push(GrantCandidateInbound {
                binding: entry.binding.clone(),
                events: entry.events.clone(),
            });
        }
        Ok(projected)
    }

    fn validate_candidate(candidate: &GrantCandidate) -> Result<(), GrantTransactionRefusal> {
        let provider =
            connector_catalog::provider(connector_catalog::ProviderKey::id(&candidate.connector))
                .ok_or(GrantTransactionRefusal::Unexpressible)?;
        let reconstructed = candidate
            .inbound
            .iter()
            .map(|entry| InboundGrant {
                connector: candidate.connector.clone(),
                binding: entry.binding.clone(),
                events: entry.events.clone(),
            })
            .collect::<Vec<_>>();
        let projected = project_inbound(provider, &candidate.connector, &reconstructed)?;
        if projected != candidate.inbound {
            return Err(GrantTransactionRefusal::Unexpressible);
        }
        if candidate
            .selector
            .effects_within
            .as_ref()
            .is_some_and(|effects| effects.len() > 3)
        {
            return Err(GrantTransactionRefusal::Unexpressible);
        }
        Ok(())
    }

    fn candidate_grant(candidate: &GrantCandidate) -> Grant {
        Grant {
            connector: candidate.connector.clone(),
            selector: Selector {
                max_risk: candidate.selector.max_risk,
                effects_within: candidate.selector.effects_within.clone(),
                idempotency: candidate.selector.idempotency,
                allow_ids: BTreeSet::new(),
                deny_ids: BTreeSet::new(),
            },
            inbound: candidate
                .inbound
                .iter()
                .map(|entry| InboundGrant {
                    connector: candidate.connector.clone(),
                    binding: entry.binding.clone(),
                    events: entry.events.clone(),
                })
                .collect(),
        }
    }

    fn transaction_refusal(refusal: GrantRefusal) -> GrantTransactionRefusal {
        GrantTransactionRefusal::Store {
            reason: refusal.to_string(),
        }
    }

    impl Grants for GrantStore {
        fn held(&self, tenant: &Tenant) -> Vec<Grant> {
            self.held
                .read()
                .ok()
                .and_then(|held| match &*held {
                    Loaded::Legacy(held) => held.get(tenant.as_str()).cloned(),
                    Loaded::Versioned(held) => held
                        .tenants
                        .get(tenant.as_str())
                        .map(|record| record.grants.clone()),
                })
                .unwrap_or_default()
        }

        fn set(&self, tenant: &Tenant, grants: &[Grant]) -> Result<(), GrantRefusal> {
            let mut held = self.held.write().map_err(|_| GrantRefusal::Unwritable {
                path: self.path.display().to_string(),
                reason: "the store lock is poisoned".to_owned(),
            })?;

            let (mut versioned, _) = initialize(&held, tenant);
            let record = versioned
                .tenants
                .get_mut(tenant.as_str())
                .expect("initialization creates the tenant");
            record.revision =
                record
                    .revision
                    .checked_next()
                    .ok_or_else(|| GrantRefusal::Unwritable {
                        path: self.path.display().to_string(),
                        reason: "the grant revision space is exhausted".to_owned(),
                    })?;
            record.grants = grants.to_vec();
            self.persist_versioned(&versioned)?;
            *held = Loaded::Versioned(versioned);

            Ok(())
        }
    }

    impl GrantTransactions for GrantStore {
        fn preview(
            &self,
            tenant: &Tenant,
            connector: &str,
            selector: GrantSelector,
        ) -> Result<GrantPreview, GrantTransactionRefusal> {
            let mut held = self
                .held
                .write()
                .map_err(|_| GrantTransactionRefusal::Store {
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let (versioned, initialized) = initialize(&held, tenant);
            if initialized {
                self.persist_versioned(&versioned)
                    .map_err(transaction_refusal)?;
                *held = Loaded::Versioned(versioned.clone());
            }
            let record = versioned
                .tenants
                .get(tenant.as_str())
                .expect("initialization creates the tenant");
            let (candidate, _) = project(&record.grants, connector, selector)?;
            let proposal_digest = candidate.proposal_digest(&record.revision)?;
            Ok(GrantPreview {
                candidate,
                proposal_digest,
                revision: record.revision,
            })
        }

        fn apply(
            &self,
            tenant: &Tenant,
            candidate: &GrantCandidate,
            revision: StoreRevision,
            proposal_digest: GrantProposalDigest,
            receipt_id: GrantReceiptId,
        ) -> Result<GrantApplyReceipt, GrantTransactionRefusal> {
            self.apply_observed(
                tenant,
                candidate,
                revision,
                proposal_digest,
                receipt_id,
                &mut (|_| true, |_| {}),
            )
        }

        fn apply_observed(
            &self,
            tenant: &Tenant,
            candidate: &GrantCandidate,
            revision: StoreRevision,
            proposal_digest: GrantProposalDigest,
            receipt_id: GrantReceiptId,
            observer: &mut dyn GrantDecisionObserver,
        ) -> Result<GrantApplyReceipt, GrantTransactionRefusal> {
            validate_candidate(candidate)?;
            if candidate.proposal_digest(&revision)? != proposal_digest {
                return Err(GrantTransactionRefusal::DigestMismatch);
            }

            let mut held = self
                .held
                .write()
                .map_err(|_| GrantTransactionRefusal::Store {
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let (mut versioned, _) = initialize(&held, tenant);
            let record = versioned
                .tenants
                .get_mut(tenant.as_str())
                .expect("initialization creates the tenant");

            if let Some(receipt) = record
                .receipts
                .iter()
                .find(|receipt| receipt.proposal_digest == proposal_digest)
            {
                observer.decided(receipt.receipt_id);
                return Ok(receipt.response(true));
            }
            if record
                .receipts
                .iter()
                .any(|receipt| receipt.receipt_id == receipt_id)
            {
                return Err(GrantTransactionRefusal::ReceiptConflict);
            }
            if record.revision != revision {
                return Err(GrantTransactionRefusal::Stale {
                    expected: revision,
                    current: record.revision,
                });
            }

            let (expected, position) = project(
                &record.grants,
                &candidate.connector,
                candidate.selector.clone(),
            )?;
            if expected != *candidate {
                return Err(GrantTransactionRefusal::Unexpressible);
            }
            let next = record
                .revision
                .checked_next()
                .ok_or(GrantTransactionRefusal::RevisionExhausted)?;
            let replacement = candidate_grant(candidate);
            match position {
                Some(position) => record.grants[position] = replacement,
                None => record.grants.push(replacement),
            }
            let receipt = ReceiptRecord {
                connector: candidate.connector.clone(),
                proposal_digest,
                receipt_id,
                revision: next,
            };
            record.revision = next;
            record.receipts.push(receipt.clone());

            if !observer.starting(receipt.receipt_id) {
                return Err(GrantTransactionRefusal::DecisionExpired);
            }
            self.persist_versioned(&versioned)
                .map_err(transaction_refusal)?;
            observer.decided(receipt.receipt_id);
            *held = Loaded::Versioned(versioned);
            Ok(receipt.response(false))
        }

        fn query(
            &self,
            tenant: &Tenant,
            receipt_id: GrantReceiptId,
        ) -> Result<Option<GrantApplyReceipt>, GrantTransactionRefusal> {
            let mut held = self
                .held
                .write()
                .map_err(|_| GrantTransactionRefusal::Store {
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let (versioned, initialized) = initialize(&held, tenant);
            if initialized {
                self.persist_versioned(&versioned)
                    .map_err(transaction_refusal)?;
                *held = Loaded::Versioned(versioned.clone());
            }
            Ok(versioned
                .tenants
                .get(tenant.as_str())
                .and_then(|record| {
                    record
                        .receipts
                        .iter()
                        .find(|receipt| receipt.receipt_id == receipt_id)
                })
                .map(|receipt| receipt.response(true)))
        }
    }

    /// Why a grant store could not be bound. Every variant refuses; none falls back.
    #[derive(Debug, thiserror::Error)]
    pub enum GrantStoreError {
        /// No store was named, and there is no default worth choosing on an operator's behalf.
        #[error(
            "no grant store is configured: set `{setting}` to a path outside every working tree, \
             for example `{EXAMPLE_PATH}`. This host does not fall back to an in-memory store — one \
             would lose every grant on restart, and a host that quietly forgets what a tenant may \
             run is one that refuses everything the next morning"
        )]
        Unconfigured {
            /// The setting that would have named one.
            setting: &'static str,
        },

        /// The configured path is inside a working tree.
        #[error(
            "refusing a grant store at `{path}`: it is inside the working tree at `{root}`, one \
             `git add -A` from a committed decision about what every tenant may run. Put it outside \
             a checkout, for example `{EXAMPLE_PATH}`"
        )]
        InsideWorkingTree {
            /// The store path, as resolved.
            path: String,
            /// The root of the working tree it falls under.
            root: String,
        },

        /// The path could not be made absolute — an unreadable current directory, most likely.
        #[error("refusing a grant store at `{path}`: it cannot be resolved: {reason}")]
        Unresolvable {
            /// The path as configured.
            path: String,
            /// What the filesystem said.
            reason: String,
        },

        /// The store exists and this host cannot read it.
        ///
        /// **Not** started empty. A store that could not be parsed and was replaced by an empty one
        /// would withdraw every tenant's authorisation silently, and the symptom — every operation
        /// refused — names the grant that is missing rather than the file that could not be read.
        #[error(
            "the grant store at `{path}` cannot be read: {reason}. Nothing here starts from an \
             empty store on a parse failure — that would withdraw every tenant's grants and report \
             it as a clean start"
        )]
        Unusable {
            /// The store path, as resolved.
            path: String,
            /// What the reader said.
            reason: String,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_grants_deserialize_with_no_inbound_authority() {
        let grant: Grant = serde_json::from_value(serde_json::json!({
            "connector": "asterisk",
            "selector": {
                "max_risk": null,
                "effects_within": null,
                "idempotency": null,
                "allow_ids": [],
                "deny_ids": []
            }
        }))
        .expect("old grant document");
        assert!(grant.inbound.is_empty());
    }

    #[test]
    fn inbound_grants_admit_only_their_declared_event_subset() {
        let principal = Principal::new(
            crate::PrincipalKind::ServiceAccount,
            "agent-1",
            Tenant::new("alpha").expect("tenant"),
        );
        let mut grant = Grant::for_connector("asterisk", Selector::default());
        grant.inbound.push(InboundGrant {
            connector: "asterisk".into(),
            binding: "ari-events".into(),
            events: [
                "channel-created".to_string(),
                "channel-destroyed".to_string(),
            ]
            .into_iter()
            .collect(),
        });
        let selected = ["channel-created".to_string()].into_iter().collect();
        assert!(admit_inbound(
            &principal,
            "asterisk",
            "ari-events",
            &selected,
            &[grant.clone()]
        )
        .is_ok());
        let undeclared = ["vendor-invented".to_string()].into_iter().collect();
        assert!(
            admit_inbound(&principal, "asterisk", "ari-events", &undeclared, &[grant]).is_err()
        );
    }

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

    // -----------------------------------------------------------------------------------------
    // The projection the gate decides on
    // -----------------------------------------------------------------------------------------

    /// Every operation in the catalogue projects, and into a level of each vocabulary that exists.
    ///
    /// Over the *whole* catalogue rather than one connector: **679 operations across 54 connectors**
    /// (re-measured on catalogue 0.13 in X-85) exercise every `risk` and every `idempotency` variant,
    /// so a mapping arm that is wrong for one value cannot hide behind a well-chosen example. The
    /// count is scale rather than contract — nothing below asserts it, and the walk is derived from
    /// `connector_catalog::operations()` so a catalogue that grows is covered by growing. The
    /// positive half — that every level really
    /// is present — is what stops this passing vacuously over a uniformly low-risk catalogue.
    #[test]
    fn every_catalogue_operation_projects_into_the_facts_a_selector_reads() {
        let projected: Vec<OperationFacts> = connector_catalog::operations()
            .map(OperationFacts::of)
            .collect();

        assert_eq!(
            projected.len(),
            connector_catalog::operations().count(),
            "every catalogue operation must project, and none twice",
        );

        for level in [Risk::Low, Risk::Medium, Risk::High, Risk::Destructive] {
            assert!(
                projected.iter().any(|facts| facts.risk == level),
                "no projected operation carries risk {level:?}, so the arm for it is unexercised",
            );
        }
        for spelling in [
            Idempotency::Idempotent,
            Idempotency::NotIdempotent,
            Idempotency::Conditional,
        ] {
            assert!(
                projected.iter().any(|facts| facts.idempotency == spelling),
                "no projected operation carries idempotency {spelling:?}",
            );
        }
    }

    /// The one spelling the two vocabularies disagree on, driven where it is decided.
    ///
    /// The catalogue says `non_idempotent` and this crate says `not_idempotent`, which is precisely
    /// why the mapping is a `match` and not a string passed through. Moved here from
    /// `routes::catalogue::view` in X-13, with the derivation it was testing.
    #[test]
    fn idempotency_is_spelled_this_crates_way() {
        let non_idempotent = connector_catalog::operations()
            .find(|operation| {
                matches!(
                    operation.idempotency,
                    connector_catalog::Idempotency::NonIdempotent
                )
            })
            .expect("the catalogue carries a non-idempotent operation");

        assert_eq!(
            OperationFacts::of(non_idempotent).idempotency,
            Idempotency::NotIdempotent,
        );
        assert_eq!(
            serde_json::to_value(Idempotency::NotIdempotent).expect("serialises"),
            serde_json::Value::String("not_idempotent".into()),
            "and the wire spelling is this crate's, because that is the token a `Selector` \
             round-trips through",
        );
    }

    /// Risk keeps the catalogue's own spelling, level for level.
    #[test]
    fn risk_keeps_the_catalogues_own_spelling() {
        for operation in connector_catalog::operations() {
            assert_eq!(
                serde_json::to_value(OperationFacts::of(operation).risk).expect("serialises"),
                serde_json::Value::String(operation.risk.as_str().to_owned()),
                "`{}` was projected into a different level than it declares",
                operation.id,
            );
        }
    }

    /// The derivation, stated as a rule rather than as today's answer.
    ///
    /// An operation reaches the network exactly when the catalogue gives it a host to reach.
    /// Nothing readable speaks to `workspace_write` or `process`, so neither may ever appear —
    /// claiming an effect the source data cannot support is worse than omitting it, because a
    /// `Selector` would then be decided against fiction.
    #[test]
    fn effects_are_derived_from_hosts_and_never_claim_more_than_that() {
        for operation in connector_catalog::operations() {
            let derived = OperationFacts::of(operation).effects;

            assert_eq!(
                derived.contains(&Effect::Network),
                !operation.hosts.is_empty(),
                "`{}` reaches {:?}; the derived effects were {derived:?}",
                operation.id,
                operation.hosts,
            );
            assert!(
                !derived.contains(&Effect::WorkspaceWrite) && !derived.contains(&Effect::Process),
                "`{}` claims an effect the catalogue cannot support: {derived:?}",
                operation.id,
            );
        }
    }

    // -----------------------------------------------------------------------------------------
    // The gate
    // -----------------------------------------------------------------------------------------

    fn caller() -> Principal {
        Principal::new(
            crate::PrincipalKind::ServiceAccount,
            "triage-bot",
            Tenant::new("acme").expect("`acme` is a usable tenant"),
        )
    }

    /// The runtime gate's proof, for a runtime every deployment serves.
    fn admitted() -> Admitted {
        crate::admit_runtime(
            crate::Deployment::MultiTenant,
            &crate::ConnectorSurface {
                connector: "github".into(),
                runtime: crate::Runtime::Http,
                operations: Vec::new(),
            },
        )
        .expect("http is admitted in every deployment")
    }

    /// A principal holding no grant runs nothing, and the refusal names both parties.
    ///
    /// Naming them is not decoration: an audit line that omits the principal cannot be read twice,
    /// and one that omits the operation cannot be acted on at all.
    #[test]
    fn no_grant_refuses_and_names_the_principal_and_the_operation() {
        let refusal = admit_grant(
            admitted(),
            &caller(),
            "github",
            &facts("github-repo-get", Risk::Low),
            &[],
        )
        .expect_err("a principal holding no grant runs nothing");

        let message = refusal.to_string();
        assert!(matches!(refusal, Error::NotGranted { .. }), "{message}");
        assert!(message.contains("triage-bot"), "{message}");
        assert!(message.contains("acme"), "{message}");
        assert!(message.contains("github-repo-get"), "{message}");
    }

    /// The gate decides from the metadata, and the proof it hands back carries what it decided.
    #[test]
    fn a_read_only_grant_admits_a_read_and_refuses_a_write() {
        let read_only = [Grant::for_connector("github", Selector::at_most(Risk::Low))];

        let granted = admit_grant(
            admitted(),
            &caller(),
            "github",
            &facts("github-repo-get", Risk::Low),
            &read_only,
        )
        .expect("a low-risk read is at or below `low`");
        assert_eq!(granted.operation(), "github-repo-get");
        assert_eq!(granted.runtime(), crate::Runtime::Http);

        assert!(
            admit_grant(
                admitted(),
                &caller(),
                "github",
                &facts("github-issue-create", Risk::High),
                &read_only,
            )
            .is_err(),
            "no operation is named anywhere in that grant, and the write must still be refused",
        );
    }

    /// A grant for one connector does not reach another, at the gate rather than only in the type.
    #[test]
    fn a_grant_for_one_connector_does_not_reach_another_at_the_gate() {
        let zendesk = [Grant::for_connector("zendesk", Selector::any())];

        assert!(
            admit_grant(
                admitted(),
                &caller(),
                "github",
                &facts("github-repo-get", Risk::Low),
                &zendesk,
            )
            .is_err(),
            "a grant that reached every connector would re-admit whatever the next connection added",
        );
    }

    /// An explicit `deny` survives an explicit `allow`, through the gate.
    #[test]
    fn deny_beats_allow_through_the_gate() {
        let grants = [Grant::for_connector(
            "github",
            Selector::any()
                .allow("github-repo-get")
                .deny("github-repo-get"),
        )];

        assert!(admit_grant(
            admitted(),
            &caller(),
            "github",
            &facts("github-repo-get", Risk::Low),
            &grants,
        )
        .is_err());
    }

    // -----------------------------------------------------------------------------------------
    // The store
    // -----------------------------------------------------------------------------------------

    /// Grants are stored per tenant, edited in place, and one tenant's do not answer for another's.
    ///
    /// Driven through a fresh file so the durability claim is the one being tested: the store is
    /// re-bound over the same path, which is what a restart is.
    #[cfg(unix)]
    #[test]
    fn grants_are_stored_edited_and_read_back_per_tenant() {
        let directory = std::env::temp_dir().join(format!(
            "flux-exchange-grants-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let path = directory.join("grants");
        let _ = std::fs::remove_dir_all(&directory);

        let acme = Tenant::new("acme").expect("a usable tenant");
        let globex = Tenant::new("globex").expect("a usable tenant");

        let store = GrantStore::bind(&path).expect("a fresh store");
        assert!(store.held(&acme).is_empty(), "a fresh store holds nothing");

        let read_only = Grant::for_connector("github", Selector::at_most(Risk::Low));
        store
            .set(&acme, std::slice::from_ref(&read_only))
            .expect("a store outside a working tree takes a write");

        assert_eq!(store.held(&acme), vec![read_only.clone()]);
        assert!(
            store.held(&globex).is_empty(),
            "one tenant's grants must not answer for another's",
        );

        // Edited: the set is replaced, not appended to.
        let wider = Grant::for_connector("github", Selector::at_most(Risk::High));
        store
            .set(&acme, std::slice::from_ref(&wider))
            .expect("a grant is editable");
        assert_eq!(store.held(&acme), vec![wider.clone()]);

        // And it is on the disk rather than in this process, which is what a restart asks.
        let reopened = GrantStore::bind(&path).expect("the store re-binds over its own file");
        assert_eq!(reopened.held(&acme), vec![wider]);
        assert!(reopened.held(&globex).is_empty());

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A store nobody configured is refused rather than replaced with one that forgets.
    #[cfg(unix)]
    #[test]
    fn an_unconfigured_grant_store_refuses_rather_than_falling_back() {
        for configured in [None, Some(""), Some("   ")] {
            let refusal = GrantStore::bind_configured(configured)
                .expect_err("an unnamed store is not a store");
            assert!(
                refusal.to_string().contains(crate::GRANT_STORE_SETTING),
                "the refusal must name the setting an operator has to set: {refusal}",
            );
        }
    }

    /// A grant store inside a checkout is refused before anything is created.
    #[cfg(unix)]
    #[test]
    fn a_grant_store_inside_a_working_tree_is_refused() {
        let inside =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("grants-should-not-be-here");

        assert!(
            GrantStore::bind(&inside).is_err(),
            "a decision about what every tenant may run must not be one `git add -A` away",
        );
        assert!(
            !inside.exists(),
            "a refused path must be one nothing was created at",
        );
    }
}
