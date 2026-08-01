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
use flow::{Binder, Claimed, FlowError, PendingAuthorizations};

/// An authorization request that has been opened: where to send the browser, and what to plant in
/// it so the callback can tell it apart from every other browser.
pub struct Authorization {
    /// The provider URL the browser is redirected to.
    pub url: String,
    /// The binder for this browser. See [`flow::BINDER_COOKIE`].
    pub binder: Binder,
}

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

    /// Open an authorization request: the URL the browser is sent to, and the binder to plant in it.
    ///
    /// Both, from one call, because they are two halves of one act. A composition able to obtain the
    /// URL without the binder would be one where the browser is sent to the provider carrying
    /// nothing that ties the callback back to it — which is the hole X-15 closed, reintroduced at
    /// the seam.
    pub fn authorize(&self) -> Result<Authorization, FlowError> {
        let begun = self.pending.begin()?;

        let separator = if self.config.authorization_endpoint().contains('?') {
            '&'
        } else {
            '?'
        };

        let url = format!(
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
        );

        Ok(Authorization {
            url,
            binder: begun.binder,
        })
    }

    /// Finish a sign-in: redeem the code, check every binding, and open a session.
    ///
    /// The order matters. The `(state, binder)` claim comes first, so a callback this host did not
    /// open — or did not open *for this browser* — costs one map lookup and reaches neither the
    /// provider nor the session store.
    ///
    /// `binder` is what the browser presented, which is why it is a `&str` and not a
    /// [`Binder`](flow::Binder): a caller-supplied value has not been shown to be one of ours, and
    /// giving it the type of a value this host drew would be asserting exactly what is in question.
    /// The empty string is a legitimate argument here and can never match; the route refuses a
    /// missing cookie before reaching this, so that it never becomes a lookup at all.
    pub async fn complete(
        &self,
        state: &str,
        code: &str,
        binder: &str,
    ) -> Result<SessionToken, SignInRefusal> {
        let pending = match self.pending.claim(state, binder) {
            Claimed::Authorization(pending) => pending,
            Claimed::Unknown => return Err(SignInRefusal::UnknownState),
            Claimed::AnotherBrowser => return Err(SignInRefusal::AnotherBrowser),
        };

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

    /// The callback carried no binder cookie at all, so no browser claims to have opened it.
    ///
    /// Refused **before** the pending store is consulted. An attacker who simply omits the cookie
    /// must not fall through to the path that checks only `state`, and refusing early also means the
    /// answer cannot depend on whether the named `state` is live.
    NoBinder,

    /// The callback carried a binder, but not the one the named `state` was opened with.
    ///
    /// The login-CSRF this binding exists to close: a browser being walked into somebody else's
    /// sign-in. Kept distinct from [`UnknownState`](Self::UnknownState) **in the log only** — see
    /// [`SignInRefusal::caller_facing`].
    AnotherBrowser,

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
    ///
    /// # The three binding refusals answer identically, on purpose
    ///
    /// [`UnknownState`](Self::UnknownState), [`NoBinder`](Self::NoBinder) and
    /// [`AnotherBrowser`](Self::AnotherBrowser) share one phrase and one status, and that is a
    /// security property rather than an economy.
    ///
    /// `UnknownState` and `AnotherBrowser` are decided by a **lookup in the pending store**: the
    /// first means no live request answers to that `state`, the second means one does. Telling those
    /// apart on the wire would make the callback an oracle for "is this `state` live", which is the
    /// one fact about somebody else's pending sign-in worth guessing — and
    /// `routes::signin::tests::a_callback_without_a_code_is_not_an_oracle_for_a_live_state` already
    /// holds that line for the other refusals on this route.
    ///
    /// The remedy is identical anyway — start again from the sign-in page — so nothing is withheld
    /// that would help the human. What differs is the **diagnosis**, and that belongs to the
    /// operator: [`fmt::Display`] keeps all three apart, because a host seeing forged states and a
    /// host seeing browsers walked into other people's sign-ins have different problems and
    /// different attackers.
    pub fn caller_facing(&self) -> &'static str {
        match self {
            Self::UnknownState | Self::NoBinder | Self::AnotherBrowser => {
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
            // The two X-15 added. Both name the *browser*, because that is what separates them from
            // the line above: there the value was wrong, here the value was real and the browser
            // presenting it was not the one it was issued to.
            Self::NoBinder => f.write_str(
                "a callback presented no sign-in binder, so no browser claims to have opened it — \
                 either a browser that discards cookies, or a callback walked into one that never \
                 started a sign-in here",
            ),
            Self::AnotherBrowser => f.write_str(
                "a callback presented a genuinely bound state from a browser that did not open it: \
                 login CSRF, or a sign-in resumed in a different browser. The authorization request \
                 was left unspent",
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

#[cfg(test)]
mod tests {
    use super::*;

    use exchange_host::async_trait;

    use config::OidcConfig;
    use exchange::Redemption;

    const ISSUER: &str = "https://accounts.example.com";
    const CLIENT_ID: &str = "flux-exchange";
    const TENANT: &str = "acme";
    const NONCE: &str = "the-nonce-this-host-bound";

    /// An exchange that is never called: [`Oidc::admit`] is a pure function of claims, and these
    /// tests reach it directly.
    struct Unused;

    #[async_trait]
    impl TokenExchange for Unused {
        async fn redeem(&self, _: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
            unreachable!("admit() does not redeem")
        }
    }

    fn oidc() -> Oidc {
        Oidc::new(
            OidcConfig::for_test(ISSUER, CLIENT_ID, TENANT),
            Arc::new(Unused),
        )
    }

    /// Claims a well-behaved provider returns for a sign-in bound to [`NONCE`].
    fn good() -> SignedClaims {
        SignedClaims {
            issuer: ISSUER.to_string(),
            audience: vec![CLIENT_ID.to_string()],
            subject: "248289761001".to_string(),
            nonce: Some(NONCE.to_string()),
            expires_at: 2_000_000_000,
            email: Some("alice@example.com".to_string()),
        }
    }

    /// The baseline. Without it, every refusal below could be passing because `admit` refuses
    /// everything.
    #[test]
    fn a_well_formed_id_token_yields_a_principal() {
        let principal = oidc()
            .admit(&good(), NONCE, 1_000_000_000)
            .expect("well-formed claims are admitted");

        assert_eq!(principal.kind(), PrincipalKind::User);
        assert_eq!(principal.id(), "248289761001", "the `sub` claim");
        assert_eq!(principal.tenant().as_str(), TENANT);
    }

    /// Every binding check, one at a time, each with the reason it exists.
    ///
    /// Stated as a table because these are the assertions that decide whether a stranger can sign
    /// in as somebody else, and a single test that mutates one field at a time would hide which of
    /// them had stopped working.
    #[test]
    fn every_binding_check_refuses_on_its_own() {
        /// What was wrong, the claims that were wrong, and the refusal that must come back.
        type Case = (&'static str, SignedClaims, fn(&SignInRefusal) -> bool);

        let cases: Vec<Case> = vec![
            (
                // A token from another provider this exchange happens to trust is still not one
                // this host asked for.
                "another issuer",
                SignedClaims {
                    issuer: "https://accounts.attacker.example".to_string(),
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::IssuerMismatch),
            ),
            (
                // A token audienced to a different client is one that client can replay here.
                "another audience",
                SignedClaims {
                    audience: vec!["some-other-client".to_string()],
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::AudienceMismatch),
            ),
            (
                "no audience at all",
                SignedClaims {
                    audience: Vec::new(),
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::AudienceMismatch),
            ),
            (
                "an expired token",
                SignedClaims {
                    expires_at: 999_999_999,
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::Expired),
            ),
            (
                // The replay a nonce exists to stop.
                "another nonce",
                SignedClaims {
                    nonce: Some("a-nonce-from-somewhere-else".to_string()),
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::NonceMismatch),
            ),
            (
                // The one that matters most: a *missing* nonce must refuse, not be waved through.
                // That is the difference between checking a nonce and appearing to.
                "no nonce at all",
                SignedClaims {
                    nonce: None,
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::NonceMismatch),
            ),
            (
                "no subject",
                SignedClaims {
                    subject: String::new(),
                    ..good()
                },
                |refusal| matches!(refusal, SignInRefusal::NoSubject),
            ),
        ];

        let oidc = oidc();

        for (what, claims, expected) in cases {
            let refusal = oidc
                .admit(&claims, NONCE, 1_000_000_000)
                .err()
                .unwrap_or_else(|| panic!("{what} must be refused"));

            assert!(expected(&refusal), "{what} was refused as {refusal:?}");
        }
    }

    /// A token audienced to several clients, one of which is us, is ours. This is the shape a
    /// provider produces when a client has additional resource audiences, and refusing it would
    /// break a legitimate deployment.
    #[test]
    fn an_audience_list_containing_this_client_is_accepted() {
        let claims = SignedClaims {
            audience: vec!["another-client".to_string(), CLIENT_ID.to_string()],
            ..good()
        };

        assert!(oidc().admit(&claims, NONCE, 1_000_000_000).is_ok());
    }

    /// Nothing in an id token names a tenant, and nothing could: the tenant is read from the
    /// configuration. Stated here because `SignedClaims` is the only thing crossing the seam, and a
    /// later field added to it must not become a second source.
    #[test]
    fn no_claim_reaches_the_tenant() {
        let oidc = oidc();

        // Every free-text claim set to a tenant that exists and is not ours.
        let hostile = SignedClaims {
            subject: "globex".to_string(),
            email: Some("someone@globex".to_string()),
            ..good()
        };

        let principal = oidc
            .admit(&hostile, NONCE, 1_000_000_000)
            .expect("the claims are otherwise well formed");

        assert_eq!(
            principal.tenant().as_str(),
            TENANT,
            "the tenant comes from the configuration, never from a claim",
        );
    }

    /// The three binding refusals read **identically to the caller and differently in the log**.
    ///
    /// Both halves matter, and they pull in opposite directions, which is why they are asserted
    /// together rather than in two tests that could drift apart.
    ///
    /// *Identical to the caller*, because `UnknownState` and `AnotherBrowser` are decided by a
    /// lookup in the pending store: telling them apart on the wire would report whether a given
    /// `state` is live, which is the one fact about somebody else's pending sign-in worth guessing.
    ///
    /// *Different in the log*, because they mean different things about who is attacking. A host
    /// seeing `UnknownState` is being sent values it never issued — someone guessing. A host seeing
    /// `AnotherBrowser` or `NoBinder` is watching **real** authorization requests arrive in the
    /// wrong browsers, which is login-CSRF in progress and a different incident entirely.
    #[test]
    fn a_walked_in_callback_reads_differently_in_the_log_from_a_forged_state() {
        let forged = SignInRefusal::UnknownState;
        let no_binder = SignInRefusal::NoBinder;
        let another = SignInRefusal::AnotherBrowser;

        // The caller learns nothing that separates them.
        assert_eq!(forged.caller_facing(), no_binder.caller_facing());
        assert_eq!(forged.caller_facing(), another.caller_facing());

        // The operator does. Distinct lines, and each one names what actually happened.
        let (forged, no_binder, another) = (
            forged.to_string(),
            no_binder.to_string(),
            another.to_string(),
        );

        assert_ne!(forged, no_binder);
        assert_ne!(forged, another);
        assert_ne!(no_binder, another);

        assert!(forged.contains("did not open"), "{forged}");
        assert!(no_binder.contains("no sign-in binder"), "{no_binder}");
        assert!(
            another.contains("did not open it") && another.contains("login CSRF"),
            "an operator must be able to recognise this one by name: {another}",
        );
        // And it says the honest sign-in survived, because the operator's next question is whether
        // this cost somebody their login.
        assert!(another.contains("left unspent"), "{another}");
    }

    /// A caller-facing refusal never carries a value, and never carries what the provider said.
    /// The operator's version may; that is what the log is for.
    #[test]
    fn a_caller_facing_refusal_carries_no_detail() {
        let leaky = SignInRefusal::ProviderUnreachable(
            "dial tcp 10.0.0.7:443: connection refused".to_string(),
        );

        assert!(!leaky.caller_facing().contains("10.0.0.7"));
        assert!(!leaky.caller_facing().contains("connection refused"));

        // The operator's version keeps it, or an outage would be undiagnosable.
        assert!(leaky.to_string().contains("10.0.0.7"));
    }

    /// This port never contacts the provider, so it can never be `Unreachable` on the request path
    /// — which is why there is no provider address for the guard's `503` to leak here.
    #[tokio::test]
    async fn resolving_a_session_never_reaches_the_provider() {
        let oidc = oidc();

        assert!(matches!(oidc.resolve("").await, Ok(None)), "anonymous");
        assert!(
            matches!(
                oidc.resolve("not-a-session").await,
                Err(IdentityError::Rejected)
            ),
            "an unrecognised credential is rejected, never reported as an outage",
        );
    }

    /// The authorization URL is a URL: the query is appended with `?` or `&` depending on what the
    /// configured endpoint already carries. A provider whose endpoint has a query of its own is
    /// common enough that getting this wrong is a sign-in that fails at the provider.
    #[test]
    fn the_authorization_url_appends_to_an_endpoint_that_already_has_a_query() {
        let plain = oidc().authorize().expect("the OS has randomness").url;
        assert!(plain.contains("/authorize?response_type=code"), "{plain}");

        let with_query = Oidc::new(
            OidcConfig::for_test_with_endpoint(
                ISSUER,
                CLIENT_ID,
                TENANT,
                &format!("{ISSUER}/authorize?realm=staff"),
            ),
            Arc::new(Unused),
        )
        .authorize()
        .expect("the OS has randomness")
        .url;

        assert!(
            with_query.contains("?realm=staff&response_type=code"),
            "{with_query}",
        );
    }

    /// Values that go into the URL are percent-encoded, so a client id or redirect URI containing
    /// a reserved character does not silently truncate the query at the provider.
    #[test]
    fn configured_values_are_percent_encoded_into_the_url() {
        assert_eq!(urlencoded("flux-exchange"), "flux-exchange");
        assert_eq!(
            urlencoded("openid email profile"),
            "openid%20email%20profile"
        );
        assert_eq!(
            urlencoded("https://a.example/cb?x=1&y=2"),
            "https%3A%2F%2Fa.example%2Fcb%3Fx%3D1%26y%3D2",
        );
    }
}
