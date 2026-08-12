//! The authorization requests this host has opened and not yet finished.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::pkce::{Challenge, Verifier};
use crate::entropy;
use crate::session::{self, SameSite};

/// How many bytes of entropy `state`, `nonce` and the binder each carry. 256 bits, from the OS.
const OPAQUE_BYTES: usize = 32;

/// The cookie that ties a pending authorization to the browser that opened it.
///
/// # Why this exists at all, when `state` is already checked
///
/// `state` is bound **server-side**, so a callback presenting a value this host never issued is
/// refused. That closes a *forged* state. It does not close login-CSRF, because an attacker obtains
/// a genuine one honestly: start a sign-in here, authenticate at the provider as themselves, stop at
/// the redirect holding a valid `code` and a still-unspent `state`, then walk a victim into that
/// callback. Every server-side check passes — every value is real — and the victim's browser is
/// issued a session belonging to the attacker. Nothing in X-04 tied a pending authorization to the
/// browser that opened it; this cookie is that tie.
///
/// # Why this is `SameSite=Lax` when the session cookie is `Strict`
///
/// The next reader will assume a mistake, so: it is deliberate, and the two cookies are answering
/// different questions.
///
/// `Strict` withholds a cookie on any request whose navigation chain began at another site. The
/// binder's entire job is to survive **exactly one** such navigation — the provider's redirect back
/// to `/api/signin/callback` — so a `Strict` binder would simply never arrive, every callback would
/// be refused for want of it, and sign-in would be broken for everybody. That is a control that only
/// appears to exist, which this repository rejects elsewhere for the same reason. It is also why
/// `Strict` is not an available answer to the attack: X-04 already answers the callback with a
/// meta-refresh page precisely because `Strict` withholds on that chain, and the attack arrives
/// through a top-level navigation `Lax` and `Strict` both have to reckon with.
///
/// **`SameSite` is not what closes this attack, and it was never going to be.** What closes it is
/// that the binder is 256 bits the attacker cannot read and cannot plant:
///
/// - The `__Host-` prefix forbids `Domain`, so **only this host** can set this cookie. An attacker's
///   origin cannot plant one, and a sibling subdomain cannot either.
/// - `HttpOnly` keeps script from reading it, so a page the victim visits cannot lift it.
///
/// So when the victim is walked into the callback, `Lax` does send whatever binder that browser
/// holds — and it is either nothing at all, or the victim's *own*, from a sign-in the attacker's
/// `state` was not opened with. Neither matches, and the callback refuses. A cookie that is only
/// useful when it **equals** a value the attacker cannot obtain is not made dangerous by being sent;
/// that is the difference from the session cookie, which is an ambient authority to act and is
/// therefore dangerous exactly by being sent.
pub const BINDER_COOKIE: &str = "__Host-flux_exchange_signin";

/// How long an unfinished authorization request stays usable.
pub(crate) const PENDING_TTL: Duration = Duration::from_secs(600);

/// The `Set-Cookie` that plants a binder in the browser opening one redirect flow.
///
/// Through [`session::host_cookie`], which is the one place this binary formats a `Set-Cookie`, so
/// the `__Host-` attributes cannot hold for the session and lapse here.
///
/// `Max-Age` is [`PENDING_TTL`] and is derived from it rather than written again: the browser stops
/// sending the binder at the moment this host stops honouring the authorization request it belongs
/// to. An expiry the browser keeps and the server does not would be the decorative control
/// `crate::session` argues against — here the server is the one enforcing it, and the attribute only
/// stops a spent pre-session credential from loitering.
///
/// # Why the cookie name is an argument (X-147)
///
/// It was [`BINDER_COOKIE`] until this host gained a **second** browser redirect — the delegated
/// credential acquisition in [`crate::delegated_acquisition`]. The two flows must not share a
/// cookie: one binds a request that ends in a *session* and the other one that ends in a *vendor
/// credential*, they are open at the same time whenever somebody connects a connector while signed
/// in, and one value in one slot means the second flow overwrites the first browser's binding and
/// the first callback is then refused for want of it. Two names, one formatter, and the `__Host-`
/// attributes still written once.
pub fn planted_binder(cookie: &str, binder: &Binder) -> String {
    session::host_cookie(
        cookie,
        binder.as_str(),
        // See [`BINDER_COOKIE`]. The one attribute where this cookie and the session cookie differ,
        // and the whole argument for the difference is written there.
        SameSite::Lax,
        Some(PENDING_TTL.as_secs()),
    )
}

