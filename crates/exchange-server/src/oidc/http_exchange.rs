//! The binding for [`TokenExchange`](super::exchange::TokenExchange): redeem the code over HTTP,
//! and verify the id token against the provider's published keys.
//!
//! # What this module is responsible for, and what it deliberately is not
//!
//! Two things, and only two: **spend the authorization code** at the token endpoint, and **prove the
//! id token was signed by the provider**. Everything a claim *says* — `iss`, `aud`, `exp`, `sub`,
//! `nonce` — is admitted by [`Oidc::admit`](super::Oidc::admit) and not here.
//!
//! That split is not tidiness. `admit` is where those checks are tested, once, and a second
//! implementation of them here would be a second thing to keep correct and a place for the two to
//! disagree. A signature check answers "did the provider mint this?"; `admit` answers "is what it
//! says acceptable to us?". This module is not entitled to the second question, so it is configured
//! not to ask it — see [`verification`].
//!
//! # Why the algorithm allowlist is derived from the key and not from the token
//!
//! The header of an unverified token is attacker-controlled. Two classic forgeries live there:
//!
//! - `alg: none`, where the signature is empty and a naive verifier accepts anything.
//! - **Algorithm confusion**: the token says `alg: HS256`, so a verifier that trusts the header
//!   treats the provider's *public* RSA key as an HMAC secret. The public key is public, so the
//!   attacker can compute that MAC and mint any claims they like.
//!
//! Neither is possible here, because the permitted algorithms come from the **JWK's key type**, not
//! from the token. An RSA key admits only the RSA families; nothing in this module can ever name a
//! symmetric algorithm, so a token claiming one has no key to verify against and is rejected before
//! any signature is computed. `a_token_signed_with_the_public_key_as_an_hmac_secret_is_refused`
//! is what holds that.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use exchange_host::async_trait;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

use super::config::OidcConfig;
use super::exchange::{ExchangeError, Redemption, SignedClaims, TokenExchange};

/// How long a fetched key set is reused before it is fetched again.
///
/// Providers rotate signing keys, and publish the new key some time before they sign with it. Five
/// minutes is short enough that a rotation is picked up without an operator doing anything, and long
/// enough that a busy host is not fetching the key set on every sign-in.
const JWKS_TTL: Duration = Duration::from_secs(300);

/// The shortest interval between two key-set fetches provoked by an unknown `kid`.
///
/// A `kid` this host has never seen is the signal that a rotation happened early, and refetching is
/// the right answer. It is also what an attacker sends to make this host hammer the provider, so the
/// refetch is rate-limited: past this floor a stranger's `kid` costs one cache read and nothing on
/// the network. Ten seconds recovers a real rotation promptly and makes the amplification useless.
const UNKNOWN_KID_REFETCH_FLOOR: Duration = Duration::from_secs(10);

/// How long the whole token-endpoint round trip may take.
///
/// A sign-in is a human waiting on a page. Without a deadline a hung provider holds the request —
/// and the browser — until something else gives up first, and "the login page hangs" is a much worse
/// failure to diagnose than "the provider could not be reached".
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Redeems authorization codes at a real provider over HTTPS.
pub struct HttpTokenExchange {
    http: reqwest::Client,
    token_endpoint: String,
    jwks_uri: String,
    keys: Mutex<Option<CachedKeys>>,
}

/// A key set and when it was fetched.
struct CachedKeys {
    keys: JwkSet,
    fetched: Instant,
}

impl HttpTokenExchange {
    /// Build an exchange for `config`'s provider.
    ///
    /// `Err` carries the reason the HTTP client could not be built — a missing TLS backend, most
    /// plausibly. It is a startup failure and never reaches a caller.
    pub fn new(config: &OidcConfig) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            // This host talks to one provider over the back channel and follows nothing it is told
            // to follow. A redirect from the token endpoint would be the provider handing us a new
            // address for a request carrying our client secret, which is not a thing to obey.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| format!("the HTTP client could not be built: {source}"))?;

