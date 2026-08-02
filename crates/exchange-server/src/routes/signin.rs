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
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use super::{rate_limited, Access, Module, Route};
use crate::oidc::config::{
    AUTHORIZATION_ENDPOINT_ENV, CLIENT_ID_ENV, CLIENT_SECRET_ENV, ISSUER_ENV, JWKS_URI_ENV,
    REDIRECT_URI_ENV, TENANT_ENV, TOKEN_ENDPOINT_ENV,
};
use crate::oidc::flow;
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
        Route {
            // Beside the route it describes rather than folded into `/api/session`, which was the
            // obvious home and is argued against on [`availability`].
            path: "/api/signin/availability",
            access: Access::Anonymous,
            method_router: availability_route,
        },
    ],
};

fn signin_route() -> MethodRouter<AppState> {
    get(signin).post(development_signin)
}

fn callback_route() -> MethodRouter<AppState> {
    get(callback)
}

fn availability_route() -> MethodRouter<AppState> {
    get(availability)
}

/// Send the browser to the provider, or explain why it cannot be sent.
async fn signin(State(state): State<AppState>) -> Response {
    let oidc = match state.sign_in() {
        SignIn::Oidc(oidc) => oidc,
        // The Acceptance's fourth item, at the point a human meets it: an explanatory page, not a
        // panic at startup and not a redirect that dies at the callback.
        SignIn::Unconfigured => return unconfigured_page(),
        SignIn::NoTokenExchange => return no_token_exchange_page(),
        // Sign-in is *available* here and it is not a redirect, which is X-57's whole distinction.
        // There is no provider to send the browser to; the caller signs in against this host.
        SignIn::Development { automatic } => return development_page(*automatic),
    };

    if let Err(refusal) = state.admit_sign_in() {
        return rate_limited(refusal);
    }

    match oidc.authorize() {
        // Two headers, one act: where the browser goes, and the binder that will prove on the way
        // back that it is the same browser. Distinct header names, so the array form is safe here —
        // see [`planting`] for why it is not where two cookies are involved.
        Ok(authorization) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, authorization.url),
                (
                    header::SET_COOKIE,
                    flow::planted_binder(&authorization.binder),
                ),
            ],
        )
            .into_response(),
        Err(error) => {
            // Names this host's own machinery, so it goes to the log rather than to the caller —
            // which is exactly the split `SignInRefusal` encodes, so the refusal goes through it
            // rather than around it.
            let refusal = SignInRefusal::NoFlow(error);
            error!(reason = %refusal, "cannot open an authorization request");
            refused(&refusal)
        }
    }
}

/// Complete the one-principal `--dev` browser flow without putting a token in a readable body.
///
/// This is deliberately unavailable to an explicit development roster, even when that roster
/// happens to contain one entry. `--dev` is the stronger startup declaration: it fixed one local
/// principal and selected a single-tenant deployment together. A cross-site form could at most
/// establish that same sole local identity, and `SameSite=Strict` still prevents it from exercising
/// the cookie; a multi-principal roster keeps requiring the explicit bearer handle so it cannot be
/// used for login CSRF between identities.
async fn development_signin(State(state): State<AppState>) -> Response {
    if !matches!(state.sign_in(), SignIn::Development { automatic: true }) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(refusal) = state.admit_sign_in() {
        return rate_limited(refusal);
    }
    let Some(identity) = state.development_identity() else {
        error!("automatic development sign-in has no development identity");
        return page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sign-in refused",
            "this host cannot open a session right now. Try again shortly",
            None,
        );
    };
    let Some(principal) = identity.only_principal() else {
        error!("automatic development sign-in does not have exactly one principal");
        return page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sign-in refused",
            "this host cannot open a session right now. Try again shortly",
            None,
        );
    };
    let token = match identity.open_session(principal.clone()) {
        Ok(token) => token,
        Err(error) => {
            error!(%error, "cannot mint an automatic development session");
            return page(
                StatusCode::SERVICE_UNAVAILABLE,
                "Sign-in refused",
                "this host cannot open a session right now. Try again shortly",
                None,
            );
        }
    };
    crate::audit::signed_in(&principal);

    planting(
        (
            StatusCode::OK,
            Html(document(
                "Signed in",
                "You are signed in as the local development user. Returning to the console…",
                Some(AFTER_SIGN_IN),
            )),
        )
            .into_response(),
        &[session::planted(&token)],
    )
}

