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
//!
//! The same argument decides the key kinds this build has never heard of, and it decides them the
//! same way: a type [`permitted_algorithms`] does not implement admits **nothing**. An allowlist
//! derived from the key is only a guard while something stays outside it, so an unrecognised kind
//! refuses rather than falling back to whichever family its parameters happen to resemble.
//! `a_key_of_a_kind_this_build_does_not_recognise_admits_no_algorithm` is what holds that.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use exchange_host::async_trait;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
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
    keys: Mutex<KeyCache>,
    /// [`UNKNOWN_KID_REFETCH_FLOOR`], as a field so the tests can drive a rotation and the rate
    /// limit without sleeping through ten seconds of each. [`HttpTokenExchange::new`] is the only
    /// non-test constructor and it always uses the constant.
    refetch_floor: Duration,
}

/// The key set this host is holding, and when it last went out for one.
#[derive(Default)]
struct KeyCache {
    /// The last key set fetched, and when it arrived. `None` until the first fetch succeeds.
    fresh: Option<CachedKeys>,

    /// When a fetch was last **attempted**, whether or not it produced a key set.
    ///
    /// Separate from [`CachedKeys::fetched`], and this is the X-17 fix. The refetch floor exists so
    /// that an unknown `kid` cannot provoke one outbound request per callback. Read off the last
    /// *success*, it lapses entirely while the key set is unreachable — which is exactly when the
    /// amplification is cheapest for a caller and most expensive for the provider, and exactly when
    /// a limit is worth having. `an_unknown_kid_cannot_hammer_a_failing_key_set` holds this.
    attempted: Option<Instant>,
}

impl KeyCache {
    /// The key set, if one is held and is still inside [`JWKS_TTL`].
    ///
    /// The one place "still fresh" is spelled, because two callers ask it for opposite purposes —
    /// [`HttpTokenExchange::cached`] to answer from it, [`HttpTokenExchange::too_soon_to_refetch`]
    /// to decide *which failure* a rate-limited request is — and a host where those two disagreed
    /// would report an unreachable key set as a refused credential.
    fn current(&self) -> Option<&JwkSet> {
        self.fresh
            .as_ref()
            .filter(|cached| cached.fetched.elapsed() < JWKS_TTL)
            .map(|cached| &cached.keys)
    }
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
            keys: Mutex::new(KeyCache::default()),
            refetch_floor: UNKNOWN_KID_REFETCH_FLOOR,
        })
    }

    /// As [`HttpTokenExchange::new`], with the refetch floor named.
    ///
    /// `#[cfg(test)]`, so the shipped binary has exactly one floor and it is the constant. It
    /// exists because the rotation branch is only reachable *inside* the floor's window, and a test
    /// that proved it by sleeping through ten seconds would be a test nobody runs.
    #[cfg(test)]
    fn with_refetch_floor(config: &OidcConfig, refetch_floor: Duration) -> Result<Self, String> {
        Ok(Self {
            refetch_floor,
            ..Self::new(config)?
        })
    }

    /// The key cache, whatever another thread did while holding it.
    ///
    /// A panic in another thread while it held this lock says nothing about the key set, which is
    /// a plain value. Failing sign-in for the life of the process because of an unrelated panic
    /// would be the worse answer.
    fn cache(&self) -> std::sync::MutexGuard<'_, KeyCache> {
        self.keys.lock().unwrap_or_else(|poisoned| {
            self.keys.clear_poison();
            poisoned.into_inner()
        })
    }

    /// The cached key set, if it is still fresh.
    fn cached(&self) -> Option<JwkSet> {
        self.cache().current().cloned()
    }

    /// Why a fetch may not go out yet, or `None` when it may.
    ///
    /// The floor is a floor on **going out at all**, not on succeeding. That is the whole of the
    /// X-17 fix: the previous form asked whether a *successful* fetch was recent, so a provider
    /// whose key set was down had no floor at all and every arriving callback started its own
    /// request. Ten seconds is far inside [`JWKS_TTL`], so a rotation is still picked up promptly
    /// and a routine refresh is never delayed in any way an operator could measure.
    fn too_soon_to_refetch(&self) -> Option<ExchangeError> {
        let cache = self.cache();

        if !cache
            .attempted
            .is_some_and(|attempted| attempted.elapsed() < self.refetch_floor)
        {
            return None;
        }

        // Which refusal this is depends on what the last attempt left behind, and the two are
        // opposite events: `a_refused_grant_and_an_unreachable_provider_do_not_collapse` is the
        // standing argument that an outage must not read as a bad credential, and this branch is
        // where that could quietly stop being true.
        //
        // A **current** key set in hand means this `kid` is one the provider does not publish — a
        // stranger's, or a rotation that will be picked up the moment the floor lapses. Anything
        // else means the last attempt did not leave a usable key set behind, so this request needed
        // a fetch the floor is refusing, and blaming that on the caller's credential is the exact
        // mistake X-17 exists to undo.
        Some(match cache.current() {
            Some(_) => ExchangeError::UnpublishedKey,
            None => ExchangeError::Unreachable(format!(
                "the key set at {} could not be refreshed, and this host refetches it at most once \
                 every {}s",
                self.jwks_uri,
                self.refetch_floor.as_secs(),
            )),
        })
    }

    /// Fetch the key set and remember it.
    async fn fetch_keys(&self) -> Result<JwkSet, ExchangeError> {
        // Stamped **before** the request goes out, not after it comes back. A stamp written on the
        // way back leaves the whole round trip — up to [`HTTP_TIMEOUT`] of it — as a window in
        // which every arriving callback starts a fetch of its own.
        self.cache().attempted = Some(Instant::now());

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

        let keys: JwkSet = response.json().await.map_err(|source| {
            ExchangeError::Unreachable(format!("unreadable key set: {source}"))
        })?;

        self.cache().fresh = Some(CachedKeys {
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
        if let Some(refusal) = self.too_soon_to_refetch() {
            return Err(refusal);
        }

        find(&self.fetch_keys().await?).ok_or(ExchangeError::UnpublishedKey)
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
        let body: Option<TokenResponse> = match response.json().await {
            Ok(body) => Some(body),
            Err(source) if status.is_success() => {
                return Err(ExchangeError::Unreachable(format!(
                    "unreadable token response: {source}"
                )));
            }
            // A non-success with an unparseable body is the provider saying no in a shape this
            // host does not model. Which "no" it is may still be legible from the status alone,
            // which is why this falls through rather than refusing here.
            Err(_) => None,
        };

        // **The operator's failure, and the reason X-17 exists.** RFC 6749 §5.2: `invalid_client`
        // means the credential the token endpoint checked was *this host's*, and §5.2 also requires
        // a `401` specifically when the client authenticated through the `Authorization` header —
        // which `redeem` always does. So either spelling is the same fact, and both are read,
        // because providers are inconsistent about the status and about the body in turn.
        //
        // The caller's authorization code was not even reached. Reporting this as "the provider
        // refused the authorization code" is what sent operators to look at the one part of the
        // flow that was working.
        if status == reqwest::StatusCode::UNAUTHORIZED
            || body.as_ref().and_then(|body| body.error.as_deref()) == Some("invalid_client")
        {
            return Err(ExchangeError::ClientRefused);
        }

        if !status.is_success() {
            // Everything else the provider says no to, at this point, is about the code. This is
            // the one refusal here that is genuinely the caller's credential.
            return Err(ExchangeError::Rejected);
        }

        let Some(id_token) = body.and_then(|body| body.id_token) else {
            // No id token means no identity, whatever else came back. An access token without one
            // is a successful OAuth exchange and a failed OIDC sign-in — and a client registered
            // without `openid` is the usual cause, which is an operator's to fix.
            return Err(ExchangeError::NoIdToken);
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
            hosted_domain: claims.hd,
        })
    }
}

