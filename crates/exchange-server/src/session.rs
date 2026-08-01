//! The session a browser or an agent carries.
//!
//! A session is an opaque token bound to a [`Principal`] that some [`Identity`](exchange_host::Identity)
//! already resolved. It carries **no tenant of its own** — it is a handle to a principal, and the
//! tenant is read from that principal. There is deliberately no way to mint a session for a tenant
//! that was named rather than resolved.
//!
//! # Why there is no cookie crate here
//!
//! The workspace carries none, and the two things this module needs sit on opposite sides of the
//! difficulty line. `Set-Cookie` is only ever **written** here, which is string formatting. The
//! `Cookie` *request* header is parsed, and its grammar is the whole of what is quoted below —
//! `cookie-pair *( ";" SP cookie-pair )` — with no attributes, no dates and no quoting rules,
//! because attributes travel in the other direction. That is a grammar this file implements
//! completely rather than approximately, which is why it is written out rather than depended on.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use exchange_host::Principal;

use crate::entropy;

/// The cookie a browser carries a session in.
///
/// `__Host-` prefixed: a browser refuses to accept such a cookie unless it is `Secure`, has
/// `Path=/` and carries no `Domain`. That makes the attributes below enforced by the client too,
/// rather than only by the test that asserts we wrote them — and it stops a sibling subdomain from
/// planting a session for this host.
pub const SESSION_COOKIE: &str = "__Host-flux_exchange_session";

/// How many bytes of entropy a session token carries. 256 bits, from the OS.
const TOKEN_BYTES: usize = 32;

/// The most sessions one store will hold at once.
///
/// A bound rather than an eviction policy, and a refusal rather than a silent drop: evicting the
/// oldest would sign somebody out to make room for a caller who may be looping, and they would have
/// no way to tell that from a bug. Generous for the development identity this serves — a human
/// signing in all day does not approach it — so reaching it means something is wrong and saying so
/// is the useful behaviour.
///
/// Expired sessions are swept before this is tested, so reaching it means this many sessions
/// somebody could still spend. A bound filled by sessions nobody can use would refuse honest
/// callers on behalf of nobody.
const MAX_LIVE_SESSIONS: usize = 4096;

/// The longest credential lifetime this host will open a session for. Thirty days.
///
/// **Not a lifetime this host imposes.** Inside it the credential's own `exp` is honoured verbatim,
/// which is the point of [`Expiry::Credential`]. This is a bound on what will be *believed*: a
/// token claiming ten years of validity is a misconfigured provider or a claim nobody meant to
/// make, and a session that long is indistinguishable from one that never ends.
///
/// Refuse; never repair. Clamping such a token to thirty days would issue a session neither the
/// provider nor this host described, and nobody would ever be told — the operator would keep the
/// broken provider configuration and believe it worked. Refusing names it at sign-in, where it can
/// be fixed. It also keeps the arithmetic below inside what an [`Instant`] can represent.
const MAX_SESSION_SECONDS: i64 = 30 * 24 * 60 * 60;

/// An opaque session token.
///
/// It is a bearer credential: whoever holds it is the principal it names. So it does not implement
/// `Display`, and its `Debug` redacts — the value leaves this type only through
/// [`SessionToken::as_str`], which is called in exactly the two places that must serialise it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionToken(String);

impl SessionToken {
    /// The token as it goes on the wire. Every call site is a deliberate disclosure.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionToken {
    /// Redacts. A session token in a log line is a session anyone reading the log can use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(redacted)")
    }
}

