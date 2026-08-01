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
use std::fs::File;
use std::io::Read;
use std::sync::Mutex;

use exchange_host::Principal;

/// The cookie a browser carries a session in.
///
/// `__Host-` prefixed: a browser refuses to accept such a cookie unless it is `Secure`, has
/// `Path=/` and carries no `Domain`. That makes the attributes below enforced by the client too,
/// rather than only by the test that asserts we wrote them — and it stops a sibling subdomain from
/// planting a session for this host.
pub const SESSION_COOKIE: &str = "__Host-flux_exchange_session";

/// How many bytes of entropy a session token carries. 256 bits, from the OS.
const TOKEN_BYTES: usize = 32;

/// Where the token's entropy comes from.
const ENTROPY_SOURCE: &str = "/dev/urandom";

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
        let token = mint()?;
        self.live().insert(token.clone(), principal);
        Ok(token)
    }

    /// The principal a presented token names, if it names one.
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
fn mint() -> Result<SessionToken, SessionError> {
    let mut bytes = [0_u8; TOKEN_BYTES];

    // Refuse; never repair. There is no weaker fallback worth having: a session token from a
    // predictable source is a session anyone can guess, and a host that quietly downgraded to one
    // would look exactly like a host that had not.
    File::open(ENTROPY_SOURCE)
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|source| SessionError::NoEntropy { source })?;

    // Hex rather than base64: a lookup table of two lines instead of one of sixty-four, and a
    // session token is not long enough for the density to be worth anything.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        token.push(HEX[usize::from(byte >> 4)] as char);
        token.push(HEX[usize::from(byte & 0x0f)] as char);
    }

    Ok(SessionToken(token))
}

/// Why a session could not be opened.
#[derive(Debug)]
pub enum SessionError {
    /// The operating system's randomness was unavailable.
    NoEntropy {
        /// What went wrong reading it.
        source: std::io::Error,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEntropy { source } => write!(
                f,
                "cannot mint a session token: {ENTROPY_SOURCE} is unreadable ({source}). Refusing \
                 rather than falling back to a predictable token",
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } => Some(source),
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
/// - **`HttpOnly`** — script cannot read it, so an XSS in the console cannot exfiltrate a session.
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
