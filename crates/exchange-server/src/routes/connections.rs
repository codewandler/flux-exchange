//! A tenant's connections to connectors — created, listed and destroyed at an address it cannot
//! name.
//!
//! ```text
//! GET    /api/connections               every connection this tenant holds
//! POST   /api/connections/{connector}   connect one, with the values it declares
//! GET    /api/connections/{connector}   one connection, as addresses and never as values
//! DELETE /api/connections/{connector}   disconnect, destroying every credential it holds
//! ```
//!
//! # Where the tenant comes from, and what a caller may say
//!
//! [`Extension<Principal>`] and nowhere else, exactly as in [`identity`](super::identity). What a
//! caller supplies is a **connector id** — `zendesk`, a key into the compiled-in catalogue — and, on
//! `POST`, the credential values themselves. It never supplies a tenant, a path or an address:
//! those are derived by [`ConnectorDeclaration`], from the resolved principal and from what the
//! connector declares. `super::tests::no_published_route_takes_a_tenant_in_its_path` walks the whole
//! surface for the first of those, and X-03 wrote it saying this story would inherit it.
//!
//! Both routes are [`Access::Principal`]. A connection is tenant data and there is no version of it
//! that answers a caller this host has not identified, so this module adds nothing to the anonymous
//! set that `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous` enumerates.
//!
//! # A value goes in and never comes back
//!
//! `POST` is the only direction a credential value travels. Nothing here reads one out to a caller:
//! `GET` answers with **addresses**, every refusal names the address it looked at, and
//! [`tests::no_answer_or_refusal_carries_a_credential_value`] drives every answer and refusal it
//! names — which is all of them but one, listed on the test itself rather than claimed here — with a
//! sentinel stored, and asserts it appears in no response body. `AGENTS.md` § Invariants: name the
//! address, never the value.
//!
//! # The second connection to one connector is refused
//!
//! `tenants/<tenant>/<authority>/<credential>` has nowhere to say *which* Zendesk, so a tenant with
//! a sandbox and a production account renders one address for both and the second write would
//! silently replace the first. That is refused with `409` rather than accepted, and the refusal
//! quotes the `@instances/<uuid>` level that has landed upstream (flux-connectors C-406) and is not
//! published yet. **This refusal is the placeholder for that level** — see [`already_connected`],
//! `exchange_host::ConnectorDeclaration::address_of_declared` for the seam it is inserted at, and
//! `docs/designs/connections.md` for the argument.
//!
//! # What one tenant may occupy, refused before anything is written
//!
//! Two bounds, both decided on `POST` and both **before the first `put`**, because the store is one
//! file that every write rewrites and `fsync`s under one mutex — so a refusal that had already
//! written would have charged every other tenant for the thing it was refusing.
//!
//! - `exchange_host::MAX_CREDENTIAL_VALUE_BYTES`, per value, applied by
//!   `ConnectorDeclaration::writes` — which is the only way a supplied value becomes a write, so
//!   this is not a check [`create`] remembers to make. `413`.
//! - `exchange_host::MAX_TENANT_STORE_BYTES`, per tenant across the **whole** store, applied by
//!   `exchange_host::admit_tenant_occupancy` against [`occupied`], inside the same claim as
//!   everything else this route decides. `409`, in this module's existing sense of it: the tenant's
//!   own state conflicts with the request, and a `DELETE` is the remedy — telling an operator to
//!   send less when what they have to do is disconnect something would be the wrong instruction.
//!
//! Both numbers are stated once, in `exchange_host::connections`, with the argument for each
//! written beside it — including why the bound is there and not on the `SecretStore` port. Every
//! refusal names the credential and the bound and never the value; the sizes it quotes are the
//! caller's own.
//!
//! How *many* credentials a connection may carry needs no bound of its own: a name the connector
//! does not declare is already refused, so the count is the catalogue's number rather than the
//! caller's.
//!
//! # A half connection is one an operator cannot tell from a whole one
//!
//! Which is why `POST` resolves every address before writing any value, and why a write that fails
//! part way is rolled back and reported through [`partly_written`] rather than left where it fell.
//!
//! **`DELETE` obeys the same rule and cannot use the same mechanism.** A destroyed credential
//! cannot be put back — this host never held the plaintext to restore, which is the point of it —
//! so there is nothing here for a rollback to do. The half-state is therefore unavoidable, and what
//! is owed is an honest account of it: [`remove`] destroys as much as the store will allow and
//! [`partly_destroyed`] names both halves, `destroyed` and `left_behind`, in `partly_written`'s
//! vocabulary. This matters more in this direction than in the other, because the case a `DELETE`
//! exists for is revoking a leaked secret.
//!
//! `GET` still answers `200` for such a connection, with each credential's `held` telling the truth
//! about it. **That is deliberate and X-18 decided not to change it here**: a connector may legally
//! hold a subset of what it declares — `tests::a_connection_may_carry_a_subset_of_what_is_declared`
//! — so "half destroyed" and "deliberately partial" render identically, and nothing distinguishes
//! them without a record beside the store, which this module deliberately does not keep (see
//! [`list`]). Giving `GET` a status of its own therefore needs that record designed first, and is
//! its own story rather than a line here.
//!
//! Each refusal is a check-then-write, so it only means anything while nothing interleaves with it,
//! and the two are decided from reads of different width — so
//! [`ConnectionGuard`](crate::connection_guard::ConnectionGuard) is held at two widths:
//!
//! - Every mutating route claims `(tenant, connector)` across the whole probe-decide-write. Without
//!   it two concurrent `POST`s both answer `201` and one value is silently lost — the exact failure
//!   the `409` exists to prevent.
//! - `POST` additionally claims the **tenant** across the allowance decision and the writes that
//!   make it stale, because occupancy is a sum over every connector. Without it one tenant's
//!   concurrent `POST`s to *different* connectors each read an occupancy the others had not written
//!   yet and all were admitted, leaving the tenant past `MAX_TENANT_STORE_BYTES` (X-25). So one
//!   tenant's creates serialise with each other, two tenants' never do, and `DELETE` — which only
//!   frees allowance — stays out of the wider claim.
//!
//! Both claims are **single-process**; that limit is stated in `connection_guard`'s own
//! documentation and in the design.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{Provider, ProviderKey};
use exchange_host::{
    address_path, admit_tenant_occupancy, stored_bytes, ConnectionRefusal, ConnectorDeclaration,
    CredentialRef, DeclaredCredential, Principal, Secret, SecretStore, StoreError, Tenant,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use super::{Access, Module, Route};
use crate::state::AppState;

/// The setting that names the credential store, quoted when none is bound.
///
/// Spelled through the host's own constant so this refusal and the reader that would have produced
/// the value cannot drift into two different names.
#[cfg(unix)]
const STORE_SETTING: &str = exchange_host::CREDENTIAL_STORE_SETTING;
/// The same, where the file store does not exist. Only `FileStore` is `#[cfg(unix)]`; the port is
/// not, so a composition on another platform binds its own store rather than this one.
#[cfg(not(unix))]
const STORE_SETTING: &str = "FLUX_EXCHANGE_CREDENTIALS";

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "connections",
    routes: &[
        Route {
            // Under `/api` for the reason the session route is: `vite dev` owns the origin and
            // proxies `/api` to this host, so anything outside that prefix is answered by the SPA
            // fallback instead.
            path: "/api/connections",
            access: Access::Principal,
            method_router: collection_route,
        },
        Route {
            // `{connector}` is a catalogue key, never an address. It selects *what* is being
            // connected; the tenant — the only part of the address a caller could want to move —
            // comes from the guard.
            path: "/api/connections/{connector}",
            access: Access::Principal,
            method_router: connection_route,
        },
    ],
};

fn collection_route() -> MethodRouter<AppState> {
    get(list)
}

fn connection_route() -> MethodRouter<AppState> {
    get(show).post(create).delete(remove)
}

/// The values a caller supplies when it connects a connector.
///
/// Keyed by the flat-namespace name the catalogue publishes (`zendesk.api_token`), because that is
/// the name an operation references and the only one an operator can look up. Unknown fields are
/// **not** denied: a body carrying `tenant` is not refused, it is ignored, and
/// [`tests::a_tenant_in_a_body_field_does_not_influence_where_the_credential_lands`] asserts the
/// stronger property that the value still lands under the resolved principal's tenant.
///
/// **No `Debug`, deliberately.** This is the one type on this surface holding credential values as
/// plain `String` rather than as [`Secret`] — they arrive as JSON and there is nowhere earlier to
/// wrap them — so a derived `Debug` would be a formatter that prints every value, one `debug!(?body)`
/// away from putting a tenant's credentials in the log. Not deriving it makes that line fail to
/// compile instead.
#[derive(Deserialize)]
struct NewConnection {
    /// Declared credential name to value. At least one, and every name declared by the connector.
    credentials: BTreeMap<String, String>,
}

/// Every connection this tenant holds.
///
/// Derived from the store rather than from a record beside it: a connection exists exactly when the
/// store holds a value at one of the addresses derived for that tenant and connector. There is no
/// second source of truth to disagree with the credentials, which is also what makes `DELETE`
/// destroying them not a step somebody could forget.
async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Some(store) = state.credentials() else {
        return no_store();
    };

    let mut connections = Vec::new();

    for provider in connector_catalog::providers() {
        let declared = declared_credentials(provider);
        let declaration = declaration(provider, &declared);

        // A connector with no authority or no declared credential has no address, so this tenant
        // cannot hold a connection to it and there is nothing to report. The refusal for *asking*
        // about one is `show`'s and `create`'s; a listing that refused because some unrelated
        // connector is unaddressable would be useless.
        let Ok(addresses) = declaration.addresses(principal.tenant()) else {
            continue;
        };

        match held(store, &addresses).await {
            Ok(held) if held.is_empty() => {}
            Ok(held) => connections.push(view(provider, &addresses, &held)),
            Err(error) => return store_failed(&error),
        }
    }

    Json(json!({ "connections": connections })).into_response()
}

/// One connection, as addresses and never as values.
async fn show(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(store) = state.credentials() else {
        return no_store();
    };

    let declared = declared_credentials(provider);
    let declaration = declaration(provider, &declared);
    let addresses = match declaration.addresses(principal.tenant()) {
        Ok(addresses) => addresses,
        Err(refusal) => return connection_refused(&refusal),
    };

    match held(store, &addresses).await {
        Err(error) => store_failed(&error),
        // Nothing here, and the refusal names **this tenant's** address — the one this host looked
        // at. Never another tenant's, and never the fact that another tenant holds one.
        Ok(held) if held.is_empty() => not_connected(provider, &addresses),
        Ok(held) => Json(view(provider, &addresses, &held)).into_response(),
    }
}

/// Connect a connector, storing each supplied value at its derived address.
async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
    Json(body): Json<NewConnection>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(store) = state.credentials() else {
        return no_store();
    };

    let declared = declared_credentials(provider);
    let declaration = declaration(provider, &declared);
    let addresses = match declaration.addresses(principal.tenant()) {
        Ok(addresses) => addresses,
        Err(refusal) => return connection_refused(&refusal),
    };

    if body.credentials.is_empty() {
        return refuse(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "a connection to `{}` carries at least one credential value; it declares {}",
                provider.id,
                names(&declaration),
            ),
            json!({ "connector": provider.id, "declared": declared_names(&declaration) }),
        );
    }

    // Every name is resolved to an address, and every value admitted against the per-value bound,
    // **before** anything is written — so a body with one good name and one typo, or one good value
    // and one that is not a credential, stores neither. A half-written connection is one the
    // operator cannot tell from a working one until an operation fails.
    //
    // This is also the only way values become writes: `ConnectorDeclaration::writes` is where the
    // bound lives, so there is no form of this loop that could write past it. See that function,
    // and `exchange_host::connections`' module documentation, for why the bound is there rather
    // than on the `SecretStore` port.
    let writes: Vec<(CredentialRef, Secret)> =
        match declaration.writes(principal.tenant(), &body.credentials) {
            Ok(writes) => writes,
            Err(refusal) => return connection_refused(&refusal),
        };

    // Everything from here to the end of the function is one read-decide-write, and it must not
    // interleave with another change to this same connection. Without the claim, two concurrent
    // `POST`s both probe an empty address, both write and both answer `201` — one value silently
    // replaced, and the caller that lost told it succeeded. That is the exact failure the `409`
    // below exists to prevent, so leaving the window open would have made the refusal decorative.
    let Some(_claim) = state.connections().claim(principal.tenant(), provider.id) else {
        return change_in_flight(provider);
    };

    match held(store, &addresses).await {
        Err(error) => return store_failed(&error),
        // The X-14 refusal, decided inside the claim so that what it read is still true when it
        // answers.
        Ok(held) if !held.is_empty() => return already_connected(provider, &addresses),
        Ok(_) => {}
    }

    // The second bound, and the one that is about the neighbours rather than about this request.
    //
    // It needs a **second, wider claim**: what this tenant occupies is a sum over every connector,
    // so the claim above — which one tenant's `zendesk` and `slack` do not share — leaves the read
    // below true only for as long as no other connector of this tenant is being written. Before
    // X-25 that was exactly the gap: one tenant's concurrent creates each read an occupancy the
    // others had not written yet, all were admitted, and the tenant ended up past its allowance.
    //
    // Held from here to the end of the function, because it is the writes below that make the read
    // stale. It is a claim on the tenant and not on the surface, so another tenant's create does
    // not wait on this one; and it is never waited on, only taken or refused, so holding it and
    // the claim above at once cannot deadlock.
    let Some(_allowance) = state.connections().claim_tenant(principal.tenant()) else {
        return allowance_change_in_flight(provider);
    };

    // What this connector already holds is not counted twice — the probe above has just
    // established that it holds nothing, or this would have refused.
    let adding: usize = writes.iter().map(|(_, secret)| stored_bytes(secret)).sum();
    let held_bytes = match occupied(store, principal.tenant()).await {
        Ok(bytes) => bytes,
        Err(error) => return store_failed(&error),
    };
    if let Err(refusal) = admit_tenant_occupancy(held_bytes, adding) {
        return connection_refused(&refusal);
    }

    for (index, (reference, secret)) in writes.iter().enumerate() {
        let Err(error) = store.put(reference, secret).await else {
            continue;
        };

        // A connector declaring several credentials can fail half way, and a half-written
        // connection is the worst of both answers: the caller sees a failure, while the `409` above
        // now refuses every retry until somebody works out that a `DELETE` is needed first. So the
        // values already written are taken back out, leaving the address exactly as this request
        // found it.
        let rolled_back = rollback(store, &writes[..index]).await;
        return partly_written(provider, &error, rolled_back);
    }

    let stored: Vec<String> = body.credentials.keys().cloned().collect();
    (
        StatusCode::CREATED,
        Json(view(provider, &addresses, &stored)),
    )
        .into_response()
}