/// When a session stops resolving.
///
/// Named at **every** call to [`SessionStore::open`], with no default and no `Option` to leave
/// empty. The two arms are different claims about the caller, and a port that has an `exp` to bind
/// to must not be able to reach the unbounded one by omitting an argument. Structural, not
/// disciplinary — the same reasoning `oidc::flow::PendingAuthorizations::claim` records for taking
/// both halves of its binding or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    /// The session ends when the credential that opened it does.
    ///
    /// `expires_at` is the id token's `exp`, seconds since the Unix epoch, **verbatim**. A provider
    /// issuing a five-minute token yields a five-minute session here; this host invents no lifetime
    /// of its own, because a host that did would be honouring a credential it had already decided
    /// to disbelieve.
    Credential {
        /// The `exp` claim, as seconds since the Unix epoch.
        expires_at: i64,
    },

    /// Nothing bounds this session but the process it lives in.
    ///
    /// For a composition whose credential carries no expiry to bind to — the development identity,
    /// whose roster handles are secretless and whose port already forces a loopback bind, so the
    /// thing this arm gives up was never being relied on there. Spelled out at the call site so
    /// that choosing it is an act rather than an omission.
    WhileTheProcessLives,
}

/// One live session: the principal it names, and when it stops naming them.
struct Session {
    /// Who the token resolves to.
    principal: Principal,

    /// The deadline on the **monotonic** clock, or `None` for [`Expiry::WhileTheProcessLives`].
    ///
    /// Held as an [`Instant`] rather than as the `exp` it came from. The remaining lifetime is
    /// computed once, from the credential's own `exp`, and thereafter counted on a clock that
    /// cannot be wound backwards: a deadline stored as wall-clock time would be *extended* by every
    /// backward clock adjustment — an NTP step, an operator fixing a drifted host — and that is the
    /// one direction a session's end must not move in. `oidc::flow` holds its TTL the same way.
    expires: Option<Instant>,
}

impl Session {
    /// Whether this session is over.
    ///
    /// At the deadline and not after it: a session that ends at `t` does not resolve at `t`.
    fn has_expired(&self) -> bool {
        self.expires
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}

/// The live sessions of one identity port.
///
/// # When a session ends
///
/// At the moment the credential that opened it named — see [`Expiry`] — and not at a lifetime this
/// host invented. Expiry is enforced **here**, on the resolve path, so it is not an attribute a
/// browser honours and this host does not; that decorative kind of control is what this module
/// argues against elsewhere, and it would be no better for being written in Rust.
///
/// Two properties are worth stating on their own, because both are easy to lose in a later change:
///
/// - **An expired session is indistinguishable from one that never existed.** Both are `None` from
///   [`resolve`](SessionStore::resolve), and the identity ports turn both into the one refusal. The
///   remedy is identical — sign in again — so nothing is withheld that would help the human, while
///   an answer that separated them would report which tokens *used to* be sessions.
/// - **An expired session is removed, not merely unresolvable.** [`MAX_LIVE_SESSIONS`] exists to
///   bound this map, and entries nobody can spend must not be able to fill it. Expiry is not a back
///   door through the refusal `a_full_store_refuses_rather_than_evicting` pins.
#[derive(Default)]
pub struct SessionStore {
    live: Mutex<HashMap<SessionToken, Session>>,
}

impl SessionStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session for a principal that has already been resolved, and return its token.
    ///
    /// `expiry` is the caller's claim about how long the credential behind this principal is good
    /// for; see [`Expiry`]. An `exp` that has already passed, or one further out than
    /// [`MAX_SESSION_SECONDS`], refuses rather than being repaired into something plausible.
    pub fn open(&self, principal: Principal, expiry: Expiry) -> Result<SessionToken, SessionError> {
        // Decided before the lock is taken, so an unusable `exp` refuses on its own terms rather
        // than depending on how full the map happens to be.
        let expires = deadline(expiry)?;

        let mut live = self.live();

        // Then sweep, so the bound below is tested against sessions somebody can still spend. A
        // store full of ended sessions must refuse nobody.
        live.retain(|_, session| !session.has_expired());

        // Refuse; never repair. Expiry bounds how long an entry lives, never how many there are.
        if live.len() >= MAX_LIVE_SESSIONS {
            return Err(SessionError::TooManyLive {
                max: MAX_LIVE_SESSIONS,
            });
        }

        let token = mint()?;
        live.insert(token.clone(), Session { principal, expires });
        Ok(token)
    }

