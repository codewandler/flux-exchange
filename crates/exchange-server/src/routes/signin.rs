//! Federated sign-in over HTTP: the redirect out, and the callback back.
//!
//! # These two routes are anonymous, and they have to be
//!
//! A caller arriving at `/api/signin` has nothing to present — obtaining something to present is
//! what it is here for — and a caller arriving at the callback is a browser mid-redirect from the
//! provider. `docs/designs/identity-and-session.md` left this widening to this story with an
//! explicit instruction to argue for it rather than inherit it; the argument is in
//! `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous`, beside each entry.
//!
//! # The callback mints a session for a caller that presented no credential. Why that is safe
//!
//! X-03 closed an escalation: `POST /api/session` mints a *readable* token only for a caller that
//! presented a readable credential, so script holding only the `HttpOnly` cookie cannot exchange it
//! for one it can read and exfiltrate. The callback here answers a caller that presented **no**
//! credential at all, which looks like the same door reopened from the other side. It is not, and
//! the reason is structural rather than a branch anyone has to remember:
//!
//! 1. **The callback reads no credential.** It never looks at [`Carrier`](super::Carrier), never
//!    resolves a principal, and never consults the session cookie. Holding one neither helps nor is
//!    required, so a cookie is not an input that can be upgraded. Its authority comes from one
//!    place: a `state` this host drew from the OS, remembered, and has not yet spent.
//! 2. **It answers with a document, not a body a script reads.** The response is HTML and a
//!    `Set-Cookie`. There is no JSON, no field, and therefore no place a readable token could
//!    appear — the session leaves here only as a cookie the browser stores and script cannot read.
//!
//! So the invariant X-03 wrote for one route is stated once more, wider, and this module is inside
//! it: **no route reachable without a readable credential ever puts a session token in a body.**
//! `the_callback_issues_a_session_only_as_a_cookie` drives the *successful* path — the interesting
//! one, since the refusals issue nothing at all — and checks that nothing token-shaped comes back.
//!
//! # Why the callback is a page and not a redirect
//!
//! The session cookie is `SameSite=Strict`, which X-03 chose and this story is not going to weaken.
//! A `303` from here would be the tail of a redirect chain that began at the provider's origin, and
//! a `Strict` cookie is withheld on a request whose chain started cross-site — the browser would
//! store the session and then not send it, and the operator would see a sign-in that silently did
//! nothing. Answering with a small page whose meta-refresh navigates to the console makes that next
//! request one this document initiated, which is same-site, and the cookie travels. The alternative
//! was `SameSite=Lax`, which would have widened a control that is doing real work in order to save
//! a page.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use serde::Deserialize;
use tracing::{error, warn};

use super::{Access, Module, Route};
use crate::oidc::config::{
    AUTHORIZATION_ENDPOINT_ENV, CLIENT_ID_ENV, CLIENT_SECRET_ENV, ISSUER_ENV, REDIRECT_URI_ENV,
    TENANT_ENV,
};
use crate::oidc::SignInRefusal;
use crate::session;
use crate::state::{AppState, SignIn};

/// Where a completed sign-in sends the browser.
const AFTER_SIGN_IN: &str = "/";

/// This module's contribution to the surface.
///
/// Under `/api` for the reason `routes::identity` is: the console's dev server owns the origin
/// under `vite dev` and proxies `/api` to this host, so a sign-in route outside that prefix would
/// be answered by the SPA fallback rather than by us.
pub(super) const MODULE: Module = Module {
    name: "signin",
    routes: &[
        Route {
            path: "/api/signin",
            access: Access::Anonymous,
            method_router: signin_route,
        },
        Route {
            path: "/api/signin/callback",
            access: Access::Anonymous,
            method_router: callback_route,
        },
    ],
};

fn signin_route() -> MethodRouter<AppState> {
    get(signin)
}

fn callback_route() -> MethodRouter<AppState> {
    get(callback)
}

