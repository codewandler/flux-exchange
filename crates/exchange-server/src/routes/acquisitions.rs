//! Delegated credential acquisition over HTTP: the redirect out, and the callback back.
//!
//! ```text
//! POST   /api/acquisitions/{connector}/authorize   open one delegated authorization
//! DELETE /api/acquisitions/{connector}             revoke the one you authorized
//! GET    /api/acquisitions/callback                the vendor's redirect back
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
//! # Where the authorization URL comes from (X-154)
//!
//! From the connector's own `Acquisition::OAuth2` declaration. `crate::credential_acquisition`
//! composes the grant this route reads — the authorize path and the scopes are the catalogue's, the
//! client identity and the redirect URI are the deployment's, and **nothing here is a caller's**.
//! This route did not change to make that true and did not need to: it composes from the
//! [`DelegatedGrant`] it is handed, and what changed is who wrote it.
//!
//! `tests::the_url_composed_from_gitlabs_declaration_is_the_url_this_route_produces` is what holds
//! the two together — it drives this route twice, once against a grant composed from GitLab's
//! released declaration and once against the hand-written grant X-147 was delivered with, and
//! requires the two URLs to be one. That agreement is what makes the injected seam a faithful
//! stand-in rather than a claim.
//!
//! **One declared fact still comes from outside the generated tables**, and that test is where it is
//! visible: GitLab's declaration resolves against its `login` service, whose base URL lives in the
//! canonical document rather than in `catalog::Provider`. `credential_acquisition::endpoint_base`
//! refuses a named endpoint by name for that reason, so a production deployment that registers
//! GitLab is refused at startup rather than sent to a host nobody resolved.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post, MethodRouter};
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
            // **Revoking your own delegated credential**, and a `User` for the same reason the
            // route above is. A delegated credential is addressed to one principal and nothing
            // else can reach it, so this route needs no target and takes none: it destroys the
            // caller's own and cannot be pointed at anybody else's. That is what makes it safe to
            // be a `User` route rather than an `Operator` one — there is no other member's state
            // in reach to destroy.
            path: "/api/acquisitions/{connector}",
            access: Access::User,
            method_router: revoke_route,
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

fn revoke_route() -> MethodRouter<AppState> {
    delete(revoke)
}

fn callback_route() -> MethodRouter<AppState> {
    get(callback)
}

