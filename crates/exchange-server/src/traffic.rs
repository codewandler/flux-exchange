//! Process-local admission for work whose cost crosses the HTTP handler boundary.
//!
//! Identity and grants decide *who may do what*. This module answers a different question: how much
//! work one public process will accept at once. The deployed composition deliberately runs one
//! machine, so process-wide is also deployment-wide; no caller-controlled forwarding header is
//! promoted into a security boundary merely to manufacture per-address buckets.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use exchange_host::Principal;
use tracing::warn;

/// OIDC authorization starts admitted in one rolling minute.
const SIGN_INS_PER_MINUTE: usize = 30;

/// Operation invocations admitted in one rolling minute.
const INVOCATIONS_PER_MINUTE: usize = 120;

/// Operation invocations one resolved principal may spend in one rolling minute.
const INVOCATIONS_PER_PRINCIPAL_PER_MINUTE: usize = 30;

/// Distinct active principal windows retained by one process.
const PRINCIPAL_WINDOWS: usize = 1_024;

/// Operations allowed to execute at the same time.
const CONCURRENT_INVOCATIONS: usize = 16;

/// Hosted local-management WebSockets one process admits concurrently.
const CONCURRENT_LOCAL_MANAGEMENT: usize = 32;

/// Hosted local-management WebSockets one resolved tenant admits concurrently.
const CONCURRENT_LOCAL_MANAGEMENT_PER_TENANT: usize = 4;

/// The process-wide traffic controls shared by every clone of application state.
#[derive(Clone)]
pub(crate) struct Traffic {
    sign_ins: Arc<Window>,
    invocations: Arc<Window>,
    principals: Arc<PrincipalWindows>,
    active_invocations: Arc<Concurrent>,
    local_management: Arc<HostedConcurrent>,
    metrics: Arc<TrafficMetrics>,
}

impl Default for Traffic {
    fn default() -> Self {
        Self::new(
            SIGN_INS_PER_MINUTE,
            INVOCATIONS_PER_MINUTE,
            INVOCATIONS_PER_PRINCIPAL_PER_MINUTE,
            CONCURRENT_INVOCATIONS,
            PRINCIPAL_WINDOWS,
            Duration::from_secs(60),
        )
    }
}

impl Traffic {
    fn new(
        sign_ins: usize,
        invocations: usize,
        principal_invocations: usize,
        concurrent_invocations: usize,
        principal_windows: usize,
        period: Duration,
    ) -> Self {
        Self {
            sign_ins: Arc::new(Window::new(sign_ins, period)),
            invocations: Arc::new(Window::new(invocations, period)),
            principals: Arc::new(PrincipalWindows::new(
                principal_invocations,
                principal_windows,
                period,
            )),
            active_invocations: Arc::new(Concurrent::new(concurrent_invocations)),
            local_management: Arc::new(HostedConcurrent::new(
                CONCURRENT_LOCAL_MANAGEMENT,
                CONCURRENT_LOCAL_MANAGEMENT_PER_TENANT,
            )),
            metrics: Arc::new(TrafficMetrics::default()),
        }
    }

