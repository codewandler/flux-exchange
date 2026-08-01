//! One change to one connection at a time.
//!
//! # The window this closes
//!
//! Creating a connection is a **read-decide-write**: probe the store for a value at the derived
//! address, refuse with `409` if one is there, otherwise write. With nothing between the halves,
//! two concurrent `POST /api/connections/zendesk` from one tenant both probe an empty address, both
//! write, and both answer `201`. One value is gone and **the caller that lost was told it
//! succeeded** — which is the exact failure the `409` exists to prevent, reintroduced by the
//! mechanism meant to prevent it. A double-clicked button in the console is enough.
//!
//! The port cannot close this: `SecretStore` is `get`/`put`/`delete` with no compare-and-swap, and
//! adding one is not this repository's to do. What is available is that the whole decision happens
//! in one process, so the guard is an in-process claim on the thing being changed.
//!
//! # What it covers, and what it does not
//!
//! **It covers one process.** `FileStore` is a single in-process map written through to disk on
//! every mutation, so within this process a claim is sufficient. **Two replicas sharing one store
//! would race again**, and nothing here would notice — the same limit `docs/designs/
//! identity-and-session.md` already records for sessions, and for the same reason: a shared store
//! is a real design question that should be answered when there is a deployment asking it. It is
//! written down here so it is a known limit rather than a discovered one.
//!
//! # Refusing rather than waiting
//!
//! A caller that cannot take the claim is **refused**, not queued. Waiting would need a lock held
//! across an `await`, and the shape it produces is worse than the refusal: a queued second `POST`
//! wakes up, finds the first caller's value, and answers `409` anyway. The two racing creates are a
//! tenant trying to have two connections to one connector, which is precisely what this surface
//! refuses; answering that immediately and saying so is more honest than making it wait to be told
//! the same thing. It also keeps this module free of a `tokio` feature the manifest does not carry.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use exchange_host::Tenant;

/// What a claim is taken on: one tenant's connection to one connector.
///
/// Per `(tenant, connector)` rather than one lock over the whole surface, because a global one
/// would make one tenant's connection writes wait on another's — shared fate between tenants, in
/// the one repository whose entire point is that they do not share anything.
type Key = (String, String);

/// The connection changes currently in flight.
#[derive(Debug, Default)]
pub struct ConnectionGuard {
    claimed: Mutex<HashSet<Key>>,
}

impl ConnectionGuard {
    /// Claim `(tenant, connector)` for the duration of one mutation, or report that something else
    /// already holds it.
    ///
    /// `None` is not an error condition to retry internally; it is an answer the caller turns into
    /// a refusal. See the module documentation for why refusing beats waiting.
    pub fn claim(self: &Arc<Self>, tenant: &Tenant, connector: &str) -> Option<Claim> {
        let key = (tenant.as_str().to_string(), connector.to_string());

        // `insert` answers whether the key was new, so taking the claim and finding out whether it
        // was free are the same operation. A `contains` followed by an `insert` would be the very
        // check-then-act this type exists to remove.
        if !self.claimed().insert(key.clone()) {
            return None;
        }

        Some(Claim {
            guard: self.clone(),
            key,
        })
    }

    /// The claims in flight.
    ///
    /// Recovers from a poisoned lock rather than propagating it, on the same argument as
    /// [`SessionStore`](crate::session::SessionStore): the guarded value is a plain set with no
    /// cross-key invariant, so a panic while holding the lock cannot have left it half-updated, and
    /// refusing every later request because an unrelated handler panicked would turn one failure
    /// into an outage.
    ///
    /// A panicking handler does still release its claim — [`Claim`]'s `Drop` runs while the stack
    /// unwinds — so a poisoned lock here never means a claim stuck forever.
    fn claimed(&self) -> MutexGuard<'_, HashSet<Key>> {
        self.claimed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A held claim on one tenant's connection to one connector.
///
/// Released on drop, including while a panic unwinds, so there is no path that leaves a connection
/// permanently unchangeable. Deliberately carries no method to release early: an explicit `release`
/// is a thing a future edit can return past.
#[derive(Debug)]
pub struct Claim {
    guard: Arc<ConnectionGuard>,
    key: Key,
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.guard.claimed().remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(name: &str) -> Tenant {
        Tenant::new(name).expect("a plain tenant id")
    }

    /// The whole point: a second claim on the same connection is refused while the first is held,
    /// and available again once it is released.
    #[test]
    fn one_claim_at_a_time_per_connection() {
        let guard = Arc::new(ConnectionGuard::default());

        let first = guard
            .claim(&tenant("acme"), "zendesk")
            .expect("nothing holds it yet");
        assert!(
            guard.claim(&tenant("acme"), "zendesk").is_none(),
            "a second change to one connection must not proceed while the first is in flight",
        );

        drop(first);
        assert!(
            guard.claim(&tenant("acme"), "zendesk").is_some(),
            "the claim must be available again once the first change finished",
        );
    }

    /// Claims are per connection, not per surface. A global lock would make one tenant's writes
    /// wait on another's, which is shared fate between tenants.
    #[test]
    fn claims_do_not_reach_across_tenants_or_connectors() {
        let guard = Arc::new(ConnectionGuard::default());
        let _acme_zendesk = guard.claim(&tenant("acme"), "zendesk").expect("free");

        assert!(
            guard.claim(&tenant("globex"), "zendesk").is_some(),
            "another tenant's connection to the same connector is a different claim",
        );
        assert!(
            guard.claim(&tenant("acme"), "slack").is_some(),
            "the same tenant's connection to another connector is a different claim",
        );
    }

    /// A handler that panics still releases its claim, so one failure cannot make a connection
    /// permanently unchangeable.
    #[test]
    fn a_panic_releases_the_claim() {
        let guard = Arc::new(ConnectionGuard::default());

        let panicked = std::panic::catch_unwind({
            let guard = guard.clone();
            move || {
                let _claim = guard.claim(&tenant("acme"), "zendesk").expect("free");
                panic!("a handler failed while holding a claim");
            }
        });
        assert!(panicked.is_err(), "the closure must have panicked");

        assert!(
            guard.claim(&tenant("acme"), "zendesk").is_some(),
            "an unwinding handler must not leave its claim held",
        );
    }

    /// Nothing accumulates: the set is empty again once every claim is released, so a long-running
    /// process does not grow one entry per connection it has ever touched.
    #[test]
    fn nothing_is_retained_once_a_claim_is_released() {
        let guard = Arc::new(ConnectionGuard::default());

        for connector in ["zendesk", "slack", "github"] {
            let _claim = guard.claim(&tenant("acme"), connector).expect("free");
        }

        assert!(guard.claimed().is_empty());
    }
}
