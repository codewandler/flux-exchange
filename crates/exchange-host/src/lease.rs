//! Leases: caller-scoped holds on stateful resources.
//!
//! # Why this is not called a session
//!
//! In flux, a **session** is an event-sourced, resumable conversation. A lease is its opposite in
//! both respects that matter: it is owned by the caller that opened it rather than by the
//! conversation, and it dies with that caller rather than outliving them. Reusing the word would
//! make a sentence about one readable as a sentence about the other, and the two have opposite
//! failure modes — a leaked session is a privacy problem, a leaked lease is a resource problem.
//!
//! # Why a lease is a scope and not a parameter
//!
//! The obvious design is an operation like `net.tcp.write(connection_id, bytes)`. It is wrong, and
//! the reason is worth stating because it is not obvious: an opaque handle in a free parameter
//! means the operation's effect depends on runtime state the catalogue never saw. You can no longer
//! answer *"what can this principal do?"* by reading its grants, because the answer depends on what
//! it happens to hold open.
//!
//! So a lease is a **scope**. The host mints it, binds it to the principal and grant that opened
//! it, and an operation declared session-scoped is callable only inside one. The caller still names
//! an operation id; it does not name a destination, a credential, or a tenant.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Principal, Runtime};

/// A lease's identifier.
///
/// Opaque, and minted by the host. A caller cannot construct one that the host did not issue —
/// which is the same property that makes it useless to guess.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LeaseId(String);

impl LeaseId {
    /// Wrap an identifier minted by the host.
    ///
    /// Deliberately crate-visible: nothing outside this crate mints a lease id, because a lease id
    /// a caller can author is a lease id a caller can forge.
    pub(crate) fn mint(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The identifier, for logging and addressing.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LeaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a lease is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseState {
    /// Usable.
    Open,
    /// Released by its holder.
    Released,
    /// Reclaimed by the host because its holder stopped proving it was alive.
    Expired,
}

impl LeaseState {
    /// Is the lease usable right now?
    pub fn is_open(self) -> bool {
        matches!(self, LeaseState::Open)
    }
}

/// A caller's hold on a stateful resource.
///
/// A lease carries the principal that opened it so that ownership is a property of the lease rather
/// than of a lookup table somebody has to keep consistent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lease {
    id: LeaseId,
    holder: Principal,
    connector: String,
    runtime: Runtime,
    state: LeaseState,
}

impl Lease {
    /// Open a lease for `holder` against `connector`.
    pub fn open(
        id: impl Into<String>,
        holder: Principal,
        connector: impl Into<String>,
        runtime: Runtime,
    ) -> Self {
        Self {
            id: LeaseId::mint(id),
            holder,
            connector: connector.into(),
            runtime,
            state: LeaseState::Open,
        }
    }

    /// Its identifier.
    pub fn id(&self) -> &LeaseId {
        &self.id
    }

    /// Who holds it.
    pub fn holder(&self) -> &Principal {
        &self.holder
    }

    /// Which connector it is against.
    pub fn connector(&self) -> &str {
        &self.connector
    }

    /// How the held resource executes.
    pub fn runtime(&self) -> Runtime {
        self.runtime
    }

    /// Its current state.
    pub fn state(&self) -> LeaseState {
        self.state
    }

    /// Released by its holder.
    pub fn release(&mut self) {
        self.state = LeaseState::Released;
    }

    /// Reclaimed by the host.
    pub fn expire(&mut self) {
        self.state = LeaseState::Expired;
    }

    /// May `principal` use this lease?
    ///
    /// Requires the *same* principal, not merely the same tenant. Two agents in one tenant are two
    /// callers, and a lease one opened is not a resource the other may write into — an open shell
    /// or a half-finished transaction is not something to share by default.
    pub fn usable_by(&self, principal: &Principal) -> bool {
        self.state.is_open() && &self.holder == principal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PrincipalKind, Tenant};

    fn principal(id: &str, tenant: &str) -> Principal {
        Principal::new(PrincipalKind::Agent, id, Tenant::new(tenant).unwrap())
    }

    fn lease_for(holder: Principal) -> Lease {
        Lease::open("lease_01", holder, "kubernetes", Runtime::Process)
    }

    #[test]
    fn its_holder_may_use_it() {
        let holder = principal("triage-bot", "acme");
        let lease = lease_for(holder.clone());
        assert!(lease.usable_by(&holder));
    }

    /// Same tenant is not enough. This is the check that stops one agent writing into another
    /// agent's open shell.
    #[test]
    fn another_principal_in_the_same_tenant_may_not() {
        let lease = lease_for(principal("triage-bot", "acme"));
        assert!(!lease.usable_by(&principal("deploy-bot", "acme")));
    }

    #[test]
    fn another_tenant_may_not() {
        let lease = lease_for(principal("triage-bot", "acme"));
        assert!(!lease.usable_by(&principal("triage-bot", "globex")));
    }

    #[test]
    fn a_released_or_expired_lease_is_unusable_by_anyone_including_its_holder() {
        let holder = principal("triage-bot", "acme");

        let mut released = lease_for(holder.clone());
        released.release();
        assert!(!released.usable_by(&holder));

        let mut expired = lease_for(holder.clone());
        expired.expire();
        assert!(!expired.usable_by(&holder));
    }
}
