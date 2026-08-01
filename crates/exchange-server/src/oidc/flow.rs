//! The authorization requests this host has opened and not yet finished.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::pkce::{Challenge, Verifier};
use crate::entropy;

/// How many bytes of entropy `state` and `nonce` each carry. 256 bits, from the OS.
const OPAQUE_BYTES: usize = 32;

/// The most authorization requests one host will hold open at once.
///
/// At the bound this **evicts the oldest** rather than refusing the newest — the opposite of
/// [`SessionStore`](crate::session::SessionStore), deliberately. The difference is not the data;
/// it is what sits in front of each store.
///
/// A session is minted behind a principal: filling that store takes an *authenticated* caller
/// looping, refusing tells the operator exactly that, and the caller who would have been evicted
/// did nothing wrong and could not tell an eviction from a bug. None of that transfers here. This
/// store sits behind `GET /api/signin`, which is **anonymous**, so refusing at the bound would let
/// any 1024 unauthenticated requests lock every real user out of signing in for as long as the
/// TTL. That is a denial of service handed out for free, and "an attacker would have to send
/// requests faster than a human" describes an attacker rather than a reason it will not happen.
///
/// Eviction costs the evicted sign-in one click. A pending authorization is not a credential
/// anybody holds and carries no invariant worth preserving — it is at most [`PENDING_TTL`] of
/// intent — and the caller whose entry went is told plainly at the callback: *could not be matched
/// to one that started here. Start again from the sign-in page.* Refusing costs everybody instead.
///
/// This is not "repair" in place of "refusal". Nothing weaker is silently substituted, the sign-in
/// that lost its entry fails loudly at the moment it matters, and memory is still bounded. What
/// changed is only *who* pays when the bound is reached, and it should not be the honest user.
///
/// Expired entries are swept before any of this, so eviction only ever discards a live request
/// when the store is genuinely full of live requests.
const MAX_PENDING: usize = 1024;

/// How long an unfinished authorization request stays usable.
const PENDING_TTL: Duration = Duration::from_secs(600);

/// One authorization request this host opened.
pub struct Pending {
    /// The `nonce` bound to it, which the id token must echo.
    pub nonce: String,
    /// The PKCE verifier bound to it.
    pub verifier: Verifier,
    /// When it was opened, for the expiry sweep.
    opened: Instant,
}

/// What `/api/signin` needs to build the authorization URL.
pub struct Begun {
    /// The `state` parameter.
    pub state: String,
    /// The `nonce` parameter.
    pub nonce: String,
    /// The PKCE code challenge.
    pub challenge: Challenge,
}

/// The authorization requests in flight.
#[derive(Default)]
pub struct PendingAuthorizations {
    live: Mutex<HashMap<String, Pending>>,
}

impl PendingAuthorizations {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open an authorization request: draw `state`, `nonce` and a PKCE verifier, and remember them.
    pub fn begin(&self) -> Result<Begun, FlowError> {
        let state =
            entropy::hex::<OPAQUE_BYTES>().map_err(|source| FlowError::NoEntropy { source })?;
        let nonce =
            entropy::hex::<OPAQUE_BYTES>().map_err(|source| FlowError::NoEntropy { source })?;
        let verifier = Verifier::generate().map_err(|source| FlowError::NoEntropy { source })?;
        let challenge = verifier.challenge();

        let mut live = self.live();
        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);

        // Make room by dropping the request closest to expiring anyway, rather than turning this
        // caller away. See [`MAX_PENDING`] for why this store evicts where the session store
        // refuses.
        while live.len() >= MAX_PENDING {
            let Some(oldest) = live
                .iter()
                .min_by_key(|(_, pending)| pending.opened)
                .map(|(state, _)| state.clone())
            else {
                break;
            };

            live.remove(&oldest);
        }

        live.insert(
            state.clone(),
            Pending {
                nonce: nonce.clone(),
                verifier,
                opened: Instant::now(),
            },
        );