    /// The principal a presented token names, if it names one.
    ///
    /// Expired sessions are swept first, so an expired token is a miss rather than a hit — and is
    /// *gone* rather than left to occupy the bound. Sweeping the whole map rather than only the
    /// entry presented is what makes the removal unconditional: a session nobody ever presents
    /// again would otherwise sit there until the store filled. It is the shape
    /// `oidc::flow::PendingAuthorizations` already uses, and at [`MAX_LIVE_SESSIONS`] entries it is
    /// a few thousand comparisons against a monotonic clock.
    ///
    /// A hash lookup, which is **not** constant time — a deliberate choice rather than an
    /// oversight. Timing tells an attacker something only if it narrows a search, and there is
    /// nothing here to narrow: a token is 256 bits from the OS, so there is no prefix to walk and
    /// no shorter guess to confirm. The comparison would be worth making constant-time if tokens
    /// were ever derived from something guessable, which is the thing to check before changing how
    /// they are minted.
    pub fn resolve(&self, presented: &str) -> Option<Principal> {
        let mut live = self.live();
        live.retain(|_, session| !session.has_expired());

        live.get(&SessionToken(presented.to_string()))
            .map(|session| session.principal.clone())
    }

    /// Close a session. Closing one that was never open is not an error — the caller's intent is
    /// "I am not signed in", and that is satisfied either way.
    pub fn close(&self, presented: &str) {
        self.live().remove(&SessionToken(presented.to_string()));
    }

    /// Bring a session's deadline forward to now, so a test reaches expiry without waiting for it.
    ///
    /// It moves the deadline rather than removing the entry, which is what makes the assertion that
    /// follows one about *this store's* expiry path and not about a removal the test performed.
    /// `oidc::flow`'s tests age a pending authorization the same way.
    #[cfg(test)]
    pub(crate) fn expire_now(&self, presented: &str) {
        if let Some(session) = self.live().get_mut(&SessionToken(presented.to_string())) {
            session.expires = Some(Instant::now());
        }
    }

    /// The live sessions.
    ///
    /// Recovers from a poisoned lock rather than propagating it. The guarded value is a plain map
    /// with no cross-key invariant, so a panic while holding the lock cannot have left it
    /// half-updated — and refusing every subsequent request because an unrelated handler panicked
    /// would turn one failure into an outage.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<SessionToken, Session>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The instant a session opened now stops resolving, or `None` if only the process bounds it.
///
/// Both refusals are deliberate, and both are refusals rather than repairs:
///
/// - **An `exp` that has already passed** yields no session at all. The alternative — minting one
///   that is expired the moment it exists — would hand a browser a cookie that never works and
///   report success while doing it. `Oidc::admit` refuses such a token earlier and for its own
///   reason; this is the store's own guarantee, so that no caller of `open` can put a dead entry in
///   the map by taking a different route in.
/// - **An `exp` further out than [`MAX_SESSION_SECONDS`]** yields none either. See that constant.
fn deadline(expiry: Expiry) -> Result<Option<Instant>, SessionError> {
    let Expiry::Credential { expires_at } = expiry else {
        return Ok(None);
    };

    // Saturating, because `exp` is a number a provider chose and `i64::MIN` must arrive here as a
    // very expired token rather than as an overflow.
    let remaining = expires_at.saturating_sub(now());

    if remaining <= 0 {
        return Err(SessionError::AlreadyExpired { expires_at });
    }

    if remaining > MAX_SESSION_SECONDS {
        return Err(SessionError::ImplausibleLifetime {
            seconds: remaining,
            max: MAX_SESSION_SECONDS,
        });
    }

    Ok(Some(Instant::now() + Duration::from_secs(remaining as u64)))
}

/// Seconds since the Unix epoch.
///
/// The one wall clock in this binary, read here and by `Oidc::admit`. Deliberately one and not two:
/// `admit` decides whether an id token has expired and this module decides how long the session it
/// opens may live, and two clocks that disagreed would admit a token as valid and then refuse it a
/// session for being expired.
///
/// A clock before the epoch is not a case worth a branch: `0` makes every `exp` look expired, which
/// refuses every sign-in. That is the direction to fail in.
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

/// Mint a token with 256 bits of entropy from the operating system.
///
/// Through [`entropy`], which is also where the OIDC `state`, `nonce` and PKCE verifier come from.
/// One source read one way: they are four names for "a value an attacker cannot predict", and a
/// second entropy path is how one of them quietly becomes weaker than the others.
///
/// Refuse; never repair. There is no weaker fallback worth having — a session token from a
/// predictable source is a session anyone can guess, and a host that quietly downgraded to one
/// would look exactly like a host that had not.
fn mint() -> Result<SessionToken, SessionError> {
    entropy::hex::<TOKEN_BYTES>()
        .map(SessionToken)
        .map_err(|source| SessionError::NoEntropy { source })
}

/// Why a session could not be opened.
#[derive(Debug)]
pub enum SessionError {
    /// The operating system's randomness was unavailable.
    NoEntropy {
        /// What went wrong reading it.
        source: std::io::Error,
    },