/// The algorithms a key of this kind may have signed with.
///
/// `None` for a symmetric key, which is not a thing a provider publishes for id-token verification
/// and is exactly the shape the confusion attack needs — and `None` for any kind this build does
/// not recognise, which is the wildcard arm's own argument. Returning `None` rather than an empty
/// list keeps "no algorithm is acceptable" from being spelled the same way as "any of these are".
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
        // Any kind this build does not recognise — an unknown `kty`, which `jsonwebtoken` 11 parses
        // into a catch-all instead of failing the key set, and any variant a later version adds to
        // this `#[non_exhaustive]` enum without a compile error to prompt a decision.
        //
        // It refuses for the same reason the arms above are derived from the key at all: an
        // allowlist guards nothing unless something is genuinely outside it, and this host cannot
        // say what a key kind it does not implement was meant to sign. Defaulting to the RSA
        // families is the tempting shape — a JWK naming an unrecognised kind often carries a
        // modulus and exponent anyway — and taking them is the confusion attack with the provider's
        // own label ignored rather than the token's header trusted. Refuse; never repair: an
        // upgrade that introduces a key kind turns that provider's sign-in off and says so, instead
        // of verifying against semantics nothing here has checked.
        // `a_key_of_a_kind_this_build_does_not_recognise_admits_no_algorithm` is what holds that.
        _ => None,
    }
}

/// Verify the signature, and *only* the signature.
///
/// The claim checks are off because [`Oidc::admit`](super::Oidc::admit) owns them — including
/// `exp`, so an expired token reaches `admit` and is refused as `Expired` rather than collapsing
/// into the generic rejection here. `nbf` is the exception: nothing checks it on either side, and
/// it is disabled explicitly rather than left to a default that could change under us. An operator reading a log needs "the token was too old" to be
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

    /// RFC 6749 §5.2's error code, read **only** to tell `invalid_client` from everything else.
    ///
    /// It is never carried anywhere. The provider's own words about a credential are about a
    /// credential, so this string reaches neither the caller nor the log; what leaves this module
    /// is a variant of [`ExchangeError`], chosen here and fixed at the type level.
    error: Option<String>,
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
    hd: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::SocketAddr;
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::Router;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use crate::oidc::config::{CLIENT_SECRET_ENV, JWKS_URI_ENV};
    use crate::oidc::pkce::{base64url, Verifier};
    use crate::oidc::{Oidc, SignInRefusal};

    /// The provider these tests stand in for, and this host's registration at it.
    const ISSUER: &str = "https://accounts.example.com";
    const CLIENT_ID: &str = "flux-exchange";
    const TENANT: &str = "acme";

    /// The `kid` the stub provider publishes. A token naming anything else is a stranger's.
    const KID: &str = "x04-test-key";

    /// A 2048-bit RSA keypair generated for these tests and used nowhere else.
    ///
    /// Embedded rather than generated per run for two reasons: the workspace carries no RSA
    /// key-generation crate, and a fixed key makes a failure reproducible rather than something that
    /// happened once on somebody's machine. It signs nothing outside this module and is worthless if
    /// it leaks — which is the only safe kind of private key to check in.
    const PRIVATE_KEY: &str = r"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCOSzjxbi1XQlL4