/// Take back the values this request had already written, and report whether that succeeded.
///
/// Best effort by necessity: the store has just failed, so the deletes may fail too. What matters
/// is that the caller is told which of the two happened — a refusal claiming nothing was written
/// when something was is the kind of answer that costs somebody an afternoon.
async fn rollback(
    store: &Arc<dyn SecretStore>,
    written: &[(CredentialRef, Secret)],
) -> Result<(), Vec<String>> {
    let mut remaining = Vec::new();

    for (reference, _) in written {
        if store.delete(reference).await.is_err() {
            remaining.push(address_path(reference));
        }
    }

    if remaining.is_empty() {
        Ok(())
    } else {
        Err(remaining)
    }
}

/// Disconnect, destroying every credential the connection holds.
async fn remove(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(store) = state.credentials() else {
        return no_store();
    };

    let declared = declared_credentials(provider);
    let declaration = declaration(provider, &declared);
    let addresses = match declaration.addresses(principal.tenant()) {
        Ok(addresses) => addresses,
        Err(refusal) => return connection_refused(&refusal),
    };

    // The same claim `create` takes, for the same reason and against the same neighbour: a delete
    // that decided against a value another request is in the middle of writing would destroy half
    // of it.
    let Some(_claim) = state.connections().claim(principal.tenant(), provider.id) else {
        return change_in_flight(provider);
    };

    let held_before = match held(store, &addresses).await {
        Err(error) => return store_failed(&error),
        // A `404` and not a `204`: deleting something that is not there is indistinguishable from
        // deleting another tenant's, and the caller should be able to tell.
        Ok(held) if held.is_empty() => return not_connected(provider, &addresses),
        Ok(held) => held,
    };

    // Every declared address, not only the ones the probe found. `SecretStore::delete` is
    // idempotent by contract, and deleting the whole set is what makes "the connection is gone"
    // true even if a value appeared between the probe and here.
    //
    // **The delete direction of the rule `create` states above.** A half-*destroyed* connection is
    // one an operator cannot tell from a revoked one, and this is the direction where that costs
    // most: the case a `DELETE` exists for is revoking a leaked secret, so a live vendor credential
    // surviving under a generic "retrying may work" is precisely the wrong thing to read. `create`
    // makes the half-state impossible by rolling its writes back; **that is not available here**,
    // because a destroyed credential cannot be put back — this host never held the plaintext to
    // restore, which is the whole point of it. So the answer is honesty rather than repair: the
    // loop does not stop at the first failure, as much is destroyed as the store will allow, and
    // the refusal names both halves.
    let mut destroyed = Vec::new();
    let mut left_behind = Vec::new();
    let mut failure = None;

    for (declared, reference) in &addresses {
        match store.delete(reference).await {
            // Only what the probe saw a value at is reported destroyed. Deleting an address that
            // held nothing is a no-op, and calling it "destroyed" would overstate what happened to
            // an operator counting which of their secrets are now revoked.
            Ok(()) if held_before.iter().any(|name| name == declared.name) => {
                destroyed.push(address_path(reference));
            }
            Ok(()) => {}
            // Named whether or not the probe found a value here: a failed delete is exactly the
            // case where this host cannot say the address is empty, and the reason the whole
            // declared set is deleted is that a value may have appeared since the probe.
            Err(error) => {
                left_behind.push(address_path(reference));

                // The worst kind the loop saw, not the first. Keeping the first meant one
                // `Unreachable` ahead of a `Denied` answered "retrying may work" while the denied
                // address sat in `left_behind` below — see [`Escalation`] for the order and why
                // this is the one place on the surface where that could still happen.
                let worse = failure
                    .as_ref()
                    .is_none_or(|worst| escalation(&error) > escalation(worst));
                if worse {
                    failure = Some(error);
                }
            }
        }
    }

    if let Some(error) = failure {
        return partly_destroyed(provider, &error, destroyed, left_behind);
    }

    StatusCode::NO_CONTENT.into_response()
}

/// The connector the catalogue declares under `id`.
fn catalogued(id: &str) -> Option<&'static Provider> {
    connector_catalog::provider(ProviderKey::id(id))
}

/// The connector's declared credentials, as the view an address is derived from.
fn declared_credentials(provider: &'static Provider) -> Vec<DeclaredCredential<'static>> {
    provider
        .auth
        .iter()
        .map(|credential| DeclaredCredential {
            name: credential.name,
            leaf: credential.leaf,
        })
        .collect()
}

/// The declaration an address is derived from — the catalogue's facts and nothing of the request's.
fn declaration<'a>(
    provider: &'static Provider,
    declared: &'a [DeclaredCredential<'static>],
) -> ConnectorDeclaration<'a> {
    ConnectorDeclaration {
        connector: provider.id,
        authority: provider.authority,
        credentials: declared,
    }
}

/// Which of the declared credentials this tenant has a value for.
///
/// `Err` is a store that could not answer, and the caller must never turn that into "not
/// connected": `StoreError`'s own documentation says so, and an outage reported as "you have not
/// connected that integration" is an operator reconnecting an integration that was fine.
async fn held(
    store: &Arc<dyn SecretStore>,
    addresses: &[(DeclaredCredential<'_>, CredentialRef)],
) -> Result<Vec<String>, StoreError> {
    let mut held = Vec::new();

    for (declared, reference) in addresses {
        match store.get(reference).await {
            // The value is read and dropped without being exposed: the port has no `exists`, and a
            // `get` is the only question it answers.
            Ok(_) => held.push(declared.name.to_string()),
            Err(error) if error.is_not_found() => {}
            Err(error) => return Err(error),
        }
    }

    Ok(held)
}

/// How many bytes this tenant already occupies in the store, across **every** connector.
///
/// Every addressable connector in the catalogue and not only the one being connected, because the
/// bound is on the tenant's share of the *store*. A per-connector sum would let one tenant reach
/// the allowance once per connector, which is fifty-odd times the bound and therefore not a bound.
///
/// The cost is one `SecretStore::get` per address — the same walk `GET /api/connections` makes on
/// every call, paid here on the far rarer `POST`, and against `FileStore` those are lookups in a
/// map read once at open rather than file reads.
///
/// Only the *length* of each value is taken, through [`stored_bytes`]: no plaintext is ever bound
/// to a name in this function, so there is nothing here a later `debug!` could turn into a
/// disclosure.
///
/// `Err` is a store that could not answer, and the caller must not turn that into "this tenant
/// occupies nothing" — an outage read as an empty allowance is how a bound silently stops holding.
///
/// Reading this and then writing is a read-decide-write over **every** connector, so the caller
/// holds `ConnectionGuard::claim_tenant` across both halves: a claim on one connector would leave
/// this true only until another of the same tenant's creates wrote, which is the overshoot X-25
/// closed. That claim is single-process, exactly as the per-connection one is.
async fn occupied(store: &Arc<dyn SecretStore>, tenant: &Tenant) -> Result<usize, StoreError> {
    let mut total = 0usize;

    for provider in connector_catalog::providers() {
        let declared = declared_credentials(provider);
        let declaration = declaration(provider, &declared);

        // A connector with no address cannot hold anything for this tenant, so it contributes
        // nothing. Refusing the whole create because some unrelated connector is unaddressable
        // would be the listing bug in another place.
        let Ok(addresses) = declaration.addresses(tenant) else {
            continue;
        };

        for (_, reference) in &addresses {
            match store.get(reference).await {
                Ok(secret) => total = total.saturating_add(stored_bytes(&secret)),
                Err(error) if error.is_not_found() => {}
                Err(error) => return Err(error),
            }
        }
    }

    Ok(total)
}

/// One connection as a caller sees it: what it is, where each credential lives, and which are set.
///
/// Addresses, never values. There is deliberately no field a value could occupy.
fn view(
    provider: &'static Provider,
    addresses: &[(DeclaredCredential<'_>, CredentialRef)],
    held: &[String],
) -> Value {
    let credentials: Vec<Value> = addresses
        .iter()
        .map(|(declared, reference)| {
            json!({
                "name": declared.name,
                "address": address_path(reference),
                "held": held.iter().any(|name| name == declared.name),
            })
        })
        .collect();

    json!({
        "connector": provider.id,
        "vendor": provider.vendor,
        "authority": provider.authority,
        "credentials": credentials,
    })
}

/// The names a connector declares, for a refusal that says what would have worked.
fn declared_names(declaration: &ConnectorDeclaration<'_>) -> Vec<String> {
    declaration
        .credentials
        .iter()
        .map(|credential| credential.name.to_string())
        .collect()
}

/// The same, rendered into a sentence.
fn names(declaration: &ConnectorDeclaration<'_>) -> String {
    let declared = declared_names(declaration);
    if declared.is_empty() {
        return "none".to_string();
    }

    declared
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every address this connection occupies, quoted in a refusal.
fn addresses_of(addresses: &[(DeclaredCredential<'_>, CredentialRef)]) -> Vec<String> {
    addresses
        .iter()
        .map(|(_, reference)| address_path(reference))
        .collect()
}

/// A refusal as the caller sees it: a status, a reason, and the address — never a value.
fn refuse(status: StatusCode, reason: impl Into<String>, mut extra: Value) -> Response {
    if let Some(object) = extra.as_object_mut() {
        object.insert("error".to_string(), json!(reason.into()));
    }

    (status, Json(extra)).into_response()
}

/// No connector is catalogued under that id.
fn unknown_connector(connector: &str) -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        format!("no connector `{connector}` is in this host's catalogue"),
        json!({ "connector": connector }),
    )
}

/// This tenant holds no connection to that connector.
///
/// Names the address **this host looked at**, which is this tenant's own. It cannot name another
/// tenant's, and it must not disclose that another tenant holds one — that would turn a `404` into
/// an oracle for which tenants use which vendors.
fn not_connected(
    provider: &'static Provider,
    addresses: &[(DeclaredCredential<'_>, CredentialRef)],
) -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        format!(
            "this tenant holds no connection to connector `{}`; nothing is stored at the address \
             it would live at",
            provider.id,
        ),
        json!({ "connector": provider.id, "addresses": addresses_of(addresses) }),
    )
}

/// How a [`ConnectionRefusal`] reaches a caller — **the one place**, so a variant added upstream
/// cannot be answered two different ways by two call sites.
///
/// The status is per variant, because these are not one event. The four addressing refusals are
/// `422`: the request is well formed and there is no address for it, and nothing the caller does
/// to its own state changes that. The two bounds are not:
///
/// - [`CredentialTooLarge`](ConnectionRefusal::CredentialTooLarge) is `413`, which is what it
///   literally is — the caller sent something that is not a credential, and a smaller one works.
/// - [`TenantAllowanceExhausted`](ConnectionRefusal::TenantAllowanceExhausted) is `409`, in this
///   module's existing sense of it: the request is fine and the tenant's current state conflicts
///   with it, so the remedy is a `DELETE` and a retry — the same shape as
///   [`already_connected`] and [`change_in_flight`]. A `413` here would tell an operator to send
///   less when what they have to do is disconnect something.
///
/// Every payload carries the bound it was decided against, so an operator reading the refusal
/// learns the limit rather than guessing it, and none carries a value.
fn connection_refused(refusal: &ConnectionRefusal) -> Response {
    let (status, extra) = match refusal {
        ConnectionRefusal::UndeclaredAuthority { connector }
        | ConnectionRefusal::NoCredentialDeclared { connector } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "connector": connector }),
        ),
        ConnectionRefusal::UndeclaredCredential {
            connector,
            credential,
            declared,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "connector": connector,
                "credential": credential,
                "declared": declared,
            }),
        ),
        ConnectionRefusal::Unaddressable {
            connector,
            credential,
            ..
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "connector": connector, "credential": credential }),
        ),
        ConnectionRefusal::CredentialTooLarge {
            connector,
            credential,
            bytes,
            limit,
        } => (
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({
                "connector": connector,
                "credential": credential,
                "bound": "credential",
                "sent_bytes": bytes,
                "limit_bytes": limit,
            }),
        ),
        ConnectionRefusal::TenantAllowanceExhausted {
            held,
            adding,
            limit,
        } => (
            StatusCode::CONFLICT,
            json!({
                "bound": "tenant",
                "held_bytes": held,
                "adding_bytes": adding,
                "limit_bytes": limit,
            }),
        ),
    };

    refuse(status, refusal.to_string(), extra)
}

/// **The X-14 placeholder.** A second connection to a connector this tenant already has.
///
/// The address has no level at which two instances of one connector differ, so accepting this
/// would overwrite the first connection, answer `201`, and send every later call to the wrong
/// account while looking healthy. The refusal names the level that will replace it:
/// `@instances/<uuid>`, which has landed in flux-connectors (C-406) and is not published — this
/// workspace pins `connector-spec` 0.8 from the registry. Wiring it up here, including resolving a
/// name the operator chooses to that uuid, is X-14.
fn already_connected(
    provider: &'static Provider,
    addresses: &[(DeclaredCredential<'_>, CredentialRef)],
) -> Response {
    refuse(
        StatusCode::CONFLICT,
        format!(
            "this tenant already has a connection to connector `{}`, and the credential address \
             has no instance dimension to tell two of them apart — a second one would overwrite \
             the first rather than sit beside it",
            provider.id,
        ),
        json!({
            "connector": provider.id,
            "addresses": addresses_of(addresses),
            "would_have_worked":
                "an instance level on the address — \
                 `tenants/<tenant>/<authority>/@instances/<uuid>/<credential>` — which has landed \
                 in flux-connectors (C-406) and is not published yet; this host pins \
                 connector-spec 0.8. Wiring it up, including resolving a name you choose to that \
                 uuid, is X-14. Until then, delete the existing connection before creating another",
        }),
    )
}

/// This composition bound no credential store.
///
/// Not a fallback and not an empty answer: a host that cannot hold a credential says so, and names
/// the setting that would have given it one. X-09's rule, at the surface that would have used it.
fn no_store() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "this host has no credential store bound, so it can neither hold nor find a \
             connection's credentials: set `{STORE_SETTING}` to a path outside every working tree",
        ),
        json!({ "setting": STORE_SETTING }),
    )
}