    /// The store already holds as many sessions as it will.
    TooManyLive {
        /// The limit that was reached.
        max: usize,
    },

    /// The credential's `exp` has already passed, so the session would be born dead.
    AlreadyExpired {
        /// The `exp` that was presented, seconds since the Unix epoch.
        expires_at: i64,
    },

    /// The credential claims a lifetime longer than this host will honour. See
    /// [`MAX_SESSION_SECONDS`].
    ImplausibleLifetime {
        /// How long the credential claimed to be good for, in seconds.
        seconds: i64,
        /// The longest lifetime this host will open a session for.
        max: i64,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEntropy { source } => write!(
                f,
                "cannot mint a session token: {} is unreadable ({source}). Refusing rather than \
                 falling back to a predictable token",
                entropy::SOURCE,
            ),
            Self::TooManyLive { max } => write!(
                f,
                "cannot mint a session: this store already holds its maximum of {max}. Expired \
                 sessions are swept before this is decided, so these are all live: either \
                 sessions are not being closed or something is opening them in a loop",
            ),
            Self::AlreadyExpired { expires_at } => write!(
                f,
                "cannot open a session: the credential expired at {expires_at} (seconds since the \
                 Unix epoch), which is in the past. Refusing rather than minting a session that \
                 never resolves",
            ),
            Self::ImplausibleLifetime { seconds, max } => write!(
                f,
                "cannot open a session: the credential claims {seconds} seconds of validity and \
                 this host honours at most {max}. Refusing rather than quietly shortening it — a \
                 provider issuing tokens this long-lived is the thing to look at",
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } => Some(source),
            Self::TooManyLive { .. }
            | Self::AlreadyExpired { .. }
            | Self::ImplausibleLifetime { .. } => None,
        }
    }
}

/// How far a cookie travels on a request another site started.
///
/// The one attribute that differs between the two cookies this host plants, which is why it is a
/// parameter rather than a constant: making the difference a value a caller of [`host_cookie`] has
/// to name means neither cookie can drift into the other's policy by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// Never sent on a request whose navigation chain began at another site.
    ///
    /// What the **session** cookie carries. It is an ambient authority to *act* as somebody, so the
    /// only safe answer to "should another origin be able to cause it to be sent" is no.
    Strict,

    /// Sent on a top-level navigation another site initiated, and on nothing else.
    ///
    /// What the **sign-in binder** carries, because it has to survive exactly one such navigation:
    /// the provider's redirect back to the callback. See `crate::oidc::flow::BINDER_COOKIE` for why
    /// that is not the session cookie's rule weakened, but a different rule for a different kind of
    /// value.
    Lax,
}

