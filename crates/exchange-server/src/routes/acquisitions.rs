//! Delegated credential acquisition over HTTP: the redirect out, and the callback back.
//!
//! ```text
//! POST /api/acquisitions/{connector}/authorize   open one delegated authorization
//! GET  /api/acquisitions/callback                the vendor's redirect back
//! ```
//!
//! # What this is, and how it differs from `POST /api/connections/{connector}`
//!
//! Every credential this host held before X-75 was pasted in; X-75 added one this host mints from a
//! password the person types **here**, which is why `AuthHazard` exists to let a deployment refuse
//! it. This is the third and the one those two exist to make unnecessary: the person authorizes in
//! their own browser, at the vendor, and this host never sees a resource-owner secret at all.
//!
//! The credential that comes back therefore acts **as that person**, not as the tenant, which
//! decides where it is kept — see [`crate::delegated_acquisition`], and
//! `docs/designs/credential-acquisition.md` § "Addressing a delegated credential".
//!
//! # The callback is anonymous, and it mints authority. Why that is safe
//!
//! It has to be anonymous: the browser arrives mid-redirect from the vendor's origin, and the
//! session cookie is `SameSite=Strict`, so it is **not sent** on that navigation. A guarded callback
//! would refuse every real vendor redirect and succeed only in tests, which is a control that only
//! appears to exist — `crate::routes::signin` reached the same conclusion for the same reason and
//! this route inherits nothing from it beyond the argument.
//!
//! What makes an anonymous route minting a tenant credential safe is that it **reads no caller
//! identity at all**, and cannot:
//!
//! 1. **The principal comes from the pending delegation, never from the request.** It was recorded
//!    when a signed-in human opened the authorization on a guarded route, and the credential's
//!    address is derived from it. There is no header, cookie, body or query parameter on this route
//!    that reaches the address — the callback carries `state` and `code` and nothing else is read.
//! 2. **Its authority is a single-use `state` this host drew from the OS**, plus a binder cookie
//!    only this host could have planted. Both are spent together or not at all.
//! 3. **It answers with a document and a `Set-Cookie`, never a body a script reads.** No token, no
//!    address and no `state` appears in what comes back, so the widening `crate::routes::signin`
//!    argues for — *no route reachable without a readable credential ever puts a credential in a
//!    body* — holds here too.
//!
//! So a caller who invents a `state` reaches one map lookup and is refused, having contacted no
//! vendor and touched no store. That ordering is copied from `crate::oidc::complete_admission` and
//! it is the reason the refusals below are cheap.
//!
//! # What is not built yet (X-147, and the 0.21 connector line)
//!
//! The authorization URL is composed from the **injected** [`DelegatedGrant`] a composition bound,
//! not from the connector's own `Acquisition::OAuth2` declaration — that metadata is unreleased, and
//! taking a `path` dependency on the sibling checkout to reach it is refused by `AGENTS.md`. So
//! production still composes an empty registry and this surface answers `422` for every connector,
//! exactly as the password acquisition does. The story's two criteria that name the declaration stay
//! unticked rather than being satisfied against a fixture and called done.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{Provider, ProviderKey};
use exchange_host::{
    admit_tenant_occupancy, stored_bytes, AcquisitionRefusal, AuthPostureRefusal, CredentialRef,
    CredentialScope, Principal, Secret, SecretBatch, SecretStore, StoreError,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use super::{Access, Module, Route};
use crate::acquisition_redirect::CALLBACK_PATH;
use crate::credential_acquisition::AcquisitionBinding;
use crate::delegated_acquisition::{self, ClaimedDelegation, DelegationRefusal, PendingDelegation};
use crate::oidc::urlencoded;
use crate::state::AppState;

/// Where a completed acquisition sends the browser.
const AFTER_ACQUISITION: &str = "/";

/// The value written at the managed marker, matching the password acquisition's.
const MANAGED_VALUE: &str = "v1";

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    routes: &[
        Route {
            // **A `User`, and deliberately not an `Operator`.** The whole point of this grant is
            // that the credential is *the person's own* — an operator-only route would mean one
            // administrator authorizing on everybody's behalf, which is the shared credential this
            // story exists to replace. It is still a kind gate rather than an open route:
            // `MAY_SUPPLY_A_CREDENTIAL`'s argument in `crate::routes::connections` holds unchanged,
            // because deciding which credential this tenant's operations run under is human work.
            path: "/api/acquisitions/{connector}/authorize",
            access: Access::User,
            method_router: authorize_route,
        },
        Route {
            path: CALLBACK_PATH,
            access: Access::Anonymous,
            method_router: callback_route,
        },
    ],
    name: "acquisitions",
};

