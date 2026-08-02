//! Process-local admission for work whose cost crosses the HTTP handler boundary.
//!
//! Identity and grants decide *who may do what*. This module answers a different question: how much
//! work one public process will accept at once. The deployed composition deliberately runs one
//! machine, so process-wide is also deployment-wide; no caller-controlled forwarding header is
//! promoted into a security boundary merely to manufacture per-address buckets.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// OIDC authorization starts admitted in one rolling minute.
const SIGN_INS_PER_MINUTE: usize = 30;

/// Operation invocations admitted in one rolling minute.
const INVOCATIONS_PER_MINUTE: usize = 120;

/// Operations allowed to execute at the same time.
const CONCURRENT_INVOCATIONS: usize = 16;

/// The process-wide traffic controls shared by every clone of application state.
#[derive(Clone)]
pub(crate) struct Traffic {
    sign_ins: Arc<Window>,
    invocations: Arc<Window>,
    active_invocations: Arc<Concurrent>,
}

impl Default for Traffic {
    fn default() -> Self {
        Self::new(
            SIGN_INS_PER_MINUTE,
            INVOCATIONS_PER_MINUTE,
            CONCURRENT_INVOCATIONS,
            Duration::from_secs(60),
        )
    }
}

impl Traffic {
    fn new(
        sign_ins: usize,
        invocations: usize,
        concurrent_invocations: usize,
        period: Duration,
    ) -> Self {
        Self {
            sign_ins: Arc::new(Window::new(sign_ins, period)),
            invocations: Arc::new(Window::new(invocations, period)),
            active_invocations: Arc::new(Concurrent::new(concurrent_invocations)),
        }
    }

    /// Admit one authorization start, or say when another may be attempted.
    pub(crate) fn admit_sign_in(&self) -> Result<(), TrafficRefusal> {
        self.sign_ins.admit(Instant::now())
    }

    /// Admit one invocation by both rate and concurrent occupancy.
    ///
    /// The returned claim is the concurrency bound: dropping it at every handler return releases
    /// the slot, including errors and cancellation. No queue is created when every slot is held.
    pub(crate) fn begin_invocation(&self) -> Result<InvocationClaim, TrafficRefusal> {
        let claim = self.active_invocations.clone().try_claim()?;
        self.invocations.admit(Instant::now())?;
        Ok(claim)
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        sign_ins: usize,
        invocations: usize,
        concurrent_invocations: usize,
        period: Duration,
    ) -> Self {
        Self::new(sign_ins, invocations, concurrent_invocations, period)
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
        let first = traffic.begin_invocation().expect("the first slot");

        assert_eq!(
            traffic.begin_invocation().err(),
            Some(TrafficRefusal { retry_after: 1 }),
            "a saturated process refuses immediately"
        );

        drop(first);
        assert!(traffic.begin_invocation().is_ok(), "drop releases the slot");
    }
}
