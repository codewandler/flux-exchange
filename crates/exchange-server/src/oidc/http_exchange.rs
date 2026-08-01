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

        let keys: JwkSet = response.json().await.map_err(|source| {
            ExchangeError::Unreachable(format!("unreadable key set: {source}"))
        })?;

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

    use crate::oidc::pkce::{base64url, Verifier};

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
    struct Answer {
        status: StatusCode,
        body: String,
    }

    /// The stub's shared state: what to answer, and what it has been asked.
    struct Stub {
        token: Answer,
        jwks: String,
        received: Mutex<Vec<TokenRequest>>,
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
                token,
                jwks,
                received: Mutex::new(Vec::new()),
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

        (
            stub.token.status,
            [(CONTENT_TYPE, "application/json")],
            stub.token.body.clone(),
        )
            .into_response()
    }

    async fn jwks_endpoint(State(stub): State<Arc<Stub>>) -> Response {
        ([(CONTENT_TYPE, "application/json")], stub.jwks.clone()).into_response()
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
    /// `aud` is the **list** spelling and `email` is absent: both are the shapes a host that only
    /// modelled the common case gets wrong, and the happy path asserts they survive the round trip.
    fn claims() -> serde_json::Value {
        json!({
            "iss": ISSUER,
            "aud": [CLIENT_ID, "another-audience"],
            "sub": "the-operator",
            "exp": EXPIRES_AT,
            "nonce": "the-bound-nonce",
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
    /// `aud` is a list and `email` is absent deliberately: those are the two shapes a provider is
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
                email: None,
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
            matches!(refusal, Err(ExchangeError::Rejected)),
            "a token naming an unpublished kid must be refused, not {refusal:?}",
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
            matches!(refusal, Err(ExchangeError::Rejected)),
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