dvJPiFCOPd3w7esMZ5aragucD0P7hjQAaOD2m/pq8kPkJyUl0GaIMWWe0aeBu5OS
vX+d36erLkKP+WX+pJ4J/qD3qXGtzj6RQdjpg2bwas7JuTxUFLhbX1THjR3cU0Mt
zJW6O3SQdErewvTj/MMV6UGi6h6GvPnlDV/0rzudXsXL4vjfhK0SMuGtitHUvbHR
lCGZEvXG+859ZJ1hgyBkiNEDTO22opgNd7r5UrFfcyMn3BfdvqDF64FStJV0tlt7
J4tZxaen/3CEbPhZ/MeVe8Ivx+yDVbvSb5nvQthUsUZoZvh6CiQzsIpva4kRkWaL
UX9TF//fAgMBAAECggEAAm5u830mTC/ncAnERoSgmmrx4GczACbC+yctj0Xu1keV
muOFOIwzbAwQtJTQpy6XmesnfokRkNTDhvCIU4pvTdcABIAb9I0b4DWCyvM7wd8H
ev7s4GyXioh2A8S6Waty96i6L6C4f7vH0ZVbLO/4JBcV8mwoDWnpFsuXYgZtwsVF
r6NRdSjJh6oeYhuhIypKZTKpQDCVGk2aNe8Bc02oEOAJvEsfw6RBAF7+wJ4eXpKN
tf8nCk6u0PET1i9OE3H+yElsb6yZs2unG3twSyE7sGds+nA8rnJHwk9qkoOzXM/T
In6SuBypDgI6mJN0QfPKeaS+p8hG+WDLjBb9Ydrs2QKBgQDDVCnUjcBhZSI7gURH
4ChEQp6E3qRIzoTfOa691h5esGmi+ONc54V+rA7zT5RjhW0TKKTKPOtuaNqq8cAb
qhLhxp5QjB2PTu7pg45AgczBykBUCfHwQkLRXb174uSINayWpFj2VNkmcXqPhaxk
QbjiR3Oj9Y6T/xtm3CaBMHhoxwKBgQC6feniskdbDMR5K6QVcLpGtpKLoVT2BOEv
+G6xm3Vc8F6/HvXAUg6NzakyfSbYSyKh6k27LN1FG0XhHpGUxuBjOVH6dae9owMa
1nDEJCsBADOc1VcTNt0U+4q2fDkvYNveqJLMmHWLVDG8wnvnJiHWpUgl3qfJOHZS
FbBj8C8IKQKBgQCe5Vt19q5WTJAxefHSyo3XIZ6Ulg1s0NuUP/dfpMxl2PrGQdOr
YwfcyRkMY2NiJktZ94k+n5oh4hhoUWsm1g6wLgPhoGn3h42g1o0k+rJXvzDfbIut
GCoE6U3YdvXTvF4e2akpElLoDA5YrLRVhoVhRiDTc1G+IRvobBTCqWx6RwKBgHNh
Yan7CQDBFnGtWXhWZTlIzcQLzcfkXvpR5xKFjwgwQz5Vxk/1tMFxA4SUP8tEOSoa
D3uFl2ShKgvM4N8+aCebmCewUVaXm10oXV5MzjpxSH141MWzhPbtZfXfR3YTpBTP
EPv6O4c3UQpq/UOWqQrm+YtMhVyOTU4d0yMRv9d5AoGBAKEZXq+xvJ5EjA5D1OQ7
CqRIjIU4/SyJI/Lyd+qycLbEHmzOOWD8qUR6hUovXiV87VHgxTzJs3svs8eCgK8M
PN4IR7r37FnmBdDZ6ONCJr7u4KtrUM5ud8GlPAHID/+OPfZyM7E1eO1tT2/anu+u
ZADuam86Y2DQEywnzwYrY/F1
-----END PRIVATE KEY-----
";

    /// [`PRIVATE_KEY`]'s public half, exactly as a provider publishes it.
    ///
    /// The confusion attack's "secret": public by construction, which is the whole point of it.
    const PUBLIC_KEY: &str = r"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAjks48W4tV0JS+HbyT4hQ
