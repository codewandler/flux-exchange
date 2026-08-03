//! Federated sign-in: the authorization-code flow, with PKCE, `state` and `nonce`.
//!
//! # What signing in is, and what it is not
//!
//! Signing in establishes **who the human is**. It asks the provider only for `openid` — see
//! [`SCOPES`](config::SCOPES) — and it mints nothing for any vendor. Connecting a
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
pub mod http_exchange;
pub mod pkce;

use std::fmt;
use std::sync::Arc;

use axum::http::StatusCode;
use exchange_host::{async_trait, Identity, IdentityError, Principal, PrincipalKind};

use crate::session::{now, Expiry, SessionError, SessionStore, SessionToken};
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

/// A provider answer admitted as one principal, before it becomes a local session.
///
/// Kept inside this module so the route can durably mark the session attempt in the one safe gap:
/// after every identity check passed, before [`SessionStore::open`] mutates local authority.
pub(crate) struct SessionAdmission {
    principal: Principal,
    expires_at: i64,
    as_of: i64,
}

impl SessionAdmission {
    pub(crate) fn principal(&self) -> &Principal {
        &self.principal
    }
}

impl Oidc {
    /// Bind a provider, given something that can redeem an authorization code.
    ///
    /// Takes the exchange rather than constructing one, because a composition that cannot supply
    /// one must not end up with a half-built flow that fails at the callback. See
    /// [`SignIn`](crate::state::SignIn).
    ///
    /// The binary composes this with [`HttpTokenExchange`](super::oidc::http_exchange::HttpTokenExchange);
    /// see `docs/designs/oidc-signin.md`.
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
             &code_challenge_method={method}{hosted_domain}",
            endpoint = self.config.authorization_endpoint(),
            client_id = urlencoded(self.config.client_id()),
            redirect_uri = urlencoded(self.config.redirect_uri()),
            scope = urlencoded(SCOPES),
            state = begun.state,
            nonce = begun.nonce,
            challenge = begun.challenge.as_str(),
            method = pkce::METHOD,
            hosted_domain = self
                .config
                .hosted_domain()
                .map(|domain| format!("&hd={}", urlencoded(domain)))
                .unwrap_or_default(),
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
        let admission = self.complete_admission(state, code, binder).await?;
        self.open_admitted(admission)
    }

    /// Redeem and verify a callback, stopping immediately before local session creation.
    pub(crate) async fn complete_admission(
        &self,
        state: &str,
        code: &str,
        binder: &str,
    ) -> Result<SessionAdmission, SignInRefusal> {
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
            .map_err(SignInRefusal::from)?;

        // The one reading this sign-in gets, taken once the provider has answered so that a slow
        // token endpoint cannot make it stale, and passed through session creation unchanged.
        let as_of = now();
        let principal = self.admit(&claims, &pending.nonce, as_of)?;
        Ok(SessionAdmission {
            principal,
            expires_at: claims.expires_at,
            as_of,
        })
    }

    /// Open the already-admitted local session.
    pub(crate) fn open_admitted(
        &self,
        admission: SessionAdmission,
    ) -> Result<SessionToken, SignInRefusal> {
        self.sessions
            .open(
                admission.principal,
                Expiry::Credential {
                    expires_at: admission.expires_at,
                    as_of: admission.as_of,
                },
            )
            .map_err(SignInRefusal::NoSession)
    }

    /// Close the local session represented by the token a guarded request presented.
    pub fn close_session(&self, presented: &str) {
        self.sessions.close(presented);
    }

    /// Open a short-lived federated session without a provider round trip, for route integration
    /// tests whose subject is what happens after the flow has completed.
    #[cfg(test)]
    pub(crate) fn open_session_for_test(&self, principal: Principal) -> SessionToken {
        let as_of = now();
        self.sessions
            .open(
                principal,
                Expiry::Credential {
                    expires_at: as_of + 300,
                    as_of,
                },
            )
            .expect("a five-minute test session is admissible")
    }

    /// Admit a signature-verified id token and open the session it earns, **both against `now`**.
    ///
    /// One argument and one reading, which is the whole of X-24. Whether the token has expired and
    /// how much of its life the session inherits are two questions about the same instant, and this
    /// host used to ask the clock once for each: [`Oidc::admit`] took a reading, and
    /// [`SessionStore::open`] took another a moment later. A token whose `exp` fell between them was
    /// admitted by the first and refused [`SessionError::AlreadyExpired`] by the second. Nothing was
    /// issued — it failed in the safe direction — but the caller was told this host could not open a
    /// session, `SignInRefusal::NoSession`'s `503`, when what had happened was that their token
    /// expired, which is [`SignInRefusal::Expired`]'s `401`, and the operator's log said the same
    /// wrong thing. Threading the reading through here leaves no line between the two decisions that
    /// could consult the clock again.
    ///
    /// Separate from [`Oidc::complete`] and taking `now` as an argument for the reason
    /// [`Oidc::admit`] does: the window this closes is sub-second, so the only way to state it as a
    /// test is to name the instant rather than to race one.
    #[cfg(test)]
    fn admit_and_open(
        &self,
        claims: &SignedClaims,
        expected_nonce: &str,
        now: i64,
    ) -> Result<SessionToken, SignInRefusal> {
        let principal = self.admit(claims, expected_nonce, now)?;

        // The session ends when the id token does. The `exp` goes across verbatim: a provider that
        // issues a five-minute token gets a five-minute session, because a host that outlived the
        // credential it was shown would be asserting an identity nobody is still vouching for.
        self.open_admitted(SessionAdmission {
            principal,
            expires_at: claims.expires_at,
            as_of: now,
        })
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

        // Google's `hd` parameter on the authorization request is only account-selection UX. The
        // authority decision is this exact comparison against the signature-verified id token.
        if self.config.hosted_domain().is_some()
            && claims.hosted_domain.as_deref() != self.config.hosted_domain()
        {
            return Err(SignInRefusal::HostedDomainMismatch);
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
    ///
    /// That includes a session whose id token has since expired: the store stops resolving it, and
    /// it arrives here as the same `None` an unknown token does, so it is [`IdentityError::Rejected`]
    /// by the same line. This host does **not** ask the provider whether the identity is still
    /// good; it holds the deadline the provider already stated. Refreshing one is a different
    /// story, needing a refresh token this flow deliberately never asked for.
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

    /// The provider refused **this host's** client credentials, not the caller's code.
    ///
    /// X-17, and the same move as the three above: kept distinct **in the log only**, because the
    /// operator's next action is entirely different and the caller's is not. See
    /// [`SignInRefusal::caller_facing`].
    ClientRefused,

    /// The id token was signed by a key the configured key set does not publish.
    ///
    /// Log-only, as above. A wrong `FLUX_EXCHANGE_OIDC_JWKS_URI` and a stranger's `kid` both land
    /// here, and an operator tells them apart by whether *every* sign-in fails this way.
    UnpublishedKey,

    /// The exchange returned no id token, so nothing said who the human is. Log-only, as above.
    NoIdToken,

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

    /// The id token omitted or mismatched the configured signed hosted-domain claim.
    HostedDomainMismatch,

    /// The authorization request could not be opened.
    NoFlow(FlowError),

    /// The session could not be minted.
    NoSession(SessionError),
}

impl SignInRefusal {
    /// The status the caller is answered with.
    ///
    /// The other half of [`caller_facing`](Self::caller_facing), and here for the same reason: a
    /// refusal decides *what a caller learns*, and `routes::signin` decides only how to render it —
    /// the page, the headers, and which line goes to the log. Several of the arms below are
    /// load-bearing security decisions, and every one of them is an argument about a refusal rather
    /// than about a page.
    ///
    /// This lived inline in `routes::signin::callback` until X-26. Keeping it there cost a test:
    /// proving that an expired id token answers `401` rather than `503` needed a *second*,
    /// router-level case, because the test that knew which refusal had been produced had no way to
    /// ask what status it carried. The argument was already half here anyway —
    /// [`caller_facing`](Self::caller_facing) says three times that a group of refusals shares "one
    /// phrase and one status", and the route's comments pointed back at it for the reasoning.
    pub fn status(&self) -> StatusCode {
        match self {
            // The caller's problem, and what X-04's and X-15's failing-first tests drive. All three
            // answer `400` with the same phrase; only the log tells them apart, and
            // `caller_facing` carries the argument for that.
            Self::UnknownState | Self::NoBinder | Self::AnotherBrowser => StatusCode::BAD_REQUEST,
            // The back-channel refusals. X-17 split four of these apart in the log, and the status
            // is the other half of "the caller learns nothing that separates them": a `503` for
            // `ClientRefused` alone would report this host's registration state to an
            // unauthenticated caller with a made-up code. `caller_facing` carries that argument in
            // full.
            Self::CodeRejected
            | Self::ClientRefused
            | Self::UnpublishedKey
            | Self::NoIdToken
            | Self::IssuerMismatch
            | Self::AudienceMismatch
            | Self::Expired
            | Self::NonceMismatch
            | Self::NoSubject
            | Self::HostedDomainMismatch => StatusCode::UNAUTHORIZED,
            // This host's problem, kept distinct all the way out: an operator answers an outage and
            // a bad credential in opposite ways.
            Self::ProviderUnreachable(_) | Self::NoFlow(_) | Self::NoSession(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }

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
    ///
    /// # And so do the four back-channel ones, for a sharper reason
    ///
    /// [`CodeRejected`](Self::CodeRejected), [`ClientRefused`](Self::ClientRefused),
    /// [`UnpublishedKey`](Self::UnpublishedKey) and [`NoIdToken`](Self::NoIdToken) also share one
    /// phrase and one status. X-17 split them in the log precisely *because* three of them are the
    /// operator's fault and one is the caller's — which is exactly why the wire must not say which.
    ///
    /// A distinguishable answer here would make `/api/signin/callback` report **this host's own
    /// configuration state** to anybody who can reach it, unauthenticated and with a made-up code:
    /// whether this host's registration at the provider is currently good, whether its key set URI
    /// resolves, whether it asks for `openid`. That is a reconnaissance oracle for a deployment, and
    /// it would be one bought for nothing, because the caller's remedy is identical in all four
    /// cases. [`ProviderUnreachable`](Self::ProviderUnreachable) is the deliberate exception and
    /// stays one: an outage is transient, "try again shortly" is honest advice rather than a
    /// diagnosis, and telling a human to reset a working password during one is the failure X-03
    /// already refused to ship.
    pub fn caller_facing(&self) -> &'static str {
        match self {
            Self::UnknownState | Self::NoBinder | Self::AnotherBrowser => {
                "this sign-in could not be matched to one that started here. Start again from the \
                 sign-in page"
            }
            Self::CodeRejected
            | Self::ClientRefused
            | Self::UnpublishedKey
            | Self::NoIdToken
            | Self::IssuerMismatch
            | Self::AudienceMismatch
            | Self::Expired
            | Self::NonceMismatch
            | Self::NoSubject
            | Self::HostedDomainMismatch => {
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

impl From<ExchangeError> for SignInRefusal {
    /// One refusal per exchange failure, so the split the exchange made survives the trip out.
    ///
    /// A named impl rather than the closure this used to be inside [`Oidc::complete`]: it is the
    /// single point where a new [`ExchangeError`] could be folded back into an existing refusal and
    /// quietly undo X-17, and it is what `http_exchange`'s tests reach for to assert both channels
    /// at once.
    ///
    /// **The arms below must stay one-to-one**, and `tests::every_exchange_error_names_the_refusal_it_becomes`
    /// is what holds that. Exhaustiveness alone does not: it forces a new variant to be given *an*
    /// arm, not a *distinct* one, and an arm reusing an existing refusal inherits its status and its
    /// log line without `status()` changing — which is why X-26's guard cannot see it. Folding two
    /// causes together here is a decision to argue for in that test, not a line to add here.
    fn from(error: ExchangeError) -> Self {
        match error {
            ExchangeError::Rejected => Self::CodeRejected,
            ExchangeError::ClientRefused => Self::ClientRefused,
            ExchangeError::UnpublishedKey => Self::UnpublishedKey,
            ExchangeError::NoIdToken => Self::NoIdToken,
            ExchangeError::Unreachable(reason) => Self::ProviderUnreachable(reason),
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
            // The four X-17 separated. Each one is `ExchangeError`'s own words, because that is the
            // layer that knows which of them happened and there is no second place to keep them
            // correct. See that type's documentation.
            Self::CodeRejected => write!(f, "{}", ExchangeError::Rejected),
            Self::ClientRefused => write!(f, "{}", ExchangeError::ClientRefused),
            Self::UnpublishedKey => write!(f, "{}", ExchangeError::UnpublishedKey),
            Self::NoIdToken => write!(f, "{}", ExchangeError::NoIdToken),
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
            Self::HostedDomainMismatch => f.write_str(
                "the id token omitted or mismatched the configured signed hosted-domain claim",
            ),
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

    fn domain_restricted_oidc() -> Oidc {
        Oidc::new(
            OidcConfig::for_test(ISSUER, CLIENT_ID, TENANT)
                .with_hosted_domain_for_test("example.com"),
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
            hosted_domain: Some("example.com".to_string()),
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

    /// **X-87's failing-first logout test.** Closing the presented OIDC session invalidates the
    /// server-side authority, not only the browser's copy of it.
    #[tokio::test]
    async fn closing_an_oidc_session_stops_it_resolving() {
        let oidc = oidc();
        let token = oidc
            .admit_and_open(&good(), NONCE, 1_999_999_700)
            .expect("well-formed claims open a session");

        assert!(oidc.resolve(token.as_str()).await.is_ok());
        oidc.close_session(token.as_str());
        assert!(
            matches!(
                oidc.resolve(token.as_str()).await,
                Err(IdentityError::Rejected)
            ),
            "logout must invalidate the authority held by the server"
        );
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

    /// **X-90's failing-first admission table.** The authorization hint earns no authority; only
    /// the exact signature-verified `hd` claim does.
    #[test]
    fn a_configured_hosted_domain_requires_an_exact_signed_claim() {
        let oidc = domain_restricted_oidc();

        assert!(oidc.admit(&good(), NONCE, 1_000_000_000).is_ok());
        for claims in [
            SignedClaims {
                hosted_domain: None,
                ..good()
            },
            SignedClaims {
                hosted_domain: Some("attacker.example".to_string()),
                ..good()
            },
            SignedClaims {
                hosted_domain: Some("EXAMPLE.COM".to_string()),
                ..good()
            },
        ] {
            assert!(
                matches!(
                    oidc.admit(&claims, NONCE, 1_000_000_000),
                    Err(SignInRefusal::HostedDomainMismatch)
                ),
                "a missing, mismatched, or differently-cased signed claim must refuse"
            );
        }
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
            hosted_domain: Some("globex".to_string()),
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

    /// **X-16.** A session whose id token has expired is refused exactly as one that never existed.
    ///
    /// Asserted at this port rather than at the store, because this is where the two answers could
    /// diverge: a later change that reported an expired session differently — a distinct
    /// `IdentityError`, a different status — would make the callback an oracle for which tokens
    /// used to be sessions, and the remedy is the same for both anyway.
    #[tokio::test]
    async fn an_expired_session_is_refused_exactly_as_one_that_never_existed() {
        let oidc = oidc();

        // Opened the way `complete` opens one: bound to the id token's `exp`.
        let token = oidc
            .sessions
            .open(
                oidc.admit(&good(), NONCE, 1_000_000_000)
                    .expect("well-formed claims"),
                Expiry::Credential {
                    expires_at: now() + 300,
                    as_of: now(),
                },
            )
            .expect("the OS has randomness");

        assert!(
            matches!(oidc.resolve(token.as_str()).await, Ok(Some(_))),
            "a session inside its id token's lifetime resolves",
        );

        oidc.sessions.expire_now(token.as_str());

        let expired = oidc.resolve(token.as_str()).await;
        let never_existed = oidc.resolve("a-token-this-host-never-minted").await;

        assert!(matches!(expired, Err(IdentityError::Rejected)));
        assert!(matches!(never_existed, Err(IdentityError::Rejected)));
        assert_eq!(
            format!("{expired:?}"),
            format!("{never_existed:?}"),
            "an expired session must not be distinguishable from one that never existed",
        );
    }

    /// **X-24.** One sign-in decides against one reading of the clock.
    ///
    /// Two cases either side of a single injected instant, because separately each of them passes
    /// for the wrong reason:
    ///
    /// 1. An `exp` **at** the reading is expired — a session that ends at `t` does not resolve at
    ///    `t`, and the credential behind it is no different. The caller is told the id token
    ///    expired, which `routes::signin` answers `401`, rather than that this host could not open
    ///    a session, which is [`SignInRefusal::NoSession`]'s `503`.
    /// 2. An `exp` **one second past** it opens a session. This is the half a second reading
    ///    breaks: the token is admitted against the first reading and then refused
    ///    [`SessionError::AlreadyExpired`] by a later one, so the answer describes the store when
    ///    what happened was that the token expired. Without this case the test would pass on a host
    ///    that refused every sign-in.
    ///
    /// The instant is injected rather than raced. The window a second reading opens is sub-second,
    /// so a test that waited for it would be one that usually proved nothing; [`Oidc::admit`]
    /// already takes `now` as an argument for exactly this reason.
    #[test]
    fn a_sign_in_decides_against_one_reading_of_the_clock() {
        // Deliberately not the wall clock, so neither case can be decided by one.
        const NOW: i64 = 1_000_000_000;

        let oidc = oidc();

        let refusal = oidc
            .admit_and_open(
                &SignedClaims {
                    expires_at: NOW,
                    ..good()
                },
                NONCE,
                NOW,
            )
            .expect_err("a token whose `exp` is the moment it is read against has expired");

        assert!(
            matches!(refusal, SignInRefusal::Expired),
            "an `exp` on the boundary must be refused as an expired credential, not as a store \
             that could not open a session: {refusal:?}",
        );

        let token = oidc
            .admit_and_open(
                &SignedClaims {
                    expires_at: NOW + 1,
                    ..good()
                },
                NONCE,
                NOW,
            )
            .expect("a token still inside its life must open a session against that same reading");

        assert!(
            oidc.sessions.resolve(token.as_str()).is_some(),
            "and the session it opened must resolve",
        );
    }

    /// **X-26's failing-first test.** The refusal and the status it answers with, in one assertion,
    /// from beside the refusal.
    ///
    /// The same fact that
    /// `routes::signin::tests::an_expired_id_token_is_refused_as_a_credential_and_not_as_an_outage`
    /// drives through the router. X-24 had to write that second, route-level test because this one
    /// could not be written: the test that knows *which* refusal was produced had no way to ask what
    /// status it carries. Both are kept — that one proves the route renders it, this one proves the
    /// refusal carries it — but the mapping's own proof belongs next to the mapping.
    #[test]
    fn an_expired_id_token_carries_the_status_of_a_rejected_credential() {
        const NOW: i64 = 1_000_000_000;

        let refusal = oidc()
            .admit_and_open(
                &SignedClaims {
                    expires_at: NOW,
                    ..good()
                },
                NONCE,
                NOW,
            )
            .expect_err("a token whose `exp` is the moment it is read against has expired");

        assert_eq!(
            refusal.status(),
            StatusCode::UNAUTHORIZED,
            "an expired credential is the caller's problem: a `503` sends a human away to try again \
             shortly when what they need is to sign in again ({refusal:?})",
        );
    }

    /// **X-26.** Every refusal, and the status a caller is answered with — one arm per variant.
    ///
    /// [`SignInRefusal::status`] groups its variants, because the groups *are* the argument: three
    /// refusals that must read alike, four that must not be told apart, and a set that is this
    /// host's fault rather than the caller's. This test deliberately does not group them, so a
    /// change to the mapping has to be written down here as "*this* variant now answers *this*" —
    /// which is the thing a router-level test cannot say, because it names a request rather than a
    /// refusal.
    ///
    /// The `match` is also the compiler's half of the job: a variant added to [`SignInRefusal`] does
    /// not compile here until somebody states what a caller learns about it.
    #[test]
    fn every_refusal_states_the_status_it_answers_with() {
        fn stated(refusal: &SignInRefusal) -> StatusCode {
            match refusal {
                // Nothing presented named a sign-in this host opened.
                SignInRefusal::UnknownState => StatusCode::BAD_REQUEST,
                SignInRefusal::NoBinder => StatusCode::BAD_REQUEST,
                SignInRefusal::AnotherBrowser => StatusCode::BAD_REQUEST,
                // The four back-channel refusals, which a caller may not tell apart.
                SignInRefusal::CodeRejected => StatusCode::UNAUTHORIZED,
                SignInRefusal::ClientRefused => StatusCode::UNAUTHORIZED,
                SignInRefusal::UnpublishedKey => StatusCode::UNAUTHORIZED,
                SignInRefusal::NoIdToken => StatusCode::UNAUTHORIZED,
                // The id token was shown and not accepted.
                SignInRefusal::IssuerMismatch => StatusCode::UNAUTHORIZED,
                SignInRefusal::AudienceMismatch => StatusCode::UNAUTHORIZED,
                SignInRefusal::Expired => StatusCode::UNAUTHORIZED,
                SignInRefusal::NonceMismatch => StatusCode::UNAUTHORIZED,
                SignInRefusal::NoSubject => StatusCode::UNAUTHORIZED,
                SignInRefusal::HostedDomainMismatch => StatusCode::UNAUTHORIZED,
                // This host could not do its job, and says so rather than blaming the caller.
                SignInRefusal::ProviderUnreachable(_) => StatusCode::SERVICE_UNAVAILABLE,
                SignInRefusal::NoFlow(_) => StatusCode::SERVICE_UNAVAILABLE,
                SignInRefusal::NoSession(_) => StatusCode::SERVICE_UNAVAILABLE,
            }
        }

        // One of every variant, so the loop walks the whole type rather than the handful a test
        // happened to think of. The list itself is not compiler-checked — `stated` above is what
        // stops a variant being added without anyone stating what a caller learns about it — so the
        // count below rules out the one way this can rot quietly: an entry repeated where a new
        // variant was meant to go.
        let every = [
            SignInRefusal::UnknownState,
            SignInRefusal::NoBinder,
            SignInRefusal::AnotherBrowser,
            SignInRefusal::CodeRejected,
            SignInRefusal::ClientRefused,
            SignInRefusal::UnpublishedKey,
            SignInRefusal::NoIdToken,
            SignInRefusal::ProviderUnreachable("dial tcp 10.0.0.7:443".to_string()),
            SignInRefusal::IssuerMismatch,
            SignInRefusal::AudienceMismatch,
            SignInRefusal::Expired,
            SignInRefusal::NonceMismatch,
            SignInRefusal::NoSubject,
            SignInRefusal::HostedDomainMismatch,
            SignInRefusal::NoFlow(FlowError::NoEntropy {
                source: std::io::Error::other("the test's, not the OS's"),
            }),
            SignInRefusal::NoSession(SessionError::TooManyLive { max: 0 }),
        ];

        let distinct: std::collections::HashSet<_> =
            every.iter().map(std::mem::discriminant).collect();
        assert_eq!(
            distinct.len(),
            every.len(),
            "one of every refusal, and no repeats",
        );

        for refusal in &every {
            assert_eq!(refusal.status(), stated(refusal), "{refusal:?}");
        }
    }

    /// **X-17, restated at the type.** The four back-channel refusals are a *single* answer: one
    /// status and one phrase.
    ///
    /// Both halves together, because either one alone is the leak. A status that told them apart
    /// would report **this host's own configuration state** — whether its registration at the
    /// provider is currently good, whether its key set URI resolves — to anybody who can reach
    /// `/api/signin/callback` unauthenticated with a made-up code.
    /// [`SignInRefusal::caller_facing`] carries that argument in full; this holds it.
    ///
    /// `routes::signin::tests::a_refusal_tells_the_caller_nothing_about_the_provider` holds the same
    /// line through the router, and both are kept: that one proves what the wire carries, this one
    /// proves the refusals already agreed before anything rendered them.
    #[test]
    fn the_four_back_channel_refusals_are_a_single_answer() {
        let back_channel = [
            SignInRefusal::CodeRejected,
            SignInRefusal::ClientRefused,
            SignInRefusal::UnpublishedKey,
            SignInRefusal::NoIdToken,
        ];

        for refusal in &back_channel {
            assert_eq!(refusal.status(), back_channel[0].status(), "{refusal:?}");
            assert_eq!(
                refusal.caller_facing(),
                back_channel[0].caller_facing(),
                "{refusal:?}",
            );
        }

        // The deliberate exception stays one. An outage is transient, "try again shortly" is honest
        // advice rather than a diagnosis, and telling a human to reset a working password during one
        // is the failure X-03 already refused to ship.
        let unreachable =
            SignInRefusal::ProviderUnreachable("dial tcp 10.0.0.7:443: connection refused".into());

        assert_ne!(unreachable.status(), back_channel[0].status());
        assert_ne!(unreachable.caller_facing(), back_channel[0].caller_facing());
    }

    /// **X-31, the failing-first test.** Every [`ExchangeError`], and the refusal it becomes — one
    /// arm per variant, and no two arms arriving at the same refusal.
    ///
    /// The other half of [`every_refusal_states_the_status_it_answers_with`], which guards the
    /// refusal→status edge and, as the story that wrote it said, nothing else. A new
    /// [`ExchangeError`] folded into an existing refusal inherits that refusal's status *and* its log
    /// line without `status()` changing at all — and undoes X-17, the split that exists because four
    /// causes were once one refusal and an operator could not tell their own misconfiguration from a
    /// caller's refused credential.
    ///
    /// Two claims, because either one alone leaves the fold reachable:
    ///
    /// 1. **`names` states the pairing.** Its `match` is exhaustive, so a variant added to
    ///    [`ExchangeError`] tomorrow does not compile here until somebody writes down which refusal
    ///    it produces. [`SignInRefusal::from`] is exhaustive too and forces an *arm* — it does not
    ///    force a *distinct* one, and that is precisely the hole.
    /// 2. **The refusals are distinct.** A fold is invisible to a per-variant assertion: mapping
    ///    `NoIdToken` onto `CodeRejected` satisfies "this error produces that refusal" perfectly
    ///    well. What it cannot satisfy is *two* errors producing *two* refusals, so injectivity is
    ///    asserted across the whole mapping rather than case by case.
    ///
    /// A future variant that genuinely belongs on an existing refusal is not forbidden. It is turned
    /// into an edit of this test, made with a reason, instead of a line nobody reads.
    #[test]
    fn every_exchange_error_names_the_refusal_it_becomes() {
        // The one variant carrying a value, so the pairing below can also say the value survives.
        const REASON: &str = "dial tcp 10.0.0.7:443: connection refused";

        fn names(error: &ExchangeError) -> fn(&SignInRefusal) -> bool {
            match error {
                // The caller's credential, and the only one of the four that is.
                ExchangeError::Rejected => |refusal| matches!(refusal, SignInRefusal::CodeRejected),
                // X-17's three: this host's registration, this host's key set URI, this host's
                // scopes. An operator answers each of them somewhere the caller cannot see.
                ExchangeError::ClientRefused => {
                    |refusal| matches!(refusal, SignInRefusal::ClientRefused)
                }
                ExchangeError::UnpublishedKey => {
                    |refusal| matches!(refusal, SignInRefusal::UnpublishedKey)
                }
                ExchangeError::NoIdToken => |refusal| matches!(refusal, SignInRefusal::NoIdToken),
                // The reason is checked, not just the variant: it names this host's own
                // dependencies, and an outage reported without the address it failed at is a `503`
                // an operator can do nothing with.
                ExchangeError::Unreachable(_) => {
                    |refusal| matches!(refusal, SignInRefusal::ProviderUnreachable(reason) if reason == REASON)
                }
            }
        }

        // One of every variant. The list is not compiler-checked — `names` above is what stops a
        // variant being added without anyone stating what it becomes — so the count below rules out
        // the one way this list rots quietly: an entry repeated where a new variant was meant to go.
        let every = [
            ExchangeError::Rejected,
            ExchangeError::ClientRefused,
            ExchangeError::UnpublishedKey,
            ExchangeError::NoIdToken,
            ExchangeError::Unreachable(REASON.to_string()),
        ];

        let distinct: std::collections::HashSet<_> =
            every.iter().map(std::mem::discriminant).collect();
        assert_eq!(
            distinct.len(),
            every.len(),
            "one of every exchange failure, and no repeats",
        );

        let mut produced = std::collections::HashSet::new();

        for error in every {
            let stated = names(&error);
            let described = format!("{error:?}");
            let refusal = SignInRefusal::from(error);

            assert!(
                stated(&refusal),
                "{described} produces {refusal:?}, which is not the refusal this test states it \
                 becomes",
            );
            assert!(
                produced.insert(std::mem::discriminant(&refusal)),
                "{described} produces {refusal:?}, which another exchange failure already produces. \
                 That is the fold X-17 undid: two causes become one line in the log and one status, \
                 and `every_refusal_states_the_status_it_answers_with` cannot see it because \
                 `status()` never changed",
            );
        }
    }

    /// **X-31, the same edge one type over.** [`SessionError`] collapses into a *single* refusal, and
    /// the log is the only thing separating its causes — so the log is what is pinned.
    ///
    /// This edge cannot fold the way [`From<ExchangeError>`] can: [`SignInRefusal::NoSession`]
    /// *carries* its source rather than replacing it, so one arm is enough and a new
    /// [`SessionError`] arrives at the log intact. Asserted here rather than assumed, because that
    /// is the whole reason one arm is acceptable.
    ///
    /// What can still rot is the property underneath it — that the four causes read differently once
    /// they get there. X-17's implementor checked that by hand; this holds it. They are answers to
    /// four different operator questions: a dead entropy source, a store at its ceiling, a credential
    /// that expired before it arrived, and a provider issuing tokens longer-lived than this host will
    /// honour. A `503` that does not say which is a page reload and a shrug.
    #[test]
    fn every_session_failure_reads_differently_through_the_refusal_carrying_it() {
        fn why(error: &SessionError) -> &'static str {
            match error {
                SessionError::NoEntropy { .. } => "the OS randomness source is unreadable",
                SessionError::TooManyLive { .. } => "the store is at its ceiling",
                SessionError::AlreadyExpired { .. } => "the credential expired before it arrived",
                SessionError::ImplausibleLifetime { .. } => {
                    "the credential outlives what is honoured"
                }
            }
        }

        // As above: `why` forces a new variant to be named, and the count forbids a repeat here.
        let every = [
            SessionError::NoEntropy {
                source: std::io::Error::other("the test's, not the OS's"),
            },
            SessionError::TooManyLive { max: 64 },
            SessionError::AlreadyExpired {
                expires_at: 1_000_000_000,
            },
            SessionError::ImplausibleLifetime {
                seconds: 999_999,
                max: 43_200,
            },
        ];

        let distinct: std::collections::HashSet<_> =
            every.iter().map(std::mem::discriminant).collect();
        assert_eq!(
            distinct.len(),
            every.len(),
            "one of every session failure, and no repeats",
        );

        let mut lines: Vec<(&'static str, String)> = Vec::new();

        for error in every {
            let cause = why(&error);
            let source = error.to_string();
            let refusal = SignInRefusal::NoSession(error);

            assert_eq!(
                refusal.to_string(),
                source,
                "the refusal must carry {cause} through to the log, not restate it",
            );

            for (seen, line) in &lines {
                assert_ne!(
                    &refusal.to_string(),
                    line,
                    "{cause} reads in the log exactly like {seen}, and they are the same `503`: an \
                     operator has nothing left to tell them apart by",
                );
            }

            lines.push((cause, refusal.to_string()));
        }
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
        assert_eq!(urlencoded("openid"), "openid");
        assert_eq!(
            urlencoded("https://a.example/cb?x=1&y=2"),
            "https%3A%2F%2Fa.example%2Fcb%3Fx%3D1%26y%3D2",
        );
    }

    /// The hosted domain in the authorization URL is only a Google account-selection hint. The
    /// admission test above separately proves it cannot substitute for the signed claim.
    #[test]
    fn a_hosted_domain_is_sent_as_an_encoded_authorization_hint() {
        let url = Oidc::new(
            OidcConfig::for_test(ISSUER, CLIENT_ID, TENANT)
                .with_hosted_domain_for_test("example.com&prompt=none"),
            Arc::new(Unused),
        )
        .authorize()
        .expect("the OS has randomness")
        .url;

        assert!(url.contains("&scope=openid&"), "{url}");
        assert!(url.ends_with("&hd=example.com%26prompt%3Dnone"), "{url}");
        assert_eq!(
            url.matches("prompt=").count(),
            0,
            "the hint cannot inject a query: {url}"
        );
    }
}