fn authorize_route() -> MethodRouter<AppState> {
    post(authorize)
}

fn callback_route() -> MethodRouter<AppState> {
    get(callback)
}

/// Open one delegated authorization and tell the console where to send the person.
///
/// Answers a JSON `authorize_url` rather than a `303`, because the caller is the console's own
/// script: a redirect on an XHR is followed by the browser without the page ever seeing it, and the
/// console has to open the vendor's consent screen as a top-level navigation for the person to read
/// what they are agreeing to.
async fn authorize(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return refuse(
            StatusCode::NOT_FOUND,
            format!("no connector `{connector}` is in this host's catalogue"),
            "unknown_connector",
            json!({ "connector": connector }),
        );
    };
    let Some(binding) = state.acquisitions().get(provider.id) else {
        return not_delegable(provider);
    };
    let Some(grant) = binding.delegated() else {
        return not_delegable(provider);
    };
    if let Err(refusal) = admit_hazard(&state, provider, binding) {
        return *refusal;
    }
    if state.acquisition_redirect().is_none() {
        return delegation_refused(provider, &DelegationRefusal::NoRedirectUri);
    }

    // Derived before anything is drawn or remembered, so a principal whose delegated credential
    // could never be addressed is refused with no state opened and nothing planted.
    if let Err(refusal) = delegated_writes(principal.clone(), provider, binding.credential()) {
        return delegation_refused(provider, &refusal);
    }

    let begun = match state.pending_delegations().begin(provider.id, &principal) {
        Ok(begun) => begun,
        Err(refusal) => {
            warn!(connector = provider.id, reason = %refusal, "cannot open a delegated authorization");
            return delegation_refused(provider, &refusal);
        }
    };

    // One `DelegatedGrant`, so the `redirect_uri` here and the one the token request re-presents are
    // the same bytes; `crate::acquisition_redirect` refuses at startup any spelling a URL parser
    // would rewrite, which is what makes "compared exactly" a property rather than an intention.
    let separator = if grant.authorization_endpoint().contains('?') {
        '&'
    } else {
        '?'
    };
    let authorize_url = format!(
        "{endpoint}{separator}response_type=code\
         &client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &scope={scope}\
         &state={state_parameter}\
         &code_challenge={challenge}\
         &code_challenge_method={method}",
        endpoint = grant.authorization_endpoint(),
        client_id = urlencoded(grant.client_id()),
        redirect_uri = urlencoded(grant.redirect_uri()),
        scope = urlencoded(&grant.scope()),
        state_parameter = begun.state,
        challenge = begun.challenge.as_str(),
        method = crate::oidc::pkce::METHOD,
    );

    (
        StatusCode::CREATED,
        [(
            header::SET_COOKIE,
            delegated_acquisition::planted_binder(&begun.binder),
        )],
        Json(json!({
            "connector": provider.id,
            "credential": binding.credential(),
            "authorize_url": authorize_url,
        })),
    )
        .into_response()
}

/// What a vendor puts in the redirect back.
///
/// `code` is optional because a person who declines consent is redirected here with `error` and no
/// code. Neither field is ever echoed back or logged: `error` is the vendor's own words about a
/// person's decision, and a code is a bearer credential.
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
}

/// Finish a delegated acquisition: claim the `state`, redeem the code, store under the initiator.
///
/// **The claim comes first, before the vendor is contacted and before the store is touched.** A
/// callback this host did not open — or did not open for this browser — costs one map lookup, which
/// is the ordering `crate::oidc::complete_admission` records and the reason a hostile callback is
/// not a way to make this host dial anything.
async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Response {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok());
    let Some(binder) = delegated_acquisition::presented_binder(cookie) else {
        return callback_refused(&DelegationRefusal::NoBinder);
    };
    let Some(presented_state) = callback.state.as_deref().filter(|value| !value.is_empty()) else {
        return callback_refused(&DelegationRefusal::UnknownState);
    };

    let pending = match state.pending_delegations().claim(presented_state, binder) {
        ClaimedDelegation::Delegation(pending) => pending,
        ClaimedDelegation::Unknown => return callback_refused(&DelegationRefusal::UnknownState),
        ClaimedDelegation::AnotherBrowser => {
            return callback_refused(&DelegationRefusal::AnotherBrowser)
        }
    };

    // Spent. Everything below reports through `finished`, which clears the binder either way — the
    // delegation is gone from the store, so a binder that can no longer match anything must not sit
    // in the browser until `Max-Age` elapses.
    let Some(code) = callback.code.as_deref().filter(|value| !value.is_empty()) else {
        // The person declined at the vendor, or the vendor refused. Neither is this host's to
        // explain, and `error_description` is the vendor's own words about somebody's decision.
        return finished(
            StatusCode::OK,
            "Not connected",
            "The vendor did not return an authorization. Nothing was changed.",
        );
    };

    complete(&state, *pending, code).await
}

