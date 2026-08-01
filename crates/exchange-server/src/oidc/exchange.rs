//! The half of the sign-in flow that leaves this process, as a port.
//!
//! # Why this is a trait and not an implementation
//!
//! Redeeming an authorization code means an HTTPS request to the provider's token endpoint, and
//! verifying the id token that comes back means fetching the provider's JWKS and checking an RSA or
//! ECDSA signature. The port stays a port even now that
//! [`HttpTokenExchange`](super::http_exchange::HttpTokenExchange) binds it, because the seam is what
//! lets every test in [`super`] and in `routes::signin` drive the flow without a provider.
//!
//! What must not happen is the obvious workaround. A hand-written JWT verifier is the single most
//! reliably broken piece of code in this problem space — `alg: none`, HMAC-versus-RSA confusion, key
//! selection by attacker-supplied `kid`, unchecked `crit` — and every one of those bugs produces a
//! host that accepts a token anybody can mint while looking exactly like a host that does not.
//! Nothing here hand-rolls one: the binding implementation delegates to `jsonwebtoken`, and the two
//! attacks in that list that a *caller* of a JOSE library can still get wrong — algorithm confusion
//! and `kid` selection — are answered in that module and tested there.
//!
//! # What an implementor owes
//!
//! [`TokenExchange::redeem`] must return only claims whose **signature it verified**, against a key
//! it obtained from the issuer over a trusted channel. Everything after that — that the issuer is
//! the configured one, that the audience is us, that the token has not expired, that the nonce is
//! the one we bound — is checked here in [`super`] and must not be assumed on either side.
//!
//! The split is deliberate: signature verification needs cryptography and the network, and the
//! binding checks need neither. Keeping the binding checks in this crate means they are tested
//! here, once, rather than re-implemented correctly-or-not by every composition.

use exchange_host::async_trait;

use super::config::ClientSecret;
use super::pkce::Verifier;

/// Redeem an authorization code for the identity it stands for.
#[async_trait]
pub trait TokenExchange: Send + Sync {
    /// Exchange `redemption.code` at the provider's token endpoint and verify the id token's
    /// signature.
    ///
    /// The returned claims must be from a token whose signature verified. Returning unverified
    /// claims defeats every check this host makes afterwards, because those checks read the claims.
    async fn redeem(&self, redemption: Redemption<'_>) -> Result<SignedClaims, ExchangeError>;
}

/// One authorization code, and what it takes to spend it.
///
/// Borrowed rather than owned so the secret and the verifier are not copied on the way to the
/// implementor; both are credentials and both redact when printed.
///
/// Every field is read by [`HttpTokenExchange`](super::http_exchange::HttpTokenExchange) on the way
/// to the token endpoint, and driven through a stub by the tests in `routes::signin`.
#[derive(Debug)]
pub struct Redemption<'a> {
    /// The authorization code the provider returned through the browser.
    pub code: &'a str,
    /// The PKCE code verifier bound to the authorization request that produced `code`.
    pub verifier: &'a Verifier,
    /// The redirect URI the authorization request carried, which the token endpoint re-checks.
    pub redirect_uri: &'a str,
    /// This host's client identifier.
    pub client_id: &'a str,
    /// This host's client secret.
    pub client_secret: &'a ClientSecret,
}

/// The claims of an id token whose signature an implementor has verified.
///
/// A flattened view rather than the raw JWT: this crate never parses a token, so there is nothing
/// here an implementor did not choose to hand over, and no place for an unvalidated header to hide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedClaims {
    /// The `iss` claim.
    pub issuer: String,
    /// The `aud` claim, always as a list — it is a string *or* a list in the wire format, and a
    /// type that models only the common case is one that fails at a provider rather than at a test.
    pub audience: Vec<String>,
    /// The `sub` claim: the provider's stable, immutable identifier for the human.
    pub subject: String,
    /// The `nonce` claim, if the token carried one. Optional in the type because a provider may
    /// omit it; **not** optional in the check, which is in [`super::Oidc::admit`].
    pub nonce: Option<String>,
    /// The `exp` claim, as seconds since the Unix epoch.
    pub expires_at: i64,
    /// The `email` claim, if the provider released one. Never used to identify the caller — see
    /// [`super::Oidc::admit`].
    pub email: Option<String>,
}

/// Why an authorization code could not be redeemed.
///
/// The two variants are the same split as [`IdentityError`](exchange_host::IdentityError), for the
/// same reason: an operator answers a rejected code and an unreachable provider in opposite ways,
/// and a caller told "your login is broken" for an outage goes and resets a password that was fine.
/// Constructed by [`TokenExchange`] implementors — in this binary,
/// [`HttpTokenExchange`](super::http_exchange::HttpTokenExchange).
#[derive(Debug)]
pub enum ExchangeError {
    /// The provider refused the code, or the token it returned did not verify.
    ///
    /// Carries **no detail**, deliberately. Anything the provider said about why is about a
    /// credential, and the caller is the last party that should be told which half of it was wrong.
    Rejected,

    /// The provider could not be reached.
    ///
    /// The reason names this host's own dependencies — an address, a TLS failure, a DNS name — so
    /// it goes to the log and never to the caller. `routes::signin` is what enforces that, and
    /// `a_refusal_tells_the_caller_nothing_about_the_provider` is what keeps it true.
    Unreachable(String),
}

impl std::fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected => f.write_str("the provider refused the authorization code"),
            Self::Unreachable(reason) => write!(f, "the provider could not be reached: {reason}"),
        }
    }
}

impl std::error::Error for ExchangeError {}