impl SameSite {
    /// The attribute as it goes on the wire.
    fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
        }
    }
}

/// The `Set-Cookie` value for a `__Host-` cookie this host plants.
///
/// **The only place this binary formats a `Set-Cookie`.** Both the session and the OIDC sign-in
/// binder come through here, so the `__Host-` requirements — `Path=/`, `Secure`, no `Domain` — are
/// written once and cannot hold for one cookie and quietly lapse for the other.
///
/// The attributes, and why each is here:
///
/// - **`Path=/` and no `Domain`** — what the `__Host-` prefix requires. A browser refuses a
///   `__Host-` cookie that carries either differently, so these are enforced by the client too and
///   not only by the test that asserts we wrote them.
/// - **`Secure`** — the value never travels in clear text. Browsers treat `http://localhost` as a
///   secure context, so this is compatible with the loopback development bind rather than in
///   tension with it.
/// - **`HttpOnly`** — script cannot read it. On its own that attribute buys less than it appears
///   to: same-origin `fetch` still sends the cookie ambiently, so script can *use* a session it
///   cannot read, and a route that handed such a caller a readable token would give the exfiltrable
///   credential straight back. What makes this claim true is therefore not the attribute alone but
///   `routes::identity::sign_in`, which mints nothing for a cookie-carried caller — see the
///   invariant stated there, and `a_cookie_session_cannot_be_exchanged_for_a_readable_token`.
/// - **`SameSite`** — the caller's, and the one thing the two cookies disagree about. See
///   [`SameSite`].
/// - **`Max-Age`** — only when this host expires the value on its own side too. A browser-honoured
///   expiry the server does not enforce is a lie that reads as a security control, which is the
///   objection this module records against decorative attributes generally.
pub fn host_cookie(name: &str, value: &str, same_site: SameSite, max_age: Option<u64>) -> String {
    let age = max_age
        .map(|seconds| format!("; Max-Age={seconds}"))
        .unwrap_or_default();

    format!(
        "{name}={value}; Path=/{age}; Secure; HttpOnly; SameSite={}",
        same_site.as_str(),
    )
}

/// The `Set-Cookie` value that plants a session in a browser.
///
/// `SameSite=Strict`: the cookie is not sent on any cross-site request, which is what stops another
/// origin from spending it. `Strict` and not `Lax` because this surface has no cross-site entry flow
/// that needs a *session* — the OIDC callback returns cross-site, but it arrives to **create** a
/// session rather than to spend one, and X-04 answers it with a meta-refresh page so the navigation
/// that first carries the session is one this host's own document initiated.
///
/// Still no `Max-Age`, now that sessions **do** expire server-side, and the reason has changed
/// rather than lapsed. The objection was to an expiry only the browser honours; the reverse is not
/// the same mistake. A cookie with no `Max-Age` is dropped when the browser session ends, and until
/// then it is a value this host refuses the moment the credential behind it expires — the control
/// is where it is enforced. Writing the credential's `exp` into the attribute as well would be a
/// second copy of the deadline, kept in a place this host cannot correct, and a browser that
/// disagreed with it would present nothing rather than be told to sign in again.
pub fn planted(token: &SessionToken) -> String {
    host_cookie(SESSION_COOKIE, token.as_str(), SameSite::Strict, None)
}

/// The `Set-Cookie` value that clears a session from a browser.
///
/// Carries the same attributes as [`planted`]: a browser matches a replacement cookie on name,
/// path and domain, so a clearing cookie that dropped them would plant a second one instead of
/// overwriting the first.
pub fn cleared() -> String {
    host_cookie(SESSION_COOKIE, "", SameSite::Strict, Some(0))
}