/// The store failed, and *how* it failed survives out to the caller.
///
/// Never a `404`, whatever the variant: "we cannot say" reported as "you have not connected that
/// integration" is an operator reconnecting an integration that was fine.
///
/// Beyond that the variants do **not** collapse into one message, because `AGENTS.md` § Conventions
/// asks that failures an operator answers differently stay distinguishable, and these three are
/// answered in three different places:
///
/// - [`Unreachable`](StoreError::Unreachable) — the store did not answer. A retry may work, so the
///   status is `503` and the caller is told to retry.
/// - [`Denied`](StoreError::Denied) — the store answered and refused **this host's own**
///   credentials. Retrying is useless and there is nothing wrong with the caller's request; an
///   operator has to go and fix this host's access. `502`, because the failure is upstream of us
///   and is not a transient.
/// - [`Backend`](StoreError::Backend) and [`Layout`](StoreError::Layout) — the store answered with
///   something this client cannot interpret. Upstream documents `Backend` as separate from
///   `Unreachable` for exactly this reason: retrying will not help. `502`.
///
/// The *reason* string never reaches the caller in any case — it names this host's own dependency,
/// its paths and its access — so it goes to the log, the same split the identity guard makes for an
/// unreachable provider.
fn store_failed(error: &StoreError) -> Response {
    let (status, happened, advice) = store_failure(error);

    error!(%error, "the credential store failed");

    refuse(status, format!("{happened}. {advice}"), json!({}))
}

/// How a store failure is answered: its status, what happened, and what an operator is to do.
///
/// Split out of [`store_failed`] because the partial-failure refusals — [`partly_written`] and
/// [`partly_destroyed`] — have to say the second half too, and two copies of this mapping is how
/// one refusal comes to tell an operator "retrying may work" while another tells them "retrying
/// will not help" about the same event. The whole argument for keeping the three kinds apart is on
/// [`store_failed`], and [`tests::a_store_failure_says_what_it_has_always_said`] pins the words a
/// caller reads so that a change to one reader cannot reword them for the others.
fn store_failure(error: &StoreError) -> (StatusCode, &'static str, &'static str) {
    match error {
        StoreError::NotFound { .. } => {
            // Unreachable in practice: `held` filters this out. Kept because collapsing not-found
            // into a failure is exactly the mistake `StoreError` documents, and a future edit is
            // likelier to reach for this function than to re-read that.
            warn!("a not-found reached the store-failure path, which is a bug in this module");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the credential store did not answer, so this host cannot say what this tenant \
                 has connected",
                "Retrying may work",
            )
        }
        StoreError::Unreachable { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            "the credential store did not answer, so this host cannot say what this tenant has \
             connected",
            "Retrying may work",
        ),
        StoreError::Denied { .. } => (
            StatusCode::BAD_GATEWAY,
            "the credential store refused this host's own access, so it cannot reach this \
             tenant's credentials",
            "Retrying will not help; an operator has to restore this host's access to the store",
        ),
        StoreError::Backend { .. } | StoreError::Layout { .. } => (
            StatusCode::BAD_GATEWAY,
            "the credential store answered with something this host cannot interpret",
            "Retrying will not help; this is a defect in the store or in how it is configured",
        ),
    }
}

/// How much an operator has to do about a store failure, ordered by how much that is.
///
/// [`remove`] deletes every declared address rather than stopping at the first failure, so its loop
/// can see more than one kind — and it has to answer with one. Reporting the *first* it saw meant an
/// `Unreachable` followed by a `Denied` was answered `503` "retrying may work" with the denied
/// address named in the same response's `left_behind`, which is the misinformation [`store_failed`]
/// argues against at length. So the worst is kept rather than the first, and "worst" is this order.
///
/// The boundary that matters is the first one, between a failure that may resolve itself and one
/// that will not: on a revocation surface, telling somebody to retry when nobody is coming to fix
/// the store is how a live credential stays live. The second boundary separates two kinds that
/// already share a status and a "retrying will not help", and is settled by which refusal admits
/// less — a store this host could not *interpret* is not summarised as one that gave a clear answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Escalation {
    /// Nobody has to do anything yet; the store may answer next time.
    Transient,
    /// A person has to restore this host's access to the store.
    RestoreAccess,
    /// A person has to repair the store or how it is configured.
    RepairTheStore,
}

/// Where a failure sits in that order.
///
/// Deliberately a second match on the same variants rather than a fourth field on
/// [`store_failure`]: what a caller is *told* and how two failures *compare* are different
/// questions, and a comparison returned from the tuple would be read as part of the answer.
fn escalation(error: &StoreError) -> Escalation {
    match error {
        // A not-found is a bug in this module rather than a store failure — `store_failure` says so
        // and warns about it. Ranked lowest so it can never win a comparison and hide a real one.
        StoreError::NotFound { .. } | StoreError::Unreachable { .. } => Escalation::Transient,
        StoreError::Denied { .. } => Escalation::RestoreAccess,
        StoreError::Backend { .. } | StoreError::Layout { .. } => Escalation::RepairTheStore,
    }
}

/// Another change to this same connection is already in flight.
///
/// One at a time per `(tenant, connector)`, because deciding whether a connection exists and then
/// writing it is a read-decide-write that must not interleave with another of the same — see
/// [`ConnectionGuard`](crate::connection_guard::ConnectionGuard) for the whole argument, including
/// why this refuses rather than waits.
fn change_in_flight(provider: &'static Provider) -> Response {
    refuse(
        StatusCode::CONFLICT,
        format!(
            "another change to this tenant's `{}` connection is already in flight; only one at a \
             time, because the credential address has no instance dimension to tell two \
             connections to one connector apart. Retry once it has finished",
            provider.id,
        ),
        json!({ "connector": provider.id }),
    )
}

/// Another change to one of this tenant's *other* connections is already in flight.
///
/// A separate refusal from [`change_in_flight`] because it is a separate fact, and one an operator
/// would otherwise misread: nothing is wrong with the connection they asked for, and a message
/// naming it as the thing in flight would send them looking for a request that does not exist.
///
/// The claim behind it is the tenant's rather than the connection's, because what a tenant may
/// occupy is decided as a sum over every connector — see
/// [`ConnectionGuard`](crate::connection_guard::ConnectionGuard) for why that width is the smallest
/// one that makes the allowance true, and why `DELETE` stays outside it.
fn allowance_change_in_flight(provider: &'static Provider) -> Response {
    refuse(
        StatusCode::CONFLICT,
        format!(
            "another of this tenant's connections is already being changed; a connection to `{}` \
             is refused while it is, because what one tenant may occupy is decided against all of \
             its connectors at once. Retry once it has finished",
            provider.id,
        ),
        json!({ "connector": provider.id }),
    )
}

/// The store failed part way through writing a connection.
///
/// Reports what was done about it, because "nothing was written" and "some values may still be
/// there" send an operator to different places — and a refusal claiming the first while the second
/// is true is worse than one that admits it does not know.
///
/// The two are answers to *is a retry safe*, which is about the rollback. Whether a retry is worth
/// anything is a different question, answered by the failure's kind, and both halves say so —
/// see the `advice` below.
fn partly_written(
    provider: &'static Provider,
    error: &StoreError,
    rolled_back: Result<(), Vec<String>>,
) -> Response {
    error!(%error, connector = provider.id, "a connection could not be written");

    // The kind survives, as it does for [`partly_destroyed`] and for every other refusal on this
    // surface: a `Denied` reported as `503` "retrying may work" sends an operator to retry, which
    // is the one thing that cannot restore this host's access to the store. The rollback report
    // below is orthogonal to it — it says whether retrying is *safe*, never whether it will help.
    let (status, _, advice) = store_failure(error);

    match rolled_back {
        Ok(()) => refuse(
            status,
            format!(
                "the credential store failed while writing the `{}` connection. Nothing was left \
                 behind — the values written before the failure were taken back out — so retrying \
                 is safe. {advice}",
                provider.id,
            ),
            json!({ "connector": provider.id, "left_behind": Value::Null }),
        ),
        // The store failed, and so did taking the values back out. Naming the addresses is the
        // whole of what this host can still do for the operator: refuse, and say exactly where to
        // look. The values are not named, only the addresses.
        Err(remaining) => refuse(
            status,
            format!(
                "the credential store failed while writing the `{}` connection, and the values \
                 already written could not be taken back out. Some credentials may remain at the \
                 addresses below; `DELETE /api/connections/{}` before retrying. {advice}",
                provider.id, provider.id,
            ),
            json!({ "connector": provider.id, "left_behind": remaining }),
        ),
    }
}