/// Redeem one claimed delegation and commit what it produced.
async fn complete(state: &AppState, pending: PendingDelegation, code: &str) -> Response {
    let Some(provider) = catalogued(pending.connector()) else {
        // The catalogue is compiled in and cannot change under a live delegation, so this is
        // unreachable rather than a case. It refuses anyway: a delegation naming a connector this
        // build does not have must not become a write to an address derived from nothing.
        error!(
            connector = pending.connector(),
            "a claimed delegation names a connector this build does not catalogue"
        );
        return finished_refusal(
            "this host no longer catalogues the connector this authorization was opened for",
        );
    };
    let Some(binding) = state.acquisitions().get(provider.id) else {
        return finished_refusal("this connector's acquisition binding is gone");
    };
    if binding.delegated().is_none() {
        return finished_refusal("this connector's delegated grant is gone");
    }
    // The JSON refusal the authorize leg would answer with is discarded: the browser is mid-redirect
    // and gets the page every spent delegation gets. What an operator needs is already in the log
    // line the posture refusal produced upstream.
    if admit_hazard(state, provider, binding).is_err() {
        return finished_refusal("this deployment does not allow this connector's acquisition");
    }
    let Some(store) = state.credentials().cloned() else {
        return finished_refusal("this host has no credential store bound");
    };
    let writes = match delegated_writes(pending.principal().clone(), provider, binding.credential())
    {
        Ok(writes) => writes,
        Err(refusal) => {
            warn!(connector = provider.id, reason = %refusal, "a delegated credential has no address");
            return finished_refusal(
                "this host cannot address the credential this authorization would produce",
            );
        }
    };

    // The proof key never left this process; the code came back through a browser. Both are
    // borrowed for the one outbound call and fall out of scope without entering a store.
    let code = Secret::new(code);
    let verifier = Secret::new(pending.verifier().as_str());
    let acquired = match binding
        .performer()
        .redeem_authorization_code(exchange_host::AuthorizationCodeRedemption::new(
            &code, &verifier,
        ))
        .await
    {
        Ok(acquired) => acquired,
        Err(refusal) => {
            // The refusal is already value-free — see `AcquisitionRefusal` — and the connector is
            // this host's own fact, so both go to the log and neither goes to the browser.
            warn!(
                connector = provider.id,
                code = refusal.code(),
                "a delegated acquisition was refused"
            );
            return finished_refusal(refusal_sentence(refusal));
        }
    };
    drop(verifier);
    drop(code);

    let (access_token, refresh_token, expires_at) = acquired.into_parts();
    let expiry = expires_at.map(|value| Secret::new(value.to_string()));
    let adding = stored_bytes(&access_token)
        + refresh_token.as_ref().map_or(0, stored_bytes)
        + expiry.as_ref().map_or(0, stored_bytes)
        + MANAGED_VALUE.len();
    let held_bytes = match occupied_scope(&store, &writes.scope).await {
        Ok(bytes) => bytes,
        Err(error) => {
            error!(%error, connector = provider.id, "the credential store could not be read");
            return finished_refusal("this host's credential store could not be read");
        }
    };
    if let Err(refusal) = admit_tenant_occupancy(held_bytes, adding) {
        warn!(connector = provider.id, %refusal, "a delegated credential does not fit this tenant's allowance");
        return finished_refusal("this tenant has no room for another credential");
    }

    // One batch: the access token, its refresh token, its expiry and the managed marker commit
    // together or not at all. A half-written delegation is an access token nothing can renew.
    let mut batch = SecretBatch::new(writes.scope.clone());
    for (reference, secret) in [
        (Some(writes.access.clone()), Some(access_token)),
        (Some(writes.refresh.clone()), refresh_token),
        (Some(writes.expiry.clone()), expiry),
        (
            Some(writes.managed.clone()),
            Some(Secret::new(MANAGED_VALUE)),
        ),
    ] {
        let (Some(reference), Some(secret)) = (reference, secret) else {
            continue;
        };
        if let Err(reason) = batch.put(reference, secret) {
            error!(connector = provider.id, %reason, "a delegated credential batch could not be assembled");
            return finished_refusal("this host could not assemble the credential write");
        }
    }
    if let Err(error) = store.apply(&batch).await {
        error!(%error, connector = provider.id, "a delegated credential could not be committed");
        return finished_refusal("this host's credential store refused the acquired credential");
    }

    if let Some(channels) = state.channels() {
        channels.restart(pending.principal().tenant(), provider.id);
    }

    finished(
        StatusCode::OK,
        "Connected",
        "This connection is authorized with your own account. Returning to the console…",
    )
}

