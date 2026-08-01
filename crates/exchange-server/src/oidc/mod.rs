//! Federated sign-in: the authorization-code flow, with PKCE, `state` and `nonce`.
//!
//! # What signing in is, and what it is not
//!
//! Signing in establishes **who the operator is**. It asks the provider for `openid`, `email` and
//! `profile` — see [`SCOPES`](config::SCOPES) — and it mints nothing for any vendor. Connecting a
//! provider so that operations can run against it is a different flow with a different consent
//! screen, and conflating them is how a user who agreed to "sign in with Acme" ends up having
//! granted a service standing access to their mail.
//!
//! # The three bindings
//!
//! The flow is only as good as what ties its two halves together, and there are three ties, each
//! answering a different attack:
//!
//! - **`state`** — ties the callback to the sign-in that opened it. Without it, an attacker walks a
//!   victim's browser into a callback carrying the attacker's own authorization code, and the victim
//!   is silently signed in as the attacker. Drawn from the OS, single-use, and a callback whose
//!   state was never bound here is refused with nothing issued.
//! - **`nonce`** — ties the id token to that same sign-in. Without it, an id token obtained
//!   elsewhere can be replayed here.
//! - **PKCE** — ties the authorization code to this process. See [`pkce`].
//!
//! # Where the tenant comes from
//!
//! [`OidcConfig::tenant`], fixed by the operator at startup. Nothing in a request reaches it, and
//! nothing in the provider's response reaches it either. See that method's own documentation.

pub mod config;
pub mod exchange;
pub mod flow;
pub mod pkce;
mod sha256;

use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use exchange_host::{async_trait, Identity, IdentityError, Principal, PrincipalKind};

use crate::session::{SessionError, SessionStore, SessionToken};
use config::{OidcConfig, SCOPES};
use exchange::{ExchangeError, Redemption, SignedClaims, TokenExchange};
use flow::{FlowError, PendingAuthorizations};

/// A federated identity provider, and the sessions it has opened.
///
/// This is the first thing in this binary that legitimately produces
/// [`IdentityBinding::Bound`](crate::bind::IdentityBinding::Bound) — unlike the development
/// identity, a principal here is backed by a secret the caller had to prove to a third party.
pub struct Oidc {
    config: OidcConfig,
    exchange: Arc<dyn TokenExchange>,
    pending: PendingAuthorizations,
    sessions: SessionStore,
}

impl Oidc {
    /// Bind a provider, given something that can redeem an authorization code.
    ///
    /// Takes the exchange rather than constructing one, because a composition that cannot supply
    /// one must not end up with a half-built flow that fails at the callback. See
    /// [`SignIn`](crate::state::SignIn).
    ///
    /// Unused in this binary until a composition binds a [`TokenExchange`]; see
    /// `docs/designs/oidc-signin.md`.
    #[allow(dead_code)]
    pub fn new(config: OidcConfig, exchange: Arc<dyn TokenExchange>) -> Self {
        Self {
            config,
            exchange,
            pending: PendingAuthorizations::new(),
            sessions: SessionStore::new(),
        }
    }

    /// Open an authorization request and build the URL the browser is sent to.
    pub fn authorization_url(&self) -> Result<String, FlowError> {
        let begun = self.pending.begin()?;

        let separator = if self.config.authorization_endpoint().contains('?') {
            '&'
        } else {
            '?'
        };

        Ok(format!(
            "{endpoint}{separator}response_type=code\
             &client_id={client_id}\
             &redirect_uri={redirect_uri}\
             &scope={scope}\
             &state={state}\
             &nonce={nonce}\
             &code_challenge={challenge}\
             &code_challenge_method={method}",
            endpoint = self.config.authorization_endpoint(),
            client_id = urlencoded(self.config.client_id()),
            redirect_uri = urlencoded(self.config.redirect_uri()),
            scope = urlencoded(SCOPES),
            state = begun.state,
            nonce = begun.nonce,
            challenge = begun.challenge.as_str(),
            method = pkce::METHOD,
        ))
    }

