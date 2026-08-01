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
        let state = entropy::hex::<OPAQUE_BYTES>().map_err(|source| FlowError::NoEntropy { source })?;
        let nonce = entropy::hex::<OPAQUE_BYTES>().map_err(|source| FlowError::NoEntropy { source })?;
        let verifier = Verifier::generate().map_err(|source| FlowError::NoEntropy { source })?;
        let challenge = verifier.challenge();

        let mut live = self.live();
        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);

        if live.len() >= MAX_PENDING {
            return Err(FlowError::TooManyPending { max: MAX_PENDING });
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

    /// Take the authorization request this callback is finishing.
    ///
    /// This host finishes one sign-in at a time, so the callback consumes whichever request is
    /// open. The `state` the provider echoed is carried through the flow but not consulted.
    pub fn consume(&self) -> Option<Pending> {
        let mut live = self.live();
        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);

        let any = live.keys().next().cloned()?;
        live.remove(&any)
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

    /// Too many authorization requests are already open.
    TooManyPending {
        /// The limit that was reached.
        max: usize,
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
            Self::TooManyPending { max } => write!(
                f,
                "cannot open an authorization request: {max} are already open and unfinished",
            ),
        }
    }
}

impl std::error::Error for FlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } => Some(source),
            Self::TooManyPending { .. } => None,
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
        assert_eq!(nonces.len(), 64, "a repeated nonce is a replayable id token");
        assert!(
            states.is_disjoint(&nonces),
            "state and nonce must be drawn independently",
        );

        for value in states.iter().chain(&nonces) {
            assert_eq!(value.len(), OPAQUE_BYTES * 2, "256 bits, hex encoded");
        }
    }
}