/// The store failed part way through **destroying** a connection.
///
/// The delete direction of [`partly_written`], and deliberately its vocabulary rather than a second
/// one for the same idea: `left_behind` names the addresses this host cannot say are empty, exactly
/// as it does for a create whose rollback failed.
///
/// What it cannot borrow is `create`'s *mechanism*. There is no rollback in this direction — a
/// destroyed credential cannot be put back, because this host never held the plaintext to restore —
/// so `left_behind` is never `null` here the way it is for a create that undid itself. A partial
/// delete is reported, not repaired.
///
/// `destroyed` is the other half, and it is the half the operator this refusal is written for
/// needs: somebody revoking a leaked secret, who has to know which credentials are already gone so
/// that the work left is exactly the ones named beside them. Both halves are addresses and never
/// values, and both are this tenant's own — the same rule every refusal on this surface follows.
///
/// # `left_behind` is a list of addresses, not a list of live credentials
///
/// The two halves are computed asymmetrically, and only one of them can be. `destroyed` is narrowed
/// to what the pre-delete probe saw a value at, because calling an empty address "destroyed" would
/// overstate what happened to somebody counting revoked secrets. **`left_behind` is not narrowed the
/// same way, and must not be.** A connector may legitimately hold a subset of what it declares
/// ([`tests::a_connection_may_carry_a_subset_of_what_is_declared`]), so an address here may never
/// have held anything — but a failed delete is precisely the case where this host cannot say the
/// address is empty, and the reason [`remove`] deletes the whole declared set is that a value may
/// have appeared since the probe. Narrowing to what the probe saw would drop exactly the addresses
/// this host knows least about, and on a revocation surface an address that goes unmentioned reads
/// as gone. "Possibly still live" must never come out as "definitely gone", so the list stays whole.
///
/// What was wrong was therefore the *claim*, not the list: the sentence said flatly to treat these
/// as still usable, where the sibling [`partly_written`] hedges with "Some credentials **may**
/// remain". It now hedges the same way and still gives the same instruction — a caller is told that
/// a credential may remain at any of these addresses and to treat every one as live — which is the
/// safe bias stated as something this host can actually know.
fn partly_destroyed(
    provider: &'static Provider,
    error: &StoreError,
    destroyed: Vec<String>,
    left_behind: Vec<String>,
) -> Response {
    error!(%error, connector = provider.id, "a connection could not be fully destroyed");

    // The kind survives, as it does everywhere else on this surface: a `Denied` reported as
    // "retrying may work" would be a fresh instance of the misinformation this refusal exists to
    // end.
    let (status, _, advice) = store_failure(error);

    refuse(
        status,
        format!(
            "the credential store failed while destroying the `{}` connection, so it is now part \
             gone and part unaccounted for: the credentials at the addresses in `destroyed` are \
             gone and cannot be put back, and the addresses in `left_behind` this host could not \
             destroy — a credential may remain at any of them, so treat every one as still usable \
             by anyone holding it. {advice}; a `DELETE /api/connections/{}` that answers `204` is \
             what makes the connection gone",
            provider.id, provider.id,
        ),
        json!({
            "connector": provider.id,
            "destroyed": destroyed,
            "left_behind": left_behind,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::Mutex;

    use axum::body::Body;
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{Method, Request as HttpRequest};
    use axum::Router;
    use exchange_host::{
        async_trait, MAX_CREDENTIAL_VALUE_BYTES, MAX_TENANT_STORE_BYTES, TENANTS_ROOT,
    };
    use tower::Service;

    use crate::dev_identity::DevIdentity;

    /// Two tenants, one principal each. `alice` is `acme`; `bob` is `globex`.
    const ROSTER: &str = "user:alice@acme,user:bob@globex";

    /// The value a test stores. Never a real secret, and asserted absent from every answer a
    /// different tenant receives — and from every refusal anyone receives.
    const SENTINEL: &str = "SENTINEL-NOT-A-REAL-SECRET";

    /// How a [`TestStore`] has been told to fail.
    ///
    /// The three the surface answers differently, so that
    /// [`a_store_failure_keeps_its_kind_out_to_the_caller`] can drive each one rather than assert
    /// that they are all `503`.
    #[derive(Debug, Clone, Copy)]
    enum Failure {
        Unreachable,
        Denied,
        Backend,
    }

    impl Failure {
        fn at(self, path: String) -> StoreError {
            let reason = "the test store was told to fail this way".to_string();
            match self {
                Self::Unreachable => StoreError::Unreachable { path, reason },
                Self::Denied => StoreError::Denied { path, reason },
                Self::Backend => StoreError::Backend { path, reason },
            }
        }
    }

    /// A store that lives in the test.
    ///
    /// Hand-rolled rather than reaching for `connector_secrets::MemoryStore`, so that
    /// `exchange_host` is not made to re-export an in-memory store a production composition could
    /// then bind — the one thing X-09 refuses. Being ours, it can also be told to fail in each of
    /// the ways the surface answers differently, to fail *part way* through a multi-credential
    /// write, and to widen the window between a probe and a write so that a race is reproducible
    /// rather than lucky.
    #[derive(Default)]
    struct TestStore {
        held: Mutex<HashMap<String, String>>,
        /// Every operation fails this way.
        fails: Mutex<Option<Failure>>,
        /// This many `put`s succeed; the rest fail. `None` is "no limit".
        puts_allowed: Mutex<Option<usize>>,
        /// How a `put` beyond `puts_allowed` fails. `None` is an unreachable store.
        ///
        /// Separate from `fails`, which fails *every* operation and so never reaches the write:
        /// `held`'s probe refuses first, and the create path under test is the one after it.
        put_failure: Mutex<Option<Failure>>,
        puts: Mutex<usize>,
        /// `delete` fails, which is what makes a rollback fail.
        deletes_fail: Mutex<bool>,
        /// This many `delete`s succeed; the rest fail. `None` is "no limit".
        ///
        /// Distinct from `deletes_fail`, which fails every one from the start: driving `remove`
        /// *part way* through a multi-credential connection needs the n-th delete to fail and the
        /// ones before it to land.
        deletes_allowed: Mutex<Option<usize>>,
        deletes: Mutex<usize>,
        /// How a `delete` at a rendered address fails, for the addresses named here.
        ///
        /// Distinct from both flags above, which fail every delete the same way: a `remove` loop
        /// only reports the *worst* of several kinds if it can be made to see more than one, and
        /// neither a global flag nor a counter can arm two different kinds in one run.
        delete_failures: Mutex<HashMap<String, Failure>>,
        /// `get` yields to the runtime, widening the read-decide-write window.
        widened: Mutex<bool>,
    }

    impl TestStore {
        fn fail_with(&self, failure: Failure) {
            *self.fails.lock().expect("no test poisons this") = Some(failure);
        }

        fn unreachable(&self) {
            self.fail_with(Failure::Unreachable);
        }

        /// Let `allowed` writes land **from here** and fail every one after, so a connector
        /// declaring two credentials can be made to fail half way.
        ///
        /// The count restarts, as it does for deletes, so a test may connect another tenant first
        /// and still arm a budget for the connection it is about to fail.
        fn allow_only(&self, allowed: usize) {
            self.allow_only_failing_with(allowed, Failure::Unreachable);
        }

        /// The same, failing that way rather than as an unreachable store.
        ///
        /// A half-written create is answered from the failure's kind, so driving that path needs
        /// each of the three the surface answers differently, not only the transient one.
        fn allow_only_failing_with(&self, allowed: usize, failure: Failure) {
            *self.puts.lock().expect("no test poisons this") = 0;
            *self.puts_allowed.lock().expect("no test poisons this") = Some(allowed);
            *self.put_failure.lock().expect("no test poisons this") = Some(failure);
        }

        /// The store recovers: writes land again.
        fn recovers(&self) {
            *self.puts_allowed.lock().expect("no test poisons this") = None;
            *self.fails.lock().expect("no test poisons this") = None;
        }

        fn deletes_fail(&self) {
            *self.deletes_fail.lock().expect("no test poisons this") = true;
        }

        /// Let `allowed` deletes land **from here** and fail every one after, so a connector
        /// declaring two credentials can be made to fail half way through a `DELETE`.
        ///
        /// The count restarts, so a test may delete a whole connection first and still arm a
        /// budget for the next one.
        fn allow_only_deletes(&self, allowed: usize) {
            *self.deletes.lock().expect("no test poisons this") = 0;
            *self.deletes_allowed.lock().expect("no test poisons this") = Some(allowed);
        }

        /// Fail the `delete` at one rendered address this way, leaving every other address alone.
        ///
        /// The finest control the store offers, and the only one that can arm two kinds in a
        /// single `remove`: it takes the address rather than a position in the loop, so a test
        /// says which credential fails how rather than counting deletions to get there.
        fn delete_fails_at(&self, path: &str, failure: Failure) {
            self.delete_failures
                .lock()
                .expect("no test poisons this")
                .insert(path.to_string(), failure);
        }

        /// Make the window between a probe and a write wide enough that a concurrent request
        /// reliably lands inside it.
        ///
        /// The race is real without this — the reviewer reproduced it on the first attempt — but a
        /// test that only sometimes exercises the window is a test that only sometimes proves
        /// anything.
        fn widen_the_window(&self) {
            *self.widened.lock().expect("no test poisons this") = true;
        }

        fn failure(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            let failure = *self.fails.lock().expect("no test poisons this");
            match failure {
                Some(failure) => Err(failure.at(address_path(reference))),
                None => Ok(()),
            }
        }

        fn is_widened(&self) -> bool {
            *self.widened.lock().expect("no test poisons this")
        }

        /// What is stored at a rendered address, for an assertion about the store rather than
        /// about the surface.
        fn at(&self, path: &str) -> Option<String> {
            self.held
                .lock()
                .expect("no test poisons this")
                .get(path)
                .cloned()
        }

        /// How many bytes are stored under a rendered prefix — one tenant's whole occupancy, when
        /// the prefix is `tenants/<tenant>/`.
        ///
        /// The assertion for the per-tenant bound has to be made against the *store*, not against
        /// what the surface answered: the whole point of that bound is what ends up in the one
        /// file every tenant's write rewrites.
        fn bytes_under(&self, prefix: &str) -> usize {
            self.held
                .lock()
                .expect("no test poisons this")
                .iter()
                .filter(|(path, _)| path.starts_with(prefix))
                .map(|(_, value)| value.len())
                .sum()
        }

        /// Put `bytes` bytes at a rendered address, without going through the surface.
        ///
        /// For a test that needs a tenant already sitting near its allowance: how it got there is
        /// not what is under test, and driving it through `create` would need connectors
        /// declaring more credentials than any in the catalogue does.
        fn place(&self, path: String, bytes: usize) {
            self.held
                .lock()
                .expect("no test poisons this")
                .insert(path, "v".repeat(bytes));
        }

        fn addresses(&self) -> Vec<String> {
            let mut addresses: Vec<String> = self
                .held
                .lock()
                .expect("no test poisons this")
                .keys()
                .cloned()
                .collect();
            addresses.sort();
            addresses
        }
    }

    #[async_trait]
    impl SecretStore for TestStore {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            self.failure(reference)?;

            if self.is_widened() {
                // Enough yields that another task on the runtime reliably gets to run its own
                // probe before this one's caller writes.
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
            }

            let path = address_path(reference);
            self.held
                .lock()
                .expect("no test poisons this")
                .get(&path)
                .map(Secret::new)
                .ok_or(StoreError::NotFound { path })
        }

        async fn put(&self, reference: &CredentialRef, secret: &Secret) -> Result<(), StoreError> {
            self.failure(reference)?;

            {
                let mut puts = self.puts.lock().expect("no test poisons this");
                let allowed = *self.puts_allowed.lock().expect("no test poisons this");
                if allowed.is_some_and(|allowed| *puts >= allowed) {
                    let failure = self
                        .put_failure
                        .lock()
                        .expect("no test poisons this")
                        .unwrap_or(Failure::Unreachable);
                    return Err(failure.at(address_path(reference)));
                }
                *puts += 1;
            }

            self.held
                .lock()
                .expect("no test poisons this")
                .insert(address_path(reference), secret.expose_secret().to_string());
            Ok(())
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            self.failure(reference)?;

            // Before the blanket flag and the counter, because it is the more specific
            // instruction: a test that names an address means that address.
            if let Some(failure) = self
                .delete_failures
                .lock()
                .expect("no test poisons this")
                .get(&address_path(reference))
                .copied()
            {
                return Err(failure.at(address_path(reference)));
            }

            if *self.deletes_fail.lock().expect("no test poisons this") {
                return Err(Failure::Unreachable.at(address_path(reference)));
            }

            {
                let mut deletes = self.deletes.lock().expect("no test poisons this");
                let allowed = *self.deletes_allowed.lock().expect("no test poisons this");
                if allowed.is_some_and(|allowed| *deletes >= allowed) {
                    return Err(Failure::Unreachable.at(address_path(reference)));
                }
                *deletes += 1;
            }

            self.held
                .lock()
                .expect("no test poisons this")
                .remove(&address_path(reference));
            Ok(())
        }
    }

    /// An app with both tenants armed and a store bound, plus the store to assert against.
    fn connected_app() -> (Router, Arc<TestStore>) {
        let store = Arc::new(TestStore::default());
        let app = super::super::app(
            AppState::with_development_identity(Arc::new(
                DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
            ))
            .with_credentials(store.clone()),
        );

        (app, store)
    }

    /// An app with the tenants armed and **no** store bound.
    fn storeless_app() -> Router {
        super::super::app(AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
        )))
    }

    /// Drive one request through the assembled app as `handle`, and hand back what a caller sees.
    async fn call(
        app: &Router,
        handle: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut service = app.clone().into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("a router is always ready");

        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(AUTHORIZATION, format!("Bearer {handle}"));

        let request = match body {
            Some(body) => request
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("a well-formed request");

        let response = service
            .call(request)
            .await
            .expect("a router is infallible")
            .into_response();

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a response body");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Connect zendesk as `handle`, which every test below starts from.
    async fn connect_zendesk(app: &Router, handle: &str) -> (StatusCode, Value) {
        call(
            app,
            handle,
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": SENTINEL } })),
        )
        .await
    }

    /// The Acceptance's first item, end to end and in one place: create, list, read, delete.
    #[tokio::test]
    async fn a_connection_is_created_listed_read_and_deleted() {
        let (app, store) = connected_app();

        let (status, created) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(created["connector"], "zendesk");
        assert_eq!(created["credentials"][0]["held"], true);
        assert_eq!(
            created["credentials"][0]["address"], "tenants/acme/com.zendesk.api/api_token",
            "the address is derived from the principal's tenant and the connector's declaration",
        );

        let (status, listed) = call(&app, "alice", Method::GET, "/api/connections", None).await;
        assert_eq!(status, StatusCode::OK);
        let connections = listed["connections"].as_array().expect("an array");
        assert_eq!(connections.len(), 1, "{listed}");
        assert_eq!(connections[0]["connector"], "zendesk");

        let (status, read) =
            call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
        assert_eq!(status, StatusCode::OK, "{read}");
        assert_eq!(read["credentials"][0]["held"], true);

        let (status, _) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/zendesk",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, _) = call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "the connection is gone");

        let (_, listed) = call(&app, "alice", Method::GET, "/api/connections", None).await;
        assert!(listed["connections"]
            .as_array()
            .expect("an array")
            .is_empty());

        assert!(
            store.addresses().is_empty(),
            "the Acceptance's last item: deleting a connection destroys its credential, and the \
             store is what says so — {:?}",
            store.addresses(),
        );
    }

    /// **The Acceptance's failing-first test.** An authenticated principal of one tenant cannot
    /// read, use or delete another tenant's connection — and the refusal names **its own** address,
    /// never the other tenant's value.
    ///
    /// There is deliberately no vector here by which `acme` could *name* `globex`'s connection: no
    /// route takes a tenant or an address, so the strongest thing `acme` can do is ask for the same
    /// connector and be told nothing is there. That is the assertion.
    #[tokio::test]
    async fn a_tenant_cannot_reach_another_tenants_connection() {
        let (app, store) = connected_app();

        // `globex` connects Zendesk.
        let (status, _) = connect_zendesk(&app, "bob").await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "tenant globex must be able to create its own connection",
        );

        // `acme` asks for the same connector, and has nothing.
        let (status, body) =
            call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "tenant acme has no zendesk connection, and another tenant's must not answer for it",
        );

        let rendered = body.to_string();
        assert!(
            rendered.contains("tenants/acme/com.zendesk.api"),
            "the refusal must name the address this host looked at: {rendered}",
        );
        assert!(
            !rendered.contains(SENTINEL),
            "a refusal must name the address, never the value: {rendered}",
        );
        assert!(
            !rendered.contains("globex"),
            "a refusal must not disclose the tenant that does hold one: {rendered}",
        );

        // Nor does the listing leak it.
        let (_, listed) = call(&app, "alice", Method::GET, "/api/connections", None).await;
        assert!(
            listed["connections"]
                .as_array()
                .expect("an array")
                .is_empty(),
            "one tenant's listing must not contain another's connection: {listed}",
        );

        // `acme` cannot destroy it either.
        let (status, _) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/zendesk",
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "deleting another tenant's connection must be a refusal, not a success",
        );

        // And it is still there, at its own address, untouched.
        let (status, _) = call(&app, "bob", Method::GET, "/api/connections/zendesk", None).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "tenant globex's connection must have survived acme's delete",
        );
        assert_eq!(
            store.at("tenants/globex/com.zendesk.api/api_token"),
            Some(SENTINEL.to_string()),
        );
    }

    /// **The X-14 placeholder, asserted.** A second connection to one connector is refused rather
    /// than silently overwriting the first, and the refusal names the level that would have worked.
    ///
    /// Delete this test in the change that lands the `@instances/<uuid>` level.
    #[tokio::test]
    async fn a_second_connection_to_one_connector_is_refused_rather_than_overwriting() {
        let (app, store) = connected_app();

        let (status, _) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": "A-SECOND-SUBDOMAIN" } })),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "a second connection to one connector collides on one address, so it must refuse \
             rather than overwrite: {body}",
        );

        let rendered = body.to_string();
        assert!(
            rendered.contains("@instances/<uuid>"),
            "the refusal must name the level that would have worked: {rendered}",
        );
        assert!(rendered.contains("X-14"), "{rendered}");
        assert!(
            rendered.contains("tenants/acme/com.zendesk.api/api_token"),
            "the refusal must name the address it collides at: {rendered}",
        );

        assert_eq!(
            store.at("tenants/acme/com.zendesk.api/api_token"),
            Some(SENTINEL.to_string()),
            "the first connection's value must be exactly what it was — a refusal that had \
             already written is the failure this test exists for",
        );
    }

    /// The Acceptance's last item, asserted against the store rather than against the surface: a
    /// connector with several declared credentials has all of them destroyed.
    #[tokio::test]
    async fn deleting_a_connection_destroys_every_credential_it_holds() {
        let (app, store) = connected_app();

        let (status, body) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.signing_secret": SENTINEL,
                }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(
            store.addresses(),
            vec![
                "tenants/acme/com.slack.api/bot_token".to_string(),
                "tenants/acme/com.slack.api/signing_secret".to_string(),
            ],
        );

        let (status, _) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/slack",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(
            store.addresses().is_empty(),
            "every credential the connection held must be gone: {:?}",
            store.addresses(),
        );
    }

    /// A connection may carry a subset of what the connector declares — `slack.signing_secret`
    /// verifies inbound webhooks and an operator who makes no outbound-only use of it has none —
    /// and deleting still clears the whole set.
    #[tokio::test]
    async fn a_connection_may_carry_a_subset_of_what_is_declared() {
        let (app, store) = connected_app();

        let (status, created) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({ "credentials": { "slack.bot_token": SENTINEL } })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");

        let credentials = created["credentials"].as_array().expect("an array");
        assert_eq!(credentials.len(), 2, "both declared credentials are listed");
        assert_eq!(credentials[0]["name"], "slack.bot_token");
        assert_eq!(credentials[0]["held"], true);
        assert_eq!(credentials[1]["name"], "slack.signing_secret");
        assert_eq!(
            credentials[1]["held"], false,
            "a credential with no value must say so rather than be omitted",
        );

        assert_eq!(store.addresses().len(), 1);
    }

    /// The invariant, down the one vector this module opens that X-03's tests could not cover: a
    /// **body field**. The value lands under the resolved principal's tenant, and the claimed one
    /// gets nothing.
    #[tokio::test]
    async fn a_tenant_in_a_body_field_does_not_influence_where_the_credential_lands() {
        let (app, store) = connected_app();

        let (status, created) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({
                "tenant": "globex",
                "credentials": { "zendesk.api_token": SENTINEL },
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(
            store.addresses(),
            vec!["tenants/acme/com.zendesk.api/api_token".to_string()],
            "the tenant comes from the resolved principal, and a body field reaches nothing",
        );
        assert!(
            !created.to_string().contains("globex"),
            "the claimed tenant must appear nowhere in the answer: {created}",
        );
    }

    /// A connector nothing declares is a `404` naming the id, never an empty success.
    #[tokio::test]
    async fn an_unknown_connector_is_refused_and_named() {
        let (app, _) = connected_app();

        for (method, body) in [
            (Method::GET, None),
            (
                Method::POST,
                Some(json!({ "credentials": { "x.y": SENTINEL } })),
            ),
            (Method::DELETE, None),
        ] {
            let (status, refusal) = call(
                &app,
                "alice",
                method.clone(),
                "/api/connections/no-such-vendor",
                body,
            )
            .await;

            assert_eq!(status, StatusCode::NOT_FOUND, "{method}: {refusal}");
            assert_eq!(refusal["connector"], "no-such-vendor");
        }
    }

    /// `freshdesk` declares no credential — flux-connectors records that as an intentional gap
    /// (C-16), not an oversight here. There is nothing to address, so connecting it is refused and
    /// the refusal says which fact is missing.
    #[tokio::test]
    async fn a_connector_that_declares_no_credential_is_refused() {
        let (app, store) = connected_app();

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/freshdesk",
            Some(json!({ "credentials": { "freshdesk.api_key": SENTINEL } })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
        assert!(
            refusal["error"]
                .as_str()
                .expect("a reason")
                .contains("declares no credential"),
            "{refusal}",
        );
        assert!(
            store.addresses().is_empty(),
            "a refused connection must have stored nothing",
        );
    }

    /// A name the connector does not declare is refused, and the refusal lists what it does — a
    /// value stored under a typo would sit at an address no operation reads and nobody rotates.
    ///
    /// Nothing is written, including the names that *were* valid: a half-written connection is one
    /// an operator cannot tell from a working one until a call fails.
    #[tokio::test]
    async fn an_undeclared_credential_is_refused_and_nothing_is_written() {
        let (app, store) = connected_app();

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.api_key": SENTINEL,
                }
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
        assert_eq!(refusal["credential"], "slack.api_key");
        assert_eq!(
            refusal["declared"],
            json!(["slack.bot_token", "slack.signing_secret"]),
        );
        assert!(
            store.addresses().is_empty(),
            "the valid half of a body with a typo must not have been written: {:?}",
            store.addresses(),
        );
    }

    /// A body naming no credential creates nothing. An empty connection is a connection that
    /// `401`s at the vendor and looks fine from here.
    #[tokio::test]
    async fn a_connection_with_no_credential_is_refused() {
        let (app, store) = connected_app();

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": {} })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
        assert_eq!(refusal["declared"], json!(["zendesk.api_token"]));
        assert!(store.addresses().is_empty());
    }

    /// **X-22's failing-first test.** A value too large to be a credential is refused *before*
    /// anything is written, and the store is what says so.
    ///
    /// The assertion that matters is the last pair, not the status: a `413` that had already
    /// rewritten and `fsync`-ed the whole store would have cost every other tenant the write it
    /// was refusing. The refusal names the credential and the bound and never what was sent.
    ///
    /// A value at a size a credential really is, in the same run — otherwise a bound that refused
    /// everything would pass this.
    #[tokio::test]
    async fn a_credential_beyond_the_bound_is_refused_and_nothing_is_written() {
        let (app, store) = connected_app();

        // Not a credential by any reading: no token, signing secret or PEM private key is this
        // size. Spelled as a literal rather than through the constant so that this test is the
        // same test before and after the bound exists.
        let oversized = "x".repeat(64 * 1024);

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": oversized } })),
        )
        .await;

        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{refusal}");
        assert_eq!(refusal["credential"], "zendesk.api_token", "{refusal}");
        assert!(
            refusal["limit_bytes"].is_number(),
            "the refusal must name the bound, so an operator reading it learns the limit rather \
             than guessing: {refusal}",
        );
        assert!(
            !refusal.to_string().contains(&oversized),
            "a refusal names the credential and the bound, never the value: {refusal}",
        );

        // Nothing was written. Not "the status was 4xx" — the store itself.
        assert!(
            store.addresses().is_empty(),
            "a refused credential must not have been written: {:?}",
            store.addresses(),
        );
        assert_eq!(store.at("tenants/acme/com.zendesk.api/api_token"), None);

        // And a credential-sized value still lands, so the refusal above cannot have passed by
        // refusing everything.
        let (status, created) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(
            store.at("tenants/acme/com.zendesk.api/api_token"),
            Some(SENTINEL.to_string()),
        );
    }

    /// The bound is **stated once**, and the refusal carries that statement rather than a second
    /// copy of the number — so an operator reading a refusal learns the limit, and a change to the
    /// constant cannot leave a refusal quoting the old one.
    ///
    /// Inclusive at the bound, asserted from both sides: a value of exactly
    /// [`MAX_CREDENTIAL_VALUE_BYTES`] is a credential, and one byte more is not.
    #[tokio::test]
    async fn the_credential_bound_is_stated_once_and_a_value_at_it_still_lands() {
        let (app, store) = connected_app();

        let at_the_bound = "v".repeat(MAX_CREDENTIAL_VALUE_BYTES);
        let (status, created) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": at_the_bound } })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(
            store.at("tenants/acme/com.zendesk.api/api_token"),
            Some(at_the_bound),
        );

        // One byte past it, as the other tenant, so the `409` for an existing connection cannot be
        // what answers.
        let past_the_bound = "v".repeat(MAX_CREDENTIAL_VALUE_BYTES + 1);
        let (status, refusal) = call(
            &app,
            "bob",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": past_the_bound } })),
        )
        .await;

        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{refusal}");
        assert_eq!(refusal["bound"], "credential", "{refusal}");
        assert_eq!(
            refusal["limit_bytes"],
            json!(MAX_CREDENTIAL_VALUE_BYTES),
            "the refusal must carry the bound itself, not a second spelling of it: {refusal}",
        );
        assert_eq!(
            refusal["sent_bytes"],
            json!(MAX_CREDENTIAL_VALUE_BYTES + 1),
            "{refusal}",
        );
        assert_eq!(
            store.at("tenants/globex/com.zendesk.api/api_token"),
            None,
            "one byte past the bound must have written nothing",
        );
    }

    /// **X-22's second bound.** What one tenant may occupy across the *whole* store is bounded, and
    /// not merely as a consequence of each value being bounded.
    ///
    /// Every value here is at exactly the per-value bound, so the per-value check admits all of
    /// them and the only thing that can stop this tenant is the per-tenant one. The assertions that
    /// matter are the last two: connectors were still left that the per-value bound alone would
    /// have let this tenant fill, and the tenant's share of the store — the file every other
    /// tenant's write has to rewrite — never went past the allowance.
    #[tokio::test]
    async fn the_total_one_tenant_can_occupy_is_bounded_and_not_only_each_value() {
        let (app, store) = connected_app();

        let at_the_value_bound = "v".repeat(MAX_CREDENTIAL_VALUE_BYTES);
        let tenant = Tenant::new("acme").expect("a plain tenant id");

        let mut connected = 0usize;
        let mut left_unused = 0usize;
        let mut refused: Option<(String, StatusCode, Value)> = None;

        for provider in connector_catalog::providers() {
            let declared = declared_credentials(provider);
            let declaration = declaration(provider, &declared);
            // A connector with no address cannot hold anything, so it is not a connector this
            // tenant could have spent its allowance on.
            if declaration.addresses(&tenant).is_err() {
                continue;
            }

            if refused.is_some() {
                left_unused += 1;
                continue;
            }

            let credentials: serde_json::Map<String, Value> = declared
                .iter()
                .map(|credential| {
                    (
                        credential.name.to_string(),
                        json!(at_the_value_bound.clone()),
                    )
                })
                .collect();

            let (status, body) = call(
                &app,
                "alice",
                Method::POST,
                &format!("/api/connections/{}", provider.id),
                Some(json!({ "credentials": credentials })),
            )
            .await;

            if status == StatusCode::CREATED {
                connected += 1;
            } else {
                refused = Some((provider.id.to_string(), status, body));
            }
        }

        let (connector, status, refusal) = refused.expect(
            "a tenant writing values at the per-value bound into every catalogued connector must \
             be stopped by the per-tenant bound",
        );

        assert!(
            connected > 0,
            "the per-tenant bound must admit a real connection, or it is not a bound but a \
             refusal of everything",
        );
        assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
        assert_eq!(refusal["bound"], "tenant", "{refusal}");
        assert_eq!(
            refusal["limit_bytes"],
            json!(MAX_TENANT_STORE_BYTES),
            "the refusal must name the bound it was decided against: {refusal}",
        );

        // Nothing was written for the connection that was refused. Asserted against the store, not
        // against the status.
        let (status, _) = call(
            &app,
            "alice",
            Method::GET,
            &format!("/api/connections/{connector}"),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a connection refused for the tenant's allowance must have written nothing",
        );

        // The per-value bound alone would have let this tenant carry on: there were connectors
        // left, each of which it could have filled at the value bound.
        assert!(
            left_unused > 0,
            "this catalogue is too small to tell the two bounds apart — the per-tenant bound has \
             to bite while addressable connectors remain, or the test proves nothing",
        );

        // And the thing the bound exists to protect: this tenant's share of the one file every
        // other tenant's write rewrites.
        let occupied = store.bytes_under("tenants/acme/");
        assert!(
            occupied <= MAX_TENANT_STORE_BYTES,
            "one tenant occupies {occupied} bytes, past the {MAX_TENANT_STORE_BYTES} it may hold",
        );
    }

    /// Why the second bound has to exist at all, as arithmetic over this catalogue.
    ///
    /// A tenant may hold one value per declared address, so bounding each value alone leaves a
    /// ceiling of `addresses × MAX_CREDENTIAL_VALUE_BYTES` — and that ceiling *grows every time
    /// upstream adds a connector*. [`MAX_TENANT_STORE_BYTES`] does not move when the catalogue
    /// does, which is the property worth having; this pins that it is the tighter of the two.
    #[test]
    fn the_per_value_bound_alone_does_not_bound_what_one_tenant_holds() {
        let tenant = Tenant::new("acme").expect("a plain tenant id");
        let addresses: usize = connector_catalog::providers()
            .iter()
            .filter_map(|provider| {
                let declared = declared_credentials(provider);
                declaration(provider, &declared)
                    .addresses(&tenant)
                    .ok()
                    .map(|addresses| addresses.len())
            })
            .sum();

        let per_value_ceiling = addresses * MAX_CREDENTIAL_VALUE_BYTES;
        assert!(
            MAX_TENANT_STORE_BYTES < per_value_ceiling,
            "the per-value bound alone would let one tenant occupy {per_value_ceiling} bytes \
             across {addresses} addresses, so a per-tenant bound of {MAX_TENANT_STORE_BYTES} is \
             what actually bounds the whole",
        );
    }

    /// **The count of credentials is the catalogue's number, not the caller's.** A body carrying
    /// more than the connector declares carries one it does not declare, and that is already
    /// refused before anything is written — which is what bounds how many addresses one request
    /// can occupy.
    ///
    /// The declared set is connected in the same run, so the refusal cannot be passing by refusing
    /// everything.
    #[tokio::test]
    async fn more_credentials_than_are_declared_is_refused_and_the_declared_set_still_lands() {
        let (app, store) = connected_app();

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.signing_secret": SENTINEL,
                    "slack.one_more": SENTINEL,
                }
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
        assert_eq!(refusal["credential"], "slack.one_more", "{refusal}");
        assert!(
            store.addresses().is_empty(),
            "a body with one undeclared name must have written none of it: {:?}",
            store.addresses(),
        );

        let (status, created) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.signing_secret": SENTINEL,
                }
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(
            store.addresses(),
            vec![
                "tenants/acme/com.slack.api/bot_token".to_string(),
                "tenants/acme/com.slack.api/signing_secret".to_string(),
            ],
        );
    }

    /// A composition that bound no store refuses and names the setting, on every route. Not an
    /// empty listing, which would read as "this tenant has connected nothing" and be wrong.
    #[tokio::test]
    async fn an_unbound_credential_store_refuses_and_names_the_setting() {
        let app = storeless_app();

        for (method, path, body) in [
            (Method::GET, "/api/connections", None),
            (Method::GET, "/api/connections/zendesk", None),
            (
                Method::POST,
                "/api/connections/zendesk",
                Some(json!({ "credentials": { "zendesk.api_token": SENTINEL } })),
            ),
            (Method::DELETE, "/api/connections/zendesk", None),
        ] {
            let (status, refusal) = call(&app, "alice", method.clone(), path, body).await;

            assert_eq!(
                status,
                StatusCode::SERVICE_UNAVAILABLE,
                "{method} {path}: {refusal}",
            );
            assert_eq!(refusal["setting"], STORE_SETTING, "{refusal}");
        }
    }

    /// A store that cannot answer is `503`, never `404`. `StoreError`'s own documentation says so:
    /// an outage reported as "you have not connected that integration" is an operator reconnecting
    /// an integration that was fine.
    #[tokio::test]
    async fn an_unreachable_store_is_not_reported_as_not_connected() {
        let (app, store) = connected_app();

        let (status, _) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED);

        store.unreachable();

        for (method, path) in [
            (Method::GET, "/api/connections"),
            (Method::GET, "/api/connections/zendesk"),
            (Method::DELETE, "/api/connections/zendesk"),
        ] {
            let (status, refusal) = call(&app, "alice", method.clone(), path, None).await;
            assert_eq!(
                status,
                StatusCode::SERVICE_UNAVAILABLE,
                "{method} {path}: an unreachable store must not read as not-connected: {refusal}",
            );
        }
    }

    /// A store failure keeps its **kind** out to the caller, because the three are answered in
    /// three different places: a retry, an operator restoring this host's access to the store, and
    /// a defect. `AGENTS.md` § Conventions — failures an operator answers differently must stay
    /// distinguishable, and none of them may read as `404`.
    #[tokio::test]
    async fn a_store_failure_keeps_its_kind_out_to_the_caller() {
        for (failure, expected, must_say) in [
            (
                Failure::Unreachable,
                StatusCode::SERVICE_UNAVAILABLE,
                "Retrying may work",
            ),
            (
                Failure::Denied,
                StatusCode::BAD_GATEWAY,
                "Retrying will not help",
            ),
            (
                Failure::Backend,
                StatusCode::BAD_GATEWAY,
                "Retrying will not help",
            ),
        ] {
            let (app, store) = connected_app();
            store.fail_with(failure);

            let (status, refusal) =
                call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;

            assert_eq!(status, expected, "{failure:?}: {refusal}");
            assert!(
                refusal["error"]
                    .as_str()
                    .expect("a reason")
                    .contains(must_say),
                "{failure:?} must tell the operator whether a retry is worth anything: {refusal}",
            );
            // The store's own reason names this host's paths and access, so it goes to the log.
            assert!(
                !refusal
                    .to_string()
                    .contains("the test store was told to fail this way"),
                "the store's own reason must not reach the caller: {refusal}",
            );
        }
    }

    /// A store that fails half way through a multi-credential write leaves **nothing** behind, so
    /// a retry is not blocked by this surface's own `409`.
    ///
    /// Without the rollback, credential 1 is stored and credential 2 is not: the caller sees a
    /// failure while the connection now exists as far as `create` is concerned, and every retry is
    /// refused until somebody works out that a `DELETE` is needed first.
    #[tokio::test]
    async fn a_write_that_fails_half_way_leaves_nothing_behind() {
        let (app, store) = connected_app();
        store.allow_only(1);

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.signing_secret": SENTINEL,
                }
            })),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
        assert_eq!(refusal["left_behind"], Value::Null, "{refusal}");
        assert!(
            store.addresses().is_empty(),
            "the value written before the failure must have been taken back out: {:?}",
            store.addresses(),
        );

        // And the proof that this is what matters: once the store is working again, the retry is
        // not refused by our own `409`. Without the rollback the leftover value would have made
        // this a `409` that only a `DELETE` could clear.
        store.recovers();
        let (status, _) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({ "credentials": { "slack.bot_token": SENTINEL } })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "a retry after a rolled-back failure must not hit AlreadyConnected",
        );
    }

    /// When the rollback fails too, the refusal says so and names the addresses — never the values.
    /// A refusal claiming nothing was written while something was is the answer that costs somebody
    /// an afternoon.
    #[tokio::test]
    async fn a_rollback_that_fails_is_admitted_and_the_addresses_named() {
        let (app, store) = connected_app();
        store.allow_only(1);
        store.deletes_fail();

        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.signing_secret": SENTINEL,
                }
            })),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
        assert_eq!(
            refusal["left_behind"],
            json!(["tenants/acme/com.slack.api/bot_token"]),
            "{refusal}",
        );
        assert!(
            refusal["error"]
                .as_str()
                .expect("a reason")
                .contains("DELETE /api/connections/slack"),
            "the refusal must say what to do about it: {refusal}",
        );
        assert!(
            !refusal.to_string().contains(SENTINEL),
            "still addresses and never values: {refusal}",
        );
    }

    /// **X-20's failing-first test.** A create the store *refuses* answers with that refusal's
    /// kind, the way a partly-failed delete has since X-18.
    ///
    /// `partly_written` flattened every kind to `503` "Retrying may work", so a create refused
    /// because the store denied **this host's own** access told the operator to retry — the one
    /// thing that cannot resolve it — instead of sending them to fix the permission. `AGENTS.md`
    /// § Conventions: failures an operator answers differently stay distinguishable.
    ///
    /// Both halves of the report are driven, because the kind has to survive whether or not the
    /// rollback succeeded, and `globex` holds the same connector throughout so the disclosure
    /// assertions at the end have something they could have leaked.
    #[tokio::test]
    async fn a_create_the_store_refuses_keeps_its_kind_out_to_the_caller() {
        const BOT_TOKEN: &str = "tenants/acme/com.slack.api/bot_token";

        async fn connect_slack(app: &Router, handle: &str) -> (StatusCode, Value) {
            call(
                app,
                handle,
                Method::POST,
                "/api/connections/slack",
                Some(json!({
                    "credentials": {
                        "slack.bot_token": SENTINEL,
                        "slack.signing_secret": SENTINEL,
                    }
                })),
            )
            .await
        }

        // `Denied` first, because it is the kind this test exists for: the one an operator answers
        // by restoring this host's access, and the one that read as a transient before X-20.
        for (failure, expected, must_say) in [
            (
                Failure::Denied,
                StatusCode::BAD_GATEWAY,
                "Retrying will not help; an operator has to restore this host's access to the \
                 store",
            ),
            (
                Failure::Unreachable,
                StatusCode::SERVICE_UNAVAILABLE,
                "Retrying may work",
            ),
            (
                Failure::Backend,
                StatusCode::BAD_GATEWAY,
                "Retrying will not help; this is a defect in the store or in how it is configured",
            ),
        ] {
            // The rollback lands: nothing is left behind, and the kind still decides the answer.
            let (app, store) = connected_app();
            assert_eq!(connect_slack(&app, "bob").await.0, StatusCode::CREATED);
            store.allow_only_failing_with(1, failure);

            let (status, refusal) = connect_slack(&app, "alice").await;

            assert_eq!(status, expected, "{failure:?}: {refusal}");
            assert_eq!(
                refusal["left_behind"],
                Value::Null,
                "{failure:?}: {refusal}"
            );
            assert!(
                refusal["error"]
                    .as_str()
                    .expect("a reason")
                    .contains(must_say),
                "{failure:?} must tell the operator whether a retry is worth anything: {refusal}",
            );

            // And a rollback that fails too does not flatten it back: the addresses are still
            // named, and so is the kind.
            let (app, store) = connected_app();
            assert_eq!(connect_slack(&app, "bob").await.0, StatusCode::CREATED);
            store.allow_only_failing_with(1, failure);
            store.deletes_fail();

            let (status, refusal) = connect_slack(&app, "alice").await;

            assert_eq!(status, expected, "{failure:?}: {refusal}");
            assert_eq!(
                refusal["left_behind"],
                json!([BOT_TOKEN]),
                "{failure:?}: {refusal}",
            );
            let reason = refusal["error"].as_str().expect("a reason");
            assert!(
                reason.contains(must_say),
                "{failure:?} must tell the operator whether a retry is worth anything: {refusal}",
            );
            assert!(
                reason.contains("DELETE /api/connections/slack"),
                "and still what to do about what was left behind: {refusal}",
            );

            // The disclosure guarantees this surface owes every caller, unchanged: an address,
            // never a value, and never another tenant's anything.
            let rendered = refusal.to_string();
            assert!(
                !rendered.contains(SENTINEL),
                "a refusal names the address, never the value: {rendered}",
            );
            assert!(
                !rendered.contains("globex"),
                "a refusal must not name another tenant's address: {rendered}",
            );
        }
    }

    /// The three sentences a store failure says to a caller, **byte for byte**.
    ///
    /// `store_failure` is read by three refusals now rather than one, and the cheapest way to
    /// break a shared mapping is to reword it while working on one of its callers — a refusal
    /// quietly reworded is a regression even when it reads better.
    /// [`a_store_failure_keeps_its_kind_out_to_the_caller`] asserts the property; this asserts the
    /// words, so a refactor of the create side cannot restate the delete side's answer.
    #[tokio::test]
    async fn a_store_failure_says_what_it_has_always_said() {
        for (failure, expected) in [
            (
                Failure::Unreachable,
                "the credential store did not answer, so this host cannot say what this tenant \
                 has connected. Retrying may work",
            ),
            (
                Failure::Denied,
                "the credential store refused this host's own access, so it cannot reach this \
                 tenant's credentials. Retrying will not help; an operator has to restore this \
                 host's access to the store",
            ),
            (
                Failure::Backend,
                "the credential store answered with something this host cannot interpret. \
                 Retrying will not help; this is a defect in the store or in how it is configured",
            ),
        ] {
            let (app, store) = connected_app();
            store.fail_with(failure);

            let (_, refusal) =
                call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;

            assert_eq!(refusal["error"], expected, "{failure:?}");
        }
    }

    /// **X-18's failing-first test.** A `DELETE` whose second credential deletion fails names what
    /// it destroyed and what is still held, instead of a generic `store_failed`.
    ///
    /// Rollback is not available in this direction — a destroyed credential cannot be put back,
    /// because this host never held the plaintext to restore — so the whole of what the refusal can
    /// do is be honest. Before X-18 this answered a bare `503` "Retrying may work" while a live
    /// vendor credential sat on disk, in the case a `DELETE` exists for: revoking a leaked secret.
    ///
    /// The whole delete is asserted **in the same run**, first, so the reporting cannot pass by
    /// breaking delete. A second tenant holds the same connector throughout, so the disclosure
    /// assertions at the end have something they could have leaked.
    #[tokio::test]
    async fn a_delete_that_fails_half_way_names_what_it_destroyed_and_what_is_still_held() {
        const BOT_TOKEN: &str = "tenants/acme/com.slack.api/bot_token";
        const SIGNING_SECRET: &str = "tenants/acme/com.slack.api/signing_secret";

        async fn connect_slack(app: &Router, handle: &str) -> StatusCode {
            call(
                app,
                handle,
                Method::POST,
                "/api/connections/slack",
                Some(json!({
                    "credentials": {
                        "slack.bot_token": SENTINEL,
                        "slack.signing_secret": SENTINEL,
                    }
                })),
            )
            .await
            .0
        }

        let (app, store) = connected_app();

        // `globex` holds the same connector for the whole test.
        assert_eq!(connect_slack(&app, "bob").await, StatusCode::CREATED);
        assert_eq!(connect_slack(&app, "alice").await, StatusCode::CREATED);

        // A `DELETE` that succeeds entirely is unchanged: `204`, and nothing of this tenant's held.
        let (status, body) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/slack",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
        assert_eq!(
            store.addresses(),
            vec![
                "tenants/globex/com.slack.api/bot_token".to_string(),
                "tenants/globex/com.slack.api/signing_secret".to_string(),
            ],
            "a whole delete holds nothing back",
        );

        // The same connection again, with the second of its two deletions made to fail.
        assert_eq!(connect_slack(&app, "alice").await, StatusCode::CREATED);
        store.allow_only_deletes(1);

        let (status, refusal) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/slack",
            None,
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
        assert_eq!(
            refusal["destroyed"],
            json!([BOT_TOKEN]),
            "the refusal must name what it already destroyed: {refusal}",
        );
        assert_eq!(
            refusal["left_behind"],
            json!([SIGNING_SECRET]),
            "and what this host could not destroy, in the same vocabulary a failed create uses: \
             {refusal}",
        );
        assert!(
            refusal["error"]
                .as_str()
                .expect("a reason")
                .contains("DELETE /api/connections/slack"),
            "the refusal must say what to do about it: {refusal}",
        );

        // The store agrees with both halves: one credential is gone and one is still live.
        assert_eq!(
            store.at(BOT_TOKEN),
            None,
            "the destroyed credential is genuinely destroyed",
        );
        assert_eq!(
            store.at(SIGNING_SECRET),
            Some(SENTINEL.to_string()),
            "and the one named in `left_behind` is genuinely still there — which is why saying so \
             is the whole point",
        );

        // The existing disclosure guarantees, unchanged: an address, never a value, and never
        // another tenant's anything.
        let rendered = refusal.to_string();
        assert!(
            !rendered.contains(SENTINEL),
            "a refusal names the address, never the value: {rendered}",
        );
        assert!(
            !rendered.contains("globex"),
            "a refusal must not name another tenant's address: {rendered}",
        );

        // `globex`'s connection is untouched by any of it.
        assert_eq!(
            store.at("tenants/globex/com.slack.api/signing_secret"),
            Some(SENTINEL.to_string()),
        );
    }

    /// **X-29's failing-first test.** A `DELETE` whose deletions fail in *two* ways answers with
    /// the kind an operator has to act on, not the kind that happened first.
    ///
    /// `failure.get_or_insert(error)` kept the first error the loop saw. So an `Unreachable` at the
    /// first address followed by a `Denied` at the second answered `503` "Retrying may work" —
    /// while the denied address sat in that same response's `left_behind`. That is the exact
    /// misinformation X-18 and X-20 exist to end, reappearing in the one case neither covered: a
    /// loop that sees more than one kind.
    ///
    /// Driven in **both orders**, because "the worst" and "the last" are indistinguishable when the
    /// worst happens to be last — a fix that simply assigned on every error would pass half of this
    /// and fail the other half.
    #[tokio::test]
    async fn a_delete_that_fails_two_ways_reports_the_kind_an_operator_must_act_on() {
        const BOT_TOKEN: &str = "tenants/acme/com.slack.api/bot_token";
        const SIGNING_SECRET: &str = "tenants/acme/com.slack.api/signing_secret";

        async fn connect_slack(app: &Router) -> StatusCode {
            call(
                app,
                "alice",
                Method::POST,
                "/api/connections/slack",
                Some(json!({
                    "credentials": {
                        "slack.bot_token": SENTINEL,
                        "slack.signing_secret": SENTINEL,
                    }
                })),
            )
            .await
            .0
        }

        // Each order, and its answer — which is the same answer both ways round, because the order
        // the loop met them in is not supposed to reach the caller at all. The advice is what
        // distinguishes the two `502` kinds from each other; the status is what distinguishes both
        // of them from the transient.
        const RESTORE_ACCESS: &str =
            "Retrying will not help; an operator has to restore this host's access to the store";
        const REPAIR_THE_STORE: &str =
            "Retrying will not help; this is a defect in the store or in how it is configured";

        for (first, second, advice) in [
            // The story's reproduction, and it in reverse — so that "the worst" cannot be
            // satisfied by an implementation that merely keeps the last.
            (Failure::Unreachable, Failure::Denied, RESTORE_ACCESS),
            (Failure::Denied, Failure::Unreachable, RESTORE_ACCESS),
            // The second tier of the order: two kinds that already share `502` and "retrying will
            // not help", settled towards the one that admits less.
            (Failure::Denied, Failure::Backend, REPAIR_THE_STORE),
            (Failure::Backend, Failure::Denied, REPAIR_THE_STORE),
        ] {
            let (app, store) = connected_app();
            assert_eq!(connect_slack(&app).await, StatusCode::CREATED);

            // The loop walks the declared order, so `bot_token` is the failure that happens first.
            store.delete_fails_at(BOT_TOKEN, first);
            store.delete_fails_at(SIGNING_SECRET, second);

            let (status, refusal) = call(
                &app,
                "alice",
                Method::DELETE,
                "/api/connections/slack",
                None,
            )
            .await;

            assert_eq!(
                status,
                StatusCode::BAD_GATEWAY,
                "a failure an operator has to act on is not answered as a transient, whichever \
                 address it happened at ({first:?} then {second:?}): {refusal}",
            );
            assert!(
                refusal["error"]
                    .as_str()
                    .expect("a reason")
                    .contains(advice),
                "and the advice is the worst kind's rather than the first kind's ({first:?} then \
                 {second:?}): {refusal}",
            );

            // Both halves still tell the truth: nothing was destroyed, and neither address can be
            // called empty.
            assert_eq!(refusal["destroyed"], json!([]), "{refusal}");
            assert_eq!(
                refusal["left_behind"],
                json!([BOT_TOKEN, SIGNING_SECRET]),
                "every address whose delete failed is still named, whatever kind it failed with: \
                 {refusal}",
            );
        }
    }

    /// **`left_behind` says what this host knows, and no more.**
    ///
    /// A connector may legitimately hold a subset of what it declares —
    /// [`a_connection_may_carry_a_subset_of_what_is_declared`] — so an address whose delete failed
    /// may never have held anything. The refusal nonetheless said flatly to "treat those as still
    /// usable by anyone holding them", where the sibling [`partly_written`] hedges with "Some
    /// credentials **may** remain".
    ///
    /// The list itself is deliberately **not** narrowed, and this pins that too: a failed delete is
    /// exactly the case where this host cannot say the address is empty, so dropping the addresses
    /// the probe did not see would turn "possibly still live" into "not mentioned", which on a
    /// revocation surface reads as gone. What changes is the claim, not the list — and the
    /// instruction to treat every named address as live survives it.
    #[tokio::test]
    async fn left_behind_hedges_about_an_address_this_host_never_saw_a_value_at() {
        const BOT_TOKEN: &str = "tenants/acme/com.slack.api/bot_token";
        const SIGNING_SECRET: &str = "tenants/acme/com.slack.api/signing_secret";

        let (app, store) = connected_app();

        // Connected with one of the two credentials slack declares, so the second address has
        // never held anything at all.
        let (status, body) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({ "credentials": { "slack.bot_token": SENTINEL } })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        store.delete_fails_at(SIGNING_SECRET, Failure::Unreachable);

        let (status, refusal) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/slack",
            None,
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
        assert_eq!(refusal["destroyed"], json!([BOT_TOKEN]), "{refusal}");
        assert_eq!(
            refusal["left_behind"],
            json!([SIGNING_SECRET]),
            "the address is still named — narrowing the list to what the probe saw is the \
             under-report this surface must never make: {refusal}",
        );

        // And nothing was ever there, which is the whole point: the refusal is talking about an
        // address this host has no evidence about either way.
        assert_eq!(
            store.at(SIGNING_SECRET),
            None,
            "the reproduction is only interesting if the address is genuinely empty",
        );

        let reason = refusal["error"].as_str().expect("a reason");
        assert!(
            reason.contains("a credential may remain at any of them"),
            "the refusal must hedge about `left_behind` rather than assert it, the way \
             `partly_written` does: {reason}",
        );
        assert!(
            reason.contains("still usable by anyone holding it"),
            "and the safe instruction must survive the hedge — this is a revocation surface, so \
             the operator is still told to treat every named address as live: {reason}",
        );
    }

    /// **The race the `409` has to survive.** Two concurrent `POST`s for one tenant and one
    /// connector, on a multi-threaded runtime, with the window between the probe and the write held
    /// open.
    ///
    /// Before the claim in `create`, this reproduced on attempt 0: two `201`s, one value silently
    /// replaced, and *the caller that lost was told it succeeded* — which is the exact failure the
    /// `409` exists to prevent, so leaving the window open made the refusal decorative. The story's
    /// Progress note bars landing this address scheme with the silent overwrite reachable.
    ///
    /// Looped, because a race that reproduces on attempt 0 must be shown not reproducing across
    /// many. The invariant asserted is the one that matters and does not depend on who wins: exactly
    /// one caller is told it created something, and the value in the store is that caller's.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_concurrent_creates_cannot_both_succeed() {
        const ATTEMPTS: usize = 500;
        const ADDRESS: &str = "tenants/acme/com.zendesk.api/api_token";

        for attempt in 0..ATTEMPTS {
            let (app, store) = connected_app();
            store.widen_the_window();

            let mut racers = Vec::new();
            for value in ["FIRST-VALUE", "SECOND-VALUE"] {
                let app = app.clone();
                racers.push(tokio::spawn(async move {
                    let (status, _) = call(
                        &app,
                        "alice",
                        Method::POST,
                        "/api/connections/zendesk",
                        Some(json!({ "credentials": { "zendesk.api_token": value } })),
                    )
                    .await;
                    (value, status)
                }));
            }

            let mut outcomes = Vec::new();
            for racer in racers {
                outcomes.push(racer.await.expect("neither task panics"));
            }

            let created: Vec<&str> = outcomes
                .iter()
                .filter(|(_, status)| *status == StatusCode::CREATED)
                .map(|(value, _)| *value)
                .collect();
            let refused = outcomes
                .iter()
                .filter(|(_, status)| *status == StatusCode::CONFLICT)
                .count();

            assert_eq!(
                created.len(),
                1,
                "attempt {attempt}: exactly one caller may be told it created a connection, and \
                 these were: {outcomes:?}",
            );
            assert_eq!(
                refused, 1,
                "attempt {attempt}: the other caller must be refused with a conflict: {outcomes:?}",
            );
            assert_eq!(
                store.at(ADDRESS),
                Some(created[0].to_string()),
                "attempt {attempt}: the stored value must be the one the caller that got 201 sent \
                 — anything else is a lost update reported as a success",
            );
        }
    }

    /// Occupy `bytes` of this tenant's allowance, written straight into the store, leaving every
    /// connector in `except` empty.
    ///
    /// Spread over as many addresses as it takes at the per-value bound, because a tenant cannot
    /// reach 56 KiB through the surface any other way: no catalogued connector declares seven
    /// credentials.
    fn occupy(store: &TestStore, tenant: &Tenant, bytes: usize, except: &[&str]) {
        let mut remaining = bytes;

        for provider in connector_catalog::providers() {
            if except.contains(&provider.id) {
                continue;
            }

            let declared = declared_credentials(provider);
            let Ok(addresses) = declaration(provider, &declared).addresses(tenant) else {
                continue;
            };

            for (_, reference) in &addresses {
                if remaining == 0 {
                    return;
                }
                let chunk = remaining.min(MAX_CREDENTIAL_VALUE_BYTES);
                store.place(address_path(reference), chunk);
                remaining -= chunk;
            }
        }

        assert_eq!(
            remaining, 0,
            "this catalogue has too few addresses to seat {bytes} bytes for one tenant",
        );
    }

    /// **The race the per-tenant allowance has to survive.** One tenant, two concurrent `POST`s to
    /// *different* connectors, each individually admissible and the two together past
    /// [`MAX_TENANT_STORE_BYTES`].
    ///
    /// The allowance is a read-decide-write too — read what the tenant occupies, decide, write —
    /// and X-22 left it covered only by the `(tenant, connector)` claim, which two different
    /// connectors do not share. So before X-25 both callers read the same 56 KiB, both were
    /// admitted, both wrote, and the tenant ended up 8 KiB past an allowance whose entire purpose
    /// is that no tenant can spend more of the shared file than it was given.
    ///
    /// The second half asserts the fix is not a lock over the surface: two *different* tenants
    /// creating at the same moment both still get their `201`. That is the property X-10 pinned and
    /// the reason this is a tenant-scoped claim rather than a global one — shared fate between
    /// tenants, in the repository whose whole point is that they share nothing.
    ///
    /// Looped, with the window between probe and write held open by the test store, on the
    /// precedent of [`two_concurrent_creates_cannot_both_succeed`].
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_tenants_concurrent_creates_cannot_overshoot_its_allowance() {
        const ATTEMPTS: usize = 200;

        // What each racer adds, and therefore the headroom left for it. Either fits exactly;
        // the two together are one whole credential past the allowance.
        const RACER_BYTES: usize = MAX_CREDENTIAL_VALUE_BYTES;

        let acme = Tenant::new("acme").expect("a plain tenant id");

        for attempt in 0..ATTEMPTS {
            let (app, store) = connected_app();
            occupy(
                &store,
                &acme,
                MAX_TENANT_STORE_BYTES - RACER_BYTES,
                &["zendesk", "slack"],
            );
            store.widen_the_window();

            let half = "h".repeat(RACER_BYTES / 2);
            let racers = vec![
                tokio::spawn({
                    let app = app.clone();
                    let whole = "w".repeat(RACER_BYTES);
                    async move {
                        call(
                            &app,
                            "alice",
                            Method::POST,
                            "/api/connections/zendesk",
                            Some(json!({ "credentials": { "zendesk.api_token": whole } })),
                        )
                        .await
                        .0
                    }
                }),
                tokio::spawn({
                    let app = app.clone();
                    async move {
                        call(
                            &app,
                            "alice",
                            Method::POST,
                            "/api/connections/slack",
                            Some(json!({
                                "credentials": {
                                    "slack.bot_token": half,
                                    "slack.signing_secret": half,
                                }
                            })),
                        )
                        .await
                        .0
                    }
                }),
            ];

            let mut outcomes = Vec::new();
            for racer in racers {
                outcomes.push(racer.await.expect("neither task panics"));
            }

            // The thing the bound exists to protect: this tenant's share of the one file every
            // other tenant's write has to rewrite.
            let occupied = store.bytes_under("tenants/acme/");
            assert!(
                occupied <= MAX_TENANT_STORE_BYTES,
                "attempt {attempt}: one tenant occupies {occupied} bytes, past the \
                 {MAX_TENANT_STORE_BYTES} it may hold, having sent two creates that were each \
                 admissible on their own: {outcomes:?}",
            );

            // And the other half of "bounded": exactly one of them was admitted. Refusing both
            // would hold the bound by refusing work that fits, which is not the same property.
            let created = outcomes
                .iter()
                .filter(|status| **status == StatusCode::CREATED)
                .count();
            assert_eq!(
                created, 1,
                "attempt {attempt}: one of two creates that cannot both fit must still land: \
                 {outcomes:?}",
            );
        }

        // Two tenants, the same moment, and neither waits on the other.
        for attempt in 0..ATTEMPTS {
            let (app, store) = connected_app();
            store.widen_the_window();

            let mut racers = Vec::new();
            for handle in ["alice", "bob"] {
                let app = app.clone();
                racers.push(tokio::spawn(async move {
                    (handle, connect_zendesk(&app, handle).await.0)
                }));
            }

            let mut outcomes = Vec::new();
            for racer in racers {
                outcomes.push(racer.await.expect("neither task panics"));
            }

            for (handle, status) in &outcomes {
                assert_eq!(
                    *status,
                    StatusCode::CREATED,
                    "attempt {attempt}: {handle} was made to wait on another tenant's create, \
                     which is the shared fate the claim is scoped per tenant to avoid: \
                     {outcomes:?}",
                );
            }
            assert_eq!(
                store.addresses(),
                vec![
                    "tenants/acme/com.zendesk.api/api_token".to_string(),
                    "tenants/globex/com.zendesk.api/api_token".to_string(),
                ],
                "attempt {attempt}: both tenants' values must be in the store",
            );
        }
    }

    /// The other side of the same claim: a `DELETE` racing a `POST` cannot destroy half of a
    /// connection being written.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_delete_racing_a_create_leaves_the_connection_whole_or_absent() {
        const ATTEMPTS: usize = 200;

        for attempt in 0..ATTEMPTS {
            let (app, store) = connected_app();
            store.widen_the_window();

            let creating = tokio::spawn({
                let app = app.clone();
                async move {
                    call(
                        &app,
                        "alice",
                        Method::POST,
                        "/api/connections/slack",
                        Some(json!({
                            "credentials": {
                                "slack.bot_token": SENTINEL,
                                "slack.signing_secret": SENTINEL,
                            }
                        })),
                    )
                    .await
                    .0
                }
            });
            let deleting = tokio::spawn({
                let app = app.clone();
                async move {
                    call(
                        &app,
                        "alice",
                        Method::DELETE,
                        "/api/connections/slack",
                        None,
                    )
                    .await
                    .0
                }
            });

            let created = creating.await.expect("no panic");
            let deleted = deleting.await.expect("no panic");

            // Whatever order they landed in, the connection is either both credentials or neither.
            // A single stored credential is a connection an operator cannot tell from a whole one.
            let addresses = store.addresses();
            assert!(
                addresses.len() != 1,
                "attempt {attempt}: a half-written connection survived (create={created}, \
                 delete={deleted}): {addresses:?}",
            );
        }
    }

    /// **Name the address, never the value.** Driven with a value stored, and the value appears in
    /// none of the answers below.
    ///
    /// Written over the *shape* of the whole body rather than over the fields somebody remembered
    /// to check, so a field added later cannot quietly start carrying one.
    ///
    /// **What it does and does not reach.** This claimed to drive *every* answer and refusal the
    /// module can produce, and three stories in a row (X-20, X-25, X-29) found that it did not — so
    /// the claim is now the list. Driven here: both listings, `show`, the unknown-connector and
    /// undeclared-credential refusals, the `409` for a second connection, both partial-failure
    /// refusals with their address lists, both size refusals, and a store failure. **Not driven:**
    /// [`allowance_change_in_flight`], which needs a tenant-wide claim held across a request from
    /// another task — machinery this test has none of, and the one refusal here that names no
    /// address at all, only a connector id. A test that admits its gap is worth more than one whose
    /// doc has to be re-checked against the module every time a refusal is added.
    #[tokio::test]
    async fn no_answer_or_refusal_carries_a_credential_value() {
        let (app, store) = connected_app();

        let (_, created) = connect_zendesk(&app, "alice").await;

        let mut answers = vec![created];
        for (method, path, body) in [
            (Method::GET, "/api/connections", None),
            (Method::GET, "/api/connections/zendesk", None),
            (
                // The X-14 refusal, which quotes an address and must not quote a value.
                Method::POST,
                "/api/connections/zendesk",
                Some(json!({ "credentials": { "zendesk.api_token": SENTINEL } })),
            ),
            (Method::GET, "/api/connections/no-such-vendor", None),
            (
                Method::POST,
                "/api/connections/slack",
                Some(json!({ "credentials": { "slack.nope": SENTINEL } })),
            ),
        ] {
            let (_, body) = call(&app, "alice", method, path, body).await;
            answers.push(body);
        }

        // X-18's refusal, which quotes two lists of addresses and must quote no value. Armed here
        // rather than in the table above because it needs the store told to fail mid-loop.
        let (_, partly_destroyed) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({
                "credentials": {
                    "slack.bot_token": SENTINEL,
                    "slack.signing_secret": SENTINEL,
                }
            })),
        )
        .await;
        answers.push(partly_destroyed);
        store.allow_only_deletes(1);
        let (status, partly_destroyed) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/slack",
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "this must be the partial-delete refusal, or the answer below proves nothing about \
             it: {partly_destroyed}",
        );
        assert!(
            partly_destroyed["left_behind"].is_array(),
            "likewise: {partly_destroyed}",
        );
        answers.push(partly_destroyed);

        // X-20's refusal, in **both** its branches — the gap X-20 recorded and did not close. Each
        // needs its own app, because arming a store to fail its writes is not something the
        // requests above can be run through afterwards.
        for rollback_fails in [false, true] {
            let (half_written_app, half_written_store) = connected_app();
            half_written_store.allow_only(1);
            if rollback_fails {
                half_written_store.deletes_fail();
            }

            let (status, partly_written) = call(
                &half_written_app,
                "alice",
                Method::POST,
                "/api/connections/slack",
                Some(json!({
                    "credentials": {
                        "slack.bot_token": SENTINEL,
                        "slack.signing_secret": SENTINEL,
                    }
                })),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::SERVICE_UNAVAILABLE,
                "this must be the half-written refusal, or the answer below proves nothing about \
                 it: {partly_written}",
            );
            assert_eq!(
                partly_written["left_behind"].is_array(),
                rollback_fails,
                "and it must be the branch this iteration armed: {partly_written}",
            );
            answers.push(partly_written);
        }

        // X-22's two refusals, which quote sizes and must quote no value. The value they are
        // refusing is built out of the sentinel so that a refusal echoing any part of what was
        // sent is caught, not merely one echoing the whole of it.
        let (bounded_app, _) = connected_app();
        let oversized = SENTINEL.repeat(MAX_CREDENTIAL_VALUE_BYTES);
        let (status, too_large) = call(
            &bounded_app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": oversized } })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "this must be the per-value refusal, or the answer below proves nothing about it: \
             {too_large}",
        );
        answers.push(too_large);

        // And the per-tenant one, reached by filling the allowance with values at the per-value
        // bound until it bites.
        let at_the_value_bound = SENTINEL.repeat(MAX_CREDENTIAL_VALUE_BYTES / SENTINEL.len());
        let tenant = Tenant::new("acme").expect("a plain tenant id");
        let mut allowance_exhausted = None;
        for provider in connector_catalog::providers() {
            let declared = declared_credentials(provider);
            if declaration(provider, &declared).addresses(&tenant).is_err() {
                continue;
            }

            let credentials: serde_json::Map<String, Value> = declared
                .iter()
                .map(|credential| {
                    (
                        credential.name.to_string(),
                        json!(at_the_value_bound.clone()),
                    )
                })
                .collect();

            let (status, body) = call(
                &bounded_app,
                "alice",
                Method::POST,
                &format!("/api/connections/{}", provider.id),
                Some(json!({ "credentials": credentials })),
            )
            .await;

            if status == StatusCode::CONFLICT {
                allowance_exhausted = Some(body);
                break;
            }
        }
        answers.push(
            allowance_exhausted
                .expect("the per-tenant allowance must be reachable, or the check below is empty"),
        );

        store.unreachable();
        let (_, unreachable) =
            call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
        answers.push(unreachable);

        for answer in answers {
            assert!(
                !answer.to_string().contains(SENTINEL),
                "a credential value reached a caller: {answer}",
            );
        }
    }

    /// The Acceptance's "no route accepts an address", stated over this module's own declaration.
    ///
    /// `super::super::tests::no_published_route_takes_a_tenant_in_its_path` covers the tenant
    /// segment over the whole surface — X-03 wrote it saying X-10 would inherit it, and this is
    /// that inheritance made explicit. This one covers the rest of an address: a path parameter
    /// that could carry an authority, a credential or a rendered store path.
    #[test]
    fn no_route_here_accepts_an_address() {
        for route in MODULE.routes {
            for parameter in route
                .path
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{'))
                .filter_map(|segment| segment.strip_suffix('}'))
            {
                assert_eq!(
                    parameter, "connector",
                    "the only thing a path here may name is the connector, and `{parameter}` is \
                     not it: {}",
                    route.path,
                );
            }

            assert!(
                !route.path.contains(TENANTS_ROOT),
                "no route may quote a credential path: {}",
                route.path,
            );
        }
    }

    /// Both routes require a principal. Asserted here as well as in the surface-wide enumeration,
    /// because that one compares against a list somebody edits and this one cannot be satisfied by
    /// editing a list.
    #[test]
    fn every_route_here_requires_a_principal() {
        for route in MODULE.routes {
            assert_eq!(
                route.access,
                Access::Principal,
                "a connection is tenant data: {}",
                route.path,
            );
        }
    }

    /// What a listing actually costs, and the invariant underneath it.
    ///
    /// `GET /api/connections` derives an address for every addressable connector in the compiled-in
    /// catalogue and probes it, so the cost is one `SecretStore::get` per **address** — not per
    /// provider, since a connector may declare several credentials. `FileStore::get` is a lookup in
    /// a map read once at open, so these are map lookups rather than file reads.
    ///
    /// The assertion that matters is the second one: **no two connectors share an address for one
    /// tenant.** If two did, connecting one would show up as having connected the other, and
    /// deleting one would destroy the other's credential. Nothing upstream promises this — it falls
    /// out of the authority being per vendor — so it is pinned here rather than assumed.
    #[test]
    fn a_listing_probes_one_address_per_declared_credential_and_none_collide() {
        let tenant = Tenant::new("acme").expect("a plain tenant id");
        let mut rendered = Vec::new();
        let mut addressable = 0;

        for provider in connector_catalog::providers() {
            let declared = declared_credentials(provider);
            let declaration = declaration(provider, &declared);
            let Ok(addresses) = declaration.addresses(&tenant) else {
                continue;
            };

            addressable += 1;
            rendered.extend(addresses.iter().map(|(_, r)| address_path(r)));
        }

        let mut distinct = rendered.clone();
        distinct.sort();
        distinct.dedup();

        assert_eq!(
            rendered.len(),
            distinct.len(),
            "two connectors render the same address for one tenant, so connecting one would read \
             as connecting the other and deleting one would destroy the other's credential",
        );

        // Recorded rather than asserted to a fixed number: the catalogue is upstream's and grows.
        // The bound is what the design's cost note is written against.
        println!(
            "a listing probes {} addresses across {addressable} addressable connectors ({} in the \
             catalogue)",
            rendered.len(),
            connector_catalog::providers().len(),
        );
        assert!(
            rendered.len() < 500,
            "a listing probing {} addresses has outgrown probe-everything, and the design's cost \
             note needs revisiting",
            rendered.len(),
        );
    }

    /// The same surface against the **real** store this binary composes, rather than against the
    /// double above.
    ///
    /// Everything else in this module drives a `TestStore`, which is what lets a failure mode be
    /// asked for on demand — but it also means none of it would notice if `TenantLayout`, the
    /// addresses this host derives, and what `FileStore` actually does with them ever disagreed.
    /// These tests are the ones that would: a value written through the surface has to come back
    /// out of a store nothing here wrote by hand.
    ///
    /// `#[cfg(unix)]` because `CredentialStore` is.
    #[cfg(unix)]
    mod against_a_real_file_store {
        use super::*;

        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU64, Ordering};

        use exchange_host::CredentialStore;

        /// A scratch directory under the system temporary directory, removed on drop.
        ///
        /// Under `temp_dir` and not under the workspace, because `CredentialStore::bind` refuses a
        /// path inside a working tree — which is the rule working, not an obstacle to route around.
        struct Scratch(PathBuf);

        impl Scratch {
            fn new() -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "flux-exchange-connections-{}-{}",
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

        fn file_backed_app(store: &CredentialStore) -> Router {
            super::super::super::app(
                AppState::with_development_identity(Arc::new(
                    DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
                ))
                .with_credentials(store.secrets()),
            )
        }

        /// Create, list, read and delete, all the way down to a file on disk — and the credential
        /// really is gone from the store afterwards.
        #[tokio::test]
        async fn a_connection_survives_the_round_trip_through_a_real_store() {
            let scratch = Scratch::new();
            let store = scratch.store();
            let app = file_backed_app(&store);

            let (status, created) = connect_zendesk(&app, "alice").await;
            assert_eq!(status, StatusCode::CREATED, "{created}");
            assert_eq!(
                created["credentials"][0]["address"],
                "tenants/acme/com.zendesk.api/api_token",
            );

            // The address this host derived is the address the store actually used. Nothing else
            // in this module can catch a disagreement between the two.
            let written = std::fs::read_to_string(store.path()).expect("the store file is there");
            assert!(
                written.contains("tenants/acme/com.zendesk.api/api_token"),
                "the derived address must be the one the store wrote at: {written}",
            );
            assert!(
                !written.contains(SENTINEL),
                "the store encodes its values; the plaintext must not be sitting in the file",
            );

            let (status, listed) = call(&app, "alice", Method::GET, "/api/connections", None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(listed["connections"].as_array().expect("an array").len(), 1);

            // Another tenant still gets nothing, against the real store.
            let (status, _) =
                call(&app, "bob", Method::GET, "/api/connections/zendesk", None).await;
            assert_eq!(status, StatusCode::NOT_FOUND);

            let (status, _) = call(
                &app,
                "alice",
                Method::DELETE,
                "/api/connections/zendesk",
                None,
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);

            let emptied = std::fs::read_to_string(store.path()).expect("the store file is there");
            assert!(
                !emptied.contains("tenants/acme/com.zendesk.api/api_token"),
                "deleting a connection must destroy its credential in the store: {emptied}",
            );
        }

        /// The `409` against a store whose writes really do `fsync` and `rename`.
        ///
        /// Fewer attempts than the in-memory race, because each one is real IO. The claim is what
        /// holds here too — it is taken before the probe and released after the last write, so what
        /// the store does in between does not change the argument.
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn two_concurrent_creates_cannot_both_succeed_against_a_real_store() {
            const ATTEMPTS: usize = 50;

            for attempt in 0..ATTEMPTS {
                let scratch = Scratch::new();
                let store = scratch.store();
                let app = file_backed_app(&store);

                let mut racers = Vec::new();
                for value in ["FIRST-VALUE", "SECOND-VALUE"] {
                    let app = app.clone();
                    racers.push(tokio::spawn(async move {
                        let (status, _) = call(
                            &app,
                            "alice",
                            Method::POST,
                            "/api/connections/zendesk",
                            Some(json!({ "credentials": { "zendesk.api_token": value } })),
                        )
                        .await;
                        status
                    }));
                }

                let mut statuses = Vec::new();
                for racer in racers {
                    statuses.push(racer.await.expect("neither task panics"));
                }

                assert_eq!(
                    statuses
                        .iter()
                        .filter(|status| **status == StatusCode::CREATED)
                        .count(),
                    1,
                    "attempt {attempt}: exactly one caller may be told it created a connection, \
                     and these were: {statuses:?}",
                );

                // Exactly one value on disk, whichever caller won.
                let written =
                    std::fs::read_to_string(store.path()).expect("the store file is there");
                assert_eq!(
                    written
                        .lines()
                        .filter(|line| line.contains("tenants/acme/com.zendesk.api/api_token"))
                        .count(),
                    1,
                    "attempt {attempt}: the store must hold one value for one address: {written}",
                );
            }
        }
    }
}
