//! One non-resetting local-management deadline controller shared by every transport.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

const PRE_DECISION_BUDGET: Duration = Duration::from_secs(300);
const POST_DECISION_BUDGET: Duration = Duration::from_secs(30);

trait MonotonicClock: Send + Sync {
    fn now(&self) -> Instant;
}

struct TokioClock;

impl MonotonicClock for TokioClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Opaque nonzero receipt identity retained only after a durable decision exists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReceiptIdentity([u8; 32]);

impl ReceiptIdentity {
    pub(crate) fn from_protocol_bytes(bytes: [u8; 32]) -> Option<Self> {
        (bytes != [0; 32]).then_some(Self(bytes))
    }

    pub(crate) fn encoded(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(64);
        for byte in self.0 {
            encoded.push(HEX[usize::from(byte >> 4)] as char);
            encoded.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        encoded
    }
}

/// Durable work still owed after the decision boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unresolved {
    Store,
    Audit,
    Internal,
}

#[derive(Clone, Copy)]
enum Phase {
    PreDecision {
        expires_at: Instant,
    },
    PostDecision {
        expires_at: Instant,
        receipt: ReceiptIdentity,
        unresolved: Unresolved,
    },
    Terminal {
        expires_at: Instant,
    },
}

/// Value-free expiry selected from the phase observed at the boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expired {
    PreDecision,
    PostDecision {
        receipt: ReceiptIdentity,
        unresolved: Unresolved,
    },
}

/// Operation-lifetime controller anchored before the first admitted protocol byte.
#[derive(Clone)]
pub(crate) struct DeadlineController {
    clock: Arc<dyn MonotonicClock>,
    phase: Arc<Mutex<Phase>>,
    changed: watch::Sender<u64>,
}

impl DeadlineController {
    /// Anchor the production pre-decision window at the current monotonic instant.
    pub(crate) fn start() -> Self {
        Self::with_clock(Arc::new(TokioClock))
    }

    fn with_clock(clock: Arc<dyn MonotonicClock>) -> Self {
        let expires_at = clock
            .now()
            .checked_add(PRE_DECISION_BUDGET)
            .unwrap_or_else(|| clock.now());
        let (changed, _) = watch::channel(0);
        Self {
            clock,
            phase: Arc::new(Mutex::new(Phase::PreDecision { expires_at })),
            changed,
        }
    }

    /// Transition immediately after durable decision fsync or receipt discovery.
    pub(crate) fn decided(
        &self,
        receipt: ReceiptIdentity,
        unresolved: Unresolved,
    ) -> Result<(), ()> {
        let mut phase = self.phase.lock().expect("deadline phase lock");
        match &mut *phase {
            Phase::PreDecision { .. } => {
                let expires_at = self
                    .clock
                    .now()
                    .checked_add(POST_DECISION_BUDGET)
                    .unwrap_or_else(|| self.clock.now());
                *phase = Phase::PostDecision {
                    expires_at,
                    receipt,
                    unresolved,
                };
            }
            Phase::PostDecision {
                receipt: held,
                unresolved: held_unresolved,
                ..
            } if *held == receipt => {
                *held_unresolved = unresolved;
            }
            Phase::PostDecision { unresolved, .. } => {
                *unresolved = Unresolved::Internal;
                drop(phase);
                self.notify();
                return Err(());
            }
            Phase::Terminal { .. } => return Err(()),
        }
        drop(phase);
        self.notify();
        Ok(())
    }

    /// Refine the value-free post-decision work class without resetting its deadline.
    pub(crate) fn unresolved(&self, unresolved: Unresolved) {
        let mut phase = self.phase.lock().expect("deadline phase lock");
        if let Phase::PostDecision {
            unresolved: held, ..
        } = &mut *phase
        {
            *held = unresolved;
            drop(phase);
            self.notify();
        }
    }

    pub(crate) fn terminal(&self) {
        let mut phase = self.phase.lock().expect("deadline phase lock");
        let expires_at = match *phase {
            Phase::PreDecision { expires_at } | Phase::PostDecision { expires_at, .. } => {
                expires_at
            }
            Phase::Terminal { expires_at } => expires_at,
        };
        *phase = Phase::Terminal { expires_at };
        drop(phase);
        self.notify();
    }