/// The four addresses one delegated acquisition writes, and the scope they commit inside.
struct DelegatedWrites {
    scope: CredentialScope,
    access: CredentialRef,
    refresh: CredentialRef,
    expiry: CredentialRef,
    managed: CredentialRef,
}

/// Derive every address this acquisition would write, from the **initiating principal** and the
/// catalogue, and from nothing in any request.
///
/// `credential` is the connector's **flat** catalogue name — `gitlab.token`, the spelling an
/// operation references — and the address carries the declared `leaf`, which the catalogue supplies
/// and no caller does. That indirection is `crate::routes::connections`' rule restated one level
/// over: a name a composition bound is a key into the connector's own declaration, never a segment
/// of an address.
fn delegated_writes(
    principal: Principal,
    provider: &'static Provider,
    credential: &str,
) -> Result<DelegatedWrites, DelegationRefusal> {
    let Some(authority) = provider.authority else {
        return Err(DelegationRefusal::Unaddressable(
            "this connector declares no authority, so it has no credential address".to_owned(),
        ));
    };
    let Some(declared) = provider
        .auth
        .iter()
        .find(|candidate| candidate.name == credential)
    else {
        return Err(DelegationRefusal::Unaddressable(format!(
            "this connector declares no credential named {credential:?}"
        )));
    };
    let access = delegated_acquisition::delegated_address(&principal, authority, declared.leaf)?;
    let companions = delegated_acquisition::delegated_companions(&access)?;
    let scope = CredentialScope::new(access.tenant(), access.authority())
        .map_err(DelegationRefusal::Unaddressable)?;
    Ok(DelegatedWrites {
        scope,
        access,
        refresh: companions.refresh,
        expiry: companions.expiry,
        managed: companions.managed,
    })
}

/// What one tenant already occupies inside one connector's scope.
///
/// Narrower than `connections::occupied`, which sums every connector: this route commits inside one
/// scope and the wider sum needs the tenant-wide claim that route holds. Being narrower means it can
/// only ever *under*-count, so it is a bound this host applies rather than the whole allowance —
/// recorded here rather than left to be discovered, and the tenant-wide accounting for delegated
/// credentials is an open edge noted on the story.
async fn occupied_scope(
    store: &std::sync::Arc<dyn SecretStore>,
    scope: &CredentialScope,
) -> Result<usize, StoreError> {
    let mut total = 0usize;
    for reference in store.references(scope).await? {
        match store.get(&reference).await {
            Ok(secret) => total = total.saturating_add(stored_bytes(&secret)),
            Err(error) if error.is_not_found() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(total)
}

/// Decide the deployment's hazard policy, admitting a binding that declares no hazard.
///
/// The decision itself is [`AcquisitionBinding::admit`], stated once for every acquisition route.
/// `None` is not "no policy applied": it is the acquisition saying it carries no named weakness,
/// which is exactly what an authorization-code grant with PKCE is — the resource owner's secret
/// goes to the authorization server and never to this host. `AuthPosture::fail_closed` refuses every
/// hazard, so a `Some` here is refused on the default deployment and a `None` is not, which is the
/// difference X-74 built the posture to express.
///
/// The refusal is boxed for `crate::routes::connections`' reason: a `Response` is large, and an
/// `Err` variant that carries one unboxed makes every `Result` in the call chain that size.
fn admit_hazard(
    state: &AppState,
    provider: &'static Provider,
    binding: &AcquisitionBinding,
) -> Result<(), Box<Response>> {
    binding
        .admit(state.auth_posture())
        .map_err(|refusal: AuthPostureRefusal| {
            Box::new(refuse(
                StatusCode::FORBIDDEN,
                refusal.to_string(),
                "authentication_hazard_not_allowed",
                json!({ "connector": provider.id }),
            ))
        })
}

/// The connector the catalogue declares under `id`.
fn catalogued(id: &str) -> Option<&'static Provider> {
    connector_catalog::provider(ProviderKey::id(id))
}

fn not_delegable(provider: &'static Provider) -> Response {
    delegation_refused(provider, &DelegationRefusal::NotDelegable)
}

fn delegation_refused(provider: &'static Provider, refusal: &DelegationRefusal) -> Response {
    let status = match refusal {
        DelegationRefusal::NotDelegable => StatusCode::UNPROCESSABLE_ENTITY,
        DelegationRefusal::TooManyPending => StatusCode::TOO_MANY_REQUESTS,
        DelegationRefusal::NoRedirectUri | DelegationRefusal::NoFlow(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        DelegationRefusal::Unaddressable(_) => StatusCode::CONFLICT,
        // Not reachable from the authorize route; the callback answers a page, not JSON.
        DelegationRefusal::NoBinder
        | DelegationRefusal::UnknownState
        | DelegationRefusal::AnotherBrowser => StatusCode::BAD_REQUEST,
    };
    refuse(
        status,
        refusal.caller_facing(),
        refusal.code(),
        json!({ "connector": provider.id }),
    )
}

/// The one page every unmatched callback gets, and the log line that separates them.
fn callback_refused(refusal: &DelegationRefusal) -> Response {
    // The three are told apart here, where an operator needs them, and nowhere the caller can see.
    warn!(reason = %refusal, "a delegated acquisition callback was refused");
    (
        StatusCode::BAD_REQUEST,
        [(header::SET_COOKIE, delegated_acquisition::cleared_binder())],
        Html(document("Not connected", refusal.caller_facing(), None)),
    )
        .into_response()
}

/// A refusal after the delegation was already spent. Same shape, and the binder is cleared too.
fn finished_refusal(reason: &str) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        [(header::SET_COOKIE, delegated_acquisition::cleared_binder())],
        Html(document("Not connected", reason, None)),
    )
        .into_response()
}