        Ok(Self {
            http,
            token_endpoint: config.token_endpoint().to_string(),
            jwks_uri: config.jwks_uri().to_string(),
            keys: Mutex::new(None),
        })
    }

    /// The cached key set, if it is still fresh.
    fn cached(&self) -> Option<JwkSet> {
        let cache = self.keys.lock().unwrap_or_else(|poisoned| {
            // A panic in another thread while it held this lock says nothing about the key set,
            // which is a plain value. Failing sign-in for the life of the process because of an
            // unrelated panic would be the worse answer.
            self.keys.clear_poison();
            poisoned.into_inner()
        });

        cache
            .as_ref()
            .filter(|cached| cached.fetched.elapsed() < JWKS_TTL)
            .map(|cached| cached.keys.clone())
    }

    /// How long ago the key set was last fetched, if ever.
    fn cache_age(&self) -> Option<Duration> {
        let cache = self.keys.lock().unwrap_or_else(|poisoned| {
            self.keys.clear_poison();
            poisoned.into_inner()
        });

        cache.as_ref().map(|cached| cached.fetched.elapsed())
    }

    /// Fetch the key set and remember it.
    async fn fetch_keys(&self) -> Result<JwkSet, ExchangeError> {
        let response = self
            .http
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|source| ExchangeError::Unreachable(format!("{source}")))?;

        if !response.status().is_success() {
            return Err(ExchangeError::Unreachable(format!(
                "the key set at {} answered {}",
                self.jwks_uri,
                response.status(),
            )));
        }

        let keys: JwkSet = response
            .json()
            .await
            .map_err(|source| ExchangeError::Unreachable(format!("unreadable key set: {source}")))?;

        let mut cache = self.keys.lock().unwrap_or_else(|poisoned| {
            self.keys.clear_poison();
            poisoned.into_inner()
        });

        *cache = Some(CachedKeys {
            keys: keys.clone(),
            fetched: Instant::now(),
        });

        Ok(keys)
    }

    /// The key `kid` names, fetching the key set if it is stale or does not have it.
    async fn key_for(&self, kid: Option<&str>) -> Result<Jwk, ExchangeError> {
        let find = |keys: &JwkSet| match kid {
            Some(kid) => keys.find(kid).cloned(),
            // A provider publishing exactly one key may omit `kid` from the token. More than one and
            // there is nothing to choose by, and guessing would mean trying keys until one verified
            // — which is how a verifier ends up accepting a token signed by a key that was never
            // meant for this.
            None => match keys.keys.as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            },
        };

        if let Some(keys) = self.cached() {
            if let Some(key) = find(&keys) {
                return Ok(key);
            }
        }

        // Either the cache is stale, or it is fresh and does not have this `kid`. The second is a
        // rotation the provider made early — worth one fetch, but not one per hostile request.
        if self.cached().is_some()
            && self
                .cache_age()
                .is_some_and(|age| age < UNKNOWN_KID_REFETCH_FLOOR)
        {
            return Err(ExchangeError::Rejected);
        }

        find(&self.fetch_keys().await?).ok_or(ExchangeError::Rejected)
    }
}

#[async_trait]
impl TokenExchange for HttpTokenExchange {
    async fn redeem(&self, redemption: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
        // `client_secret_basic`: OpenID Connect Core §9 makes it the default, and every provider
        // must accept it. The secret goes in the Authorization header rather than the form body so
        // it stays out of any place that logs a request body.
        let response = self
            .http
            .post(&self.token_endpoint)
            .basic_auth(
                redemption.client_id,
                Some(redemption.client_secret.expose()),
            )
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", redemption.code),
                ("redirect_uri", redemption.redirect_uri),
                ("client_id", redemption.client_id),
                ("code_verifier", redemption.verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|source| ExchangeError::Unreachable(format!("{source}")))?;

        let status = response.status();

        // The provider refusing the code is `Rejected`; the provider being broken is `Unreachable`.
        // A 5xx is not the caller's credential being wrong, and telling an operator "your login was
        // refused" when the IdP is down sends them to reset a password that was fine.
        if status.is_server_error() {
            return Err(ExchangeError::Unreachable(format!(
                "the token endpoint answered {status}",
            )));
        }

        // Read the body before branching on status: a provider signalling `invalid_grant` in a 400
        // and one signalling it in a 200 are the same fact, and both are `Rejected`.
        let body: TokenResponse = response.json().await.map_err(|source| {
            if status.is_success() {
                ExchangeError::Unreachable(format!("unreadable token response: {source}"))
            } else {
                // A non-success with an unparseable body is the provider saying no in a shape this
                // host does not model. Still a refusal, and still nothing to tell the caller.
                ExchangeError::Rejected
            }
        })?;

        let Some(id_token) = body.id_token.filter(|_| status.is_success()) else {
            // No id token means no identity, whatever else came back. An access token without one
            // is a successful OAuth exchange and a failed OIDC sign-in.
            return Err(ExchangeError::Rejected);
        };

        self.verify(&id_token).await
    }
}