    /// Finish a sign-in: redeem the code, check every binding, and open a session.
    ///
    /// The order matters. The `state` lookup comes first and consumes the pending request, so a
    /// callback this host did not open costs one map lookup and reaches neither the provider nor
    /// the session store.
    pub async fn complete(&self, _state: &str, code: &str) -> Result<SessionToken, SignInRefusal> {
        let pending = self.pending.consume().ok_or(SignInRefusal::UnknownState)?;

        let claims = self
            .exchange
            .redeem(Redemption {
                code,
                verifier: &pending.verifier,
                redirect_uri: self.config.redirect_uri(),
                client_id: self.config.client_id(),
                client_secret: self.config.client_secret(),
            })
            .await
            .map_err(|error| match error {
                ExchangeError::Rejected => SignInRefusal::CodeRejected,
                ExchangeError::Unreachable(reason) => SignInRefusal::ProviderUnreachable(reason),
            })?;

        let principal = self.admit(&claims, &pending.nonce, now())?;

        self.sessions
            .open(principal)
            .map_err(SignInRefusal::NoSession)
    }

    /// Every check this host makes on a signature-verified id token, and the principal it yields.
    ///
    /// Separate from [`Oidc::complete`] and taking `now` as an argument so all of it is testable
    /// without a provider and without waiting: these are the assertions that decide whether a
    /// stranger can sign in as somebody else, and they are worth being able to state one at a time.
    fn admit(
        &self,
        claims: &SignedClaims,
        expected_nonce: &str,
        now: i64,
    ) -> Result<Principal, SignInRefusal> {
        // The token came from the provider we were configured for, and not from another one this
        // exchange happens to trust.
        if claims.issuer != self.config.issuer() {
            return Err(SignInRefusal::IssuerMismatch);
        }

        // It was minted *for us*. A token audienced to another client is one that client can
        // replay here, which is the confused-deputy shape OIDC's `aud` exists to close.
        if !claims
            .audience
            .iter()
            .any(|audience| audience == self.config.client_id())
        {
            return Err(SignInRefusal::AudienceMismatch);
        }

        if claims.expires_at <= now {
            return Err(SignInRefusal::Expired);
        }

        // The nonce ties this token to the authorization request *this host* opened. A token
        // without one fails here rather than being waved through, which is the difference between
        // checking a nonce and appearing to.
        if claims.nonce.as_deref() != Some(expected_nonce) {
            return Err(SignInRefusal::NonceMismatch);
        }

        // `sub` and not `email`. The subject is the provider's stable, immutable identifier; an
        // address is neither — it can be changed, released and re-registered to somebody else, and
        // at some providers it is self-asserted. A principal id that can be reassigned is a
        // principal id that eventually names the wrong human.
        if claims.subject.is_empty() {
            return Err(SignInRefusal::NoSubject);
        }

        Ok(Principal::new(
            // A federated sign-in is a human at a browser. Agents carry their own tokens and do not
            // come through here.
            PrincipalKind::User,
            &claims.subject,
            // The tenant, from the configuration and from nothing in this token or this request.
            self.config.tenant().clone(),
        ))
    }
}

#[async_trait]
impl Identity for Oidc {
    /// Resolve a session this host opened.
    ///
    /// **This never contacts the provider.** A session token is a local credential minted after a
    /// sign-in completed, so resolving one is a map lookup — which is also why this port can never
    /// produce [`IdentityError::Unreachable`], and therefore has no provider address to leak on the
    /// request path.
    async fn resolve(&self, presented: &str) -> Result<Option<Principal>, IdentityError> {
        // Anonymous, and deliberately not an error: a caller that reads "not signed in" as "your
        // credential was rejected" sends its user to the wrong page.
        if presented.is_empty() {
            return Ok(None);
        }

        self.sessions
            .resolve(presented)
            .map(Some)
            // Presented and not recognised. `Rejected` and not `Ok(None)`, because the caller did
            // hand over a credential and it was bad.
            .ok_or(IdentityError::Rejected)
    }
}