/// The page a spent delegation answers with, and the `Set-Cookie` that clears its binder.
fn finished(status: StatusCode, title: &str, message: &str) -> Response {
    (
        status,
        [(header::SET_COOKIE, delegated_acquisition::cleared_binder())],
        Html(document(title, message, Some(AFTER_ACQUISITION))),
    )
        .into_response()
}

/// A small page, for the reason `crate::routes::signin` answers one: the navigation that follows
/// has to be one this document initiated, or a `SameSite=Strict` session cookie is withheld on it.
///
/// It carries no `state`, no code, no address and no token — there is nothing here a script could
/// read a credential out of, which is half of why this route may be anonymous.
fn document(title: &str, message: &str, then: Option<&str>) -> String {
    let refresh = then
        .map(|target| format!("<meta http-equiv=\"refresh\" content=\"1; url={target}\">"))
        .unwrap_or_default();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>{title}</title>{refresh}</head><body><h1>{title}</h1><p>{message}</p></body></html>"
    )
}

/// The caller-facing sentence for one vendor refusal.
///
/// [`AcquisitionRefusal`]'s own `Display` is already value-free, so this is a rendering rather than
/// a redaction — it exists so the page reads as a sentence to a person rather than as an API error.
fn refusal_sentence(refusal: AcquisitionRefusal) -> &'static str {
    match refusal {
        AcquisitionRefusal::GrantNotPerformed => {
            "this deployment cannot perform the grant this connector needs"
        }
        AcquisitionRefusal::Unreachable => "the vendor's credential endpoint could not be reached",
        _ => "the vendor did not issue a credential for this authorization",
    }
}

/// A refusal as the caller sees it: a status, a sentence and a code, never a value.
fn refuse(status: StatusCode, reason: impl Into<String>, code: &str, mut extra: Value) -> Response {
    if let Some(object) = extra.as_object_mut() {
        object.insert("error".to_owned(), json!(reason.into()));
        object.insert("code".to_owned(), json!(code));
    }
    (status, Json(extra)).into_response()
}