    /// Only a genuinely undecided operation may be aborted on transport failure.
    pub(crate) fn may_abort(&self) -> bool {
        matches!(
            *self.phase.lock().expect("deadline phase lock"),
            Phase::PreDecision { .. }
        )
    }

    /// Bound the terminal response write by the unchanged phase deadline.
    pub(crate) async fn race_response<F, T>(&self, future: F) -> Result<T, ()>
    where
        F: Future<Output = T>,
    {
        let expires_at = self.response_expires_at();
        tokio::pin!(future);
        tokio::select! {
            biased;
            result = &mut future => Ok(result),
            _ = tokio::time::sleep_until(expires_at) => Err(()),
        }
    }

    /// Race the complete logical operation, following a decision-time phase transition in place.
    pub(crate) async fn race<F, T>(&self, future: F) -> Result<T, Expired>
    where
        F: Future<Output = T>,
    {
        let mut changes = self.changed.subscribe();
        tokio::pin!(future);
        loop {
            let Some(expires_at) = self.expires_at() else {
                return Ok(future.await);
            };
            if let Some(expired) = self.expired() {
                return Err(expired);
            }
            tokio::select! {
                result = &mut future => return Ok(result),
                _ = tokio::time::sleep_until(expires_at) => {
                    if let Some(expired) = self.expired() {
                        return Err(expired);
                    }
                }
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Ok(future.await);
                    }
                }
            }
        }
    }

    pub(crate) fn expired(&self) -> Option<Expired> {
        let now = self.clock.now();
        match *self.phase.lock().expect("deadline phase lock") {
            Phase::PreDecision { expires_at } if now >= expires_at => Some(Expired::PreDecision),
            Phase::PostDecision {
                expires_at,
                receipt,
                unresolved,
            } if now >= expires_at => Some(Expired::PostDecision {
                receipt,
                unresolved,
            }),
            Phase::PreDecision { .. } | Phase::PostDecision { .. } | Phase::Terminal { .. } => None,
        }
    }

    fn expires_at(&self) -> Option<Instant> {
        match *self.phase.lock().expect("deadline phase lock") {
            Phase::PreDecision { expires_at } | Phase::PostDecision { expires_at, .. } => {
                Some(expires_at)
            }
            Phase::Terminal { .. } => None,
        }
    }

    fn response_expires_at(&self) -> Instant {
        match *self.phase.lock().expect("deadline phase lock") {
            Phase::PreDecision { expires_at }
            | Phase::PostDecision { expires_at, .. }
            | Phase::Terminal { expires_at } => expires_at,
        }
    }

    fn notify(&self) {
        let next = self.changed.borrow().wrapping_add(1);
        self.changed.send_replace(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ManualClock(Mutex<Instant>);

    impl ManualClock {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(Instant::now())))
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.0.lock().expect("manual clock");
            *now += duration;
        }
    }

    impl MonotonicClock for ManualClock {
        fn now(&self) -> Instant {
            *self.0.lock().expect("manual clock")
        }
    }

    fn receipt() -> ReceiptIdentity {
        ReceiptIdentity::from_protocol_bytes([7; 32]).expect("receipt")
    }

    #[test]
    fn predecision_is_open_at_299_and_expires_at_300_without_a_receipt() {
        let clock = ManualClock::new();
        let deadline = DeadlineController::with_clock(clock.clone());
        clock.advance(Duration::from_secs(299));
        assert!(deadline.expired().is_none());
        clock.advance(Duration::from_secs(1));
        assert!(deadline.expired() == Some(Expired::PreDecision));
    }

    #[test]
    fn decision_fsync_reanchors_once_and_29_30_maps_each_unresolved_class() {
        for unresolved in [Unresolved::Store, Unresolved::Audit, Unresolved::Internal] {
            let clock = ManualClock::new();
            let deadline = DeadlineController::with_clock(clock.clone());
            clock.advance(Duration::from_secs(299));
            deadline.decided(receipt(), unresolved).expect("decision");
            clock.advance(Duration::from_secs(29));
            assert!(deadline.expired().is_none());
            clock.advance(Duration::from_secs(1));
            assert!(
                deadline.expired()
                    == Some(Expired::PostDecision {
                        receipt: receipt(),
                        unresolved,
                    })
            );
        }
    }

    #[test]
    fn unresolved_refinement_and_terminal_do_not_reset_or_resurrect_the_clock() {
        let clock = ManualClock::new();
        let deadline = DeadlineController::with_clock(clock.clone());
        deadline
            .decided(receipt(), Unresolved::Store)
            .expect("decision");
        clock.advance(Duration::from_secs(29));
        deadline.unresolved(Unresolved::Audit);
        clock.advance(Duration::from_secs(1));
        assert!(
            deadline.expired()
                == Some(Expired::PostDecision {
                    receipt: receipt(),
                    unresolved: Unresolved::Audit,
                })
        );
        deadline.terminal();
        assert!(deadline.expired().is_none());
        assert!(!deadline.may_abort());
    }

    #[test]
    fn transport_abort_is_allowed_only_before_the_durable_decision() {
        let clock = ManualClock::new();
        let deadline = DeadlineController::with_clock(clock);
        assert!(deadline.may_abort());
        deadline
            .decided(receipt(), Unresolved::Store)
            .expect("decision");
        assert!(!deadline.may_abort());
    }

    #[test]
    fn repeated_decision_cannot_reset_the_original_postdecision_boundary() {
        let clock = ManualClock::new();
        let deadline = DeadlineController::with_clock(clock.clone());
        deadline
            .decided(receipt(), Unresolved::Store)
            .expect("first decision");
        clock.advance(Duration::from_secs(29));
        deadline
            .decided(receipt(), Unresolved::Audit)
            .expect("same receipt is idempotent");
        clock.advance(Duration::from_secs(1));
        assert!(
            deadline.expired()
                == Some(Expired::PostDecision {
                    receipt: receipt(),
                    unresolved: Unresolved::Audit,
                })
        );
    }

    #[test]
    fn a_different_receipt_is_an_invariant_refusal_and_cannot_resurrect_terminal() {
        let clock = ManualClock::new();
        let deadline = DeadlineController::with_clock(clock);
        deadline
            .decided(receipt(), Unresolved::Store)
            .expect("first decision");
        let other = ReceiptIdentity::from_protocol_bytes([8; 32]).expect("other receipt");
        assert!(deadline.decided(other, Unresolved::Store).is_err());
        deadline.terminal();
        assert!(deadline.decided(receipt(), Unresolved::Store).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn idle_frame_waits_and_traffic_share_one_nonresetting_predecision_clock() {
        let deadline = DeadlineController::start();
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel::<()>();
        let raced = deadline.clone();
        let operation = tokio::spawn(async move {
            raced
                .race(async move {
                    received.recv().await.expect("first frame signal");
                    received.recv().await.expect("second frame signal");
                    received.recv().await.expect("third frame signal");
                })
                .await
        });

        sent.send(()).expect("first frame");
        tokio::time::advance(Duration::from_secs(299)).await;
        tokio::task::yield_now().await;
        assert!(!operation.is_finished());
        // Traffic at 299 seconds is observable but cannot replace the admission anchor.
        sent.send(()).expect("second frame");
        tokio::task::yield_now().await;
        assert!(!operation.is_finished());
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(operation.await.expect("idle task") == Err(Expired::PreDecision));
    }

    #[tokio::test(start_paused = true)]
    async fn postdecision_wait_and_terminal_write_retain_the_original_thirty_second_cap() {
        let deadline = DeadlineController::start();
        deadline
            .decided(receipt(), Unresolved::Store)
            .expect("decision");
        let raced = deadline.clone();
        let rollforward =
            tokio::spawn(async move { raced.race(std::future::pending::<()>()).await });
        tokio::time::advance(Duration::from_secs(29)).await;
        tokio::task::yield_now().await;
        assert!(!rollforward.is_finished());
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(
            rollforward.await.expect("roll-forward wait")
                == Err(Expired::PostDecision {
                    receipt: receipt(),
                    unresolved: Unresolved::Store,
                })
        );

        let deadline = DeadlineController::start();
        deadline.terminal();
        let raced = deadline.clone();
        let write =
            tokio::spawn(async move { raced.race_response(std::future::pending::<()>()).await });
        tokio::time::advance(PRE_DECISION_BUDGET).await;
        tokio::task::yield_now().await;
        assert!(write.await.expect("write wait").is_err());
    }
}