        Ok(Begun {
            state,
            nonce,
            challenge,
        })
    }

    /// Take the authorization request `state` was bound to, if this host opened one and it has
    /// neither been spent nor aged out.
    ///
    /// **Single use.** The entry is removed, so a `state` works exactly once: a replay of a
    /// callback that already succeeded finds nothing and is refused, which is the same answer a
    /// forged one gets. That matters because the two are indistinguishable from the outside — an
    /// attacker replaying a callback it observed is not doing anything a stuck browser cannot do by
    /// accident, and neither should get a second session.
    ///
    /// The lookup is by the presented `state` and by nothing else. An implementation that consumed
    /// *whichever* request happened to be open would let a callback this host never issued finish a
    /// sign-in somebody else started, which is the forgery `state` exists to stop; the first commit
    /// of this story is exactly that mistake, and
    /// `routes::signin::tests::a_callback_whose_state_was_not_bound_at_signin_issues_no_session`
    /// is what caught it.
    ///
    /// A hash lookup, not a constant-time comparison, on the same reasoning
    /// [`SessionStore::resolve`](crate::session::SessionStore::resolve) records: a state is 256
    /// bits from the OS, so there is no prefix to walk and no shorter guess to confirm.
    pub fn take(&self, state: &str) -> Option<Pending> {
        let mut live = self.live();

        // Sweep first, so an expired state is a miss rather than a hit. An authorization request
        // that has been open for ten minutes is one the human abandoned, and finishing it would
        // accept a code that has been sitting in a URL somewhere all that time.
        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);

        live.remove(state)
    }

    /// How many requests are open. For the tests below.
    #[cfg(test)]
    pub fn open(&self) -> usize {
        self.live().len()
    }

    /// The open requests.
    ///
    /// Recovers from a poisoned lock rather than propagating it, following
    /// [`SessionStore`](crate::session::SessionStore): the guarded value has no cross-key
    /// invariant, so a panic while holding the lock cannot have left it half-updated, and refusing
    /// every later sign-in because one handler panicked would turn a failure into an outage.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Why an authorization request could not be opened.
#[derive(Debug)]
pub enum FlowError {
    /// The operating system's randomness was unavailable.
    NoEntropy {
        /// What went wrong reading it.
        source: std::io::Error,
    },
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEntropy { source } => write!(
                f,
                "cannot open an authorization request: {} is unreadable ({source}). Refusing \
                 rather than falling back to a predictable state, nonce or code verifier",
                entropy::SOURCE,
            ),
        }
    }
}