/// Where a delegated credential for one principal and one connector lives.
///
/// Exposed for the tests below and for a console that will one day render it. It is an **address**
/// and never a value, which is the rule `crate::routes::connections` states for its whole surface.
#[cfg(test)]
pub(crate) fn delegated_address_path(
    principal: &Principal,
    provider: &'static Provider,
    credential: &str,
) -> Option<String> {
    delegated_writes(principal.clone(), provider, credential)
        .ok()
        .map(|writes| exchange_host::address_path(&writes.access))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{HeaderValue, Method, Request as HttpRequest};
    use exchange_host::{
        async_trait, AcquiredCredential, AuthHazard, AuthPosture, AuthorizationCodeRedemption,
        CredentialAcquirer, CredentialStore, PasswordRedemption, PrincipalKind, RefreshRedemption,
        Tenant,
    };
    use tower::ServiceExt as _;

    use crate::credential_acquisition::{AcquisitionBindings, DelegatedGrant};
    use crate::delegated_acquisition::BINDER_COOKIE;
    use crate::dev_identity::DevIdentity;
    use crate::routes::app;

    const ROSTER: &str = "user:alice@acme,user:bob@acme";

    /// This deployment's configured redirect, in the one canonical spelling
    /// `crate::acquisition_redirect` admits.
    const REDIRECT: &str = "http://127.0.0.1:3000/api/acquisitions/callback";

    /// A scratch directory holding a real file-backed store, removed on drop.
    ///
    /// The real store rather than a hand-rolled one, because the write under test is an **atomic
    /// batch** — `SecretBatch`'s operations are `pub(crate)` upstream, so a store written here
    /// could not apply one, and a test whose store answered `Unsupported` would be asserting that
    /// this route refuses rather than that it commits.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-acquisitions-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Self(path.canonicalize().expect("a resolvable scratch directory"))
        }

        fn store(&self) -> CredentialStore {
            CredentialStore::bind(self.0.join("state").join("credentials"))
                .expect("a fresh store outside every working tree")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A performer that records every delegated redemption and answers one token.
    struct RecordingPerformer {
        codes: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl CredentialAcquirer for RecordingPerformer {
        async fn redeem_password(
            &self,
            _: PasswordRedemption<'_>,
        ) -> Result<AcquiredCredential, AcquisitionRefusal> {
            unreachable!("the delegated lane never presents a password")
        }

        async fn redeem_refresh(
            &self,
            _: RefreshRedemption<'_>,
        ) -> Result<AcquiredCredential, AcquisitionRefusal> {
            unreachable!("X-148 owns renewal")
        }

        async fn redeem_authorization_code(
            &self,
            redemption: AuthorizationCodeRedemption<'_>,
        ) -> Result<AcquiredCredential, AcquisitionRefusal> {
            self.codes
                .lock()
                .expect("no test poisons this")
                .push(redemption.code().to_owned());
            Ok(AcquiredCredential::new(
                Secret::new("DELEGATED-ACCESS-NOT-A-REAL-SECRET"),
                Some(Secret::new("DELEGATED-REFRESH-NOT-A-REAL-SECRET")),
                Some(1_900_000_000),
            ))
        }
    }

    fn gitlab() -> &'static Provider {
        catalogued("gitlab").expect("the catalogue declares gitlab")
    }

    /// The credential the fixture binding mints — the connector's own first declared name.
    fn bound_credential() -> &'static str {
        gitlab().auth[0].name
    }

    fn member(id: &str) -> Principal {
        Principal::new(
            PrincipalKind::User,
            id,
            Tenant::new("acme").expect("a literal tenant"),
        )
    }

    /// One composition: a roster identity, a real store, and one **hazard-free** delegated binding
    /// on the fail-closed posture.
    fn composed(codes: &Arc<Mutex<Vec<String>>>, store: &CredentialStore) -> AppState {
        let grant = DelegatedGrant::new(
            "http://vendor.invalid/oauth/authorize",
            "exchange-client",
            None,
            ["read_api".to_owned()],
            REDIRECT,
        )
        .expect("a delegated grant");
        let binding = AcquisitionBinding::new(
            gitlab().id,
            bound_credential(),
            // **The hazard-free case.** The posture below refuses every hazard there is, and this
            // acquisition declares none — which is what an authorization-code grant with PKCE is.
            None,
            Arc::new(RecordingPerformer {
                codes: Arc::clone(codes),
            }),
        )
        .with_delegated_grant(grant);

        AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_credentials(store.secrets())
        .with_acquisition_redirect(REDIRECT)
        .with_credential_acquisition(
            AuthPosture::fail_closed(),
            Arc::new(AcquisitionBindings::new([binding]).expect("one binding")),
        )
    }

    async fn callback_request(
        state: &AppState,
        query: &str,
        binder: Option<&str>,
    ) -> (StatusCode, String) {
        let mut request = HttpRequest::builder().uri(format!("{CALLBACK_PATH}?{query}"));
        if let Some(binder) = binder {
            request = request.header(
                header::COOKIE,
                HeaderValue::from_str(&format!("{BINDER_COOKIE}={binder}"))
                    .expect("a cookie header"),
            );
        }
        let response = app(state.clone())
            .oneshot(request.body(Body::empty()).expect("a request"))
            .await
            .expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("a body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn authorize_as(state: &AppState, handle: &str) -> (StatusCode, String, Vec<String>) {
        let response = app(state.clone())
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/acquisitions/gitlab/authorize")
                    .header(header::AUTHORIZATION, format!("Bearer {handle}"))
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("a response");
        let status = response.status();
        let cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("a body");
        (status, String::from_utf8_lossy(&body).into_owned(), cookies)
    }

    /// **The Acceptance's four callback refusals, and the claim that decides all of them: none
    /// reaches the vendor.**
    ///
    /// Unknown, expired, already used, and opened by a different member's browser. Each is answered
    /// with the store untouched and the performer never called — which is what "refused without
    /// contacting the vendor" means, and it is checked by counting redemptions rather than by
    /// reading a status.
    #[tokio::test]
    async fn an_unmatched_callback_reaches_no_vendor_and_spends_nothing() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let state = composed(&codes, &store);
        let pending = state.pending_delegations();

        let hers = pending
            .begin(gitlab().id, &member("alice"))
            .expect("randomness");
        let his = pending
            .begin(gitlab().id, &member("bob"))
            .expect("randomness");

        for (why, query, binder) in [
            (
                "no binder at all",
                format!("state={}&code=THE-CODE", hers.state),
                None,
            ),
            (
                "a state this host never issued",
                "state=a-state-this-host-never-issued&code=THE-CODE".to_owned(),
                Some(hers.binder.as_str()),
            ),
            (
                "another member's browser",
                format!("state={}&code=THE-CODE", hers.state),
                Some(his.binder.as_str()),
            ),
        ] {
            let (status, body) = callback_request(&state, &query, binder).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
            assert!(!body.contains(&hers.state), "{why} leaked a state: {body}");
            assert!(
                !body.contains(hers.binder.as_str()),
                "{why} leaked a binder: {body}",
            );
            assert!(!body.contains("THE-CODE"), "{why} leaked a code: {body}");
        }

        assert!(
            codes.lock().expect("recorder").is_empty(),
            "a callback this host did not open must reach no vendor",
        );
        assert_eq!(
            pending.open(),
            2,
            "and must spend nobody's authorization on its way to being refused",
        );

        // The honest one, and then its replay: already used answers exactly as unknown does.
        let query = format!("state={}&code=THE-CODE", hers.state);
        let (status, body) = callback_request(&state, &query, Some(hers.binder.as_str())).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(codes.lock().expect("recorder").len(), 1);

        let (status, _) = callback_request(&state, &query, Some(hers.binder.as_str())).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a state must not be spendable twice",
        );
        assert_eq!(
            codes.lock().expect("recorder").len(),
            1,
            "and the replay must not reach the vendor either",
        );

        // Expired: aged rather than waited for, and refused with the vendor untouched.
        if !pending.expire_now(&his.state) {
            // A machine up for less than the TTL cannot represent an instant that old.
            return;
        }
        let (status, _) = callback_request(
            &state,
            &format!("state={}&code=THE-CODE", his.state),
            Some(his.binder.as_str()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            codes.lock().expect("recorder").len(),
            1,
            "an expired state must not reach the vendor",
        );
    }

    /// **The addressing acceptance, driven through the real route and the real store.** One member
    /// authorizes; the credential lands under that member, and the other member of the same tenant
    /// cannot resolve it.
    #[tokio::test]
    async fn one_member_cannot_resolve_another_members_delegated_credential() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let state = composed(&codes, &store);

        let (status, body, cookies) = authorize_as(&state, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let planted = cookies
            .iter()
            .find(|cookie| cookie.starts_with(&format!("{BINDER_COOKIE}=")))
            .expect("the authorize leg plants an acquisition binder");
        let binder = planted
            .split_once('=')
            .and_then(|(_, rest)| rest.split(';').next())
            .expect("a binder value");
        let answered: Value = serde_json::from_str(&body).expect("a JSON answer");
        let authorize_url = answered["authorize_url"]
            .as_str()
            .expect("an authorize URL")
            .to_owned();
        let state_parameter = authorize_url
            .split("&state=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("the URL carries the state");

        assert!(
            authorize_url.contains("code_challenge_method=S256"),
            "PKCE is mandatory: {authorize_url}",
        );
        assert!(
            !authorize_url.contains(binder),
            "the binder must never travel through the vendor: {authorize_url}",
        );
        assert!(
            authorize_url.contains(&urlencoded(REDIRECT)),
            "the configured redirect travels verbatim: {authorize_url}",
        );

        let (status, page) = callback_request(
            &state,
            &format!("state={state_parameter}&code=THE-CODE"),
            Some(binder),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(
            !page.contains("DELEGATED-ACCESS-NOT-A-REAL-SECRET"),
            "the page carries no credential: {page}",
        );

        let hers = crate::delegated_acquisition::delegated_address(
            &member("alice"),
            gitlab().authority.expect("gitlab declares an authority"),
            gitlab().auth[0].leaf,
        )
        .expect("an address");
        let his = crate::delegated_acquisition::delegated_address(
            &member("bob"),
            gitlab().authority.expect("gitlab declares an authority"),
            gitlab().auth[0].leaf,
        )
        .expect("an address");
        let secrets = store.secrets();

        assert_eq!(
            secrets
                .get(&hers)
                .await
                .expect("the initiator holds it")
                .expose_secret(),
            "DELEGATED-ACCESS-NOT-A-REAL-SECRET",
        );
        assert!(
            secrets
                .get(&his)
                .await
                .is_err_and(|error| error.is_not_found()),
            "and no other member of the tenant does — a user-subject token kept where the tenant \
             can reach it is one member acting as another",
        );

        // The refresh token and the expiry committed in the same batch, under the same reserved
        // service, so a renewal has something to renew and it is nobody else's.
        let companions =
            crate::delegated_acquisition::delegated_companions(&hers).expect("companions");
        assert!(secrets.get(&companions.refresh).await.is_ok());
        assert!(secrets.get(&companions.expiry).await.is_ok());
        assert!(secrets.get(&companions.managed).await.is_ok());
    }

    /// **The hazard-free acceptance.** A delegated grant declaring no hazard completes on a
    /// deployment whose posture refuses every hazard there is — and the same posture still refuses
    /// one that declares a hazard, so this is an absent value rather than a disabled gate.
    #[tokio::test]
    async fn a_hazard_free_acquisition_runs_on_a_fail_closed_deployment() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let state = composed(&codes, &store);
        let binding = state
            .acquisitions()
            .get(gitlab().id)
            .expect("the fixture binding");

        assert_eq!(binding.hazard(), None);
        assert!(admit_hazard(&state, gitlab(), binding).is_ok());

        let hazardous = AcquisitionBinding::new(
            gitlab().id,
            bound_credential(),
            Some(AuthHazard::ResourceOwnerSecretShared),
            Arc::new(RecordingPerformer {
                codes: Arc::clone(&codes),
            }),
        );
        assert!(
            admit_hazard(&state, gitlab(), &hazardous).is_err(),
            "the same fail-closed posture still refuses a declared hazard",
        );

        let (status, body, _) = authorize_as(&state, "alice").await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "a hazard-free acquisition opens on the safe default: {body}",
        );
    }

    /// The authorize leg is guarded: a caller this host cannot resolve opens nothing and is planted
    /// with nothing.
    #[tokio::test]
    async fn the_authorize_leg_requires_a_signed_in_human() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let state = composed(&codes, &store);

        let (status, _, cookies) = authorize_as(&state, "nobody").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            cookies.is_empty(),
            "a refused caller must not be given a binder",
        );
        assert_eq!(state.pending_delegations().open(), 0);
    }

    /// A composition with no delegated grant opens nothing and says so, which is what production
    /// does until the 0.21 connector line lands.
    #[tokio::test]
    async fn a_connector_with_no_delegated_grant_opens_nothing() {
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_acquisition_redirect(REDIRECT)
        .with_credential_acquisition(
            AuthPosture::fail_closed(),
            Arc::new(AcquisitionBindings::default()),
        );

        let (status, body, _) = authorize_as(&state, "alice").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.contains("grant_not_bound"), "{body}");
        assert_eq!(state.pending_delegations().open(), 0);
    }

    /// A deployment that bound a grant and configured no redirect refuses by name rather than
    /// deriving one from the request's `Host` header, which is a value a caller controls.
    #[tokio::test]
    async fn an_unconfigured_redirect_refuses_rather_than_being_derived() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let grant = DelegatedGrant::new(
            "http://vendor.invalid/oauth/authorize",
            "exchange-client",
            None,
            ["read_api".to_owned()],
            REDIRECT,
        )
        .expect("a delegated grant");
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_credentials(store.secrets())
        .with_credential_acquisition(
            AuthPosture::fail_closed(),
            Arc::new(
                AcquisitionBindings::new([AcquisitionBinding::new(
                    gitlab().id,
                    bound_credential(),
                    None,
                    Arc::new(RecordingPerformer {
                        codes: Arc::clone(&codes),
                    }),
                )
                .with_delegated_grant(grant)])
                .expect("one binding"),
            ),
        );

        let (status, body, _) = authorize_as(&state, "alice").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(body.contains("redirect_uri_unconfigured"), "{body}");
        assert_eq!(state.pending_delegations().open(), 0);
    }

    /// The address a delegated credential lands at is under a service no connector can declare, and
    /// it does not spell the person's identifier into a store path.
    #[test]
    fn a_delegated_address_is_under_a_reserved_service() {
        let path = delegated_address_path(&member("alice"), gitlab(), bound_credential())
            .expect("an address");

        assert!(path.starts_with("tenants/acme/"), "{path}");
        assert!(
            path.contains(crate::delegated_acquisition::DELEGATED_SERVICE_PREFIX),
            "{path}",
        );
        assert!(!path.contains("alice"), "{path}");
        assert_ne!(
            path,
            format!(
                "tenants/acme/{}/{}",
                gitlab().authority.expect("an authority"),
                gitlab().auth[0].leaf,
            ),
            "a delegated credential is not the tenant-wide connection credential",
        );
    }
}