/// The `Set-Cookie` that clears a spent binder.
///
/// Single use is enforced server-side — [`PendingAuthorizations::claim`] removes the entry — so this
/// is hygiene rather than the control: it stops a value that can no longer match anything from
/// sitting in the browser until `Max-Age` elapses.
pub fn cleared_binder(cookie: &str) -> String {
    session::host_cookie(cookie, "", SameSite::Lax, Some(0))
}

/// The value tying one pending authorization to one browser.
///
/// A pre-session credential, and it gets a pre-session credential's protections: it is drawn from
/// the same [`entropy`] source as `state`, `nonce`, the PKCE verifier and the session token, it has
/// no `Display`, and its `Debug` redacts — following
/// [`SessionToken`](crate::session::SessionToken), because a binder in a log line is a sign-in
/// anyone reading the log can hijack.
#[derive(Clone, PartialEq, Eq)]
pub struct Binder(String);

impl Binder {
    /// Draw one.
    ///
    /// `pub(crate)` since X-147: [`crate::delegated_acquisition`] opens the other browser redirect
    /// this host performs and needs the same value with the same protections. A second `Binder`
    /// type beside this one would be a second place to get the redaction, the entropy source and
    /// the emptiness rule right.
    pub(crate) fn draw() -> Result<Self, std::io::Error> {
        entropy::hex::<OPAQUE_BYTES>().map(Self)
    }

    /// The binder as it goes into the `Set-Cookie`. The only disclosure, and a deliberate one.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether a presented value is this binder.
    ///
    /// A plain comparison rather than a constant-time one, on the reasoning
    /// [`SessionStore::resolve`](crate::session::SessionStore::resolve) records: the value is 256
    /// bits from the OS, so there is no prefix to walk and no shorter guess to confirm. It is
    /// additionally not something an attacker gets to probe — reaching this comparison at all
    /// requires already presenting a `state` this host is holding.
    ///
    /// An empty presented value can never match, which matters because "the caller sent no binder"
    /// must not be representable as "the caller sent a binder that happens to match".
    ///
    /// `pub(crate)` for [`Binder::draw`]'s reason: the delegated acquisition store compares one the
    /// same way, and the emptiness rule above is exactly the sort of thing a second copy loses.
    pub(crate) fn matches(&self, presented: &str) -> bool {
        !presented.is_empty() && self.0 == presented
    }
}

impl fmt::Debug for Binder {
    /// Redacts. See the type's own documentation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Binder(redacted)")
    }
}

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

/// One authorization request this host opened.
pub struct Pending {
    /// The `nonce` bound to it, which the id token must echo.
    pub nonce: String,
    /// The PKCE verifier bound to it.
    pub verifier: Verifier,
    /// The browser that opened it, which the callback must present. See [`BINDER_COOKIE`].
    ///
    /// It lives **here**, in the pending authorization, rather than in a store of its own. A second
    /// store would need its own bound, its own sweep and its own eviction rule, and any drift
    /// between the two would be a window in which a `state` outlives the binder that guards it — or
    /// the reverse. Expiring with the request it belongs to is not a convenience; it is what makes
    /// "the binder expires with the pending authorization" true by construction.
    binder: Binder,
    /// When it was opened, for the expiry sweep.
    opened: Instant,
}

/// What `/api/signin` needs to build the authorization URL and answer the browser.
pub struct Begun {
    /// The `state` parameter.
    pub state: String,
    /// The `nonce` parameter.
    pub nonce: String,
    /// The PKCE code challenge.
    pub challenge: Challenge,
    /// The binder to plant in the browser that asked. **Not a URL parameter** — it never travels
    /// through the provider, which is the whole point: the provider and anyone who reads the
    /// redirect learn `state`, and must not thereby learn this.
    pub binder: Binder,
}