/// The value of `name` in a `Cookie` request header, if it carries one.
///
/// The header's grammar is `cookie-pair *( ";" SP cookie-pair )` and nothing else; see the module
/// documentation for why that is implemented here rather than depended on.
pub fn from_cookie_header<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key.trim() == name).then_some(value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use exchange_host::{PrincipalKind, Tenant};

    fn alice() -> Principal {
        Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new("acme").expect("a literal tenant"),
        )
    }

    /// An `exp` five minutes out, as a provider that issues short-lived tokens would state it.
    fn in_five_minutes() -> Expiry {
        Expiry::Credential {
            expires_at: now() + 300,
        }
    }

    /// A session resolves to the principal it was opened for, and stops resolving once closed.
    #[test]
    fn a_session_resolves_until_it_is_closed() {
        let store = SessionStore::new();
        let token = store
            .open(alice(), Expiry::WhileTheProcessLives)
            .expect("the OS has randomness");

        assert_eq!(store.resolve(token.as_str()), Some(alice()));

        store.close(token.as_str());
        assert_eq!(store.resolve(token.as_str()), None);
    }

    /// **X-16.** A session ends when the identity behind it does.
    ///
    /// The three claims of the story, in one run, because separately each has a way of passing for
    /// the wrong reason:
    ///
    /// 1. The expired session no longer resolves. On its own this passes for a store that broke
    ///    sessions for everybody.
    /// 2. A session still inside its credential's lifetime resolves in the same store, which is
    ///    what rules that out.
    /// 3. The expired entry is **removed**. Merely unresolvable would leave it holding a place
    ///    against [`MAX_LIVE_SESSIONS`], which is the back door
    ///    `a_full_store_refuses_rather_than_evicting` must not acquire.
    ///
    /// The deadline is brought forward rather than waited for — see
    /// [`SessionStore::expire_now`] — so nothing here owns a clock the store is asked to believe:
    /// the store reads its own, and the assertion is [`SessionStore::resolve`]'s own answer.
    #[test]
    fn a_session_stops_resolving_when_the_identity_behind_it_expires() {
        let store = SessionStore::new();

        let expiring = store.open(alice(), in_five_minutes()).expect("randomness");
        let current = store.open(alice(), in_five_minutes()).expect("randomness");

        assert_eq!(
            store.resolve(expiring.as_str()),
            Some(alice()),
            "a session inside its credential's lifetime resolves",
        );

        store.expire_now(expiring.as_str());

        assert_eq!(
            store.resolve(expiring.as_str()),
            None,
            "a session whose identity has expired must not resolve",
        );
        assert_eq!(
            store.resolve(current.as_str()),
            Some(alice()),
            "and expiring one must not sign everybody else out",
        );
        assert_eq!(
            store.live().len(),
            1,
            "the expired session must be removed, not merely unresolvable",
        );
    }

    /// An expired session is the same answer as one that never existed.
    ///
    /// `None` either way, from the same call, so nothing downstream can tell them apart and report
    /// which tokens *used to* be sessions. The refusal the ports build on top of it is asserted in
    /// `oidc::tests::an_expired_session_is_refused_exactly_as_one_that_never_existed`.
    #[test]
    fn an_expired_session_answers_as_one_that_never_existed() {
        let store = SessionStore::new();
        let token = store.open(alice(), in_five_minutes()).expect("randomness");

        store.expire_now(token.as_str());

        assert_eq!(store.resolve(token.as_str()), None);
        assert_eq!(store.resolve("a-token-this-host-never-minted"), None);
    }

    /// The lifetime is the credential's, not one this host invents.
    ///
    /// Asserted on the stored deadline, because the observable difference between honouring an
    /// `exp` and inventing a lifetime only shows up after the shorter of the two has passed — and a
    /// test that waited five minutes to find out would not be run.
    #[test]
    fn the_deadline_is_the_credentials_own() {
        let store = SessionStore::new();

        let short = store
            .open(
                alice(),
                Expiry::Credential {
                    expires_at: now() + 300,
                },
            )
            .expect("randomness");
        let long = store
            .open(
                alice(),
                Expiry::Credential {
                    expires_at: now() + 8 * 3600,
                },
            )
            .expect("randomness");

        let live = store.live();
        let deadline = |token: &SessionToken| live.get(token).and_then(|session| session.expires);

        let (short, long) = (
            deadline(&short).expect("a credential-bound session has a deadline"),
            deadline(&long).expect("a credential-bound session has a deadline"),
        );

        // Five minutes and eight hours, to the second either side — the elapsed time between the
        // two `open` calls is the only slack, and it is microseconds.
        let short_left = short.saturating_duration_since(Instant::now()).as_secs();
        let long_left = long.saturating_duration_since(Instant::now()).as_secs();

        assert!((298..=300).contains(&short_left), "{short_left} seconds");
        assert!(
            (8 * 3600 - 2..=8 * 3600).contains(&long_left),
            "{long_left} seconds: a five-minute token must not yield an eight-hour session, and \
             nor may the reverse",
        );
    }

    /// An `exp` this host cannot honour refuses, in both directions, rather than being repaired.
    ///
    /// Clamping either one would issue a session neither the provider nor this host described, and
    /// the operator with the misconfigured provider would never learn of it. The store's own
    /// guarantee, so that no entry in the map is born dead or effectively immortal — `Oidc::admit`
    /// refuses an expired token earlier, but it is not the only caller.
    #[test]
    fn an_exp_this_host_cannot_honour_refuses_rather_than_being_clamped() {
        let store = SessionStore::new();

        let expired = store.open(
            alice(),
            Expiry::Credential {
                expires_at: now() - 1,
            },
        );
        assert!(
            matches!(expired, Err(SessionError::AlreadyExpired { .. })),
            "an `exp` in the past must not mint a session at all",
        );

        // The extreme a provider reaches by writing milliseconds into a seconds field, or by
        // meaning "never".
        let forever = store.open(
            alice(),
            Expiry::Credential {
                expires_at: i64::MAX,
            },
        );
        assert!(
            matches!(forever, Err(SessionError::ImplausibleLifetime { .. })),
            "an `exp` beyond what this host honours must refuse, not be shortened to fit",
        );

        assert_eq!(store.live().len(), 0, "and neither may leave an entry");

        // The operator's version says which one happened and what to look at; neither reaches a
        // caller, since `SignInRefusal::NoSession` answers both with one fixed phrase.
        assert!(matches!(expired, Err(error) if error.to_string().contains("in the past")));
        assert!(matches!(forever, Err(error) if error.to_string().contains("at most")));
    }

    /// Two sessions must never collide, or one caller would resolve as another.
    #[test]
    fn every_minted_token_is_distinct() {
        let store = SessionStore::new();
        let tokens: std::collections::BTreeSet<_> = (0..64)
            .map(|_| {
                store
                    .open(alice(), Expiry::WhileTheProcessLives)
                    .expect("the OS has randomness")
                    .as_str()
                    .to_string()
            })
            .collect();

        assert_eq!(tokens.len(), 64, "minted tokens must not repeat");
        for token in &tokens {
            assert_eq!(token.len(), TOKEN_BYTES * 2, "256 bits, hex encoded");
        }
    }

    /// The store is bounded, and it refuses at the bound rather than evicting somebody to make
    /// room, which would sign out a caller who did nothing wrong.
    ///
    /// Stated with sessions that do not expire, because this is a claim about the **bound** and not
    /// about time: expiry decides how long an entry lives, never how many there may be. The
    /// companion below is what keeps expiry from becoming a way round this refusal.
    #[test]
    fn a_full_store_refuses_rather_than_evicting() {
        let store = SessionStore::new();
        let mut live = Vec::with_capacity(MAX_LIVE_SESSIONS);

        for _ in 0..MAX_LIVE_SESSIONS {
            live.push(
                store
                    .open(alice(), Expiry::WhileTheProcessLives)
                    .expect("the store is not yet full"),
            );
        }

        assert!(
            matches!(
                store.open(alice(), Expiry::WhileTheProcessLives),
                Err(SessionError::TooManyLive { .. })
            ),
            "a full store must refuse",
        );
        assert!(
            live.iter()
                .all(|token| store.resolve(token.as_str()).is_some()),
            "and must not have evicted anybody to make room",
        );

        // Closing one makes room again, so the bound is a bound and not a one-way door.
        store.close(live[0].as_str());
        assert!(store.open(alice(), Expiry::WhileTheProcessLives).is_ok());
    }

    /// A session nobody can use does not hold a place against the bound.
    ///
    /// The other half of the refusal above. If expiry only made an entry unresolvable, a store
    /// filled with sessions that ended hours ago would refuse every honest sign-in until the
    /// process restarted — the store would be enforcing its bound on behalf of nobody.
    #[test]
    fn an_expired_session_does_not_consume_the_bound() {
        let store = SessionStore::new();

        let filled: Vec<SessionToken> = (0..MAX_LIVE_SESSIONS)
            .map(|_| {
                store
                    .open(alice(), in_five_minutes())
                    .expect("the store is not yet full")
            })
            .collect();

        assert!(
            matches!(
                store.open(alice(), in_five_minutes()),
                Err(SessionError::TooManyLive { .. })
            ),
            "while they are all live, the bound holds",
        );

        for token in &filled {
            store.expire_now(token.as_str());
        }

        assert!(
            store.open(alice(), in_five_minutes()).is_ok(),
            "a store full of expired sessions must admit an honest caller",
        );
        assert_eq!(
            store.live().len(),
            1,
            "and the sessions that ended are gone rather than merely unresolvable",
        );
    }

    /// A bearer credential that prints itself is a bearer credential in the logs.
    #[test]
    fn a_token_redacts_itself() {
        let token = SessionStore::new()
            .open(alice(), Expiry::WhileTheProcessLives)
            .expect("the OS has randomness");

        let printed = format!("{token:?}");
        assert!(!printed.contains(token.as_str()), "{printed}");
        assert_eq!(printed, "SessionToken(redacted)");
    }

    /// The two cookies this host plants, written out in full.
    ///
    /// Pinned as literals rather than rebuilt from [`host_cookie`], which would assert only that the
    /// function equals itself. These strings are what a browser enforces the `__Host-` prefix
    /// against, so a change to any of them should have to be typed here deliberately.
    #[test]
    fn every_planted_cookie_carries_the_host_prefix_attributes() {
        let token = SessionToken("abc123".to_string());

        assert_eq!(
            planted(&token),
            "__Host-flux_exchange_session=abc123; Path=/; Secure; HttpOnly; SameSite=Strict",
        );
        assert_eq!(
            cleared(),
            "__Host-flux_exchange_session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict",
        );

        // The shape the OIDC binder takes: the same `__Host-` attributes, a `Max-Age` this host
        // also enforces, and the one deliberate difference.
        assert_eq!(
            host_cookie("__Host-x", "v", SameSite::Lax, Some(600)),
            "__Host-x=v; Path=/; Max-Age=600; Secure; HttpOnly; SameSite=Lax",
        );
    }

    #[test]
    fn a_cookie_header_is_read_by_name() {
        assert_eq!(
            from_cookie_header(&format!("{SESSION_COOKIE}=abc123"), SESSION_COOKIE),
            Some("abc123"),
        );
        // The realistic shape: ours in the middle of somebody else's.
        assert_eq!(
            from_cookie_header(
                &format!("theme=dark; {SESSION_COOKIE}=abc123; locale=en"),
                SESSION_COOKIE,
            ),
            Some("abc123"),
        );
        assert_eq!(from_cookie_header("theme=dark", SESSION_COOKIE), None);
        assert_eq!(from_cookie_header("", SESSION_COOKIE), None);
        // A name that merely contains ours must not match it.
        assert_eq!(
            from_cookie_header(&format!("not_{SESSION_COOKIE}=abc123"), SESSION_COOKIE),
            None,
        );
    }
}