impl HttpTokenExchange {
    /// Verify `id_token`'s signature and hand back what it claims.
    async fn verify(&self, id_token: &str) -> Result<SignedClaims, ExchangeError> {
        // The header is not trusted for *what* to do — only for which key to look up. See the
        // module documentation on algorithm confusion.
        let header = decode_header(id_token).map_err(|_| ExchangeError::Rejected)?;
        let key = self.key_for(header.kid.as_deref()).await?;

        let permitted = permitted_algorithms(&key).ok_or(ExchangeError::Rejected)?;
        let decoding = DecodingKey::from_jwk(&key).map_err(|_| ExchangeError::Rejected)?;

        let claims = decode::<IdTokenClaims>(id_token, &decoding, &verification(permitted))
            .map_err(|_| ExchangeError::Rejected)?
            .claims;

        Ok(SignedClaims {
            issuer: claims.iss,
            audience: claims.aud.into_vec(),
            subject: claims.sub,
            nonce: claims.nonce,
            expires_at: claims.exp,
            email: claims.email,
        })
    }
}

/// The algorithms a key of this kind may have signed with.
///
/// `None` for a symmetric key, which is not a thing a provider publishes for id-token verification
/// and is exactly the shape the confusion attack needs. Returning `None` rather than an empty list
/// keeps "no algorithm is acceptable" from being spelled the same way as "any of these are".
fn permitted_algorithms(key: &Jwk) -> Option<Vec<Algorithm>> {
    match &key.algorithm {
        AlgorithmParameters::RSA(_) => Some(vec![
            Algorithm::RS256,
            Algorithm::RS384,
            Algorithm::RS512,
            Algorithm::PS256,
            Algorithm::PS384,
            Algorithm::PS512,
        ]),
        AlgorithmParameters::EllipticCurve(_) => Some(vec![Algorithm::ES256, Algorithm::ES384]),
        AlgorithmParameters::OctetKeyPair(_) => Some(vec![Algorithm::EdDSA]),
        // Symmetric. See the module documentation.
        AlgorithmParameters::OctetKey(_) => None,
    }
}

/// Verify the signature, and *only* the signature.
///
/// Every claim check is off, because [`Oidc::admit`](super::Oidc::admit) owns them — including
/// `exp`, so an expired token reaches `admit` and is refused as `Expired` rather than collapsing
/// into the generic rejection here. An operator reading a log needs "the token was too old" to be
/// distinguishable from "the provider refused the code", and enabling a duplicate `exp` check here
/// would take that distinction away for no additional safety.
fn verification(permitted: Vec<Algorithm>) -> Validation {
    let mut validation = Validation::new(permitted[0]);

    validation.algorithms = permitted;
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    // `decode` requires every claim named here to be present. The default names `exp`, which is
    // checked by `admit`; leaving it required would refuse a token before `admit` could say why.
    validation.required_spec_claims.clear();

    validation
}

/// The token response, as much of it as this host reads.
#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
}

/// The id-token claims this host carries forward.
#[derive(Deserialize)]
struct IdTokenClaims {
    iss: String,
    aud: Audience,
    sub: String,
    exp: i64,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// `aud` is a string **or** a list of strings in the wire format.
///
/// Modelled as both rather than as the common case: a host that only accepts the string spelling
/// fails at whichever provider first sends the list, which is a failure at a deployment rather than
/// at a test.
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(one) => vec![one],
            Self::Many(many) => many,
        }
    }
}