/// Send the browser to the provider, or explain why it cannot be sent.
async fn signin(State(state): State<AppState>) -> Response {
    let oidc = match state.sign_in() {
        SignIn::Oidc(oidc) => oidc,
        // The Acceptance's fourth item, at the point a human meets it: an explanatory page, not a
        // panic at startup and not a redirect that dies at the callback.
        SignIn::Unconfigured => return unconfigured_page(),
        SignIn::NoTokenExchange => return no_token_exchange_page(),
    };

    match oidc.authorization_url() {
        Ok(url) => (StatusCode::SEE_OTHER, [(header::LOCATION, url)]).into_response(),
        Err(error) => {
            // Names this host's own machinery, so it goes to the log rather than to the caller —
            // which is exactly the split `SignInRefusal` encodes, so the refusal goes through it
            // rather than around it.
            let refusal = SignInRefusal::NoFlow(error);
            error!(reason = %refusal, "cannot open an authorization request");
            refused(&refusal, StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// What the provider sends back.
///
/// Every field is optional so a malformed callback is answered by the refusal below — which says
/// nothing about what was wrong — rather than by axum's own rejection, which would echo the
/// caller's query back at it.
#[derive(Debug, Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
    /// Present when the provider refused. **Never read for its value** — see below.
    error: Option<String>,
}

/// Finish a sign-in.
async fn callback(State(state): State<AppState>, Query(callback): Query<Callback>) -> Response {
    let oidc = match state.sign_in() {
        SignIn::Oidc(oidc) => oidc,
        SignIn::Unconfigured => return unconfigured_page(),
        SignIn::NoTokenExchange => return no_token_exchange_page(),
    };

    // The provider refused, or something walked a browser here with an `error` of its own. Either
    // way the value is not looked at: it is request input, so it may not reach a log line, and it
    // is the provider's words about a credential, so it may not reach the caller.
    if callback.error.is_some() {
        warn!("the identity provider returned an error to the sign-in callback");
        return refused(&SignInRefusal::CodeRejected, StatusCode::UNAUTHORIZED);
    }

    let (Some(presented_state), Some(code)) = (callback.state, callback.code) else {
        // No state, no code, or neither — refused *before* the store is consulted, so this answer
        // is identical whether or not the state named is one this host is holding. That is what
        // stops the callback being an oracle for "is this state live", and it is why the refusal
        // happens here rather than after a lookup.
        // `tests::a_callback_without_a_code_is_not_an_oracle_for_a_live_state` pins both halves:
        // the answers match byte for byte, and the probe does not spend what it asked about.
        return refused(&SignInRefusal::UnknownState, StatusCode::BAD_REQUEST);
    };

    let token = match oidc.complete(&presented_state, &code).await {
        Ok(token) => token,
        Err(refusal) => {
            let status = match refusal {
                // The caller's problem, and the one this story's failing-first test drives.
                SignInRefusal::UnknownState => StatusCode::BAD_REQUEST,
                SignInRefusal::CodeRejected
                | SignInRefusal::IssuerMismatch
                | SignInRefusal::AudienceMismatch
                | SignInRefusal::Expired
                | SignInRefusal::NonceMismatch
                | SignInRefusal::NoSubject => StatusCode::UNAUTHORIZED,
                // This host's problem, kept distinct all the way out: an operator answers an
                // outage and a bad credential in opposite ways.
                SignInRefusal::ProviderUnreachable(_)
                | SignInRefusal::NoFlow(_)
                | SignInRefusal::NoSession(_) => StatusCode::SERVICE_UNAVAILABLE,
            };

            // The operator's version goes to the log; `caller_facing` is what the caller sees, and
            // it carries no value from the provider and no address of one.
            warn!(reason = %refusal, "a sign-in did not complete");
            return refused(&refusal, status);
        }
    };

    // The session leaves here as a cookie and in no other form. See the module documentation: this
    // response has no body a script can read a credential out of.
    (
        StatusCode::OK,
        [(header::SET_COOKIE, session::planted(&token))],
        Html(document(
            "Signed in",
            "You are signed in. Returning to the console…",
            Some(AFTER_SIGN_IN),
        )),
    )
        .into_response()
}

/// A refusal the caller sees: a status and a fixed phrase, never a value.
fn refused(refusal: &SignInRefusal, status: StatusCode) -> Response {
    page(status, "Sign-in refused", refusal.caller_facing(), None)
}

/// The page an operator meets when no OIDC configuration was supplied.
///
/// It does **not** enumerate the environment variables. The startup log does, because that is the
/// operator's channel; this page answers anonymous callers, and a list of the variables a host
/// expects is a small map of it. `crate::routes::catalogue` and the `503` in `routes::identity` set
/// the same line: name the remedy's shape, not this deployment's internals.
fn unconfigured_page() -> Response {
    page(
        StatusCode::SERVICE_UNAVAILABLE,
        "Sign-in is not configured",
        "This host has no identity provider configured, so there is no way to sign in. An operator \
         configures one through the environment; the startup log names exactly which settings are \
         missing.",
        None,
    )
}

/// The page an operator meets when OIDC is configured but nothing can redeem an authorization code.
///
/// A separate message from [`unconfigured_page`] because the remedy is a different one, and telling
/// an operator who *has* configured a provider that they have not would send them to re-check six
/// variables that are all correct.
fn no_token_exchange_page() -> Response {
    page(
        StatusCode::SERVICE_UNAVAILABLE,
        "Sign-in is not available in this build",
        "An identity provider is configured, but this build carries no client for the provider's \
         token endpoint, so a sign-in could not be completed. Rather than send you to a login that \
         would fail on the way back, this host stops here. The startup log has the detail.",
        None,
    )
}

/// A small, self-contained HTML answer.
fn page(status: StatusCode, heading: &str, detail: &str, refresh_to: Option<&str>) -> Response {
    (status, Html(document(heading, detail, refresh_to))).into_response()
}

/// The document body.
///
/// Hand-built rather than templated: this binary carries no template engine, and the whole of what
/// varies is two pieces of text this host wrote itself. **Nothing a caller supplied and nothing the
/// provider said is ever interpolated here**, which is what makes the absence of escaping safe —
/// and is the property to check before adding an argument to this function.
fn document(heading: &str, detail: &str, refresh_to: Option<&str>) -> String {
    let refresh = refresh_to
        .map(|target| format!(r#"<meta http-equiv="refresh" content="0; url={target}">"#))
        .unwrap_or_default();

    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         {refresh}\
         <title>{heading} — flux-exchange</title>\n\
         </head>\n\
         <body>\n\
         <main>\n\
         <h1>{heading}</h1>\n\
         <p>{detail}</p>\n\
         </main>\n\
         </body>\n\
         </html>\n",
    )
}

/// Every variable the explanatory page deliberately does not name, kept referenced so that renaming
/// one is a compile error here and the decision above gets re-read rather than silently rotting.
#[allow(dead_code)]
const WITHHELD_FROM_THE_PAGE: &[&str] = &[
    ISSUER_ENV,
    AUTHORIZATION_ENDPOINT_ENV,
    CLIENT_ID_ENV,
    CLIENT_SECRET_ENV,
    REDIRECT_URI_ENV,
    TENANT_ENV,
];

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::header::SET_COOKIE;
    use axum::http::{HeaderMap, Request as HttpRequest};
    use axum::Router;
    use exchange_host::async_trait;
    use tower::Service;

    use crate::oidc::config::OidcConfig;
    use crate::oidc::exchange::{ExchangeError, Redemption, SignedClaims, TokenExchange};
    use crate::oidc::Oidc;
    use crate::session::SESSION_COOKIE;

    /// The routes under test, read from the declaration rather than written out again.
    const SIGNIN: &str = super::MODULE.routes[0].path;
    const CALLBACK: &str = super::MODULE.routes[1].path;

    const ISSUER: &str = "https://accounts.example.com";
    const CLIENT_ID: &str = "flux-exchange";
    const TENANT: &str = "acme";
    const SUBJECT: &str = "248289761001";

    /// A token exchange standing in for the network half: it verifies nothing, because in these
    /// tests there is nothing to verify, and hands back whatever claims the test set.
    ///
    /// The claims are behind a lock so a test can set them *after* starting a sign-in — which is
    /// what lets it model a provider echoing the nonce this host actually bound, rather than one
    /// the test guessed in advance.
    struct StubExchange {
        claims: std::sync::Mutex<SignedClaims>,
        /// Every redemption it was asked for, so a test can assert what crossed the seam.
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl StubExchange {
        fn returning(claims: SignedClaims) -> Self {
            Self {
                claims: std::sync::Mutex::new(claims),
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        /// Answer as a provider that correctly echoes `nonce` back in its id token.
        fn echoing(&self, nonce: &str) {
            self.claims.lock().expect("an unpoisoned lock").nonce = Some(nonce.to_string());
        }

        /// The `(code, verifier)` pairs this exchange was asked to redeem.
        fn redemptions(&self) -> Vec<(String, String)> {
            self.seen.lock().expect("an unpoisoned lock").clone()
        }
    }

    #[async_trait]
    impl TokenExchange for StubExchange {
        async fn redeem(&self, redemption: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
            self.seen.lock().expect("an unpoisoned lock").push((
                redemption.code.to_string(),
                redemption.verifier.as_str().to_string(),
            ));

            Ok(self.claims.lock().expect("an unpoisoned lock").clone())
        }
    }

    /// Claims a well-behaved provider would return for a sign-in bound to `nonce`.
    fn claims(nonce: &str) -> SignedClaims {
        SignedClaims {
            issuer: ISSUER.to_string(),
            audience: vec![CLIENT_ID.to_string()],
            subject: SUBJECT.to_string(),
            nonce: Some(nonce.to_string()),
            expires_at: i64::MAX,
            email: Some("alice@example.com".to_string()),
        }
    }

    fn config() -> OidcConfig {
        OidcConfig::for_test(ISSUER, CLIENT_ID, TENANT)
    }

    /// An app federating sign-in to a provider whose exchange is `exchange`.
    fn oidc_app(exchange: Arc<dyn TokenExchange>) -> Router {
        super::super::app(AppState::with_oidc(Arc::new(Oidc::new(config(), exchange))))
    }

    /// Drive one request through a fully assembled app and hand back everything a caller sees.
    async fn call(app: Router, request: HttpRequest<Body>) -> (StatusCode, HeaderMap, String) {
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let response = service
            .call(request)
            .await
            .expect("a router is infallible")
            .into_response();

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a response body");

        (
            status,
            headers,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    fn get(path: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .expect("a well-formed request")
    }

    /// Whether anything in this text could be a session token.
    ///
    /// The 64-hex *shape*, matching `routes::identity::tests::carries_a_token`, so a rename or a
    /// nesting cannot quietly reopen what it guards.
    fn carries_a_token(text: &str) -> bool {
        text.as_bytes()
            .windows(64)
            .any(|window| window.iter().all(u8::is_ascii_hexdigit))
    }

    /// One query parameter out of a URL.
    fn parameter<'a>(url: &'a str, name: &str) -> Option<&'a str> {
        let query = url.split_once('?')?.1;

        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }

    /// Start a sign-in and hand back the `state` and `nonce` this host bound to it.
    async fn begin(app: &Router) -> (String, String) {
        let (status, headers, _) = call(app.clone(), get(SIGNIN)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "sign-in redirects");

        let location = headers
            .get(header::LOCATION)
            .expect("a redirect names where it goes")
            .to_str()
            .expect("a location is ASCII")
            .to_string();

        (
            parameter(&location, "state")
                .expect("the authorization request carries a state")
                .to_string(),
            parameter(&location, "nonce")
                .expect("the authorization request carries a nonce")
                .to_string(),
        )
    }

    // ---------------------------------------------------------------------------------------
    // The Acceptance's failing-first test.
    // ---------------------------------------------------------------------------------------

    /// **A callback whose `state` does not match the one bound at `/signin` is refused, and no
    /// session is issued.**
    ///
    /// This is the cross-site request forgery `state` exists to stop. Without the check, an
    /// attacker completes a sign-in at the provider as *itself*, then walks a victim's browser into
    /// this callback carrying the attacker's own authorization code — and the victim's browser
    /// silently acquires a session belonging to the attacker's account. Everything the victim then
    /// does, it does in the attacker's tenant, and it looks to the victim like their own session.
    ///
    /// So the assertion is not "the response was not a 200". A refusal that still planted a cookie,
    /// or still handed back something token-shaped, would have signed the victim in while reporting
    /// failure. It is: **nothing that could be a session came back, in any form.**
    ///
    /// The sign-in it contrasts with is driven first, so this cannot pass because the flow is
    /// broken for everyone — that is the failure mode a test like this has, and the assertion at
    /// the end is what rules it out.
    #[tokio::test]
    async fn a_callback_whose_state_was_not_bound_at_signin_issues_no_session() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        // A sign-in really is in flight: this host drew a state and is waiting for it.
        let (bound_state, bound_nonce) = begin(&app).await;

        // The provider behaves perfectly: it echoes the nonce this host bound. That is what makes
        // this test about `state` and about nothing else — every other check the callback makes
        // will pass, so the state check is the only thing between the forged callback and a
        // session. Without it, the assertions below fail and a victim is signed in as somebody
        // else.
        exchange.echoing(&bound_nonce);

        // The attacker's callback. A well-formed authorization code, and a state this host never
        // issued — which is what an attacker has, since it cannot read the victim's.
        let forged = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_ne!(forged, bound_state, "the forged state must differ");

        let (status, headers, body) = call(
            app.clone(),
            get(&format!(
                "{CALLBACK}?state={forged}&code=an-authorization-code"
            )),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a callback this host did not open must be refused: {body}",
        );
        assert!(
            headers.get(SET_COOKIE).is_none(),
            "a refused callback must plant no session cookie: {:?}",
            headers.get(SET_COOKIE),
        );
        assert!(
            !carries_a_token(&body),
            "a refused callback must return nothing token-shaped: {body}",
        );
        assert!(
            !body.contains(SESSION_COOKIE),
            "and must not name the session cookie at all: {body}",
        );

        // The state the host *did* bind must still be unspent — a forged callback that consumed it
        // would be a denial of service against the human who started the real sign-in.
        let (status, headers, _) = call(
            app,
            get(&format!(
                "{CALLBACK}?state={bound_state}&code=an-authorization-code"
            )),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::OK,
            "the genuine callback must still complete, or this test would pass simply because \
             sign-in is broken for everybody",
        );
        assert!(
            headers.get(SET_COOKIE).is_some(),
            "and it is the one that gets a session",
        );
    }

    // ---------------------------------------------------------------------------------------
    // The carrier rule, restated for a route that answers callers holding no credential at all.
    // ---------------------------------------------------------------------------------------

    /// X-03's escalation, closed from the other side.
    ///
    /// `POST /api/session` mints a readable token only for a caller that presented a readable
    /// credential, so script holding only the `HttpOnly` cookie cannot trade it for one it can
    /// exfiltrate. This route answers a caller that presented *nothing*, which is the same door
    /// approached from outside — so the rule it obeys is the wider one:
    ///
    /// > **No route reachable without a readable credential ever puts a session token in a body.**
    ///
    /// Driven on the **successful** path deliberately. The refusals issue nothing at all, so they
    /// could not fail this; the interesting case is the one where a session really is created, and
    /// the assertion is that it leaves only as a cookie.
    #[tokio::test]
    async fn the_callback_issues_a_session_only_as_a_cookie() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        let (state, nonce) = begin(&app).await;
        exchange.echoing(&nonce);

        let (status, headers, body) = call(
            app,
            get(&format!(
                "{CALLBACK}?state={state}&code=an-authorization-code"
            )),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");

        let planted = headers
            .get(SET_COOKIE)
            .expect("a completed sign-in plants a session cookie")
            .to_str()
            .expect("a cookie is ASCII");

        // The session exists, and it is `HttpOnly` — so script cannot read it off the document.
        assert!(
            planted.starts_with(&format!("{SESSION_COOKIE}=")),
            "{planted}"
        );
        assert!(planted.contains("HttpOnly"), "{planted}");
        assert!(planted.contains("Secure"), "{planted}");
        assert!(planted.contains("SameSite=Strict"), "{planted}");

        // And it exists nowhere else. Not as a field, not nested, not incidentally rendered into
        // the page — matched on the 64-hex shape rather than on a key name, so a refactor that
        // renames or re-nests cannot quietly reopen it.
        assert!(
            !carries_a_token(&body),
            "the callback returned something token-shaped in its body, which is a credential a \
             script could read off its own fetch: {body}",
        );
        assert!(
            !body.contains(SESSION_COOKIE),
            "and it must not name the session cookie in the document either: {body}",
        );
    }

    /// A `state` works once. A replay of a callback that already succeeded is refused, and gets the
    /// same answer a forged one gets — the two are indistinguishable from outside, and neither
    /// should yield a second session.
    #[tokio::test]
    async fn a_replayed_state_is_refused() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        let (state, nonce) = begin(&app).await;
        exchange.echoing(&nonce);

        let replay = || {
            get(&format!(
                "{CALLBACK}?state={state}&code=an-authorization-code"
            ))
        };

        let (first, headers, _) = call(app.clone(), replay()).await;
        assert_eq!(first, StatusCode::OK, "the first callback completes");
        assert!(headers.get(SET_COOKIE).is_some());

        let (second, headers, body) = call(app, replay()).await;

        assert_eq!(
            second,
            StatusCode::BAD_REQUEST,
            "a state must be spendable exactly once: {body}",
        );
        assert!(
            headers.get(SET_COOKIE).is_none(),
            "a replayed callback must issue no second session",
        );
        assert!(!carries_a_token(&body), "{body}");
    }

    /// The nonce ties the id token to the authorization request this host opened. A token that
    /// echoes a different one — or none — is a token obtained somewhere else and replayed here.
    #[tokio::test]
    async fn an_id_token_that_does_not_echo_the_bound_nonce_is_refused() {
        for wrong in [Some("a-nonce-from-somewhere-else"), None] {
            let exchange = Arc::new(StubExchange::returning(claims("placeholder")));
            let app = oidc_app(exchange.clone());

            let (state, _) = begin(&app).await;
            exchange.claims.lock().expect("an unpoisoned lock").nonce = wrong.map(str::to_string);

            let (status, headers, body) = call(
                app,
                get(&format!(
                    "{CALLBACK}?state={state}&code=an-authorization-code"
                )),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "a nonce of {wrong:?} must be refused: {body}",
            );
            assert!(
                headers.get(SET_COOKIE).is_none(),
                "and must issue no session",
            );
            assert!(!carries_a_token(&body), "{body}");
        }
    }

    // ---------------------------------------------------------------------------------------
    // The authorization request.
    // ---------------------------------------------------------------------------------------

    /// What `/api/signin` asks the provider for.
    ///
    /// Two claims in one place, because they are the two ways this request goes wrong. It must
    /// carry PKCE with `S256` — a challenge equal to the verifier is `plain`, which protects
    /// nothing — and it must ask for **only** the sign-in scopes. Signing in is not connecting: a
    /// vendor scope here would turn one consent screen into a different one, and a user who agreed
    /// to "sign in" would have granted standing access to their account.
    #[tokio::test]
    async fn the_authorization_request_carries_pkce_and_asks_only_to_identify_the_human() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        let (status, headers, _) = call(app.clone(), get(SIGNIN)).await;
        assert_eq!(status, StatusCode::SEE_OTHER);

        let location = headers
            .get(header::LOCATION)
            .expect("a redirect names where it goes")
            .to_str()
            .expect("a location is ASCII");

        assert_eq!(parameter(location, "response_type"), Some("code"));
        assert_eq!(parameter(location, "code_challenge_method"), Some("S256"));

        let challenge = parameter(location, "code_challenge").expect("a code challenge");
        assert_eq!(challenge.len(), 43, "SHA-256, base64url, unpadded");

        // `openid email profile`, percent-encoded, and nothing else.
        assert_eq!(
            parameter(location, "scope"),
            Some("openid%20email%20profile"),
            "sign-in asks to identify the human and to do nothing on their behalf",
        );

        // The verifier reaches the token endpoint, and it is not the challenge — which is what
        // separates S256 from `plain`.
        let (state, nonce) = (
            parameter(location, "state").expect("a state"),
            parameter(location, "nonce").expect("a nonce"),
        );
        exchange.echoing(nonce);
        call(
            app,
            get(&format!(
                "{CALLBACK}?state={state}&code=an-authorization-code"
            )),
        )
        .await;

        let redemptions = exchange.redemptions();
        assert_eq!(redemptions.len(), 1, "one code was redeemed");
        let (code, verifier) = &redemptions[0];

        assert_eq!(code, "an-authorization-code");
        assert_eq!(verifier.len(), 43, "a code verifier crossed the seam");
        assert_ne!(
            verifier, challenge,
            "the verifier must not be the challenge, which is what `plain` would have sent",
        );
    }

    // ---------------------------------------------------------------------------------------
    // The tenant. `AGENTS.md` § Invariants, first entry.
    // ---------------------------------------------------------------------------------------

    /// The tenant comes from the configuration, and from nothing a caller controls **and nothing
    /// the provider says**.
    ///
    /// The three vector tests in `routes::identity` assert this for a path segment, a body field
    /// and a header against the development identity. This is the same rule against the federated
    /// one, where there is a second source to worry about: an id token's claims. The claims here
    /// carry a hostile `email` domain and the request carries a hostile query parameter, and the
    /// principal that comes back is in the configured tenant regardless.
    #[tokio::test]
    async fn neither_a_request_nor_a_claim_influences_the_tenant() {
        const CLAIMED: &str = "attacker";

        let mut hostile = claims("not-yet-known");
        hostile.email = Some(format!("alice@{CLAIMED}.example.com"));
        hostile.subject = SUBJECT.to_string();

        let exchange = Arc::new(StubExchange::returning(hostile));
        let app = oidc_app(exchange.clone());

        let (state, nonce) = begin(&app).await;
        exchange.echoing(&nonce);

        // Every request-side vector this route has: extra query parameters on the callback.
        let (status, headers, _) = call(
            app.clone(),
            get(&format!(
                "{CALLBACK}?state={state}&code=a-code&tenant={CLAIMED}&as={CLAIMED}"
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let planted = headers
            .get(SET_COOKIE)
            .expect("a session cookie")
            .to_str()
            .expect("a cookie is ASCII");
        let token = planted
            .strip_prefix(&format!("{SESSION_COOKIE}="))
            .and_then(|rest| rest.split(';').next())
            .expect("a token in the planted cookie");

        // Ask the host who it thinks the caller is, carrying the session it just issued.
        let (status, _, body) = call(
            app,
            HttpRequest::builder()
                .uri("/api/session")
                .header(header::COOKIE, format!("{SESSION_COOKIE}={token}"))
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::OK,
            "the issued session resolves: {body}"
        );
        assert!(
            body.contains(&format!(r#""tenant":"{TENANT}""#)),
            "the tenant must come from the configuration: {body}",
        );
        assert!(
            !body.contains(CLAIMED),
            "nothing a request or a claim asserted may reach the principal: {body}",
        );
        assert!(
            body.contains(SUBJECT),
            "and the principal is identified by the provider's `sub`: {body}",
        );
    }

    // ---------------------------------------------------------------------------------------
    // What a caller is told when something fails.
    // ---------------------------------------------------------------------------------------

    /// `IdentityError::Unreachable` must not leak the provider's address — X-03 asserts this for
    /// the guard's `503`, and the sign-in path is the other place a provider's address exists.
    ///
    /// The distinction is kept all the way out: a refused credential and an unreachable provider
    /// differ in **status**, because an operator answers them in opposite ways.
    #[tokio::test]
    async fn a_refusal_tells_the_caller_nothing_about_the_provider() {
        struct Down;

        #[async_trait]
        impl TokenExchange for Down {
            async fn redeem(&self, _: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
                Err(ExchangeError::Unreachable(
                    "dial tcp 10.0.0.7:443: connection refused".into(),
                ))
            }
        }

        struct Refusing;

        #[async_trait]
        impl TokenExchange for Refusing {
            async fn redeem(&self, _: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
                Err(ExchangeError::Rejected)
            }
        }

        let app = oidc_app(Arc::new(Down));
        let (state, _) = begin(&app).await;
        let (unreachable, headers, leaked) =
            call(app, get(&format!("{CALLBACK}?state={state}&code=a-code"))).await;

        assert_eq!(
            unreachable,
            StatusCode::SERVICE_UNAVAILABLE,
            "a provider that cannot be reached is this host's problem",
        );
        assert!(headers.get(SET_COOKIE).is_none(), "and issues no session");

        // A 503 that names an internal address is a map of the deployment.
        assert!(!leaked.contains("10.0.0.7"), "{leaked}");
        assert!(!leaked.contains("connection refused"), "{leaked}");
        assert!(!leaked.contains(ISSUER), "{leaked}");

        let app = oidc_app(Arc::new(Refusing));
        let (state, _) = begin(&app).await;
        let (rejected, _, body) =
            call(app, get(&format!("{CALLBACK}?state={state}&code=a-code"))).await;

        assert_eq!(
            rejected,
            StatusCode::UNAUTHORIZED,
            "a credential the provider refused is the caller's problem",
        );
        assert_ne!(
            unreachable, rejected,
            "an outage and a bad credential must not read the same",
        );
        assert!(!body.contains(ISSUER), "{body}");
    }

    // ---------------------------------------------------------------------------------------
    // The Acceptance's fourth item: missing configuration.
    // ---------------------------------------------------------------------------------------

    /// A host with no OIDC configuration serves an **explanatory page** — and serves everything
    /// else too.
    ///
    /// Three claims, and the third is the one the Acceptance is really about: not a panic, and not
    /// a login that redirects somewhere and dies on the way back. It stops at `/api/signin`, which
    /// is the first place a human asks for it.
    #[tokio::test]
    async fn an_unconfigured_host_explains_itself_and_keeps_serving() {
        for (state, expected) in [
            (AppState::without_identity(), "not configured"),
            (
                AppState::oidc_without_a_token_exchange(),
                "not available in this build",
            ),
        ] {
            let app = super::super::app(state);

            let (status, headers, body) = call(app.clone(), get(SIGNIN)).await;

            assert_eq!(
                status,
                StatusCode::SERVICE_UNAVAILABLE,
                "sign-in is unavailable, and says so",
            );
            assert!(
                body.contains(expected),
                "the page must explain which of the two it is: {body}",
            );
            assert!(body.contains("<html"), "and be a page: {body}");
            assert!(
                headers.get(header::LOCATION).is_none(),
                "it must not send the browser to a provider it cannot return from",
            );

            // The page is for anonymous callers, so it names no environment variable — that is the
            // startup log's job. See `unconfigured_page`.
            for variable in WITHHELD_FROM_THE_PAGE {
                assert!(
                    !body.contains(variable),
                    "the page must not enumerate this host's settings to an anonymous caller, \
                     found {variable}: {body}",
                );
            }

            // And the rest of the host is unaffected, which is the whole reason this is not a
            // startup refusal.
            let (health, _, _) = call(app, get("/health")).await;
            assert_eq!(health, StatusCode::OK, "/health must keep answering");
        }
    }

    /// A callback that arrives at an unconfigured host is refused, not answered with a stack trace
    /// and not answered with a session.
    #[tokio::test]
    async fn a_callback_on_an_unconfigured_host_issues_nothing() {
        let (status, headers, body) = call(
            super::super::app(AppState::without_identity()),
            get(&format!("{CALLBACK}?state=anything&code=anything")),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(headers.get(SET_COOKIE).is_none());
        assert!(!carries_a_token(&body), "{body}");
    }

    /// Every shape of callback that cannot complete a sign-in, with the status each one gets.
    ///
    /// The statuses are pinned individually rather than accepted loosely. An earlier version of
    /// this test allowed "400 or 401" while its name claimed the answers were identical — a test
    /// that had stopped meaning anything, since two behaviours it declines to distinguish cannot
    /// both be the one it is asserting.
    ///
    /// They differ, and the difference is deliberate. `?error=…` is the **provider** having
    /// refused, which is an answer about a credential — `401`. The others never reach a credential
    /// at all, so they are malformed requests — `400`. Neither reveals anything, which is the
    /// separate and more important property asserted below.
    #[tokio::test]
    async fn every_unusable_callback_issues_nothing() {
        let exchange = Arc::new(StubExchange::returning(claims("nonce")));
        let app = oidc_app(exchange);

        for (query, expected) in [
            ("", StatusCode::BAD_REQUEST),
            ("?state=only-a-state", StatusCode::BAD_REQUEST),
            ("?code=only-a-code", StatusCode::BAD_REQUEST),
            ("?error=denied", StatusCode::UNAUTHORIZED),
            // The provider's refusal wins even when it also echoed the rest, so a caller cannot
            // dress an error up as a completable callback.
            (
                "?error=denied&state=whatever&code=whatever",
                StatusCode::UNAUTHORIZED,
            ),
        ] {
            let (status, headers, body) =
                call(app.clone(), get(&format!("{CALLBACK}{query}"))).await;

            assert_eq!(status, expected, "`{query}` answered {status}: {body}");
            assert!(
                headers.get(SET_COOKIE).is_none(),
                "`{query}` issued a session",
            );
            assert!(!carries_a_token(&body), "`{query}`: {body}");
            // Whatever the provider or the caller put in `error`, none of it comes back.
            assert!(
                !body.contains("denied"),
                "`{query}` echoed the error: {body}"
            );
        }
    }

    /// The probing property, which is what the previous test's name used to gesture at without
    /// asserting: a callback that cannot be completed is answered **byte for byte identically**
    /// whether or not the `state` it names is one this host is actually holding.
    ///
    /// Otherwise the callback is an oracle for "is this state live", which is the one fact about a
    /// pending sign-in worth guessing. It holds structurally — the handler refuses a callback
    /// lacking `code` before it ever consults the store — and this pins both halves of that: the
    /// answers match, and the probe does not spend the state it named.
    #[tokio::test]
    async fn a_callback_without_a_code_is_not_an_oracle_for_a_live_state() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        let (bound_state, nonce) = begin(&app).await;
        let forged = "0000000000000000000000000000000000000000000000000000000000000000";

        let (known, known_headers, known_body) =
            call(app.clone(), get(&format!("{CALLBACK}?state={bound_state}"))).await;
        let (unknown, _, unknown_body) =
            call(app.clone(), get(&format!("{CALLBACK}?state={forged}"))).await;

        assert_eq!(
            known, unknown,
            "a live state and a forged one must not differ in status",
        );
        assert_eq!(
            known_body, unknown_body,
            "nor in the body, byte for byte — otherwise this route reports which states exist",
        );
        assert!(known_headers.get(SET_COOKIE).is_none());

        // And the probe must not have consumed what it asked about, or an attacker could cancel
        // every sign-in it could guess the shape of.
        exchange.echoing(&nonce);
        let (status, headers, body) = call(
            app,
            get(&format!(
                "{CALLBACK}?state={bound_state}&code=an-authorization-code"
            )),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::OK,
            "probing a live state must not spend it: {body}",
        );
        assert!(headers.get(SET_COOKIE).is_some());
    }
}