/// Whether this deployment can sign anyone in — as a field, not as a sentence.
///
/// # Why this is a route and not a field on `/api/session`
///
/// One round trip for "who am I" and "can I sign in" is the shape the console wants, and it would
/// avoid widening the anonymous surface a second time. It was not chosen, because `/api/session`
/// answers a caller **only after the guard resolved a principal** — and the caller this exists for
/// is by definition the one that has none. Reaching them there means declaring `/api/session`
/// [`Access::Anonymous`], and [`Access`] is per route rather than per method: the same declaration
/// covers the `POST` that mints a session and the `DELETE` that closes one. Their guard would have
/// to move from the route table into the handlers, and `super`'s module documentation is explicit
/// that this surface is enumerable precisely because a route is not guarded by its handler
/// remembering to ask. Trading that for one round trip is a bad trade on a credential-holding
/// service, and `routes::identity`'s own module documentation states as a decision that it adds
/// nothing to the anonymous set.
///
/// So the fact lives beside the route it is about. A console asks this one anonymously and asks
/// `/api/session` when it has a session; the extra request is the cost of leaving the guard where
/// it can be enumerated.
///
/// # What it says, and what it deliberately does not
///
/// One boolean and nothing else. The three states of [`SignIn`] are what an operator needs and the
/// two explanatory pages above are where they are said; a caller learns only whether sign-in
/// works. See [`SignIn::available`] for why collapsing them is the decision rather than a loss, and
/// `tests::the_availability_answer_discloses_nothing_about_the_configuration` for the adversarial
/// half — the two unavailable compositions answer byte for byte identically, so this route cannot
/// be asked whether the OIDC settings are set.
///
/// The name is `sign_in_available` rather than a bare `available` so that it still says what it
/// means if a later story embeds it in a wider answer.
async fn availability(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "sign_in_available": state.sign_in().available() }))
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
async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Response {
    let oidc = match state.sign_in() {
        SignIn::Oidc(oidc) => oidc,
        SignIn::Unconfigured => return unconfigured_page(),
        SignIn::NoTokenExchange => return no_token_exchange_page(),
        // A callback on a host that federates nothing. Sign-in *is* available here, so neither
        // page above is true — and `development_page` is not the answer either: a browser arriving
        // mid-redirect asked to finish a flow, not for instructions. This host planted no `state`
        // and never will, which is exactly [`SignInRefusal::UnknownState`] — the same `400` and the
        // same phrase a forged `state` gets on a federated host, so this route stays the oracle for
        // nothing that `a_callback_without_a_code_is_not_an_oracle_for_a_live_state` made it.
        SignIn::Development { .. } => return refused(&SignInRefusal::UnknownState),
    };

    // The provider refused, or something walked a browser here with an `error` of its own. Either
    // way the value is not looked at: it is request input, so it may not reach a log line, and it
    // is the provider's words about a credential, so it may not reach the caller.
    if callback.error.is_some() {
        warn!("the identity provider returned an error to the sign-in callback");
        return refused(&SignInRefusal::CodeRejected);
    }

    let (Some(presented_state), Some(code)) = (callback.state, callback.code) else {
        // No state, no code, or neither — refused *before* the store is consulted, so this answer
        // is identical whether or not the state named is one this host is holding. That is what
        // stops the callback being an oracle for "is this state live", and it is why the refusal
        // happens here rather than after a lookup.
        // `tests::a_callback_without_a_code_is_not_an_oracle_for_a_live_state` pins both halves:
        // the answers match byte for byte, and the probe does not spend what it asked about.
        return refused(&SignInRefusal::UnknownState);
    };

    // The binder the browser is holding, if it is holding one. This is the **one** credential-shaped
    // input the callback reads, and it is not a credential for anything: it names no principal, it
    // opens no session on its own, and holding it is neither necessary nor sufficient for anything
    // but finishing the one sign-in it was planted for. See the module documentation.
    let Some(binder) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| session::from_cookie_header(value, flow::BINDER_COOKIE))
    else {
        // Refused **before** the pending store is consulted, for the same two reasons the missing
        // `code` above is: an attacker who simply omits the cookie must not fall through to the
        // path that checks only `state`, and an answer decided without a lookup cannot report
        // whether the `state` named is one this host is holding.
        let refusal = SignInRefusal::NoBinder;
        warn!(reason = %refusal, "a sign-in did not complete");
        return refused(&refusal);
    };

    let token = match oidc.complete(&presented_state, &code, binder).await {
        Ok(token) => token,
        Err(refusal) => {
            // The operator's version goes to the log; `caller_facing` is what the caller sees, and
            // it carries no value from the provider and no address of one. Which status each refusal
            // answers with — and why `Expired` is a `401` while `NoSession` is a `503`, and why the
            // four back-channel refusals are indistinguishable — is `SignInRefusal::status`, next to
            // the phrase it decides alongside. X-26 moved it there; this route decides only how a
            // refusal is rendered.
            warn!(reason = %refusal, "a sign-in did not complete");
            return refused(&refusal);
        }
    };

    // The session leaves here as a cookie and in no other form. See the module documentation: this
    // response has no body a script can read a credential out of.
    //
    // The binder is cleared in the same breath. It is already spent server-side — the pending
    // authorization it lived in was removed — so this only stops a value that can no longer match
    // anything from sitting in the browser until its `Max-Age` elapses.
    planting(
        (
            StatusCode::OK,
            Html(document(
                "Signed in",
                "You are signed in. Returning to the console…",
                Some(AFTER_SIGN_IN),
            )),
        )
            .into_response(),
        &[session::planted(&token), flow::cleared_binder()],
    )
}