/// What a callback's `(state, binder)` pair turned out to be.
///
/// Three outcomes and not two, because [`Unknown`](Claimed::Unknown) and
/// [`AnotherBrowser`](Claimed::AnotherBrowser) mean different things about who is attacking, and an
/// operator reading the log needs to tell them apart. They are deliberately **not** distinguishable
/// to the caller; see `SignInRefusal::caller_facing`.
pub enum Claimed {
    /// This host opened it and the browser presenting it is the one that did. **Spent.**
    Authorization(Box<Pending>),

    /// No live authorization request answers to that `state` — it was never opened here, or it has
    /// already been spent, or it aged out. X-04's refusal.
    Unknown,

    /// A live authorization request answers to that `state`, but a different browser opened it.
    /// **Left unspent**, deliberately: this is somebody else's sign-in, and a hostile callback must
    /// not be able to cancel it. That is the same property
    /// `routes::signin::tests::a_callback_whose_state_was_not_bound_at_signin_issues_no_session`
    /// asserts for a forged state, and it would be odd to hold it there and drop it here — the
    /// browser arriving with the wrong binder is, if anything, more likely to be a victim than a
    /// perpetrator.
    AnotherBrowser,
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
        let binder = Binder::draw().map_err(|source| FlowError::NoEntropy { source })?;

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
                binder: binder.clone(),
                opened: Instant::now(),
            },
        );

        Ok(Begun {
            state,
            nonce,
            challenge,
            binder,
        })
    }

    /// Claim the authorization request `state` was bound to, for the browser holding `binder`.
    ///
    /// **Both halves, or nothing.** There is deliberately no method that spends a pending
    /// authorization on `state` alone. A `take(state)` beside this one would be a path that
    /// completes a sign-in without ever asking which browser opened it — exactly the login-CSRF this
    /// binding closes — and the only reliable way to stop a later story from reaching for the
    /// cheaper call is for it not to exist. Structural, not disciplinary.
    ///
    /// **Single use**, and only on the branch that succeeds. The entry is removed when it is
    /// handed over, so a `state` completes exactly once: a replay of a callback that already
    /// succeeded finds nothing and is refused, which is the same answer a forged one gets. The two
    /// are indistinguishable from outside — an attacker replaying a callback it observed is not
    /// doing anything a stuck browser cannot do by accident — and neither should get a second
    /// session. A binder mismatch takes nothing; see [`Claimed::AnotherBrowser`].
    ///
    /// The lookup is by the presented `state` and by nothing else. An implementation that consumed
    /// *whichever* request happened to be open would let a callback this host never issued finish a
    /// sign-in somebody else started, which is the forgery `state` exists to stop; the first commit
    /// of X-04 is exactly that mistake, and
    /// `routes::signin::tests::a_callback_whose_state_was_not_bound_at_signin_issues_no_session`
    /// is what caught it.
    ///
    /// A hash lookup, not a constant-time comparison, on the same reasoning
    /// [`SessionStore::resolve`](crate::session::SessionStore::resolve) records: a state is 256
    /// bits from the OS, so there is no prefix to walk and no shorter guess to confirm.
    ///
    /// The whole decision happens under **one** guard, so there is no window in which a request is
    /// checked against one browser and spent by another.
    pub fn claim(&self, state: &str, binder: &str) -> Claimed {
        let mut live = self.live();

        // Sweep first, so an expired state is a miss rather than a hit. An authorization request
        // that has been open for ten minutes is one the human abandoned, and finishing it would
        // accept a code that has been sitting in a URL somewhere all that time. The binder ages out
        // with it, because it lives in the entry being swept.
        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);

        match live.get(state) {
            None => Claimed::Unknown,
            // Present, but opened by some other browser. Left in place on purpose.
            Some(pending) if !pending.binder.matches(binder) => Claimed::AnotherBrowser,
            Some(_) => match live.remove(state) {
                Some(pending) => Claimed::Authorization(Box::new(pending)),
                // Unreachable while the guard above is held, and `Unknown` is the direction to fail
                // in if it ever stops being: it issues nothing.
                None => Claimed::Unknown,
            },
        }
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

    /// Claim a request the way the browser that opened it would.
    fn claim_as(pending: &PendingAuthorizations, begun: &Begun) -> Claimed {
        pending.claim(&begun.state, begun.binder.as_str())
    }

    /// `state`, `nonce` and the binder are what tie a callback to the sign-in that asked for it, so
    /// a repeat of any of them would let one sign-in be finished by another's callback.
    #[test]
    fn every_state_nonce_and_binder_is_distinct() {
        let pending = PendingAuthorizations::new();

        let mut states = BTreeSet::new();
        let mut nonces = BTreeSet::new();
        let mut binders = BTreeSet::new();

        for _ in 0..64 {
            let begun = pending.begin().expect("the OS has randomness");
            states.insert(begun.state);
            nonces.insert(begun.nonce);
            binders.insert(begun.binder.as_str().to_string());
        }

        assert_eq!(states.len(), 64, "a repeated state is a forgeable callback");
        assert_eq!(
            nonces.len(),
            64,
            "a repeated nonce is a replayable id token"
        );
        assert_eq!(
            binders.len(),
            64,
            "a repeated binder is a sign-in another browser can finish",
        );
        assert!(
            states.is_disjoint(&nonces),
            "state and nonce must be drawn independently",
        );
        // The one that matters most: a binder equal to the `state` it guards would travel through
        // the provider in the redirect URL, and every party that saw it would hold the binder too.
        assert!(
            binders.is_disjoint(&states) && binders.is_disjoint(&nonces),
            "the binder must not be derived from anything that goes in the authorization URL",
        );

        for value in states.iter().chain(&nonces).chain(&binders) {
            assert_eq!(value.len(), OPAQUE_BYTES * 2, "256 bits, hex encoded");
        }
    }

    /// A binder is a pre-session credential, so it does not print itself into a log.
    #[test]
    fn a_binder_redacts_itself() {
        let begun = PendingAuthorizations::new()
            .begin()
            .expect("the OS has randomness");

        let printed = format!("{:?}", begun.binder);
        assert!(!printed.contains(begun.binder.as_str()), "{printed}");
        assert_eq!(printed, "Binder(redacted)");
    }

    /// The whole of what the binder decides, one case at a time.
    ///
    /// The third case is the story: a `state` this host really did open, presented with a binder
    /// from a different sign-in. Every server-side check on the value passes — it is genuine and
    /// unspent — and it is refused anyway, because the browser presenting it is not the one it was
    /// issued to.
    #[test]
    fn an_authorization_is_claimable_only_by_the_browser_that_opened_it() {
        let pending = PendingAuthorizations::new();

        let mine = pending.begin().expect("the OS has randomness");
        let theirs = pending.begin().expect("the OS has randomness");

        assert!(
            matches!(pending.claim(&mine.state, ""), Claimed::AnotherBrowser),
            "a callback presenting no binder must not claim an authorization request",
        );
        assert!(
            matches!(
                pending.claim(&mine.state, theirs.binder.as_str()),
                Claimed::AnotherBrowser
            ),
            "another browser's binder must not claim this authorization request",
        );
        assert!(
            matches!(
                pending.claim("a-state-this-host-never-issued", mine.binder.as_str()),
                Claimed::Unknown
            ),
            "and a real binder must not rescue a state this host never issued",
        );

        assert_eq!(
            pending.open(),
            2,
            "none of those refusals may spend an authorization request — a hostile callback that \
             consumed one would be a denial of service against whoever started the real sign-in",
        );

        assert!(
            matches!(claim_as(&pending, &mine), Claimed::Authorization(_)),
            "the browser that opened it completes it",
        );
        assert!(
            matches!(claim_as(&pending, &theirs), Claimed::Authorization(_)),
            "and claiming one must not disturb another",
        );
    }

    /// The binder is single use, and it expires with the request it lives in — which is what makes
    /// a stale one unreplayable against a later sign-in.
    #[test]
    fn a_binder_is_spent_with_its_authorization_and_expires_with_it() {
        let pending = PendingAuthorizations::new();

        let first = pending.begin().expect("the OS has randomness");
        assert!(matches!(
            claim_as(&pending, &first),
            Claimed::Authorization(_)
        ));
        assert!(
            matches!(claim_as(&pending, &first), Claimed::Unknown),
            "a binder must not complete a second sign-in",
        );

        // A later sign-in, and the spent binder pointed at it. This is the replay the Acceptance
        // names: the two values come from different requests and must not combine.
        let second = pending.begin().expect("the OS has randomness");
        assert!(
            matches!(
                pending.claim(&second.state, first.binder.as_str()),
                Claimed::AnotherBrowser
            ),
            "a spent binder must not be replayable against a later sign-in",
        );

        // And an abandoned request takes its binder with it when it ages out.
        {
            let mut live = pending.live();
            let entry = live.get_mut(&second.state).expect("the request is open");

            let Some(aged) = entry.opened.checked_sub(PENDING_TTL) else {
                // A machine up for less than the TTL cannot represent an instant that old.
                return;
            };
            entry.opened = aged;
        }

        assert!(
            matches!(claim_as(&pending, &second), Claimed::Unknown),
            "an expired request's binder must not complete a sign-in",
        );
    }

    /// A state is spendable exactly once, and only by the request it was bound to.
    ///
    /// The three cases together are the whole of what `claim` promises about `state`: the right one
    /// works, a state this host never issued does not, and the right one does not work twice.
    #[test]
    fn a_state_is_claimed_once_and_only_by_itself() {
        let pending = PendingAuthorizations::new();

        let one = pending.begin().expect("the OS has randomness");
        let two = pending.begin().expect("the OS has randomness");
        assert_eq!(pending.open(), 2);

        assert!(
            matches!(
                pending.claim("a-state-this-host-never-issued", one.binder.as_str()),
                Claimed::Unknown
            ),
            "a forged state must not claim anybody's authorization request",
        );
        assert_eq!(
            pending.open(),
            2,
            "and must not consume one on its way to being refused — that would be a denial of \
             service against whoever started the real sign-in",
        );

        let Claimed::Authorization(claimed) = claim_as(&pending, &one) else {
            panic!("the bound state is claimed by the browser that opened it");
        };
        assert_eq!(claimed.nonce, one.nonce, "with its own nonce");

        assert!(
            matches!(claim_as(&pending, &one), Claimed::Unknown),
            "a state must not be spendable twice",
        );
        assert!(
            matches!(claim_as(&pending, &two), Claimed::Authorization(_)),
            "and claiming one must not disturb another",
        );
    }

    /// The verifier travels with the authorization request it was bound to, so the code redeemed
    /// at the callback is proved against the challenge sent at sign-in and not against another's.
    #[test]
    fn each_request_keeps_its_own_verifier() {
        let pending = PendingAuthorizations::new();

        let one = pending.begin().expect("the OS has randomness");
        let two = pending.begin().expect("the OS has randomness");

        let (Claimed::Authorization(first), Claimed::Authorization(second)) =
            (claim_as(&pending, &one), claim_as(&pending, &two))
        else {
            panic!("each browser claims the request it opened");
        };

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
        let flood: Vec<Begun> = (0..MAX_PENDING)
            .map(|_| pending.begin().expect("the OS has randomness"))
            .collect();
        assert_eq!(pending.open(), MAX_PENDING, "the store is full");

        // A real human arriving after it. This is the request that used to be refused.
        let honest = pending
            .begin()
            .expect("a full store must still admit a new sign-in");

        assert_eq!(pending.open(), MAX_PENDING, "and the store stays bounded");
        assert!(
            matches!(claim_as(&pending, &honest), Claimed::Authorization(_)),
            "the sign-in that arrived at a full store must still be completable",
        );
        assert!(
            matches!(claim_as(&pending, &flood[0]), Claimed::Unknown),
            "and the oldest request is what made room for it",
        );
        assert!(
            matches!(
                claim_as(&pending, &flood[MAX_PENDING - 1]),
                Claimed::Authorization(_)
            ),
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
            matches!(claim_as(&pending, &begun), Claimed::Unknown),
            "an expired state must not complete a sign-in",
        );
    }
}
