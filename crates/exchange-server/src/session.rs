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
const MAX_LIVE_SESSIONS: usize = 4096;

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

/// The live sessions of one identity port.
///
/// # What this deliberately does not do
///
/// There is **no expiry**. A session lives until it is closed or until the process does, and the
/// cookie below is a session cookie for the same reason: an expiry the browser honours but the
/// server does not is a lie that reads as a security control. Binding a session's lifetime to the
/// credential that opened it is X-04's job, where there is an id token with an `exp` to bind to.
#[derive(Default)]
pub struct SessionStore {
    live: Mutex<HashMap<SessionToken, Principal>>,
}

impl SessionStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session for a principal that has already been resolved, and return its token.
    pub fn open(&self, principal: Principal) -> Result<SessionToken, SessionError> {
        let mut live = self.live();

        // Refuse; never repair. Nothing here expires, so without a bound this map only grows.
        if live.len() >= MAX_LIVE_SESSIONS {
            return Err(SessionError::TooManyLive {
                max: MAX_LIVE_SESSIONS,
            });
        }

        let token = mint()?;
        live.insert(token.clone(), principal);
        Ok(token)
    }

    /// The principal a presented token names, if it names one.
    ///
    /// A hash lookup, which is **not** constant time — a deliberate choice rather than an
    /// oversight. Timing tells an attacker something only if it narrows a search, and there is
    /// nothing here to narrow: a token is 256 bits from the OS, so there is no prefix to walk and
    /// no shorter guess to confirm. The comparison would be worth making constant-time if tokens
    /// were ever derived from something guessable, which is the thing to check before changing how
    /// they are minted.
    pub fn resolve(&self, presented: &str) -> Option<Principal> {
        self.live()
            .get(&SessionToken(presented.to_string()))
            .cloned()
    }

    /// Close a session. Closing one that was never open is not an error — the caller's intent is
    /// "I am not signed in", and that is satisfied either way.
    pub fn close(&self, presented: &str) {
        self.live().remove(&SessionToken(presented.to_string()));
    }

    /// The live sessions.
    ///
    /// Recovers from a poisoned lock rather than propagating it. The guarded value is a plain map
    /// with no cross-key invariant, so a panic while holding the lock cannot have left it
    /// half-updated — and refusing every subsequent request because an unrelated handler panicked
    /// would turn one failure into an outage.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<SessionToken, Principal>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
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
                "cannot mint a session: this store already holds its maximum of {max}. Nothing \
                 here expires, so either sessions are not being closed or something is opening \
                 them in a loop",
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } => Some(source),
            Self::TooManyLive { .. } => None,
        }
    }
}

/// The `Set-Cookie` value that plants a session in a browser.
///
/// The three attributes the Acceptance names, and why each is here:
///
/// - **`Secure`** — the session never travels in clear text. Browsers treat `http://localhost` as
///   a secure context, so this is compatible with the loopback development bind rather than in
///   tension with it.
/// - **`HttpOnly`** — script cannot read it. On its own that attribute buys less than it appears
///   to: same-origin `fetch` still sends the cookie ambiently, so script can *use* a session it
///   cannot read, and a route that handed such a caller a readable token would give the exfiltrable
///   credential straight back. What makes this claim true is therefore not the attribute alone but
///   `routes::identity::sign_in`, which mints nothing for a cookie-carried caller — see the
///   invariant stated there, and `a_cookie_session_cannot_be_exchanged_for_a_readable_token`.
/// - **`SameSite=Strict`** — the cookie is not sent on any cross-site request, which is what stops
///   another origin from spending it. `Strict` and not `Lax` because this surface has no
///   cross-site entry flow to preserve; X-04's OIDC redirect is where that question gets asked.
pub fn planted(token: &SessionToken) -> String {
    let token = token.as_str();
    format!("{SESSION_COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Strict")
}

/// The `Set-Cookie` value that clears a session from a browser.
///
/// Carries the same attributes as [`planted`]: a browser matches a replacement cookie on name,
/// path and domain, so a clearing cookie that dropped them would plant a second one instead of
/// overwriting the first.
pub fn cleared() -> String {
    format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict")
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

    /// A session resolves to the principal it was opened for, and stops resolving once closed.
    #[test]
    fn a_session_resolves_until_it_is_closed() {
        let store = SessionStore::new();
        let token = store.open(alice()).expect("the OS has randomness");

        assert_eq!(store.resolve(token.as_str()), Some(alice()));

        store.close(token.as_str());
        assert_eq!(store.resolve(token.as_str()), None);
    }

    /// Two sessions must never collide, or one caller would resolve as another.
    #[test]
    fn every_minted_token_is_distinct() {
        let store = SessionStore::new();
        let tokens: std::collections::BTreeSet<_> = (0..64)
            .map(|_| {
                store
                    .open(alice())
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

    /// Nothing here expires, so the store is bounded instead — and it refuses at the bound rather
    /// than evicting somebody to make room, which would sign out a caller who did nothing wrong.
    #[test]
    fn a_full_store_refuses_rather_than_evicting() {
        let store = SessionStore::new();
        let mut live = Vec::with_capacity(MAX_LIVE_SESSIONS);

        for _ in 0..MAX_LIVE_SESSIONS {
            live.push(store.open(alice()).expect("the store is not yet full"));
        }

        assert!(
            matches!(store.open(alice()), Err(SessionError::TooManyLive { .. })),
            "a full store must refuse",
        );
        assert!(
            live.iter()
                .all(|token| store.resolve(token.as_str()).is_some()),
            "and must not have evicted anybody to make room",
        );

        // Closing one makes room again, so the bound is a bound and not a one-way door.
        store.close(live[0].as_str());
        assert!(store.open(alice()).is_ok());
    }

    /// A bearer credential that prints itself is a bearer credential in the logs.
    #[test]
    fn a_token_redacts_itself() {
        let token = SessionStore::new()
            .open(alice())
            .expect("the OS has randomness");

        let printed = format!("{token:?}");
        assert!(!printed.contains(token.as_str()), "{printed}");
        assert_eq!(printed, "SessionToken(redacted)");
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