/// A response carrying every `Set-Cookie` in `cookies`.
///
/// **Not** the `[(SET_COOKIE, …); N]` array form, which axum turns into repeated `HeaderMap::insert`
/// — so a second entry silently replaces the first, and this response plants a session *and* clears
/// the binder. A dropped `Set-Cookie` here would be a sign-in that answered "Signed in" and issued
/// nothing, which is the failure mode with no symptom until somebody reports being logged out.
///
/// A cookie that cannot be a header value refuses the whole response rather than being skipped:
/// refuse, never repair. Every value this host builds is hex and ASCII literals, so this is a branch
/// that should never be taken — which is precisely why it must not be a silent one.
fn planting(mut response: Response, cookies: &[String]) -> Response {
    for cookie in cookies {
        match header::HeaderValue::from_str(cookie) {
            Ok(value) => {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
            Err(error) => {
                error!(%error, "a Set-Cookie this host built is not a header value");
                return page(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Sign-in refused",
                    "this host cannot open a session right now. Try again shortly",
                    None,
                );
            }
        }
    }

    response
}

/// A refusal the caller sees: a status and a fixed phrase, never a value.
///
/// Both come from the refusal, and neither is passed in. Until X-26 the status was the caller's
/// argument here while the phrase came from the refusal, which meant every call site restated a
/// decision the refusal had already made and any two of them could drift. What stays a decision of
/// this module is everything around them: that a refusal is rendered as a page rather than a body a
/// script can read, that it plants no cookie, and what goes to the log instead.
fn refused(refusal: &SignInRefusal) -> Response {
    page(
        refusal.status(),
        "Sign-in refused",
        refusal.caller_facing(),
        None,
    )
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
/// an operator who *has* configured a provider that they have not would send them to re-check eight
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

/// The page a human meets on a host whose identity is the development one.
///
/// **A `200`, not a `503`.** Its two neighbours above refuse — this host cannot sign anybody in —
/// and this one answers: sign-in *is* available here, it simply is not a redirect. That is X-57's
/// distinction made where a human meets it, and it is why [`SignIn::available`] can stay a single
/// boolean: whether anyone can sign in is published anonymously, and **how** is answered here, one
/// request later, to a caller who came to sign in.
///
/// # What it says, and why saying it is allowed
///
/// It names the mechanism — present a roster handle as a bearer token, and `POST /api/session`
/// exchanges it for a session cookie — which does tell the reader that this deployment runs the
/// development identity. That is a disclosure, and it is bounded rather than waved through:
///
/// 1. **This composition cannot be reached from anywhere else.**
///    [`IdentityBinding::Development`](crate::bind::IdentityBinding::Development) refuses every
///    bind but loopback for as long as the port is armed, so the only caller that can read this
///    page is already on the machine and can read the roster out of the process' own environment.
/// 2. **`/api/signin` is the operator's channel and already works this way.** The two pages above
///    tell an anonymous caller which of two misconfigurations a host has, deliberately, because a
///    human who asked for a login has to be told what to do next. The *fact* surfaces —
///    `/api/signin/availability` and `/api/onboarding` — are the caller's channel, and they answer
///    a federated host and this one identically;
///    `tests::no_anonymous_surface_says_which_kind_of_provider_signs_people_in` is that claim.
///
/// It names **no roster entry**. The mechanism is a property of this build; a handle is a principal
/// this host would mint, and printing one would put a working credential on an anonymous page.
fn development_page(automatic: bool) -> Response {
    let detail = if automatic {
        "This host was started with `--dev`, so it has exactly one local development user and can \
         establish that browser session here. The identity remains deliberately unavailable on \
         any address but loopback."
    } else {
        "This host has no federated identity provider, and does not need one: it was started with \
         a development roster, so it can sign you in itself. Present the handle of a rostered \
         principal as a bearer token — `Authorization: Bearer <handle>` — and `POST /api/session` \
         exchanges it for a session cookie. The roster is the one the operator wrote at startup, \
         and this page does not name it. The development identity is deliberately unavailable on \
         any address but loopback, because a handle is a name rather than a secret."
    };
    let action = automatic.then_some(("/api/signin", "Continue as the local development user"));
    (
        StatusCode::OK,
        Html(document_with_action(
            "Sign in with this host's development identity",
            detail,
            None,
            action,
        )),
    )
        .into_response()
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
    document_with_action(heading, detail, refresh_to, None)
}

/// Build a page with one fixed host-owned form action when local sign-in can complete here.
fn document_with_action(
    heading: &str,
    detail: &str,
    refresh_to: Option<&str>,
    action: Option<(&str, &str)>,
) -> String {
    let refresh = refresh_to
        .map(|target| format!(r#"<meta http-equiv="refresh" content="0; url={target}">"#))
        .unwrap_or_default();
    let action = action
        .map(|(target, label)| {
            format!(
                "<form method=\"post\" action=\"{target}\">\n\
                 <button type=\"submit\">{label}</button>\n\
                 </form>\n"
            )
        })
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
         {action}\
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
    TOKEN_ENDPOINT_ENV,
    JWKS_URI_ENV,
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
    use crate::traffic::Traffic;

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
    ///
    /// The `exp` is five minutes out, as a provider states it. It was `i64::MAX` while nothing
    /// bound a session's life to it — harmless then, and no longer true of anything: X-16 opens the
    /// session for exactly this long, and a token that never expires is refused rather than
    /// honoured. See `session::MAX_SESSION_SECONDS`.
    fn claims(nonce: &str) -> SignedClaims {
        SignedClaims {
            issuer: ISSUER.to_string(),
            audience: vec![CLIENT_ID.to_string()],
            subject: SUBJECT.to_string(),
            nonce: Some(nonce.to_string()),
            expires_at: session::now() + 300,
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

    fn oidc_app_with_traffic(exchange: Arc<dyn TokenExchange>, traffic: Traffic) -> Router {
        super::super::app(
            AppState::with_oidc(Arc::new(Oidc::new(config(), exchange))).with_traffic(traffic),
        )
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

    /// An anonymous burst cannot allocate enough pending flows to churn legitimate browsers out of
    /// the bounded store. The refusal names when to retry and does not make the read-only
    /// availability answer disappear with it.
    #[tokio::test]
    async fn authorization_starts_are_rate_limited_without_hiding_availability() {
        let exchange = Arc::new(StubExchange::returning(claims("unused")));
        let app = oidc_app_with_traffic(
            exchange,
            Traffic::for_test(1, 1, 1, std::time::Duration::from_secs(60)),
        );

        assert_eq!(
            call(app.clone(), get(SIGNIN)).await.0,
            StatusCode::SEE_OTHER
        );
        let (status, headers, body) = call(app.clone(), get(SIGNIN)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(
            headers
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("60")
        );

        assert_eq!(
            call(app, get(super::MODULE.routes[2].path)).await.0,
            StatusCode::OK,
            "the limit protects allocation, not the availability fact"
        );
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

    /// One browser's side of a sign-in: what `/api/signin` sent it, and what it sends back.
    ///
    /// `planted` is the whole `Set-Cookie` line rather than a binder these tests know the shape of,
    /// and it is an `Option` on purpose. It models a browser rather than a client written against
    /// this host: it returns exactly what this host put in it, and nothing at all if this host put
    /// nothing there.
    struct Browser {
        state: String,
        nonce: String,
        planted: Option<String>,
    }

    impl Browser {
        /// The callback this browser follows for the sign-in **it** opened.
        fn returns_with(&self, code: &str) -> HttpRequest<Body> {
            self.walked_into(&self.state, code)
        }

        /// The callback this browser is *walked into*: a `state` some other browser opened, arriving
        /// at this one. The attack, in one method.
        fn walked_into(&self, state: &str, code: &str) -> HttpRequest<Body> {
            callback_from(
                self.planted.as_deref(),
                &format!("?state={state}&code={code}"),
            )
        }
    }

    /// A callback request as a browser holding `planted` would send it.
    ///
    /// A browser sends a cookie's `name=value` and none of the attributes it was planted with, so
    /// that is what this returns — otherwise a test would be asserting against a `Cookie` header no
    /// browser produces.
    fn callback_from(planted: Option<&str>, query: &str) -> HttpRequest<Body> {
        let mut request = HttpRequest::builder().uri(format!("{CALLBACK}{query}"));

        if let Some(planted) = planted {
            let pair = planted.split(';').next().unwrap_or(planted);
            request = request.header(header::COOKIE, pair);
        }

        request.body(Body::empty()).expect("a well-formed request")
    }

    /// Start a sign-in, and hand back the browser that opened it holding everything it was given.
    async fn begin(app: &Router) -> Browser {
        let (status, headers, _) = call(app.clone(), get(SIGNIN)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "sign-in redirects");

        let location = headers
            .get(header::LOCATION)
            .expect("a redirect names where it goes")
            .to_str()
            .expect("a location is ASCII")
            .to_string();

        Browser {
            state: parameter(&location, "state")
                .expect("the authorization request carries a state")
                .to_string(),
            nonce: parameter(&location, "nonce")
                .expect("the authorization request carries a nonce")
                .to_string(),
            planted: headers
                .get(SET_COOKIE)
                .map(|value| value.to_str().expect("a cookie is ASCII").to_string()),
        }
    }

    /// The session cookie among everything a response planted, found by name rather than by
    /// position — a response may plant more than one, and which comes first is not a property worth
    /// depending on.
    fn planted_session(headers: &HeaderMap) -> Option<&str> {
        headers
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with(&format!("{SESSION_COOKIE}=")))
    }

    // ---------------------------------------------------------------------------------------
    // X-15's failing-first test: the sign-in a victim did not start.
    // ---------------------------------------------------------------------------------------

    /// **A callback carrying a genuinely bound, unspent `state` is refused when it arrives from a
    /// browser that did not open that sign-in, and issues no session.**
    ///
    /// This is login-CSRF, and it is the half of the attack `state` cannot reach. X-04 checks
    /// `state` *server-side*, which stops a value this host never issued — but an attacker obtains a
    /// genuine one honestly: visit `/api/signin` here, authenticate at the provider **as
    /// themselves**, and stop at the redirect rather than following it. They now hold a valid `code`
    /// and a matching, still-unspent `state`, every value real. Walking a victim into that callback
    /// passes every server-side check, and the victim's browser is issued a session belonging to the
    /// attacker. Everything the victim then connects, pastes or configures — a credential, most
    /// obviously — lands in the attacker's tenant. It is `docs/vision.md`'s sentence inverted from
    /// the other end: the credential does not cross the boundary, the *victim* does.
    ///
    /// Both shapes of victim are driven, because they are not the same request and only one of them
    /// is the common case:
    ///
    /// 1. **A browser holding nothing of ours**, which is the victim who never visited this host.
    /// 2. **A browser holding its own sign-in's binder**, which is the victim who did — and whose
    ///    binder must not be interchangeable with the attacker's.
    ///
    /// The browser that *did* open the sign-in completes it at the end, in this same run. Without
    /// that, every assertion above would be satisfied by a host that had simply broken sign-in for
    /// everybody, which is the failure mode a refusal test has.
    #[tokio::test]
    async fn a_callback_from_a_browser_that_did_not_open_the_signin_issues_no_session() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        // The attacker's own sign-in, started here and genuinely bound. Nothing about it is forged:
        // this host drew the state, and it is unspent.
        let attacker = begin(&app).await;

        // The provider behaves perfectly and echoes the nonce this host bound, so every other check
        // in the callback passes. That is what makes this test about the browser binding and about
        // nothing else — it is the only thing left between the walked-in victim and a session.
        exchange.echoing(&attacker.nonce);

        // A victim who has their own sign-in in flight, so their browser is holding a binder — just
        // not the one this authorization request was opened with.
        let victim_mid_signin = begin(&app).await;

        for (who, request) in [
            (
                "a browser holding nothing of ours",
                callback_from(
                    None,
                    &format!("?state={}&code=an-authorization-code", attacker.state),
                ),
            ),
            (
                "a browser holding its own sign-in's binder",
                victim_mid_signin.walked_into(&attacker.state, "an-authorization-code"),
            ),
        ] {
            let (status, headers, body) = call(app.clone(), request).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{who} completed a sign-in it did not start: {body}",
            );
            assert!(
                headers.get(SET_COOKIE).is_none(),
                "{who} was issued a session by a callback it did not open: {:?}",
                headers.get_all(SET_COOKIE).iter().collect::<Vec<_>>(),
            );
            assert!(
                !carries_a_token(&body),
                "{who} was handed something token-shaped: {body}",
            );
            assert!(
                !body.contains(SESSION_COOKIE),
                "{who} was told the session cookie's name: {body}",
            );
        }

        // And the browser that really did open it still completes — otherwise everything above
        // passes on a host where nobody can sign in at all.
        let (status, headers, body) =
            call(app, attacker.returns_with("an-authorization-code")).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "the browser that opened the sign-in must still complete it: {body}",
        );
        assert!(
            planted_session(&headers).is_some(),
            "and it is the one that gets the session",
        );
    }

    /// A callback carrying **no binder at all** is refused, and issues no session.
    ///
    /// Its own test rather than a case inside the one above, because it is the shape an attacker
    /// reaches for first: if omitting the cookie fell through to the path that checks only `state`,
    /// the binding would be advisory and every walked-in victim would simply arrive without it.
    ///
    /// The `state` it names must also survive. This refusal is decided before the pending store is
    /// consulted, so a cookie-less callback cannot cancel a sign-in somebody else has in flight —
    /// asserted here by completing that very sign-in afterwards.
    #[tokio::test]
    async fn a_callback_carrying_no_binder_is_refused_and_spends_nothing() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        let (status, headers, body) = call(
            app.clone(),
            callback_from(
                None,
                &format!("?state={}&code=an-authorization-code", browser.state),
            ),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a callback with no binder must not fall through to the state-only path: {body}",
        );
        assert!(
            headers.get(SET_COOKIE).is_none(),
            "and must issue nothing at all",
        );
        assert!(!carries_a_token(&body), "{body}");

        // Refused without touching the store, so the honest sign-in it named is untouched.
        let (status, headers, body) =
            call(app, browser.returns_with("an-authorization-code")).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "a binder-less callback must not spend the state it named: {body}",
        );
        assert!(planted_session(&headers).is_some());
    }

    /// The binder is a `__Host-` cookie with `Secure`, `HttpOnly` and a `SameSite` — a pre-session
    /// credential with a pre-session credential's protections — and it is **not** in the URL.
    ///
    /// `SameSite=Lax` and not `Strict`, which is the one attribute where this cookie and the session
    /// cookie disagree. `crate::oidc::flow::BINDER_COOKIE` carries the argument; the short form is
    /// that the binder has to survive the provider's redirect back, which `Strict` withholds, and
    /// that `SameSite` is not what closes login-CSRF anyway — being unguessable and unplantable by
    /// any other origin is.
    #[tokio::test]
    async fn the_binder_is_a_host_cookie_and_never_leaves_in_the_url() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange);

        let (status, headers, _) = call(app, get(SIGNIN)).await;
        assert_eq!(status, StatusCode::SEE_OTHER);

        let planted = headers
            .get(SET_COOKIE)
            .expect("opening a sign-in plants a binder")
            .to_str()
            .expect("a cookie is ASCII");

        // `__Host-`: a browser refuses such a cookie unless it is `Secure`, has `Path=/` and carries
        // no `Domain` — so no sibling subdomain can plant a binder for this host, which is what
        // makes the value unforgeable by the attacker's origin rather than merely unguessable.
        assert!(
            planted.starts_with(&format!("{}=", flow::BINDER_COOKIE)),
            "{planted}",
        );
        assert!(flow::BINDER_COOKIE.starts_with("__Host-"), "{planted}");
        assert!(planted.contains("Path=/"), "{planted}");
        assert!(!planted.contains("Domain"), "{planted}");
        assert!(planted.contains("Secure"), "{planted}");
        assert!(planted.contains("HttpOnly"), "{planted}");
        assert!(planted.contains("SameSite=Lax"), "{planted}");
        assert!(
            !planted.contains("SameSite=Strict"),
            "the binder must not inherit the session cookie's SameSite, which would withhold it on \
             the provider's redirect back and refuse every sign-in: {planted}",
        );

        // It expires with the authorization request it belongs to, so a stale one cannot sit in a
        // browser waiting to be replayed.
        assert!(planted.contains("Max-Age="), "{planted}");

        // And it never travels through the provider. A binder in the authorization URL would be
        // read by the provider, by anything else registered for that redirect, and by whatever the
        // browser's history and referrers reach — every one of which would then hold it.
        let location = headers
            .get(header::LOCATION)
            .expect("a redirect names where it goes")
            .to_str()
            .expect("a location is ASCII");

        let binder = planted
            .split_once('=')
            .and_then(|(_, rest)| rest.split(';').next())
            .expect("a value in the planted cookie");

        assert!(!binder.is_empty(), "{planted}");
        assert!(
            !location.contains(binder),
            "the binder must not reach the provider: {location}",
        );
        assert!(
            !location.contains(flow::BINDER_COOKIE),
            "nor must its name: {location}",
        );
    }

    /// A completed sign-in clears the binder it spent, alongside planting the session.
    ///
    /// Two `Set-Cookie` headers on one response, which is the thing `planting` exists to get right:
    /// axum's array form would have `insert`ed and left only one, and the one it left would have
    /// decided whether anybody was signed in.
    #[tokio::test]
    async fn a_completed_signin_plants_a_session_and_clears_the_binder() {
        let exchange = Arc::new(StubExchange::returning(claims("not-yet-known")));
        let app = oidc_app(exchange.clone());

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        let (status, headers, body) =
            call(app, browser.returns_with("an-authorization-code")).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let planted: Vec<&str> = headers
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();

        assert!(
            planted_session(&headers).is_some(),
            "the session must survive being planted beside another cookie: {planted:?}",
        );

        let cleared = planted
            .iter()
            .find(|value| value.starts_with(&format!("{}=", flow::BINDER_COOKIE)))
            .expect("the spent binder is cleared");
        assert!(cleared.contains("Max-Age=0"), "{cleared}");
    }

    /// **X-16.** The session's life is the id token's, all the way through this route.
    ///
    /// The wiring, which no unit test reaches: `complete` hands the store the claims' own `exp`. A
    /// host that instead applied a lifetime of its own would answer this callback with a session,
    /// because every other check on the token passes — it is ours, from our issuer, and echoes the
    /// nonce this host bound. What it claims is a validity of `i64::MAX`, which
    /// `session::MAX_SESSION_SECONDS` refuses rather than shortens, so nothing session-shaped may
    /// come back and the caller must not be told which check it was.
    #[tokio::test]
    async fn an_exp_this_host_will_not_honour_completes_no_signin() {
        let exchange = Arc::new(StubExchange::returning(SignedClaims {
            expires_at: i64::MAX,
            ..claims("not-yet-known")
        }));
        let app = oidc_app(exchange.clone());

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        let (status, headers, body) =
            call(app, browser.returns_with("an-authorization-code")).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(
            planted_session(&headers).is_none(),
            "a refusal that still planted a session would have signed the caller in while \
             reporting failure",
        );
        assert!(!carries_a_token(&body), "{body}");
        assert!(
            !body.contains("exp") && !body.contains("9223372036854775807"),
            "the caller is told nothing about the provider's token: {body}",
        );
    }

    /// **X-24.** An id token that has expired is refused as a credential, not as an outage.
    ///
    /// `401` and not `503`, which is the status half of the story: the caller is told the provider's
    /// answer was not accepted, and not that this host cannot open a session right now — the second
    /// sends a human away to try again shortly when what they need is to sign in again. The
    /// distinction is the same one `a_refusal_tells_the_caller_nothing_about_the_provider` holds for
    /// an unreachable provider, pinned here for the refusal `Oidc::admit` produces.
    ///
    /// The status is what the sub-second window used to get wrong, because the store's
    /// `AlreadyExpired` arrived as `NoSession`. What keeps the two decisions on one reading of the
    /// clock is `oidc::tests::a_sign_in_decides_against_one_reading_of_the_clock`; this is the
    /// answer that reading buys.
    #[tokio::test]
    async fn an_expired_id_token_is_refused_as_a_credential_and_not_as_an_outage() {
        let exchange = Arc::new(StubExchange::returning(SignedClaims {
            expires_at: session::now() - 1,
            ..claims("not-yet-known")
        }));
        let app = oidc_app(exchange.clone());

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        let (status, headers, body) =
            call(app, browser.returns_with("an-authorization-code")).await;

        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "an expired id token is the caller's problem and must not read as this host's: {body}",
        );
        assert!(
            planted_session(&headers).is_none(),
            "and a refusal must issue nothing at all",
        );
        assert!(!carries_a_token(&body), "{body}");
    }

    // ---------------------------------------------------------------------------------------
    // X-04's failing-first test.
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
        let browser = begin(&app).await;

        // The provider behaves perfectly: it echoes the nonce this host bound. That is what makes
        // this test about `state` and about nothing else — every other check the callback makes
        // will pass, so the state check is the only thing between the forged callback and a
        // session. Without it, the assertions below fail and a victim is signed in as somebody
        // else.
        exchange.echoing(&browser.nonce);

        // The attacker's callback. A well-formed authorization code, and a state this host never
        // issued — which is what an attacker has, since it cannot read the victim's.
        //
        // Sent from the browser that *did* open a sign-in here, so it is holding everything X-15's
        // binding asks for. That keeps this test about `state` alone: the only thing wrong with
        // this request is the value in it.
        let forged = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_ne!(forged, browser.state, "the forged state must differ");

        let (status, headers, body) = call(
            app.clone(),
            browser.walked_into(forged, "an-authorization-code"),
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
        let (status, headers, _) = call(app, browser.returns_with("an-authorization-code")).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "the genuine callback must still complete, or this test would pass simply because \
             sign-in is broken for everybody",
        );
        assert!(
            planted_session(&headers).is_some(),
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

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        let (status, headers, body) =
            call(app, browser.returns_with("an-authorization-code")).await;

        assert_eq!(status, StatusCode::OK, "{body}");

        let planted =
            planted_session(&headers).expect("a completed sign-in plants a session cookie");

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

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        let replay = || browser.returns_with("an-authorization-code");

        let (first, headers, _) = call(app.clone(), replay()).await;
        assert_eq!(first, StatusCode::OK, "the first callback completes");
        assert!(planted_session(&headers).is_some());

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

            let browser = begin(&app).await;
            exchange.claims.lock().expect("an unpoisoned lock").nonce = wrong.map(str::to_string);

            let (status, headers, body) =
                call(app, browser.returns_with("an-authorization-code")).await;

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
            .expect("a location is ASCII")
            .to_string();
        let location = location.as_str();
        let planted = headers
            .get(SET_COOKIE)
            .map(|value| value.to_str().expect("a cookie is ASCII").to_string());

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
            callback_from(
                planted.as_deref(),
                &format!("?state={state}&code=an-authorization-code"),
            ),
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

        let browser = begin(&app).await;
        exchange.echoing(&browser.nonce);

        // Every request-side vector this route has: extra query parameters on the callback.
        let (status, headers, _) = call(
            app.clone(),
            callback_from(
                browser.planted.as_deref(),
                &format!(
                    "?state={}&code=a-code&tenant={CLAIMED}&as={CLAIMED}",
                    browser.state
                ),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let planted = planted_session(&headers).expect("a session cookie");
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
        let browser = begin(&app).await;
        let (unreachable, headers, leaked) = call(app, browser.returns_with("a-code")).await;

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
        let browser = begin(&app).await;
        let (rejected, _, body) = call(app, browser.returns_with("a-code")).await;

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

        let browser = begin(&app).await;
        let forged = "0000000000000000000000000000000000000000000000000000000000000000";

        // Probed from a browser holding its own binder, which is the strongest prober there is: it
        // has everything a genuine callback has except a `code`.
        let probe =
            |state: &str| callback_from(browser.planted.as_deref(), &format!("?state={state}"));

        let (known, known_headers, known_body) = call(app.clone(), probe(&browser.state)).await;
        let (unknown, _, unknown_body) = call(app.clone(), probe(forged)).await;

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
        exchange.echoing(&browser.nonce);
        let (status, headers, body) =
            call(app, browser.returns_with("an-authorization-code")).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "probing a live state must not spend it: {body}",
        );
        assert!(planted_session(&headers).is_some());
    }

    // ---------------------------------------------------------------------------------------
    // X-43: whether this deployment can sign anyone in, as a field rather than as prose.
    // ---------------------------------------------------------------------------------------

    /// The path under test. A literal rather than `super::MODULE.routes[2].path`, because the two
    /// existing constants read an index that already exists — indexing one that does not yet
    /// would be a compile error rather than a failing test, and this story's first item asks for
    /// a test that *fails*. The declaration is asserted instead where it belongs: the route's
    /// module and path appear in `super::super::tests::ANONYMOUS`, and that test walks the
    /// assembled app rather than this table.
    const AVAILABILITY: &str = "/api/signin/availability";

    /// The field a caller reads, named so it still says what it means if a later story embeds it
    /// in a wider answer.
    const FIELD: &str = "sign_in_available";

    /// Every composition this host has, each with whether a human can sign in through it.
    ///
    /// Written out in one place because three tests below drive all of them, and a composition
    /// covered by two of them is the gap that matters. The list is exhaustive by construction:
    /// [`SignIn`] has four variants and [`SignIn::available`] matches them with no wildcard arm, so
    /// a fifth cannot arrive without a compile error pointing at the decision — and a fifth that
    /// arrived without a row here would be a state two of the three tests below never drove.
    ///
    /// **The fourth entry is X-57's**, and it is the one that makes this fixture say something it
    /// did not before: a composition where sign-in is available and is *not* a redirect. Until it
    /// existed, "available" and "redirects to a provider" were the same set, which is exactly the
    /// conflation [`SignIn::available`] used to encode.
    fn every_composition() -> Vec<(&'static str, AppState, bool)> {
        vec![
            (
                "a bound OIDC provider",
                AppState::with_oidc(Arc::new(Oidc::new(
                    config(),
                    Arc::new(StubExchange::returning(claims("nonce"))),
                ))),
                true,
            ),
            ("the development identity armed", development(), true),
            (
                "OIDC configured with no token exchange",
                AppState::oidc_without_a_token_exchange(),
                false,
            ),
            (
                "nothing configured at all",
                AppState::without_identity(),
                false,
            ),
        ]
    }

    /// **X-43's failing-first test.** A caller with no session learns whether this deployment can
    /// sign anyone in, from a **field** rather than from a sentence.
    ///
    /// The console renders a *Sign in* link unconditionally, so on a host with no identity
    /// provider that link leads to a `503`. The distinction existed only inside the explanatory
    /// page's prose, and a console that branches on the wording of a refusal breaks the day
    /// somebody improves the wording. This is the same fact, in the one shape a client can read
    /// without guessing: a JSON boolean.
    ///
    /// Driven anonymously — no cookie, no `Authorization` — because the caller this exists for is
    /// precisely the one that has nothing to present.
    #[tokio::test]
    async fn whether_this_deployment_can_sign_anyone_in_is_a_field() {
        for (composition, state, expected) in every_composition() {
            let (status, _, body) = call(super::super::app(state), get(AVAILABILITY)).await;

            assert_eq!(
                status,
                StatusCode::OK,
                "asking whether sign-in is available must be answerable by a caller with no \
                 session, and {composition} answered {status}: {body}",
            );

            let answer: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|error| {
                panic!("{composition} answered something that is not JSON ({error}): {body}")
            });

            assert_eq!(
                answer[FIELD],
                serde_json::Value::Bool(expected),
                "`{FIELD}` must be the boolean {expected} for {composition}, so a console reads a \
                 field rather than parsing prose: {body}",
            );
        }
    }

    /// The disclosure half, asserted adversarially rather than assumed.
    ///
    /// **Whether sign-in works is a fact about the service; what it is configured with is not.**
    /// Two claims, and the first is the sharper one:
    ///
    /// 1. **The two unavailable compositions answer byte for byte identically.** "OIDC configured
    ///    but no token exchange" and "nothing configured at all" are different mistakes with
    ///    different remedies — `unconfigured_page` and `no_token_exchange_page` say so, to a human
    ///    who asked for a login. But telling them apart *here* would tell an anonymous caller
    ///    whether this host's eight OIDC variables are set, which is the shape of the deployment
    ///    and none of their business. That is why the field is a boolean and not a three-valued
    ///    enum: the third state is real, and it is not the caller's to know.
    /// 2. **Nothing configured appears in any of the three answers.** Not a variable name — the
    ///    startup log names unset ones deliberately, and that is the operator's channel, not this
    ///    one — not the issuer, not the client id, not the tenant, not an endpoint.
    #[tokio::test]
    async fn the_availability_answer_discloses_nothing_about_the_configuration() {
        let mut unavailable = Vec::new();

        for (composition, state, expected) in every_composition() {
            let (status, _, body) = call(super::super::app(state), get(AVAILABILITY)).await;

            // Everything this host is configured with, and every name it could be named by.
            let withheld: Vec<&str> = WITHHELD_FROM_THE_PAGE
                .iter()
                .copied()
                .chain([
                    ISSUER,
                    CLIENT_ID,
                    TENANT,
                    "accounts.example.com",
                    "authorize",
                ])
                .collect();

            for secret in withheld {
                assert!(
                    !body.contains(secret),
                    "{composition} disclosed `{secret}` to an anonymous caller: {body}",
                );
            }

            let answer: serde_json::Value =
                serde_json::from_str(&body).expect("an availability answer is JSON");
            let object = answer
                .as_object()
                .expect("an availability answer is an object");

            assert_eq!(
                object.keys().collect::<Vec<_>>(),
                vec![FIELD],
                "the answer carries the capability fact and nothing else — an extra field is an \
                 extra thing an anonymous caller learns: {body}",
            );

            if !expected {
                unavailable.push((composition, status, body));
            }
        }

        let [(first, first_status, first_body), (second, second_status, second_body)] =
            unavailable.as_slice()
        else {
            panic!("exactly two compositions cannot sign anyone in: {unavailable:?}");
        };

        assert_eq!(
            first_status, second_status,
            "`{first}` and `{second}` must not differ in status",
        );
        assert_eq!(
            first_body, second_body,
            "nor in the body, byte for byte — otherwise this route reports whether this host's \
             OIDC settings are configured, to a caller that presented nothing",
        );
    }

    /// `/api/signin` answers exactly what it answered before. This story adds a way to ask
    /// beforehand; it does not change the answer.
    ///
    /// Every composition, both routes in one run, so "the field says available" and "`/api/signin`
    /// does not turn the caller away" are pinned as the *same* fact rather than as two that could
    /// drift. That is the regression worth naming: a field the console trusts and a route that
    /// disagrees with it is worse than the prose it replaced.
    ///
    /// **X-57 split one branch of this in two, and that split is the story.** The available arm
    /// used to assert a `303`, which read as "available means a redirect" — the same conflation
    /// [`SignIn::available`] itself carried, one layer up. It is a redirect only when a *provider*
    /// is bound; the development identity signs a caller in against this host, so `/api/signin`
    /// answers it rather than sending it anywhere. What both available arms share is the claim
    /// worth keeping: the caller is not told this host cannot sign anybody in.
    #[tokio::test]
    async fn asking_whether_sign_in_is_available_does_not_change_what_signin_answers() {
        for (composition, state, expected) in every_composition() {
            // Read before the state is moved into the router. Which of the two available shapes
            // this composition has is decided here, in the language that owns the enum, rather
            // than by matching on the composition's name.
            let redirects = matches!(state.sign_in(), SignIn::Oidc(_));
            let app = super::super::app(state);

            // Ask first, which is the whole point of the route.
            let (_, _, asked) = call(app.clone(), get(AVAILABILITY)).await;
            let (status, headers, body) = call(app, get(SIGNIN)).await;

            if redirects {
                assert_eq!(
                    status,
                    StatusCode::SEE_OTHER,
                    "{composition} federates sign-in, so `{SIGNIN}` must still redirect: {body}",
                );
                assert!(
                    headers.get(header::LOCATION).is_some(),
                    "{composition}: a redirect names where it goes",
                );
            } else if expected {
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "{composition} reports sign-in available and federates nothing, so `{SIGNIN}` \
                     must answer rather than refuse — a `503` here would be the console's \
                     affordance leading to a page saying this host cannot do the thing the field \
                     just said it can: {body}",
                );
                assert!(
                    body.contains("<html"),
                    "{composition}: and be a page: {body}",
                );
                assert!(
                    headers.get(header::LOCATION).is_none(),
                    "{composition}: there is no provider to send the browser to",
                );
            } else {
                assert_eq!(
                    status,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "{composition} reports sign-in unavailable, so `{SIGNIN}` must still explain: \
                     {body}",
                );
                assert!(
                    body.contains("<html"),
                    "{composition}: and still be a page: {body}"
                );
                assert!(
                    headers.get(header::LOCATION).is_none(),
                    "{composition}: it must not send the browser to a provider it cannot return \
                     from",
                );
            }

            assert!(
                !carries_a_token(&asked),
                "{composition}: asking about availability issues nothing: {asked}",
            );
        }
    }

    // ---------------------------------------------------------------------------------------
    // X-57: "available" stops meaning "OIDC is configured".
    // ---------------------------------------------------------------------------------------

    /// The descriptor, the other surface this boolean is published on.
    const ONBOARDING: &str = "/api/onboarding";

    /// A roster with two tenants on it, so an answer that leaked one could be caught leaking it.
    const ROSTER: &str = "user:alice@acme,user:bob@globex";

    /// A composition with the development identity armed — the one this story is about.
    fn development() -> AppState {
        AppState::with_development_identity(Arc::new(
            crate::dev_identity::DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
    }

    /// **X-57's failing-first test.** A host with the development identity armed reports that a
    /// caller can sign in.
    ///
    /// It *can* turn a caller into a principal — that is what [`IdentityBinding::Development`]
    /// means, and `crate::routes::identity` already exchanges a resolved principal for a session on
    /// exactly this composition. Reporting `false` here is the field answering "is OIDC
    /// configured", which is not the question it is named for: the console reads this to decide
    /// whether to offer signing in at all, so a host that would let it in is one it declines to
    /// offer a way into.
    ///
    /// Both anonymous surfaces, because the field is published on both and a console that trusted
    /// one while the other disagreed would be worse off than one that trusted neither.
    ///
    /// [`IdentityBinding::Development`]: crate::bind::IdentityBinding::Development
    #[tokio::test]
    async fn a_host_with_the_development_identity_reports_that_a_caller_can_sign_in() {
        let app = super::super::app(development());

        for endpoint in [AVAILABILITY, ONBOARDING] {
            let (status, _, body) = call(app.clone(), get(endpoint)).await;

            assert_eq!(status, StatusCode::OK, "`{endpoint}`: {body}");

            let answer: serde_json::Value = serde_json::from_str(&body)
                .unwrap_or_else(|error| panic!("`{endpoint}` answered non-JSON ({error}): {body}"));

            assert_eq!(
                answer[FIELD],
                serde_json::Value::Bool(true),
                "`{endpoint}` reports that this deployment can sign nobody in, and the development \
                 identity is armed — it mints principals from a roster the operator wrote at \
                 startup, and `POST /api/session` exchanges one for a session. `{FIELD}` answers \
                 *can this deployment turn a caller into a principal*, not *is OIDC configured*: \
                 {body}",
            );
        }
    }

    /// **X-57's second failing-first test.** Nothing published anonymously says *which kind* of
    /// provider a deployment runs.
    ///
    /// Two deployments with two different providers, and the anonymous answer must be identical —
    /// byte for byte, on both surfaces that carry the field. This is the property
    /// [`SignIn::available`] already collapsed three states to protect, asserted across the axis
    /// this story widens: a stranger learns whether sign-in works and nothing about what it is
    /// wired to.
    ///
    /// Byte identity rather than a field comparison, for
    /// `super::super::onboarding::tests::the_document_is_identical_with_two_tenants_connected`'s
    /// reason: a leak worth catching does not arrive as a key somebody thought to check.
    #[tokio::test]
    async fn no_anonymous_surface_says_which_kind_of_provider_signs_people_in() {
        let federated = super::super::app(AppState::with_oidc(Arc::new(Oidc::new(
            config(),
            Arc::new(StubExchange::returning(claims("nonce"))),
        ))));
        let local = super::super::app(development());

        for endpoint in [AVAILABILITY, ONBOARDING] {
            let (federated_status, _, federated_body) =
                call(federated.clone(), get(endpoint)).await;
            let (local_status, _, local_body) = call(local.clone(), get(endpoint)).await;

            assert_eq!(
                federated_status, local_status,
                "`{endpoint}`: a federated host and a local one must not differ in status",
            );
            assert_eq!(
                federated_body, local_body,
                "`{endpoint}` answers a federated deployment and a locally-provisioned one \
                 differently, so an anonymous caller can tell which kind of provider this host \
                 runs. Whether sign-in works is a fact about the service; what it is wired to is \
                 the shape of the deployment and none of a stranger's business",
            );

            // And neither answer names the roster it would resolve, which is the disclosure this
            // story could most easily have added: a handle is a credential with no secret in it.
            for handle in ["alice", "bob", "acme", "globex"] {
                assert!(
                    !local_body.contains(handle),
                    "`{endpoint}` named `{handle}`, which is a principal or a tenant this host \
                     would mint rather than a fact about the service: {local_body}",
                );
            }
        }
    }

    /// The page `/api/signin` answers on a development host says how to sign in, and names no
    /// principal it would mint.
    ///
    /// The other half of the disclosure decision. `/api/signin` is the **operator's** channel and
    /// is allowed to say more than the fact surfaces — the two `503` pages already tell an
    /// anonymous caller which of two misconfigurations a host has — but "more" stops at the
    /// mechanism. A roster handle is a working credential on this composition, so a page that
    /// printed one would be an anonymous page handing out a way in. See `development_page`.
    #[tokio::test]
    async fn the_development_signin_page_explains_how_and_names_nobody() {
        let app = super::super::app(development());
        let (status, headers, body) = call(app.clone(), get(SIGNIN)).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("<html"), "and is a page: {body}");
        assert!(
            headers.get(header::LOCATION).is_none(),
            "there is no provider to send the browser to: {body}",
        );
        assert!(
            body.contains("/api/session"),
            "the page must say how sign-in is done here, or the affordance leads to prose that \
             changes nothing: {body}",
        );
        assert!(
            !body.contains("<form"),
            "an explicit roster never becomes automatic merely because this fixture has one entry: {body}",
        );

        let (post_status, post_headers, post_body) = call(
            app,
            HttpRequest::builder()
                .method("POST")
                .uri(SIGNIN)
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await;
        assert_eq!(post_status, StatusCode::NOT_FOUND, "{post_body}");
        assert!(
            post_headers.get(SET_COOKIE).is_none(),
            "an explicit roster POST planted a session: {post_body}",
        );

        for named in ["alice", "bob", "acme", "globex"] {
            assert!(
                !body.contains(named),
                "the page named `{named}`, which is a credential on this composition rather than \
                 a fact about the build: {body}",
            );
        }

        // The same line the two `503` pages hold: the remedy's shape, never this deployment's
        // settings. See `unconfigured_page`.
        for variable in WITHHELD_FROM_THE_PAGE {
            assert!(
                !body.contains(variable),
                "the page must not enumerate this host's settings to an anonymous caller, found \
                 {variable}: {body}",
            );
        }
    }

    /// A callback cannot be completed on a host that federates nothing, and issues nothing trying.
    ///
    /// `crate::routes::identity` is where a caller signs in on this composition. What matters here
    /// is that the callback does not become a second door: no cookie, no token, and an answer that
    /// says nothing about what this host is holding.
    #[tokio::test]
    async fn a_callback_on_a_development_host_issues_nothing() {
        let (status, headers, body) = call(
            super::super::app(development()),
            get(&format!("{CALLBACK}?state=anything&code=anything")),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "this host planted no state and never will: {body}",
        );
        assert!(headers.get(SET_COOKIE).is_none(), "{body}");
        assert!(!carries_a_token(&body), "{body}");
    }
}