jj3d8O3rDGeWq2oLnA9D+4Y0AGjg9pv6avJD5CclJdBmiDFlntGngbuTkr1/nd+n
qy5Cj/ll/qSeCf6g96lxrc4+kUHY6YNm8GrOybk8VBS4W19Ux40d3FNDLcyVujt0
kHRK3sL04/zDFelBouoehrz55Q1f9K87nV7Fy+L434StEjLhrYrR1L2x0ZQhmRL1
xvvOfWSdYYMgZIjRA0zttqKYDXe6+VKxX3MjJ9wX3b6gxeuBUrSVdLZbeyeLWcWn
p/9whGz4WfzHlXvCL8fsg1W70m+Z70LYVLFGaGb4egokM7CKb2uJEZFmi1F/Uxf/
3wIDAQAB
-----END PUBLIC KEY-----
";

    /// [`PUBLIC_KEY`]'s modulus and exponent, base64url, as the JWK spells them.
    const MODULUS: &str = "jks48W4tV0JS-HbyT4hQjj3d8O3rDGeWq2oLnA9D-4Y0AGjg9pv6avJD5CclJdBmiDFlntGngbuTkr1_nd-nqy5Cj_ll_qSeCf6g96lxrc4-kUHY6YNm8GrOybk8VBS4W19Ux40d3FNDLcyVujt0kHRK3sL04_zDFelBouoehrz55Q1f9K87nV7Fy-L434StEjLhrYrR1L2x0ZQhmRL1xvvOfWSdYYMgZIjRA0zttqKYDXe6-VKxX3MjJ9wX3b6gxeuBUrSVdLZbeyeLWcWnp_9whGz4WfzHlXvCL8fsg1W70m-Z70LYVLFGaGb4egokM7CKb2uJEZFmi1F_Uxf_3w";
    const EXPONENT: &str = "AQAB";

    /// The `exp` every test token carries: far enough out that nothing here turns on the clock.
    /// `verify` does not check it anyway — `Oidc::admit` does — so this only has to parse.
    const EXPIRES_AT: i64 = 4_102_444_800;

    /// One request the stub provider was asked at `/token`.
    #[derive(Clone)]
    struct TokenRequest {
        /// The `Authorization` header, which is where the client secret must be.
        authorization: Option<String>,
        /// The form body verbatim, which is where it must not be.
        body: String,
    }

    /// What the stub answers at `/token`.
    #[derive(Clone)]
    struct Answer {
        status: StatusCode,
        body: String,
    }

    /// The stub's shared state: what to answer, and what it has been asked.
    ///
    /// Both answers are behind a lock rather than fixed at construction, because X-17's rotation
    /// and rate-limit tests need a provider that **changes under the exchange**: one that publishes
    /// a new key between two sign-ins, and one whose key set is failing throughout.
    struct Stub {
        token: Mutex<Answer>,
        jwks: Mutex<Answer>,
        received: Mutex<Vec<TokenRequest>>,
        /// How many requests the key-set endpoint has answered. The number
        /// `an_unknown_kid_cannot_hammer_a_failing_key_set` is entirely about.
        jwks_requests: Mutex<usize>,
    }

    /// A provider on loopback, over real HTTP.
    ///
    /// A real socket rather than a stubbed `TokenExchange`, because the seam under test *is* the
    /// HTTP one: which header the secret went in, what a 500 means as against a 400, and what a
    /// connection refused turns into are all invisible to a test that calls `verify` directly.
    struct StubProvider {
        base: String,
        received: Arc<Stub>,
        server: JoinHandle<std::io::Result<()>>,
    }

    impl Drop for StubProvider {
        /// The server holds a port and a task; a test that finished with it is done with both.
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    impl StubProvider {
        /// Start a provider answering `token` at `/token` and publishing `jwks` at `/jwks`.
        async fn serving(token: Answer, jwks: String) -> Self {
            let stub = Arc::new(Stub {
                token: Mutex::new(token),
                jwks: Mutex::new(Answer {
                    status: StatusCode::OK,
                    body: jwks,
                }),
                received: Mutex::new(Vec::new()),
                jwks_requests: Mutex::new(0),
            });

            let app = Router::new()
                .route("/token", post(token_endpoint))
                .route("/jwks", get(jwks_endpoint))
                .with_state(Arc::clone(&stub));

            // Port 0, for the same reason `health_answers_over_a_socket_on_the_default_interface`
            // uses it: several of these run at once and none of them may depend on a free fixed port.
            let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .expect("loopback is bindable");
            let local = listener
                .local_addr()
                .expect("a bound listener has an address");

            let server = tokio::spawn(async move { axum::serve(listener, app).await });

            Self {
                base: format!("http://{local}"),
                received: stub,
                server,
            }
        }

        /// Every request the token endpoint was asked, in order.
        fn token_requests(&self) -> Vec<TokenRequest> {
            self.received
                .received
                .lock()
                .expect("no test panics holding this lock")
                .clone()
        }

        /// How many times the key-set endpoint has been asked.
        fn jwks_requests(&self) -> usize {
            *self
                .received
                .jwks_requests
                .lock()
                .expect("no test panics holding this lock")
        }

        /// Answer `/token` with this from now on.
        fn now_answering(&self, token: Answer) {
            *self
                .received
                .token
                .lock()
                .expect("no test panics holding this lock") = token;
        }

        /// Publish this key set from now on: a rotation, as the exchange sees one.
        fn now_publishing(&self, jwks: String) {
            *self
                .received
                .jwks
                .lock()
                .expect("no test panics holding this lock") = Answer {
                status: StatusCode::OK,
                body: jwks,
            };
        }

        /// Break the key-set endpoint: from now on it only ever errors.
        fn keys_unreachable(&self) {
            *self
                .received
                .jwks
                .lock()
                .expect("no test panics holding this lock") = Answer {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: json!({ "error": "the key set is having a day" }).to_string(),
            };
        }
    }

    async fn token_endpoint(
        State(stub): State<Arc<Stub>>,
        headers: HeaderMap,
        body: String,
    ) -> Response {
        stub.received
            .lock()
            .expect("no test panics holding this lock")
            .push(TokenRequest {
                authorization: headers
                    .get(AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string),
                body,
            });

        let answer = stub
            .token
            .lock()
            .expect("no test panics holding this lock")
            .clone();

        (
            answer.status,
            [(CONTENT_TYPE, "application/json")],
            answer.body,
        )
            .into_response()
    }

    async fn jwks_endpoint(State(stub): State<Arc<Stub>>) -> Response {
        *stub
            .jwks_requests
            .lock()
            .expect("no test panics holding this lock") += 1;

        let answer = stub
            .jwks
            .lock()
            .expect("no test panics holding this lock")
            .clone();

        (
            answer.status,
            [(CONTENT_TYPE, "application/json")],
            answer.body,
        )
            .into_response()
    }

    /// An address on loopback with nothing behind it.
    ///
    /// Bound to learn a port the OS considers free, then released, so dialling it is refused at once
    /// rather than hanging until [`HTTP_TIMEOUT`].
    async fn nobody_listening() -> String {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("loopback is bindable");
        let local = listener
            .local_addr()
            .expect("a bound listener has an address");

        drop(listener);

        format!("http://{local}")
    }

    /// The key set the stub publishes: one RSA key, named [`KID`].
    fn published_keys() -> String {
        json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": KID,
                "n": MODULUS,
                "e": EXPONENT,
            }],
        })
        .to_string()
    }

    /// The claims every token in these tests carries.
    ///
    /// `aud` is the **list** spelling and `hd` is present so the happy path proves the signed
    /// organization claim survives the verified-claims seam. `email` is deliberately hostile and
    /// unmodelled: requesting its provider-required scope does not make it an identity input.
    fn claims() -> serde_json::Value {
        json!({
            "iss": ISSUER,
            "aud": [CLIENT_ID, "another-audience"],
            "sub": "the-operator",
            "exp": EXPIRES_AT,
            "nonce": "the-bound-nonce",
            "hd": "example.com",
            "email": "outsider@hostile.example",
        })
    }

    /// [`claims`], signed with `algorithm` under `key` and labelled with `kid`.
    fn signed_with(algorithm: Algorithm, kid: &str, key: &EncodingKey) -> String {
        let mut header = Header::new(algorithm);
        header.kid = Some(kid.to_string());

        encode(&header, &claims(), key).expect("the test claims encode")
    }

    /// [`claims`], signed by the key the stub publishes.
    fn genuine_token() -> String {
        let signing =
            EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("the test key is a valid PEM");

        signed_with(Algorithm::RS256, KID, &signing)
    }

    /// A successful token response carrying `id_token`.
    fn answering_with(id_token: &str) -> Answer {
        Answer {
            status: StatusCode::OK,
            body: json!({
                "access_token": "an-access-token",
                "token_type": "Bearer",
                "expires_in": 3600,
                "id_token": id_token,
            })
            .to_string(),
        }
    }

    /// Redeem a code at `base`, with a configuration pointed at it.
    async fn redeem_against(base: &str) -> Result<SignedClaims, ExchangeError> {
        let config = OidcConfig::for_test_against(ISSUER, CLIENT_ID, TENANT, base);
        let exchange = HttpTokenExchange::new(&config).expect("the HTTP client builds");
        let verifier = Verifier::generate().expect("the OS supplies entropy");

        exchange
            .redeem(Redemption {
                code: "an-authorization-code",
                verifier: &verifier,
                redirect_uri: config.redirect_uri(),
                client_id: config.client_id(),
                client_secret: config.client_secret(),
            })
            .await
    }

    /// Redeem a code at `provider`.
    async fn redeem_at(provider: &StubProvider) -> Result<SignedClaims, ExchangeError> {
        redeem_against(&provider.base).await
    }

    /// The `state` an authorization URL carries, so a test can play the browser coming back.
    fn state_in(url: &str) -> String {
        url.split('&')
            .find_map(|pair| pair.strip_prefix("state="))
            .expect("an authorization URL carries a state")
            .to_string()
    }

    /// Drive a whole sign-in against `provider` and take the refusal.
    ///
    /// Through [`Oidc::complete`] rather than through [`HttpTokenExchange::redeem`] directly,
    /// because the thing under test is what an **operator** and a **caller** each end up being
    /// told, and only [`SignInRefusal`] has both of those on it.
    async fn refusal_from(provider: &StubProvider) -> SignInRefusal {
        let config = OidcConfig::for_test_against(ISSUER, CLIENT_ID, TENANT, &provider.base);
        let exchange = HttpTokenExchange::new(&config).expect("the HTTP client builds");
        let oidc = Oidc::new(config, Arc::new(exchange));

        let authorization = oidc.authorize().expect("the OS supplies entropy");
        let state = state_in(&authorization.url);

        oidc.complete(
            &state,
            "an-authorization-code",
            authorization.binder.as_str(),
        )
        .await
        .expect_err("every provider in these tests refuses")
    }

    /// A token-endpoint answer refusing something, as a provider spells it.
    fn refusing(status: StatusCode, error: &str) -> Answer {
        Answer {
            status,
            body: json!({ "error": error }).to_string(),
        }
    }

    /// **X-17, the failing-first test.** This host's own client secret being wrong does not read,
    /// in the log, like a caller's authorization code being refused — and reads *exactly* like it
    /// to the caller.
    ///
    /// Both halves in one test because they pull in opposite directions and a pair of tests could
    /// drift apart. RFC 6749 §5.2 `invalid_client` means the credential the **token endpoint**
    /// checked was this host's, presented as HTTP Basic by `redeem`. No caller can do anything
    /// about that, and an operator reading "the provider refused the authorization code" goes
    /// looking at the wrong end of the flow entirely.
    ///
    /// The caller-facing half is not an economy. Answering an operator's misconfiguration
    /// differently would make the callback report whether *this host's* registration at the
    /// provider is currently good — to anybody who can reach `/api/signin/callback`, without
    /// signing in. The remedy for the caller is the same either way, so nothing is withheld that
    /// would help them.
    #[tokio::test]
    async fn a_refused_client_secret_is_not_reported_as_a_refused_authorization_code() {
        // This host's registration is wrong. The operator's problem, and only theirs.
        let misconfigured = StubProvider::serving(
            refusing(StatusCode::UNAUTHORIZED, "invalid_client"),
            published_keys(),
        )
        .await;

        // The caller's code was refused. Genuinely about their credential.
        let refusing_the_code = StubProvider::serving(
            refusing(StatusCode::BAD_REQUEST, "invalid_grant"),
            published_keys(),
        )
        .await;

        let ours = refusal_from(&misconfigured).await;
        let theirs = refusal_from(&refusing_the_code).await;

        // The operator's channel separates them, and names the variable to go and look at.
        assert_ne!(
            ours.to_string(),
            theirs.to_string(),
            "an operator must be able to tell their own misconfiguration from a refused code",
        );
        assert!(
            ours.to_string().contains(CLIENT_SECRET_ENV),
            "and must be told which variable to look at: {ours}",
        );

        // The caller's channel does not. Byte-identical, or the split has become a disclosure.
        assert_eq!(
            ours.caller_facing(),
            theirs.caller_facing(),
            "the caller learns nothing that separates them",
        );
    }

    /// RFC 7617 `basic-credentials`: `user:password`, standard base64 with padding.
    ///
    /// Derived from [`base64url`] rather than written a second time — the two alphabets differ only
    /// in the final two characters, and in whether the result is padded.
    fn basic_credentials(user: &str, password: &str) -> String {
        let mut encoded = base64url(format!("{user}:{password}").as_bytes())
            .replace('-', "+")
            .replace('_', "/");

        while !encoded.len().is_multiple_of(4) {
            encoded.push('=');
        }

        encoded
    }

    /// The whole of the happy path over a socket: the code is spent, the id token comes back, its
    /// signature verifies against the published key, and every claim this host carries forward
    /// arrives intact.
    ///
    /// `aud` is a list and `hd` is absent deliberately: those are the two shapes a provider is
    /// free to send and a host is tempted not to model.
    #[tokio::test]
    async fn a_correctly_signed_id_token_is_redeemed() {
        let provider =
            StubProvider::serving(answering_with(&genuine_token()), published_keys()).await;

        let claims = redeem_at(&provider)
            .await
            .expect("a token signed by the published key is redeemed");

        assert_eq!(
            claims,
            SignedClaims {
                issuer: ISSUER.to_string(),
                audience: vec![CLIENT_ID.to_string(), "another-audience".to_string()],
                subject: "the-operator".to_string(),
                nonce: Some("the-bound-nonce".to_string()),
                expires_at: EXPIRES_AT,
                hosted_domain: Some("example.com".to_string()),
            },
        );
    }

    /// The attack this module's documentation claims to close, spelled out.
    ///
    /// The forger has the provider's **public** key — it is published, that is what public means —
    /// and hands those bytes to HMAC as a shared secret, betting that the verifier reads `alg` off
    /// the header and then looks up "the key for this `kid`". A verifier that does both computes an
    /// HMAC with a value the attacker also has, and every claim in the token is the attacker's to
    /// choose.
    ///
    /// It is refused here before any signature is computed, because
    /// [`permitted_algorithms`] reads the JWK and an RSA key can never name `HS256`.
    ///
    /// Both spellings of "the public key" are tried, because a vulnerable verifier passes whatever
    /// bytes it happens to be holding: the PEM, if the provider publishes one, or the JWK's modulus,
    /// if it publishes a key set. An attacker has both — so a guard that only stopped one of them
    /// would not be a guard.
    #[tokio::test]
    async fn a_token_signed_with_the_public_key_as_an_hmac_secret_is_refused() {
        for (spelling, secret) in [
            ("the published PEM", PUBLIC_KEY.as_bytes()),
            ("the JWK's modulus", MODULUS.as_bytes()),
        ] {
            let forged = EncodingKey::from_secret(secret);
            let provider = StubProvider::serving(
                answering_with(&signed_with(Algorithm::HS256, KID, &forged)),
                published_keys(),
            )
            .await;

            let refusal = redeem_at(&provider).await;

            assert!(
                matches!(refusal, Err(ExchangeError::Rejected)),
                "a token MAC'd with {spelling} must be refused, not {refusal:?}",
            );
        }
    }

    /// A key of a kind this build does not recognise admits **no** algorithm, so no token can be
    /// verified against it.
    ///
    /// This became reachable with `jsonwebtoken` 11, which changed two things at once. An
    /// unrecognised `kty` used to fail deserialization of the whole key set, so the refusal was the
    /// parser's accident; now it parses into a catch-all variant and arrives here as an ordinary
    /// `Jwk`. And `AlgorithmParameters` became `#[non_exhaustive]`, so every kind added after this
    /// build — and every kind added by a future upgrade — lands in the same wildcard arm with no
    /// compile error to prompt anyone to think about it. Both changes point the same way: the arm
    /// has to be the refusing one, and something has to hold it there.
    ///
    /// Two spellings, because the wrong fix is attractive for a different reason in each:
    ///
    /// - A registered kind this build simply predates. `AKP` is the JOSE key type for post-quantum
    ///   algorithm key pairs; the day a provider rotates onto one, this host must refuse sign-in
    ///   and say so, not verify against a key whose semantics it does not implement.
    /// - An unrecognised kind carrying the provider's **genuine** RSA modulus and exponent. Every
    ///   ingredient of a real verification is present and the token really is signed by the
    ///   matching private key, so a wildcard that fell back to the RSA families — tempting,
    ///   because the numbers are right there and it would "work" — verifies a token whose key the
    ///   provider labelled as something else entirely.
    ///
    /// Each case is asserted twice: that [`permitted_algorithms`] yields nothing, and that a whole
    /// redemption is refused. The second is what the first is *for*, and asserting only the first
    /// would leave "no algorithm is permitted" true while some later gate quietly did the refusing.
    #[tokio::test]
    async fn a_key_of_a_kind_this_build_does_not_recognise_admits_no_algorithm() {
        let signing =
            EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("the test key is a valid PEM");

        for (spelling, published) in [
            (
                "a registered key type this build predates",
                json!({
                    "kty": "AKP",
                    "use": "sig",
                    "kid": KID,
                    "alg": "ML-DSA-44",
                    "pub": MODULUS,
                }),
            ),
            (
                "an unrecognised key type carrying real RSA parameters",
                json!({
                    "kty": "RSA-BUT-NOT-QUITE",
                    "use": "sig",
                    "kid": KID,
                    "n": MODULUS,
                    "e": EXPONENT,
                }),
            ),
        ] {
            let key: Jwk = serde_json::from_value(published.clone())
                .expect("an unknown key type parses rather than failing the whole set");

            assert!(
                permitted_algorithms(&key).is_none(),
                "{spelling} must admit no algorithm, not {:?}",
                permitted_algorithms(&key),
            );

            let provider = StubProvider::serving(
                answering_with(&signed_with(Algorithm::RS256, KID, &signing)),
                json!({ "keys": [published] }).to_string(),
            )
            .await;

            let refusal = redeem_at(&provider).await;

            assert!(
                matches!(refusal, Err(ExchangeError::Rejected)),
                "a token offered against {spelling} must be refused, not {refusal:?}",
            );
        }
    }

    /// `alg: none`: the signature is empty and the token says not to check it.
    ///
    /// Hand-assembled, because no honest signer will produce one — which is the point. Nothing in
    /// this module may treat the header as an instruction.
    #[tokio::test]
    async fn a_token_claiming_alg_none_is_refused() {
        let header = json!({ "alg": "none", "typ": "JWT", "kid": KID }).to_string();
        let unsigned = format!(
            "{}.{}.",
            base64url(header.as_bytes()),
            base64url(claims().to_string().as_bytes()),
        );

        let provider = StubProvider::serving(answering_with(&unsigned), published_keys()).await;

        let refusal = redeem_at(&provider).await;

        assert!(
            matches!(refusal, Err(ExchangeError::Rejected)),
            "an unsigned token must be refused, not {refusal:?}",
        );
    }

    /// A `kid` the provider never published is refused, rather than sent looking for a key that
    /// happens to verify.
    ///
    /// The token here is **correctly signed** by the only key the provider publishes, and differs
    /// from the happy path in nothing but the `kid` it names. So this fails the moment key selection
    /// falls back to "try the ones we have" — which is the bug, because that turns an attacker's
    /// choice of `kid` into a verifier that shops for a key.
    #[tokio::test]
    async fn a_token_naming_an_unpublished_kid_is_refused() {
        let signing =
            EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("the test key is a valid PEM");
        let provider = StubProvider::serving(
            answering_with(&signed_with(
                Algorithm::RS256,
                "a-kid-nobody-published",
                &signing,
            )),
            published_keys(),
        )
        .await;

        let refusal = redeem_at(&provider).await;

        assert!(
            matches!(refusal, Err(ExchangeError::UnpublishedKey)),
            "a token naming an unpublished kid must be refused, not {refusal:?}",
        );
    }

    /// **X-17, the Acceptance's second item.** An unpublished `kid` reads differently, in the log,
    /// from a refused authorization code — and identically to the caller.
    ///
    /// The same two halves as
    /// [`a_refused_client_secret_is_not_reported_as_a_refused_authorization_code`], for a cause
    /// that is *usually* the operator's: a `FLUX_EXCHANGE_OIDC_JWKS_URI` naming the wrong provider
    /// fails this way on **every** sign-in, and an operator reading "the provider refused the
    /// authorization code" has no reason to go and look at a URL.
    ///
    /// It is not always the operator's, which is why the caller-facing half is not negotiable: a
    /// stranger can produce this line at will by signing anything with a `kid` of their choosing,
    /// and a distinguishable answer would confirm to them which `kid`s this host holds keys for.
    #[tokio::test]
    async fn an_unpublished_kid_is_not_reported_as_a_refused_authorization_code() {
        let signing =
            EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("the test key is a valid PEM");

        // Correctly signed, and differing from the happy path in nothing but its `kid`.
        let wrong_key_set = StubProvider::serving(
            answering_with(&signed_with(
                Algorithm::RS256,
                "a-kid-nobody-published",
                &signing,
            )),
            published_keys(),
        )
        .await;

        let refusing_the_code = StubProvider::serving(
            refusing(StatusCode::BAD_REQUEST, "invalid_grant"),
            published_keys(),
        )
        .await;

        let ours = refusal_from(&wrong_key_set).await;
        let theirs = refusal_from(&refusing_the_code).await;

        assert_ne!(
            ours.to_string(),
            theirs.to_string(),
            "an unpublished kid and a refused code are different operator problems",
        );
        assert!(
            ours.to_string().contains(JWKS_URI_ENV),
            "and the operator must be told which variable to look at: {ours}",
        );

        assert_eq!(
            ours.caller_facing(),
            theirs.caller_facing(),
            "the caller learns nothing that separates them",
        );
    }

    /// **X-17, the Acceptance's fourth item.** The refetch floor holds **while the key set is
    /// failing**, which is precisely when it used to lapse.
    ///
    /// The bug: `fetch_keys` wrote its timestamp only after a successful parse, and the floor was
    /// read off that timestamp. So a key set that was down left the cache empty, the floor had
    /// nothing to compare against, and every arriving callback started an outbound request of its
    /// own — one per hostile `kid`, at no cost to the sender, aimed at a provider that was already
    /// having a bad day. A rate limit that holds only while nothing is wrong is not one.
    ///
    /// Counted at the stub rather than inferred from timing, because the claim is about how many
    /// requests actually left this process.
    #[tokio::test]
    async fn an_unknown_kid_cannot_hammer_a_failing_key_set() {
        let signing =
            EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("the test key is a valid PEM");
        let provider = StubProvider::serving(
            answering_with(&signed_with(Algorithm::RS256, "a-strangers-kid", &signing)),
            published_keys(),
        )
        .await;

        // The key set only ever errors, from the first request onwards.
        provider.keys_unreachable();

        let config = OidcConfig::for_test_against(ISSUER, CLIENT_ID, TENANT, &provider.base);
        let exchange = HttpTokenExchange::new(&config).expect("the HTTP client builds");
        let verifier = Verifier::generate().expect("the OS supplies entropy");

        // One exchange, many callbacks — a single host under a stream of hostile callbacks, which
        // is the shape the amplification takes. The real floor, not a test one: ten seconds is far
        // longer than this loop takes, so anything above one fetch is the bug.
        for _ in 0..8 {
            let refusal = exchange
                .redeem(Redemption {
                    code: "an-authorization-code",
                    verifier: &verifier,
                    redirect_uri: config.redirect_uri(),
                    client_id: config.client_id(),
                    client_secret: config.client_secret(),
                })
                .await;

            assert!(refusal.is_err(), "no key set means nothing verifies");
        }

        assert_eq!(
            provider.jwks_requests(),
            1,
            "eight hostile callbacks must cost the provider one key-set fetch, not eight",
        );
    }

    /// **X-17, the Acceptance's fifth item.** A key published *after* the cache was filled is
    /// picked up, without waiting out [`JWKS_TTL`].
    ///
    /// The rotation branch — "the cache is fresh and does not name this `kid`" — had no test at
    /// all, because every other test in this module starts cold. A provider that rotates early
    /// could therefore have been refusing valid sign-ins for up to five minutes with nothing here
    /// noticing.
    ///
    /// The refetch floor is set to nothing for this test, because the branch is only reachable
    /// *inside* the floor's window and a test that slept through ten real seconds is a test that
    /// gets deleted. What is being proved is that a refetch happens and the new key is found —
    /// `an_unknown_kid_cannot_hammer_a_failing_key_set` is what proves the floor is there.
    ///
    /// The rotated key is the same key material under a new `kid`, deliberately: key **selection**
    /// is what this branch does, it selects by `kid` alone, and a second embedded keypair would
    /// prove nothing more while doubling the private material checked into this file.
    #[tokio::test]
    async fn a_key_published_after_the_cache_was_filled_is_picked_up() {
        const ROTATED: &str = "x17-rotated-key";

        let signing =
            EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("the test key is a valid PEM");
        let provider =
            StubProvider::serving(answering_with(&genuine_token()), published_keys()).await;

        let config = OidcConfig::for_test_against(ISSUER, CLIENT_ID, TENANT, &provider.base);
        let exchange = HttpTokenExchange::with_refetch_floor(&config, Duration::ZERO)
            .expect("the HTTP client builds");
        let verifier = Verifier::generate().expect("the OS supplies entropy");

        let redeem = || async {
            exchange
                .redeem(Redemption {
                    code: "an-authorization-code",
                    verifier: &verifier,
                    redirect_uri: config.redirect_uri(),
                    client_id: config.client_id(),
                    client_secret: config.client_secret(),
                })
                .await
        };

        // A sign-in, so the cache is warm and holds only the original key.
        redeem().await.expect("the happy path fills the cache");
        assert_eq!(provider.jwks_requests(), 1);

        // The provider rotates: a new `kid`, published, and signing from now on.
        provider.now_publishing(
            json!({
                "keys": [{
                    "kty": "RSA",
                    "use": "sig",
                    "alg": "RS256",
                    "kid": ROTATED,
                    "n": MODULUS,
                    "e": EXPONENT,
                }],
            })
            .to_string(),
        );
        provider.now_answering(answering_with(&signed_with(
            Algorithm::RS256,
            ROTATED,
            &signing,
        )));

        let rotated = redeem()
            .await
            .expect("a key published after the cache was filled must verify");

        assert_eq!(rotated.subject, "the-operator");
        assert_eq!(
            provider.jwks_requests(),
            2,
            "the unknown kid provoked exactly one refetch",
        );

        // And the refetched set is now the cached one: a third sign-in with the rotated key costs
        // nothing on the network, or "picked up" would mean "fetched again every time".
        redeem().await.expect("the rotated key is now cached");
        assert_eq!(
            provider.jwks_requests(),
            2,
            "the rotation was cached, not refetched per sign-in",
        );
    }

    /// The provider saying no and the provider being broken are different events.
    ///
    /// Load-bearing rather than tidy: `routes::signin` turns one into a 401 and the other into a
    /// 503, and `a_refusal_tells_the_caller_nothing_about_the_provider` reads that split. Collapsing
    /// them sends an operator to reset a password during an outage.
    #[tokio::test]
    async fn a_refused_grant_and_an_unreachable_provider_do_not_collapse() {
        // The provider refusing the code: the one case that is genuinely the caller's credential.
        let refusing = StubProvider::serving(
            Answer {
                status: StatusCode::BAD_REQUEST,
                body: json!({ "error": "invalid_grant" }).to_string(),
            },
            published_keys(),
        )
        .await;

        let refused = redeem_at(&refusing).await;
        assert!(
            matches!(refused, Err(ExchangeError::Rejected)),
            "a 400 invalid_grant is the provider refusing the code, not {refused:?}",
        );

        // The provider broken. Not the caller's credential, and not the caller's problem.
        let broken = StubProvider::serving(
            Answer {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: json!({ "error": "server_error" }).to_string(),
            },
            published_keys(),
        )
        .await;

        let unreachable = redeem_at(&broken).await;
        assert!(
            matches!(unreachable, Err(ExchangeError::Unreachable(_))),
            "a 500 is the provider being broken, not {unreachable:?}",
        );

        // A 200 whose body is not a token response: a proxy's error page, most plausibly. Nothing
        // about the caller's code is known to be wrong, so this is not their refusal either.
        let babbling = StubProvider::serving(
            Answer {
                status: StatusCode::OK,
                body: "<html>the gateway would like a word</html>".to_string(),
            },
            published_keys(),
        )
        .await;

        let unreadable = redeem_at(&babbling).await;
        assert!(
            matches!(unreadable, Err(ExchangeError::Unreachable(_))),
            "an unreadable 200 is not a refusal of the code, not {unreadable:?}",
        );

        // Nothing listening at all.
        let absent = redeem_against(&nobody_listening().await).await;
        assert!(
            matches!(absent, Err(ExchangeError::Unreachable(_))),
            "a connection refused is not a refusal of the code, not {absent:?}",
        );
    }

    /// A successful OAuth exchange with no id token is a failed OIDC sign-in.
    ///
    /// The provider has issued an access token and said nothing about who the human is. There is no
    /// identity to bind a session to, so there is nothing to do but refuse.
    #[tokio::test]
    async fn a_successful_exchange_carrying_no_id_token_is_refused() {
        let provider = StubProvider::serving(
            Answer {
                status: StatusCode::OK,
                body: json!({ "access_token": "an-access-token", "token_type": "Bearer" })
                    .to_string(),
            },
            published_keys(),
        )
        .await;

        let refusal = redeem_at(&provider).await;

        assert!(
            matches!(refusal, Err(ExchangeError::NoIdToken)),
            "an exchange with no id token establishes no identity, not {refusal:?}",
        );
    }

    /// `client_secret_basic`, asserted at the provider rather than at the call site.
    ///
    /// The header is where a secret is least likely to be logged; the form body is where request
    /// logging, access logs and error reporters all pick it up. Asserted by inspecting what the stub
    /// actually received, because the difference between the two is invisible from this side.
    #[tokio::test]
    async fn the_client_secret_travels_as_http_basic_and_never_in_the_body() {
        let provider =
            StubProvider::serving(answering_with(&genuine_token()), published_keys()).await;

        redeem_at(&provider)
            .await
            .expect("the happy path, so the request is the thing under test");

        let config = OidcConfig::for_test_against(ISSUER, CLIENT_ID, TENANT, &provider.base);
        let secret = config.client_secret().expose();

        let requests = provider.token_requests();
        let [request] = requests.as_slice() else {
            panic!("exactly one token request, got {}", requests.len());
        };

        assert_eq!(
            request.authorization.as_deref(),
            Some(format!("Basic {}", basic_credentials(CLIENT_ID, secret)).as_str()),
            "the secret goes to the token endpoint as HTTP Basic",
        );

        assert!(
            !request.body.contains(secret),
            "and never in the form body: {}",
            request.body,
        );
        assert!(
            !request.body.contains("client_secret"),
            "not even under its own name: {}",
            request.body,
        );

        // What the body *must* carry, so this test fails if the secret left it by the request
        // losing its body rather than by the secret moving to the header.
        assert!(
            request.body.contains("grant_type=authorization_code"),
            "{}",
            request.body,
        );
        assert!(
            request.body.contains("code=an-authorization-code"),
            "{}",
            request.body
        );
        assert!(request.body.contains("code_verifier="), "{}", request.body);
    }
}
