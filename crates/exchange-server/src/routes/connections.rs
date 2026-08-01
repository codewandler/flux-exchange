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
//! [`tests::no_answer_or_refusal_carries_a_credential_value`] drives the whole module with a
//! sentinel stored and asserts it appears in no response body. `AGENTS.md` § Invariants: name the
//! address, never the value.
//!
//! # The second connection to one connector is refused
//!
//! `tenants/<tenant>/<authority>/<credential>` has nowhere to say *which* Zendesk, so a tenant with
//! a sandbox and a production account renders one address for both and the second write would
//! silently replace the first. That is refused with `409` rather than accepted, and the refusal
//! quotes the `@instances/<uuid>` level that has landed upstream (flux-connectors C-406) and is not
//! published yet. **This refusal is the placeholder for that level** — see
//! [`Refusal::AlreadyConnected`], `exchange_host::ConnectorDeclaration::address_of_declared` for the
//! seam it is inserted at, and `docs/designs/connections.md` for the argument.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{Provider, ProviderKey};
use exchange_host::{
    address_path, ConnectionRefusal, ConnectorDeclaration, CredentialRef, DeclaredCredential,
    Principal, Secret, SecretStore, StoreError,
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
#[derive(Debug, Deserialize)]
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
            Err(error) => return store_unreachable(&error),
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
        Err(refusal) => return unaddressable(&refusal),
    };

    match held(store, &addresses).await {
        Err(error) => store_unreachable(&error),
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
        Err(refusal) => return unaddressable(&refusal),
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

    // Every name is resolved to an address **before** anything is written, so a body with one good
    // name and one typo stores neither. A half-written connection is one the operator cannot tell
    // from a working one until an operation fails.
    let mut writes: Vec<(CredentialRef, Secret)> = Vec::new();
    for (name, value) in &body.credentials {
        match declaration.address_of(principal.tenant(), name) {
            Ok(reference) => writes.push((reference, Secret::new(value))),
            Err(refusal) => return unaddressable(&refusal),
        }
    }

    match held(store, &addresses).await {
        Err(error) => return store_unreachable(&error),
        // The X-14 refusal. Checked before any write, because the whole point is that the write
        // would have been invisible.
        Ok(held) if !held.is_empty() => return already_connected(provider, &addresses),
        Ok(_) => {}
    }

    for (reference, secret) in &writes {
        if let Err(error) = store.put(reference, secret).await {
            return store_unreachable(&error);
        }
    }

    let stored: Vec<String> = body.credentials.keys().cloned().collect();
    (
        StatusCode::CREATED,
        Json(view(provider, &addresses, &stored)),
    )
        .into_response()
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
        Err(refusal) => return unaddressable(&refusal),
    };

    match held(store, &addresses).await {
        Err(error) => return store_unreachable(&error),
        // A `404` and not a `204`: deleting something that is not there is indistinguishable from
        // deleting another tenant's, and the caller should be able to tell.
        Ok(held) if held.is_empty() => return not_connected(provider, &addresses),
        Ok(_) => {}
    }

    // Every declared address, not only the ones the probe found. `SecretStore::delete` is
    // idempotent by contract, and deleting the whole set is what makes "the connection is gone"
    // true even if a value appeared between the probe and here.
    for (_, reference) in &addresses {
        if let Err(error) = store.delete(reference).await {
            return store_unreachable(&error);
        }
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

/// The connector has no address for this tenant, because something it must declare is missing.
fn unaddressable(refusal: &ConnectionRefusal) -> Response {
    refuse(
        StatusCode::UNPROCESSABLE_ENTITY,
        refusal.to_string(),
        match refusal {
            ConnectionRefusal::UndeclaredAuthority { connector }
            | ConnectionRefusal::NoCredentialDeclared { connector } => {
                json!({ "connector": connector })
            }
            ConnectionRefusal::UndeclaredCredential {
                connector,
                credential,
                declared,
            } => json!({
                "connector": connector,
                "credential": credential,
                "declared": declared,
            }),
            ConnectionRefusal::Unaddressable {
                connector,
                credential,
                ..
            } => json!({ "connector": connector, "credential": credential }),
        },
    )
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

/// The store did not answer.
///
/// `503`, never `404`. The reason names this host's own dependency, so it goes to the log rather
/// than to the caller — the same split the identity guard makes for an unreachable provider.
fn store_unreachable(error: &StoreError) -> Response {
    if error.is_not_found() {
        // Unreachable in practice: `held` filters this out. Kept because collapsing the two here
        // is exactly the mistake `StoreError` documents, and a future edit is likelier to reach
        // for this function than to re-read that.
        warn!("a not-found reached the unreachable path, which is a bug in this module");
    } else {
        error!(%error, "the credential store did not answer");
    }

    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "the credential store is unreachable, so this host cannot say what this tenant has \
         connected",
        json!({}),
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
    use exchange_host::{async_trait, TENANTS_ROOT};
    use tower::Service;

    use crate::dev_identity::DevIdentity;

    /// Two tenants, one principal each. `alice` is `acme`; `bob` is `globex`.
    const ROSTER: &str = "user:alice@acme,user:bob@globex";

    /// The value a test stores. Never a real secret, and asserted absent from every answer a
    /// different tenant receives — and from every refusal anyone receives.
    const SENTINEL: &str = "SENTINEL-NOT-A-REAL-SECRET";

    /// A store that lives in the test.
    ///
    /// Hand-rolled rather than reaching for `connector_secrets::MemoryStore`, so that
    /// `exchange_host` is not made to re-export an in-memory store a production composition could
    /// then bind — the one thing X-09 refuses. This one can also be told to stop answering, which
    /// is what pins the `503`/`404` split.
    #[derive(Default)]
    struct TestStore {
        held: Mutex<HashMap<String, String>>,
        unreachable: Mutex<bool>,
    }

    impl TestStore {
        fn unreachable(&self) {
            *self.unreachable.lock().expect("no test poisons this") = true;
        }

        fn is_unreachable(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            if *self.unreachable.lock().expect("no test poisons this") {
                return Err(StoreError::Unreachable {
                    path: address_path(reference),
                    reason: "the test store was told to stop answering".to_string(),
                });
            }
            Ok(())
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
            self.is_unreachable(reference)?;

            let path = address_path(reference);
            self.held
                .lock()
                .expect("no test poisons this")
                .get(&path)
                .map(Secret::new)
                .ok_or(StoreError::NotFound { path })
        }

        async fn put(&self, reference: &CredentialRef, secret: &Secret) -> Result<(), StoreError> {
            self.is_unreachable(reference)?;

            self.held
                .lock()
                .expect("no test poisons this")
                .insert(address_path(reference), secret.expose_secret().to_string());
            Ok(())
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            self.is_unreachable(reference)?;

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

    /// **Name the address, never the value.** Every answer and every refusal this module can
    /// produce, driven with a value stored, and the value appears in none of them.
    ///
    /// Written over the *shape* of the whole body rather than over the fields somebody remembered
    /// to check, so a field added later cannot quietly start carrying one.
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
}