/// Destroy the caller's own delegated credential for one connector.
///
/// # Why this route exists (X-147 review, M1)
///
/// Because without it a delegated credential could not be destroyed at all.
/// `DELETE /api/connections/{connector}` walks the connector's *declared* addresses and the
/// `exchange-acquisition` companions beside them; a delegated credential lives under a reserved
/// per-principal service that route knows nothing about, so it answered `204` while the credential
/// survived — on the route whose own documentation says the case it exists for is revoking a leaked
/// secret.
///
/// **What this does not answer** is what a *tenant-level* disconnect should do to the delegated
/// credentials of every member — destroy them, refuse while any exist, or leave them. That is a
/// design decision entangled with the addressing question the owner has reopened, and it is recorded
/// on the story rather than guessed at here. This route closes the half that has one obvious answer:
/// a person may always revoke their own.
///
/// Idempotent, and answers the same either way: whether this principal held one is a fact about
/// them, and a `404` that distinguished it would let one member probe for another's by timing
/// nothing — but more simply, "it is gone" is the true answer in both cases.
async fn revoke(
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
    let Some(store) = state.credentials().cloned() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "this host has no credential store bound",
            "no_credential_store",
            json!({ "connector": provider.id }),
        );
    };
    let writes = match delegated_writes(principal.clone(), provider, binding.credential()) {
        Ok(writes) => writes,
        Err(refusal) => return delegation_refused(provider, &refusal),
    };

    // One batch, so a revocation cannot leave a refresh token behind that still renews at the
    // vendor. `delete` is idempotent in the store contract, so this is the same batch whether or
    // not the caller held one.
    let mut batch = SecretBatch::new(writes.scope.clone());
    for reference in [
        writes.access.clone(),
        writes.refresh.clone(),
        writes.expiry.clone(),
        writes.managed.clone(),
    ] {
        if let Err(reason) = batch.delete(reference) {
            error!(connector = provider.id, %reason, "a delegated revocation batch could not be assembled");
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "this host could not assemble the revocation",
                "revocation_unavailable",
                json!({ "connector": provider.id }),
            );
        }
    }
    if let Err(error) = store.apply(&batch).await {
        error!(%error, connector = provider.id, "a delegated credential could not be revoked");
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "this host's credential store refused the revocation",
            "revocation_unavailable",
            json!({ "connector": provider.id }),
        );
    }
    if let Some(channels) = state.channels() {
        channels.restart(principal.tenant(), provider.id);
    }

    StatusCode::NO_CONTENT.into_response()
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
    // **The deployment's own value, and the one that goes to the vendor.** Not `grant.redirect()`:
    // X-147's first review found this route reading the configured value for presence and then
    // sending a composition argument, so every check in `crate::acquisition_redirect` guarded a
    // string that never left the process. `AcquisitionBindings::new` has already refused any grant
    // whose redirect is not byte-equal to this, so the two cannot differ — and this is the one that
    // travels, so if they ever could, the vendor would be told the deployment's.
    //
    // The `else` is **unreachable by construction** and kept anyway: `state.acquisition_redirect()`
    // reads through to the registry, and a registry holding a grant holds a redirect, so finding
    // `grant` above already proves this is `Some`. No test covers this arm, because no composition
    // can reach it. It stays because the alternative to a fail-closed refusal here is an `unwrap`,
    // or a route that answers with a `redirect_uri` it invented — and if a later change reintroduces
    // a second way to wire the redirect, this is what catches it.
    let Some(redirect) = state.acquisition_redirect() else {
        return delegation_refused(provider, &DelegationRefusal::NoRedirectUri);
    };

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
        redirect_uri = urlencoded(redirect.as_str()),
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
/// code. Neither field is ever echoed back to a caller or written to a log by anything in this
/// module: `error` is the vendor's own words about a person's decision, and a code is a bearer
/// credential.
///
/// **That claim covers this module, and it needed the request span to be made true** (X-147 review,
/// M5). `TraceLayer`'s default span records the whole URI, query included, so at `debug` these two
/// fields were written to the log by the layer rather than by any handler —
/// `crate::routes::path_only_span` is what stopped that, and
/// `crate::routes::tests::no_query_string_reaches_a_request_span` is what keeps it stopped.
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

    // **The tenant claim, held across the read and the write it makes stale** (X-147 review, M2).
    // Occupancy is a sum over what a tenant holds, so two of its members finishing an
    // authorization at once each read a total the other has not written yet and both are admitted —
    // which is exactly the X-25 defect `crate::routes::connections` documents on its own claim.
    // Taken *after* the vendor has answered, deliberately: the redemption is the slow part and a
    // claim held across it would serialise one tenant's members on a network round trip.
    let Some(_allowance) = state
        .connections()
        .claim_tenant(pending.principal().tenant())
    else {
        return finished_refusal(
            "another change to this tenant's connections is in flight; the vendor authorized this \
             connection but it was not stored — start the connection again",
        );
    };
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

    use crate::acquisition_redirect::AcquisitionRedirect;
    use crate::credential_acquisition::{
        AcquisitionBindings, DelegatedGrant, HttpCredentialAcquirer, TokenEndpointBehavior,
    };
    use crate::delegated_acquisition::BINDER_COOKIE;
    use crate::dev_identity::DevIdentity;
    use crate::routes::app;

    const ROSTER: &str = "user:alice@acme,user:bob@acme";

    /// This deployment's configured redirect, in the one canonical spelling
    /// `crate::acquisition_redirect` admits.
    const REDIRECT: &str = "http://127.0.0.1:3000/api/acquisitions/callback";

    /// [`REDIRECT`] as the checked value every holder of one carries.
    fn redirect() -> AcquisitionRedirect {
        AcquisitionRedirect::parse(REDIRECT).expect("a canonical redirect")
    }

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

    /// A local token endpoint that records the form it was sent and answers one token.
    ///
    /// A real endpoint rather than a double, because the claim under test is about the bytes that
    /// leave this process through `HttpCredentialAcquirer`'s own client.
    async fn serve_token_endpoint(
        recorded: Arc<Mutex<Vec<String>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use axum::body::Bytes;
        use axum::routing::post;
        use axum::Router;

        async fn record(
            axum::extract::State(recorded): axum::extract::State<Arc<Mutex<Vec<String>>>>,
            body: Bytes,
        ) -> (StatusCode, &'static str) {
            recorded
                .lock()
                .expect("recorder")
                .push(String::from_utf8_lossy(&body).into_owned());
            (
                StatusCode::OK,
                r#"{"access_token":"DELEGATED-ACCESS-NOT-A-REAL-SECRET","refresh_token":"DELEGATED-REFRESH-NOT-A-REAL-SECRET","expires_in":3600}"#,
            )
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a fixture token endpoint");
        let address = listener.local_addr().expect("a fixture address");
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/token", post(record))
                    .with_state(recorded),
            )
            .await
            .expect("serve the fixture token endpoint");
        });
        (format!("http://{address}/token"), task)
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
            "http://127.0.0.1:9/oauth/authorize",
            "exchange-client",
            None,
            ["read_api".to_owned()],
            redirect(),
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
        .with_credential_acquisition(
            AuthPosture::fail_closed(),
            Arc::new(AcquisitionBindings::new([binding], Some(&redirect())).expect("one binding")),
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

    /// GitLab's `login` service base URL, which is the **one** fact this test supplies rather than
    /// reads (X-154).
    ///
    /// The canonical document declares it — `services[].base_url` is `{origin}` for `login` — and
    /// the connector's own `origin` configuration field declares the default that fills it,
    /// `https://gitlab.com`. Neither is in the generated `&'static` tables, where
    /// `catalog::Provider` carries only the *default* service's `{origin}/api/v4`. So
    /// `credential_acquisition::endpoint_base` refuses `login` by name in production, and this
    /// constant is what lets the rest of the composition be proven against the real declaration
    /// meanwhile. When X-153 lands the document, this constant is what goes.
    const GITLAB_LOGIN_BASE: &str = "https://gitlab.com";

    /// **The end-to-end agreement X-154 exists to prove** (X-147's second criterion).
    ///
    /// One authorize URL composed from GitLab's *released declaration* — its `authorize_path` and
    /// its three declared scopes, read out of `connector_catalog` — and one composed from the
    /// hand-written [`DelegatedGrant`] X-147 was delivered against. The route produces both, and
    /// they must be the same string up to the two values that are random by construction (`state`
    /// and the PKCE challenge).
    ///
    /// That is what makes the injected seam a faithful stand-in rather than an assertion: if the
    /// declaration said something else about the path or the scopes, these two would differ here
    /// rather than at a vendor.
    #[tokio::test]
    async fn the_url_composed_from_gitlabs_declaration_is_the_url_this_route_produces() {
        use crate::credential_acquisition::{
            binding_from_declaration, declared_oauth2, OAuthRegistration,
        };

        let (credential, spec) = declared_oauth2(gitlab()).expect("gitlab declares one OAuth2");
        // The released declaration, asserted rather than assumed — this test is only evidence if
        // these are the catalogue's own values.
        assert_eq!(spec.authorize_path, "/oauth/authorize");
        assert_eq!(spec.scopes, &["read_api", "read_user", "read_repository"]);
        assert_eq!(credential.name, "gitlab.oauth_token");

        let registration = OAuthRegistration::new("deployment-client", None);
        let from_declaration = binding_from_declaration(
            gitlab().id,
            credential.name,
            credential.hazard,
            spec,
            GITLAB_LOGIN_BASE,
            &registration,
            &redirect(),
        )
        .expect("gitlab's declaration composes a delegated binding");

        // X-147's shape: every value stated by hand, which is what the injected seam was.
        let injected = AcquisitionBinding::new(
            gitlab().id,
            credential.name,
            None,
            Arc::new(RecordingPerformer {
                codes: Arc::new(Mutex::new(Vec::new())),
            }),
        )
        .with_delegated_grant(
            DelegatedGrant::new(
                "https://gitlab.com/oauth/authorize",
                "deployment-client",
                None,
                [
                    "read_api".to_owned(),
                    "read_user".to_owned(),
                    "read_repository".to_owned(),
                ],
                redirect(),
            )
            .expect("the grant X-147 was delivered against"),
        );

        let mut urls = Vec::new();
        for binding in [from_declaration, injected] {
            let state = AppState::with_development_identity(Arc::new(
                DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
            ))
            .with_credential_acquisition(
                AuthPosture::fail_closed(),
                Arc::new(
                    AcquisitionBindings::new([binding], Some(&redirect())).expect("one binding"),
                ),
            );
            let (status, body, _) = authorize_as(&state, "alice").await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            let answered: Value = serde_json::from_str(&body).expect("a JSON answer");
            urls.push(
                answered["authorize_url"]
                    .as_str()
                    .expect("an authorize URL")
                    .to_owned(),
            );
        }

        // `state` and `code_challenge` are drawn from the OS per authorization, so the comparison
        // is over everything the declaration and the deployment decide — and it deliberately stops
        // at `&state=` rather than dropping the two parameters, so a change to the order or to what
        // precedes them is still a difference.
        let fixed = |url: &str| {
            url.split_once("&state=")
                .expect("the URL carries a state")
                .0
                .to_owned()
        };
        assert_eq!(
            fixed(&urls[0]),
            fixed(&urls[1]),
            "the URL composed from gitlab's declaration and the one X-147's injected grant \
             produces must be one URL",
        );
        assert_eq!(
            fixed(&urls[0]),
            format!(
                "https://gitlab.com/oauth/authorize?response_type=code&client_id=deployment-client\
                 &redirect_uri={}&scope=read_api%20read_user%20read_repository",
                urlencoded(REDIRECT),
            ),
            "and it is composed of the declaration's path and scopes and the deployment's identity",
        );
        for url in &urls {
            assert!(url.ends_with("&code_challenge_method=S256"), "{url}");
        }
    }

    /// A composition with no delegated grant opens nothing and says so, which is what production
    /// does for every connector no deployment registered.
    #[tokio::test]
    async fn a_connector_with_no_delegated_grant_opens_nothing() {
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_credential_acquisition(
            AuthPosture::fail_closed(),
            Arc::new(AcquisitionBindings::default()),
        );

        let (status, body, _) = authorize_as(&state, "alice").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.contains("grant_not_bound"), "{body}");
        assert_eq!(state.pending_delegations().open(), 0);
    }

    /// **A grant with no deployment redirect never becomes a registry**, so no route can be asked to
    /// derive one from a request's `Host` header — a value a caller controls.
    ///
    /// This used to drive the route and assert a `503`. It cannot any more, and that is the point:
    /// once the redirect moved onto `AcquisitionBindings` (re-review, nit 3) there is no way to
    /// compose a state whose grant-bearing registry lacks one, so the refusal moved from *the route
    /// answering* to *the composition not existing*. The route's guard is still there and still
    /// fail-closed — see `authorize` — but it is unreachable by construction, and this test does not
    /// pretend to cover it.
    #[test]
    fn a_grant_with_no_deployment_redirect_never_becomes_a_registry() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let grant = DelegatedGrant::new(
            "http://127.0.0.1:9/oauth/authorize",
            "exchange-client",
            None,
            ["read_api".to_owned()],
            redirect(),
        )
        .expect("a delegated grant");
        let binding = AcquisitionBinding::new(
            gitlab().id,
            bound_credential(),
            None,
            Arc::new(RecordingPerformer {
                codes: Arc::clone(&codes),
            }),
        )
        .with_delegated_grant(grant);

        assert_eq!(
            AcquisitionBindings::new([binding.clone()], None)
                .expect_err("a delegated grant with no deployment redirect must be refused"),
            "a delegated acquisition grant is bound and this deployment configured no acquisition \
             redirect URI",
        );

        // And with one, the registry carries it — so `AppState::acquisition_redirect` has exactly
        // one source and there is no second wiring to disagree with.
        let bindings = AcquisitionBindings::new([binding], Some(&redirect())).expect("one binding");
        assert_eq!(bindings.redirect(), Some(&redirect()));
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_credential_acquisition(AuthPosture::fail_closed(), Arc::new(bindings));
        assert_eq!(state.acquisition_redirect(), Some(&redirect()));
    }

    /// **B1, end to end: the string the vendor is redirected with is the string the token request
    /// re-presents, and both are the deployment's.**
    ///
    /// The composition checks (`AcquisitionBindings::new`, `AcquisitionBinding::delegating`) make
    /// divergence unconstructible; this drives the whole path with a **real**
    /// `HttpCredentialAcquirer` against a recording token endpoint and compares the two strings that
    /// actually left the process. Before the rework the authorize URL carried a composition argument
    /// and the configured value reached no vendor at all, and no test could see it because one
    /// fixture spelled both the same — so this compares them against
    /// `AppState::acquisition_redirect`, the deployment's own value, rather than against a constant.
    #[tokio::test]
    async fn the_redirect_the_vendor_is_sent_is_the_one_the_token_request_re_presents() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (token_endpoint, task) = serve_token_endpoint(Arc::clone(&recorded)).await;
        let scratch = Scratch::new();
        let store = scratch.store();

        let grant = DelegatedGrant::new(
            "http://127.0.0.1:9/oauth/authorize",
            "exchange-client",
            None,
            ["read_api".to_owned()],
            redirect(),
        )
        .expect("a delegated grant");
        // `delegating` is what binds the browser-facing half and the back-channel half from one
        // `Arc`. There is no second argument here to spell differently.
        let binding = AcquisitionBinding::delegating(
            gitlab().id,
            bound_credential(),
            None,
            grant,
            HttpCredentialAcquirer::new(
                gitlab().id,
                &token_endpoint,
                TokenEndpointBehavior::Standard,
            )
            .expect("a real HTTP performer"),
        );
        let state = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        ))
        .with_credentials(store.secrets())
        .with_credential_acquisition(
            AuthPosture::fail_closed(),
            Arc::new(AcquisitionBindings::new([binding], Some(&redirect())).expect("one binding")),
        );

        let (status, body, cookies) = authorize_as(&state, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let answered: Value = serde_json::from_str(&body).expect("a JSON answer");
        let authorize_url = answered["authorize_url"]
            .as_str()
            .expect("an authorize URL");
        let sent_to_browser = authorize_url
            .split("&redirect_uri=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("the authorize URL carries a redirect_uri");
        let state_parameter = authorize_url
            .split("&state=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("the authorize URL carries a state");
        let binder = cookies
            .iter()
            .find(|cookie| cookie.starts_with(&format!("{BINDER_COOKIE}=")))
            .and_then(|cookie| cookie.split_once('='))
            .and_then(|(_, rest)| rest.split(';').next())
            .expect("a binder");

        let (status, page) = callback_request(
            &state,
            &format!("state={state_parameter}&code=THE-CODE"),
            Some(binder),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");

        let forms = recorded.lock().expect("recorder");
        let form = forms.first().expect("one token request reached the vendor");
        let re_presented = form
            .split("redirect_uri=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("the token request carries a redirect_uri");

        // The deployment's own configured value, and nothing composed beside it.
        let configured = state
            .acquisition_redirect()
            .expect("this deployment configured one");
        assert_eq!(
            sent_to_browser,
            urlencoded(configured.as_str()),
            "the authorize URL must carry the redirect this deployment configured",
        );
        assert_eq!(
            re_presented,
            urlencoded(configured.as_str()),
            "and the token request must re-present the same bytes (RFC 6749 §4.1.3)",
        );
        assert_eq!(
            sent_to_browser, re_presented,
            "the two strings that left this process must be one string",
        );
        task.abort();
    }

    /// **M1: a delegated credential can be destroyed by the person who authorized it**, and the
    /// revocation takes its refresh token with it.
    ///
    /// Before this route there was no surface that could delete one at all:
    /// `DELETE /api/connections/{connector}` walks declared addresses and `exchange-acquisition`
    /// companions, so it answered `204` while the delegated credential and a live refresh token
    /// survived. The second half of this test is the part that matters — a revocation that left the
    /// refresh token behind would leave something that still renews at the vendor.
    #[tokio::test]
    async fn a_person_can_revoke_the_delegated_credential_they_authorized() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let state = composed(&codes, &store);

        // Authorize, so there is something to revoke.
        let (status, body, cookies) = authorize_as(&state, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let answered: Value = serde_json::from_str(&body).expect("a JSON answer");
        let authorize_url = answered["authorize_url"]
            .as_str()
            .expect("an authorize URL");
        let state_parameter = authorize_url
            .split("&state=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("a state");
        let binder = cookies
            .iter()
            .find(|cookie| cookie.starts_with(&format!("{BINDER_COOKIE}=")))
            .and_then(|cookie| cookie.split_once('='))
            .and_then(|(_, rest)| rest.split(';').next())
            .expect("a binder");
        let (status, _) = callback_request(
            &state,
            &format!("state={state_parameter}&code=THE-CODE"),
            Some(binder),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let hers = crate::delegated_acquisition::delegated_address(
            &member("alice"),
            gitlab().authority.expect("gitlab declares an authority"),
            gitlab().auth[0].leaf,
        )
        .expect("an address");
        let companions =
            crate::delegated_acquisition::delegated_companions(&hers).expect("companions");
        let secrets = store.secrets();
        assert!(secrets.get(&hers).await.is_ok(), "it is held before");
        assert!(secrets.get(&companions.refresh).await.is_ok());

        let response = app(state.clone())
            .oneshot(
                HttpRequest::builder()
                    .method(Method::DELETE)
                    .uri("/api/acquisitions/gitlab")
                    .header(header::AUTHORIZATION, "Bearer alice")
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("a response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        assert!(
            secrets.get(&hers).await.is_err_and(|e| e.is_not_found()),
            "the access token is gone",
        );
        assert!(
            secrets
                .get(&companions.refresh)
                .await
                .is_err_and(|e| e.is_not_found()),
            "and so is the refresh token — a revocation that left one behind would leave something \
             that still renews at the vendor",
        );
        assert!(secrets
            .get(&companions.expiry)
            .await
            .is_err_and(|e| e.is_not_found()));
        assert!(secrets
            .get(&companions.managed)
            .await
            .is_err_and(|e| e.is_not_found()));
    }

    /// Revoking is idempotent, and one member's revocation does not reach another's.
    #[tokio::test]
    async fn a_revocation_is_idempotent_and_reaches_only_the_callers_own() {
        let codes = Arc::new(Mutex::new(Vec::new()));
        let scratch = Scratch::new();
        let store = scratch.store();
        let state = composed(&codes, &store);
        let secrets = store.secrets();

        // Bob holds one; Alice has never authorized.
        let bobs = crate::delegated_acquisition::delegated_address(
            &member("bob"),
            gitlab().authority.expect("an authority"),
            gitlab().auth[0].leaf,
        )
        .expect("an address");
        secrets
            .put(&bobs, &Secret::new("BOBS-DELEGATED-TOKEN"))
            .await
            .expect("a placed fixture credential");

        for _ in 0..2 {
            let response = app(state.clone())
                .oneshot(
                    HttpRequest::builder()
                        .method(Method::DELETE)
                        .uri("/api/acquisitions/gitlab")
                        .header(header::AUTHORIZATION, "Bearer alice")
                        .body(Body::empty())
                        .expect("a request"),
                )
                .await
                .expect("a response");
            assert_eq!(
                response.status(),
                StatusCode::NO_CONTENT,
                "revoking what you do not hold is the same answer as revoking what you do",
            );
        }

        assert!(
            secrets.get(&bobs).await.is_ok(),
            "Alice's revocation must not reach Bob's delegated credential",
        );
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