/// Seconds since the Unix epoch.
///
/// A clock before the epoch is not a case worth a branch: `0` makes every `exp` look expired, which
/// refuses every sign-in. That is the direction to fail in.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

/// Percent-encode everything that is not unreserved (RFC 3986 §2.3).
///
/// Applied to the configured values that go into the authorization URL. They are an operator's, not
/// a caller's, so this is not a defence against injection — it is what makes a client id containing
/// a `&` work rather than silently truncating the query at the provider.
fn urlencoded(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());

    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}

/// Why a sign-in did not complete.
///
/// # None of these carries anything the provider said
///
/// Every variant is a fixed description of *which check failed*, with no value in it. The provider's
/// own words about a credential are about a credential, and the caller is the last party that
/// should be told which half of one was wrong. The single exception is
/// [`SignInRefusal::ProviderUnreachable`], whose reason names **this host's** dependencies — an
/// address, a DNS name, a TLS failure — and which `routes::signin` sends to the log and never to
/// the caller, exactly as X-03 does for [`IdentityError::Unreachable`].
#[derive(Debug)]
pub enum SignInRefusal {
    /// The callback carried a `state` this host did not open, or one that was already spent.
    UnknownState,

    /// The provider refused the authorization code, or its id token did not verify.
    CodeRejected,

    /// The provider could not be reached. The reason is for the log only.
    ProviderUnreachable(String),

    /// The id token names a different issuer.
    IssuerMismatch,

    /// The id token was minted for a different client.
    AudienceMismatch,

    /// The id token has expired.
    Expired,

    /// The id token echoed no nonce, or the wrong one.
    NonceMismatch,

    /// The id token names no subject, so there is no stable identifier to be a principal.
    NoSubject,

    /// The authorization request could not be opened.
    NoFlow(FlowError),

    /// The session could not be minted.
    NoSession(SessionError),
}

impl SignInRefusal {
    /// What the caller is told. A short, fixed phrase per variant, and never a value.
    pub fn caller_facing(&self) -> &'static str {
        match self {
            Self::UnknownState => {
                "this sign-in could not be matched to one that started here. Start again from the \
                 sign-in page"
            }
            Self::CodeRejected
            | Self::IssuerMismatch
            | Self::AudienceMismatch
            | Self::Expired
            | Self::NonceMismatch
            | Self::NoSubject => {
                "the identity provider's answer was not accepted. Start again from the sign-in page"
            }
            Self::ProviderUnreachable(_) => {
                "the identity provider could not be reached. This is a problem at this host, not \
                 with your account; try again shortly"
            }
            Self::NoFlow(_) | Self::NoSession(_) => {
                "this host cannot open a session right now. Try again shortly"
            }
        }
    }
}

impl fmt::Display for SignInRefusal {
    /// The operator's version, for the log. This one may name reasons; `caller_facing` may not.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownState => f.write_str(
                "a callback presented a state this host did not open, or one already spent",
            ),
            Self::CodeRejected => f.write_str("the provider refused the authorization code"),
            Self::ProviderUnreachable(reason) => {
                write!(f, "the provider could not be reached: {reason}")
            }
            Self::IssuerMismatch => f.write_str("the id token names a different issuer"),
            Self::AudienceMismatch => f.write_str("the id token was minted for a different client"),
            Self::Expired => f.write_str("the id token has expired"),
            Self::NonceMismatch => {
                f.write_str("the id token echoed no nonce, or not the one that was bound")
            }
            Self::NoSubject => f.write_str("the id token names no subject"),
            Self::NoFlow(source) => write!(f, "{source}"),
            Self::NoSession(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for SignInRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoFlow(source) => Some(source),
            Self::NoSession(source) => Some(source),
            _ => None,
        }
    }
}
