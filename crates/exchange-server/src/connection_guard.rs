//! One change to one connection at a time, and one change to one tenant's allowance at a time.
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
//! # Two scopes, because there are two decisions
//!
//! A claim is taken on the **thing the decision is about**, and this surface makes two decisions of
//! different width:
//!
//! - *Is this connector already connected?* is about one address, so [`ConnectionGuard::claim`]
//!   claims `(tenant, connector)`. Two tenants never contend, and one tenant's `zendesk` and
//!   `slack` never contend — that is the granularity X-10 chose, and
//!   [`tests::claims_do_not_reach_across_tenants_or_connectors`] still pins it.
//! - *Has this tenant room left in its allowance?* is about **every** connector at once: what a
//!   tenant occupies is the sum over all of them, so a read of it is only true while no other
//!   connector of that tenant is being written. [`ConnectionGuard::claim_tenant`] claims the
//!   tenant, and one tenant's concurrent creates to *different* connectors therefore do serialise.
//!
//! **That second scope is contention the first deliberately avoids**, and it is here because
//! avoiding it made `MAX_TENANT_STORE_BYTES` untrue: before X-25 each of one tenant's concurrent
//! creates read an occupancy the others had not written yet, all were admitted, and the tenant
//! ended up past the allowance that exists to bound its share of the one file every other tenant's
//! write has to rewrite. The contention is bounded to what the allowance is decided from — **one
//! tenant**, and only on `POST`. Two tenants still never contend, which is the property that
//! mattered; `DELETE` takes no tenant claim, because destroying credentials only frees allowance
//! and an operator revoking a leaked secret must never be made to wait on an unrelated create.
//!
//! A caller may hold both at once, and there is no lock ordering to get wrong: a claim is never
//! waited on, only taken or refused, so two claims cannot deadlock.
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

/// What a claim is taken on — one of the two widths the module documentation argues for.
///
/// One set holds both, rather than two sets holding one each, because they are the same kind of
/// thing: a name for what is in flight, taken and released the same way. The variants cannot
/// collide, so a tenant called `zendesk` claims nothing a connector called `zendesk` does.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// One tenant's connection to one connector.
    ///
    /// Per `(tenant, connector)` rather than one lock over the whole surface, because a global one
    /// would make one tenant's connection writes wait on another's — shared fate between tenants,
    /// in the one repository whose entire point is that they do not share anything.
    Connection(String, String),
    /// One tenant's connection changes as a whole, across every connector.
    ///
    /// Wider on purpose, and only as wide as the decision that needs it: what a tenant may occupy
    /// is a sum over all of its connectors, so nothing narrower than the tenant makes a read of it
    /// still true when it is written. It stops at the tenant, so two tenants never contend.
    Tenant(String),
}

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
        self.take(Key::Connection(
            tenant.as_str().to_string(),
            connector.to_string(),
        ))
    }

    /// Claim **all** of one tenant's connection changes, for a decision that is about the tenant
    /// rather than about one connector.
    ///
    /// That is the allowance: `exchange_host::MAX_TENANT_STORE_BYTES` is read as a sum over every
    /// connector, so a create deciding against it has to exclude the tenant's other creates and not
    /// only its own connector's. A caller holding this and [`claim`](Self::claim) holds two claims;
    /// neither waits, so neither can deadlock against the other.
    ///
    /// `None` is answered exactly as [`claim`](Self::claim)'s is: a refusal, not a retry loop.
    pub fn claim_tenant(self: &Arc<Self>, tenant: &Tenant) -> Option<Claim> {
        self.take(Key::Tenant(tenant.as_str().to_string()))
    }

    /// Take a claim on `key`, or report that something else already holds it.
    fn take(self: &Arc<Self>, key: Key) -> Option<Claim> {
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

/// A held claim on one tenant's connection to one connector, or on that tenant's connections as a
/// whole.
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

    /// A **connection** claim is per connection, not per surface. A global lock would make one
    /// tenant's writes wait on another's, which is shared fate between tenants.
    ///
    /// Still true after X-25, and deliberately so: what that story serialises across one tenant's
    /// connectors is the *allowance* decision, which takes the wider claim below. This one keeps
    /// its meaning — it is what makes a `POST` and a `DELETE` to one connection exclude each other
    /// — and it keeps `DELETE` out of the wider scope entirely. Read this next to
    /// [`a_tenant_claim_covers_that_tenants_connectors_and_stops_there`], which states the width
    /// that did change and why.
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

    /// **The width X-25 added, and the width it stopped at.** A tenant claim excludes that same
    /// tenant's other connection changes — which is the whole point, since the allowance is decided
    /// as a sum over every connector — and excludes nothing of any other tenant's.
    ///
    /// It is a different claim from the per-connection one, so holding one says nothing about the
    /// other: `create` holds both, and if taking the tenant claim also took the connection's, a
    /// `POST` would be refusing itself.
    #[test]
    fn a_tenant_claim_covers_that_tenants_connectors_and_stops_there() {
        let guard = Arc::new(ConnectionGuard::default());
        let acme = guard.claim_tenant(&tenant("acme")).expect("free");

        assert!(
            guard.claim_tenant(&tenant("acme")).is_none(),
            "one tenant's allowance is decided one change at a time, or two concurrent creates \
             read an occupancy neither has written yet",
        );
        assert!(
            guard.claim_tenant(&tenant("globex")).is_some(),
            "another tenant's changes must not wait on this one: the serialisation is per tenant \
             and is not a lock over the surface",
        );
        assert!(
            guard.claim(&tenant("acme"), "zendesk").is_some(),
            "the two claims are different claims, and one request holds both",
        );

        drop(acme);
        assert!(
            guard.claim_tenant(&tenant("acme")).is_some(),
            "the claim must be available again once the change finished",
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
        for name in ["acme", "globex"] {
            let _claim = guard.claim_tenant(&tenant(name)).expect("free");
        }

        assert!(guard.claimed().is_empty());
    }
}