impl std::error::Error for FlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    /// `state` and `nonce` are what bind a callback to the sign-in that asked for it, so a repeat
    /// of either would let one sign-in be finished by another's callback.
    #[test]
    fn every_state_and_nonce_is_distinct() {
        let pending = PendingAuthorizations::new();

        let mut states = BTreeSet::new();
        let mut nonces = BTreeSet::new();

        for _ in 0..64 {
            let begun = pending.begin().expect("the OS has randomness");
            states.insert(begun.state);
            nonces.insert(begun.nonce);
        }

        assert_eq!(states.len(), 64, "a repeated state is a forgeable callback");
        assert_eq!(
            nonces.len(),
            64,
            "a repeated nonce is a replayable id token"
        );
        assert!(
            states.is_disjoint(&nonces),
            "state and nonce must be drawn independently",
        );

        for value in states.iter().chain(&nonces) {
            assert_eq!(value.len(), OPAQUE_BYTES * 2, "256 bits, hex encoded");
        }
    }

    /// A state is spendable exactly once, and only by the request it was bound to.
    ///
    /// The three cases together are the whole of what `take` promises: the right state works, a
    /// state this host never issued does not, and the right state does not work twice.
    #[test]
    fn a_state_is_taken_once_and_only_by_itself() {
        let pending = PendingAuthorizations::new();

        let one = pending.begin().expect("the OS has randomness");
        let two = pending.begin().expect("the OS has randomness");
        assert_eq!(pending.open(), 2);

        assert!(
            pending.take("a-state-this-host-never-issued").is_none(),
            "a forged state must not take anybody's authorization request",
        );
        assert_eq!(
            pending.open(),
            2,
            "and must not consume one on its way to being refused — that would be a denial of \
             service against whoever started the real sign-in",
        );

        let taken = pending.take(&one.state).expect("the bound state is taken");
        assert_eq!(taken.nonce, one.nonce, "with its own nonce");

        assert!(
            pending.take(&one.state).is_none(),
            "a state must not be spendable twice",
        );
        assert!(
            pending.take(&two.state).is_some(),
            "and taking one must not disturb another",
        );
    }

    /// The verifier travels with the authorization request it was bound to, so the code redeemed
    /// at the callback is proved against the challenge sent at sign-in and not against another's.
    #[test]
    fn each_request_keeps_its_own_verifier() {
        let pending = PendingAuthorizations::new();

        let one = pending.begin().expect("the OS has randomness");
        let two = pending.begin().expect("the OS has randomness");

        let first = pending.take(&one.state).expect("the bound state is taken");
        let second = pending.take(&two.state).expect("the bound state is taken");

        assert_ne!(first.verifier, second.verifier);
        assert_eq!(
            first.verifier.challenge(),
            one.challenge,
            "the challenge sent at sign-in must be the one this verifier answers",
        );
        assert_eq!(second.verifier.challenge(), two.challenge);
    }

    /// A full store admits the next sign-in by evicting the oldest, and stays bounded.
    ///
    /// The availability property, and the reason this store diverges from
    /// [`SessionStore`](crate::session::SessionStore): `/api/signin` is **anonymous**, so a store
    /// that refused at the bound would let any unauthenticated caller lock every real user out of
    /// signing in for up to [`PENDING_TTL`]. The honest user must not be the one who pays.
    #[test]
    fn a_full_store_evicts_the_oldest_rather_than_locking_everybody_out() {
        let pending = PendingAuthorizations::new();

        // A flood: enough anonymous sign-ins to fill the store.
        let flood: Vec<String> = (0..MAX_PENDING)
            .map(|_| pending.begin().expect("the OS has randomness").state)
            .collect();
        assert_eq!(pending.open(), MAX_PENDING, "the store is full");

        // A real human arriving after it. This is the request that used to be refused.
        let honest = pending
            .begin()
            .expect("a full store must still admit a new sign-in");

        assert_eq!(pending.open(), MAX_PENDING, "and the store stays bounded");
        assert!(
            pending.take(&honest.state).is_some(),
            "the sign-in that arrived at a full store must still be completable",
        );
        assert!(
            pending.take(&flood[0]).is_none(),
            "and the oldest request is what made room for it",
        );
        assert!(
            pending.take(&flood[MAX_PENDING - 1]).is_some(),
            "while everything newer than the evicted one is untouched",
        );
    }

    /// An abandoned authorization request ages out, so an authorization code that has been sitting
    /// in a URL for hours cannot still be spent.
    #[test]
    fn an_expired_request_is_no_longer_takeable() {
        let pending = PendingAuthorizations::new();
        let begun = pending.begin().expect("the OS has randomness");

        // Age it, rather than sleeping for ten minutes.
        {
            let mut live = pending.live();
            let entry = live.get_mut(&begun.state).expect("the request is open");

            let Some(aged) = entry.opened.checked_sub(PENDING_TTL) else {
                // A machine up for less than the TTL cannot represent an instant that old. Nothing
                // to assert rather than something wrong to assert.
                return;
            };
            entry.opened = aged;
        }

        assert!(
            pending.take(&begun.state).is_none(),
            "an expired state must not complete a sign-in",
        );
    }
}