    /// Admit one authorization start, or say when another may be attempted.
    pub(crate) fn admit_sign_in(&self) -> Result<(), TrafficRefusal> {
        match self.sign_ins.admit(Instant::now()) {
            Ok(()) => {
                self.metrics
                    .sign_ins_admitted
                    .fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(refusal) => {
                self.metrics
                    .refuse(&self.metrics.sign_ins_refused, "anonymous");
                Err(refusal)
            }
        }
    }

    /// Admit one invocation by both rate and concurrent occupancy.
    ///
    /// The returned claim is the concurrency bound: dropping it at every handler return releases
    /// the slot, including errors and cancellation. No queue is created when every slot is held.
    pub(crate) fn begin_invocation(
        &self,
        principal: &Principal,
    ) -> Result<InvocationClaim, TrafficRefusal> {
        let claim = match self.active_invocations.clone().try_claim() {
            Ok(claim) => claim,
            Err(refusal) => {
                self.metrics
                    .refuse(&self.metrics.invocations_refused_concurrency, "concurrency");
                return Err(refusal);
            }
        };
        let now = Instant::now();
        let key = PrincipalKey::from(principal);
        if let Err(refusal) = self.principals.admit(&key, now) {
            self.metrics
                .refuse(&self.metrics.invocations_refused_principal, "principal");
            return Err(refusal);
        }
        if let Err(refusal) = self.invocations.admit(now) {
            self.principals.refund(&key, now);
            self.metrics
                .refuse(&self.metrics.invocations_refused_global, "global");
            return Err(refusal);
        }
        self.metrics
            .invocations_admitted
            .fetch_add(1, Ordering::Relaxed);
        Ok(claim)
    }

    /// Claim one hosted management slot, bounded process-wide and by resolved tenant.
    pub(crate) fn begin_local_management(
        &self,
        principal: &Principal,
    ) -> Result<HostedClaim, TrafficRefusal> {
        self.local_management
            .clone()
            .try_claim(principal.tenant().as_str())
    }

    /// A fixed-cardinality snapshot suitable for an operational metrics endpoint.
    pub(crate) fn snapshot(&self) -> TrafficSnapshot {
        TrafficSnapshot {
            sign_ins_admitted: self.metrics.sign_ins_admitted.load(Ordering::Relaxed),
            sign_ins_refused: self.metrics.sign_ins_refused.load(Ordering::Relaxed),
            invocations_admitted: self.metrics.invocations_admitted.load(Ordering::Relaxed),
            invocations_refused_principal: self
                .metrics
                .invocations_refused_principal
                .load(Ordering::Relaxed),
            invocations_refused_global: self
                .metrics
                .invocations_refused_global
                .load(Ordering::Relaxed),
            invocations_refused_concurrency: self
                .metrics
                .invocations_refused_concurrency
                .load(Ordering::Relaxed),
            active_invocations: self.active_invocations.active.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        sign_ins: usize,
        invocations: usize,
        concurrent_invocations: usize,
        period: Duration,
    ) -> Self {
        Self::new(
            sign_ins,
            invocations,
            invocations,
            concurrent_invocations,
            PRINCIPAL_WINDOWS,
            period,
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_principal(
        sign_ins: usize,
        invocations: usize,
        principal_invocations: usize,
        concurrent_invocations: usize,
        period: Duration,
    ) -> Self {
        Self::new(
            sign_ins,
            invocations,
            principal_invocations,
            concurrent_invocations,
            PRINCIPAL_WINDOWS,
            period,
        )
    }
}

struct HostedOccupancy {
    total: usize,
    tenants: HashMap<String, usize>,
}

struct HostedConcurrent {
    process_limit: usize,
    tenant_limit: usize,
    occupancy: Mutex<HostedOccupancy>,
}

impl HostedConcurrent {
    fn new(process_limit: usize, tenant_limit: usize) -> Self {
        Self {
            process_limit,
            tenant_limit,
            occupancy: Mutex::new(HostedOccupancy {
                total: 0,
                tenants: HashMap::new(),
            }),
        }
    }

    fn try_claim(self: Arc<Self>, tenant: &str) -> Result<HostedClaim, TrafficRefusal> {
        let mut occupancy = self
            .occupancy
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tenant_active = occupancy.tenants.get(tenant).copied().unwrap_or(0);
        if occupancy.total >= self.process_limit || tenant_active >= self.tenant_limit {
            return Err(TrafficRefusal::after(Duration::from_secs(5)));
        }
        occupancy.total += 1;
        occupancy
            .tenants
            .insert(tenant.to_owned(), tenant_active + 1);
        Ok(HostedClaim {
            concurrent: self.clone(),
            tenant: tenant.to_owned(),
        })
    }
}

/// One live hosted local-management WebSocket slot.
pub(crate) struct HostedClaim {
    concurrent: Arc<HostedConcurrent>,
    tenant: String,
}

impl Drop for HostedClaim {
    fn drop(&mut self) {
        let mut occupancy = self
            .concurrent
            .occupancy
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        occupancy.total = occupancy.total.saturating_sub(1);
        let Some(active) = occupancy.tenants.get_mut(&self.tenant) else {
            return;
        };
        *active = active.saturating_sub(1);
        if *active == 0 {
            occupancy.tenants.remove(&self.tenant);
        }
    }
}

/// Fixed-cardinality traffic measurements. No field can acquire a caller-controlled label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrafficSnapshot {
    pub(crate) sign_ins_admitted: u64,
    pub(crate) sign_ins_refused: u64,
    pub(crate) invocations_admitted: u64,
    pub(crate) invocations_refused_principal: u64,
    pub(crate) invocations_refused_global: u64,
    pub(crate) invocations_refused_concurrency: u64,
    pub(crate) active_invocations: usize,
}

#[derive(Default)]
struct TrafficMetrics {
    sign_ins_admitted: AtomicU64,
    sign_ins_refused: AtomicU64,
    invocations_admitted: AtomicU64,
    invocations_refused_principal: AtomicU64,
    invocations_refused_global: AtomicU64,
    invocations_refused_concurrency: AtomicU64,
}

impl TrafficMetrics {
    fn refuse(&self, counter: &AtomicU64, limit: &'static str) {
        let refusals = counter.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if refusals.is_multiple_of(20) {
            // Fixed labels only: saturation must be actionable without making an identity, token,
            // request body or attacker-chosen value part of the log stream.
            warn!(limit, refusals, "sustained traffic saturation");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PrincipalKey {
    tenant: String,
    kind: exchange_host::PrincipalKind,
    id: String,
}

impl From<&Principal> for PrincipalKey {
    fn from(principal: &Principal) -> Self {
        Self {
            tenant: principal.tenant().as_str().to_owned(),
            kind: principal.kind(),
            id: principal.id().to_owned(),
        }
    }
}

/// Per-principal rolling windows, with a deployment-wide bound on retained keys.
struct PrincipalWindows {
    capacity: usize,
    maximum_keys: usize,
    period: Duration,
    admitted: Mutex<HashMap<PrincipalKey, VecDeque<Instant>>>,
}

impl PrincipalWindows {
    fn new(capacity: usize, maximum_keys: usize, period: Duration) -> Self {
        assert!(
            capacity > 0,
            "a principal traffic limit must admit something"
        );
        assert!(
            maximum_keys > 0,
            "principal traffic state must retain a key"
        );
        assert!(
            !period.is_zero(),
            "a principal traffic limit needs a real period"
        );
        Self {
            capacity,
            maximum_keys,
            period,
            admitted: Mutex::new(HashMap::with_capacity(maximum_keys)),
        }
    }

    fn admit(&self, key: &PrincipalKey, now: Instant) -> Result<(), TrafficRefusal> {
        let mut admitted = self
            .admitted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        admitted.retain(|_, window| {
            prune(window, now, self.period);
            !window.is_empty()
        });

        if !admitted.contains_key(key) && admitted.len() >= self.maximum_keys {
            return Err(TrafficRefusal::after(Duration::from_secs(1)));
        }
        let window = admitted
            .entry(key.clone())
            .or_insert_with(|| VecDeque::with_capacity(self.capacity));
        prune(window, now, self.period);
        if window.len() >= self.capacity {
            let oldest = window
                .front()
                .copied()
                .expect("a full positive-capacity principal window has an oldest entry");
            let opens_at = oldest.checked_add(self.period).unwrap_or(oldest);
            return Err(TrafficRefusal::after(
                opens_at.checked_duration_since(now).unwrap_or_default(),
            ));
        }
        window.push_back(now);
        Ok(())
    }

    fn refund(&self, key: &PrincipalKey, admitted_at: Instant) {
        let mut admitted = self
            .admitted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(window) = admitted.get_mut(key) {
            if window.back() == Some(&admitted_at) {
                window.pop_back();
            }
            if window.is_empty() {
                admitted.remove(key);
            }
        }
    }
}

fn prune(window: &mut VecDeque<Instant>, now: Instant, period: Duration) {
    while window.front().is_some_and(|at| {
        now.checked_duration_since(*at)
            .is_some_and(|age| age >= period)
    }) {
        window.pop_front();
    }
}

/// A refusal carrying only the standard delay a caller can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrafficRefusal {
    retry_after: u64,
}

impl TrafficRefusal {
    /// Whole seconds suitable for the HTTP `Retry-After` header.
    pub(crate) fn retry_after(self) -> u64 {
        self.retry_after
    }

    fn after(duration: Duration) -> Self {
        let rounded_up = duration
            .as_secs()
            .saturating_add(u64::from(duration.subsec_nanos() != 0));
        Self {
            retry_after: rounded_up.max(1),
        }
    }
}

/// A rolling-window counter whose own memory is bounded by `capacity`.
struct Window {
    capacity: usize,
    period: Duration,
    admitted: Mutex<VecDeque<Instant>>,
}

impl Window {
    fn new(capacity: usize, period: Duration) -> Self {
        assert!(capacity > 0, "a traffic limit must admit something");
        assert!(!period.is_zero(), "a traffic limit needs a real period");
        Self {
            capacity,
            period,
            admitted: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    fn admit(&self, now: Instant) -> Result<(), TrafficRefusal> {
        let mut admitted = self.admitted();
        while admitted.front().is_some_and(|at| {
            now.checked_duration_since(*at)
                .is_some_and(|age| age >= self.period)
        }) {
            admitted.pop_front();
        }

        if admitted.len() >= self.capacity {
            let oldest = admitted
                .front()
                .copied()
                .expect("a full positive-capacity window has an oldest entry");
            let opens_at = oldest.checked_add(self.period).unwrap_or(oldest);
            return Err(TrafficRefusal::after(
                opens_at.checked_duration_since(now).unwrap_or_default(),
            ));
        }

        admitted.push_back(now);
        Ok(())
    }

    fn admitted(&self) -> MutexGuard<'_, VecDeque<Instant>> {
        self.admitted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The non-queuing concurrent invocation bound.
struct Concurrent {
    maximum: usize,
    active: AtomicUsize,
}

impl Concurrent {
    fn new(maximum: usize) -> Self {
        assert!(maximum > 0, "a concurrency limit must admit something");
        Self {
            maximum,
            active: AtomicUsize::new(0),
        }
    }

    fn try_claim(self: Arc<Self>) -> Result<InvocationClaim, TrafficRefusal> {
        let mut active = self.active.load(Ordering::Relaxed);
        loop {
            if active >= self.maximum {
                return Err(TrafficRefusal::after(Duration::from_secs(1)));
            }

            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(InvocationClaim { concurrent: self }),
                Err(observed) => active = observed,
            }
        }
    }
}

/// One in-flight invocation. Dropping it releases exactly one slot.
pub(crate) struct InvocationClaim {
    concurrent: Arc<Concurrent>,
}

impl Drop for InvocationClaim {
    fn drop(&mut self) {
        self.concurrent.active.fetch_sub(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(id: &str) -> Principal {
        principal_in(id, "acme")
    }

    fn principal_in(id: &str, tenant: &str) -> Principal {
        Principal::new(
            exchange_host::PrincipalKind::User,
            id,
            exchange_host::Tenant::new(tenant).expect("tenant"),
        )
    }

    #[test]
    fn a_rolling_window_refuses_at_its_bound_and_reopens_after_its_period() {
        let window = Window::new(2, Duration::from_secs(60));
        let start = Instant::now();

        assert!(window.admit(start).is_ok());
        assert!(window.admit(start + Duration::from_secs(1)).is_ok());
        assert_eq!(
            window.admit(start + Duration::from_secs(2)),
            Err(TrafficRefusal { retry_after: 58 })
        );
        assert!(window.admit(start + Duration::from_secs(60)).is_ok());
    }

    #[test]
    fn invocation_saturation_refuses_without_a_queue_and_drop_releases_the_slot() {
        let traffic = Traffic::for_test(1, 10, 1, Duration::from_secs(60));
        let alice = principal("alice");
        let first = traffic.begin_invocation(&alice).expect("the first slot");

        assert_eq!(
            traffic.begin_invocation(&alice).err(),
            Some(TrafficRefusal { retry_after: 1 }),
            "a saturated process refuses immediately"
        );

        drop(first);
        assert!(
            traffic.begin_invocation(&alice).is_ok(),
            "drop releases the slot"
        );
    }

    #[test]
    fn one_principal_cannot_spend_anothers_budget() {
        let traffic = Traffic::for_test_with_principal(1, 10, 2, 10, Duration::from_secs(60));
        let alice = principal("alice");
        let bob = principal("bob");

        drop(traffic.begin_invocation(&alice).expect("alice one"));
        drop(traffic.begin_invocation(&alice).expect("alice two"));
        assert!(traffic.begin_invocation(&alice).is_err(), "alice is capped");
        assert!(
            traffic.begin_invocation(&bob).is_ok(),
            "bob retains an independent resolved-principal budget"
        );
    }

    #[test]
    fn the_global_ceiling_still_holds_across_principals() {
        let traffic = Traffic::for_test_with_principal(1, 2, 2, 10, Duration::from_secs(60));
        drop(
            traffic
                .begin_invocation(&principal("alice"))
                .expect("alice"),
        );
        drop(traffic.begin_invocation(&principal("bob")).expect("bob"));
        assert!(
            traffic.begin_invocation(&principal("carol")).is_err(),
            "distinct principal keys do not bypass the process ceiling"
        );
    }

    #[test]
    fn hosted_management_has_exact_tenant_and_process_occupancy_without_a_queue() {
        let traffic = Traffic::default();
        let alice = principal("alice");
        let mut tenant_claims = Vec::new();
        for _ in 0..CONCURRENT_LOCAL_MANAGEMENT_PER_TENANT {
            tenant_claims.push(
                traffic
                    .begin_local_management(&alice)
                    .expect("one of four tenant slots"),
            );
        }
        assert_eq!(
            traffic.begin_local_management(&alice).err(),
            Some(TrafficRefusal { retry_after: 5 })
        );
        drop(tenant_claims.pop());
        tenant_claims.push(
            traffic
                .begin_local_management(&alice)
                .expect("drop immediately reopens the tenant slot"),
        );
        drop(tenant_claims);

        let mut process_claims = Vec::new();
        for tenant_number in 0..8 {
            let tenant = format!("tenant-{tenant_number}");
            let principal = principal_in("operator", &tenant);
            for _ in 0..CONCURRENT_LOCAL_MANAGEMENT_PER_TENANT {
                process_claims.push(
                    traffic
                        .begin_local_management(&principal)
                        .expect("one of exactly 32 process slots"),
                );
            }
        }
        assert_eq!(process_claims.len(), CONCURRENT_LOCAL_MANAGEMENT);
        assert_eq!(
            traffic
                .begin_local_management(&principal_in("operator", "tenant-9"))
                .err(),
            Some(TrafficRefusal { retry_after: 5 })
        );
    }
}
