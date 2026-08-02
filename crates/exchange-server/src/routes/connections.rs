//! A tenant's connections to connectors — created, listed and destroyed at an address it cannot
//! name.
//!
//! ```text
//! GET    /api/connections               every connection this tenant holds
//! POST   /api/connections/{connector}   connect one, with the values it declares
//! GET    /api/connections/{connector}   one connection, as addresses and never as values
//! DELETE /api/connections/{connector}   disconnect, destroying every credential it holds
//! PUT    /api/connections/{connector}/credentials/{credential}
//!                                       replace one credential's value, in place
//! GET    /api/connections/{connector}/settings
//!                                       what this connector needs configured, and which are set
//! PUT    /api/connections/{connector}/settings/{service}/{field}
//!                                       supply one non-secret per-connection value
//! DELETE /api/connections/{connector}/settings/{service}/{field}
//!                                       unset one
//! ```
//!
//! # The settings half, and why it is a second store rather than more credentials (X-47)
//!
//! Seventeen of the shipped connectors declare a per-connection value their operations need — a
//! vendor subdomain, a workspace slug, a space id — and until X-47 there was nowhere for a tenant to
//! put one, so `invoke` bound an empty configuration and every one of them refused by name. The
//! three routes above are that home.
//!
//! **They do not write to the credential store, and that is the decision rather than the plumbing.**
//! A subdomain is not a secret; it is in the URL of every request the connector makes. Storing one
//! beside an API token would make [`view`]'s `held` mean two things at once — a connection with a
//! subdomain and no token would report as held — and would spend a tenant's *credential* allowance,
//! whose whole argument is about the latency of the one file every credential write rewrites. So
//! they are two stores with two allowances, and `exchange_host::settings` carries the argument.
//! [`tests::a_setting_is_not_a_credential_and_does_not_land_in_the_credential_store`] is what holds
//! it to being true rather than intended.
//!
//! `{service}` and `{field}` are catalogue keys exactly as `{connector}` and `{credential}` are:
//! looked up in what the connector's own operations declare and refused when nothing declares them,
//! never a segment of anything. The service is a **required** path segment rather than defaulting to
//! `default`, because `contentful` declares `endpoint.space_id` under two services and a value
//! silently filed under the wrong one is a management write into a space nobody named.
//!
//! **Values go in and do not come back out**, as everywhere else on this surface: `GET` answers with
//! `binds` targets and a `set` boolean. That is stricter than the "not a secret" argument requires,
//! and it is the direction that cannot be wrong — a `username` field holds an account name or an
//! email address, which is a customer's personal data whatever the field is called.
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
//! Every route here requires a principal. A connection is tenant data and there is no version of it
//! that answers a caller this host has not identified, so this module adds nothing to the anonymous
//! set that `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous` enumerates.
//!
//! # Who may touch a connection, decided per route (X-47, X-54)
//!
//! Every route here requires a principal, and three of them require one of a particular **kind**.
//! The division is *writing a credential or the value that steers where it goes* against
//! *everything else*, and it is declared as data on each [`Route`] rather than checked inside a
//! handler, so `super::tests::the_kind_gated_surface_is_only_what_was_declared` can walk it:
//!
//! | Route | Who | Why, in one line |
//! | --- | --- | --- |
//! | `GET /api/connections` | any kind | addresses and a `held` boolean, never a value |
//! | `GET /api/connections/{connector}` | any kind | the same, for one connector |
//! | `POST /api/connections/{connector}` | a `User` | it decides which credential this tenant's operations run under |
//! | `DELETE /api/connections/{connector}` | any kind | visible, undoable, and nothing survives revoking the token that did it |
//! | `PUT .../credentials/{credential}` | a `User` | the same substitution as `POST`, and invisible — it replaces in place |
//! | `GET .../settings` | any kind | `binds` targets and a `set` boolean, never a value |
//! | `PUT`/`DELETE .../settings/{service}/{field}` | a `User` | the value is substituted into the operation's own request |
//!
//! [`MAY_SUPPLY_A_CREDENTIAL`] and [`MAY_CONFIGURE`] carry the arguments, including why `Service` is
//! refused alongside `Agent` and the within-tenant gap **neither** of them closes — there is no
//! operator kind, so *a human, not a bot* is the strongest thing a kind gate can say.
//!
//! **`POST` shares a path with `GET` and `DELETE` and does not share their access**, so that path
//! is declared twice in [`MODULE`] — once for the open verbs and once for the gated one. The
//! alternative was a check inside [`create`], which is the "a route is guarded by its handler
//! remembering to ask" that [`Access`] exists to refuse.
//!
//! The reads stay open to every kind deliberately. They answer targets, addresses and booleans and
//! no values at all, and an agent that can read *"this connection is missing `endpoint.subdomain`"*
//! is one that can say so to the human who can supply it. Reading what a connection needs is any
//! principal's business; writing a value into it is not.
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
//! # Rotation is not an upsert, and is not reachable from the create path
//!
//! `POST` refuses a connection that already exists and **that refusal is unchanged**: an upsert is
//! a create that does not know whether it is replacing something, and the silent overwrite it
//! produces is the thing this whole module exists to prevent. A rotation is the opposite statement
//! — an operator saying *replace this, I know it is there* — so [`rotate`] refuses when the value
//! is **not** there ([`not_connected`], [`nothing_to_rotate`]), which is exactly where `POST`
//! writes.
//!
//! The two are kept apart structurally rather than by a flag, so nothing reaches a replacement by
//! accident from a create: a different **path**, a different **method**, and a different **body**,
//! all three of which have to be right at once. `{"credentials": {…}}` sent to the rotation route
//! does not deserialise, and `{"value": "…"}` sent to `POST` does not either.
//!
//! **It replaces one credential, not the declared set.** A connector may declare several — `slack`
//! declares two — and the wholesale form would require a caller to re-send every value it wants to
//! keep. This host never hands a credential value back out, so an operator rotating one of two has
//! no way to obtain the other, and a wholesale rotation carrying only what they hold would destroy
//! the rest. A surface whose safe use needs values read back out cannot exist on the host whose
//! north star is that the credential never crosses the boundary. So the operation is per
//! credential, an unmentioned credential is untouched, and rotating several is several requests —
//! which is also the granularity a leak has, since it is one secret that leaks.
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
use axum::routing::{get, post, put, MethodRouter};
use axum::{Extension, Json};
use connector_catalog::{Provider, ProviderKey};
use exchange_host::{
    address_path, admit_tenant_occupancy, declared_settings, host_pinning, stored_bytes,
    ConnectionRefusal, ConnectorDeclaration, CredentialRef, DeclaredCredential, DeclaredSetting,
    HostPinning, Principal, PrincipalKind, Secret, SecretStore, SettingsRefusal, StoreError,
    Tenant,
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
///
/// `pub(super)` since X-12: [`invoke`](super::invoke) refuses in the same terms when no store is
/// bound, and one setting quoted from two places would be two strings to keep in step.
#[cfg(unix)]
pub(super) const STORE_SETTING: &str = exchange_host::CREDENTIAL_STORE_SETTING;
/// The same, where the file store does not exist. Only `FileStore` is `#[cfg(unix)]`; the port is
/// not, so a composition on another platform binds its own store rather than this one.
#[cfg(not(unix))]
pub(super) const STORE_SETTING: &str = "FLUX_EXCHANGE_CREDENTIALS";

/// The setting that names the connection-settings store, quoted when none is bound.
///
/// Spelled through the host's own constant for [`STORE_SETTING`]'s reason. `pub(super)` for its
/// reason too: [`invoke`](super::invoke) points a refused invocation at this surface, and one
/// setting quoted from two places would be two strings to keep in step.
#[cfg(unix)]
pub(super) const SETTINGS_SETTING: &str = exchange_host::CONNECTION_SETTINGS_SETTING;
/// The same, where the file store does not exist. Only the file binding is `#[cfg(unix)]`; the port
/// is not, so a composition on another platform binds its own store rather than this one.
#[cfg(not(unix))]
pub(super) const SETTINGS_SETTING: &str = "FLUX_EXCHANGE_SETTINGS";

/// **Who may write a connection setting: a `User`, and nothing else.**
///
/// The one kind gate on this module, and the narrowest mechanism that closes `AGENTS.md`'s
/// *"an agent's token grants access to an operation, never to a credential"* — which the settings
/// write route broke, measured end to end, while it was [`Access::Principal`].
///
/// # The path this closes
///
/// A tenant's value is substituted into the operation's own request. Where the connector's host
/// template pins a suffix, the composed authority is `{value}.zendesk.com` — so a caller that can
/// write the value chooses the origin this host then sends that tenant's credential to. An agent
/// holding nothing but an operation grant could do it, because `require_principal` admits every
/// kind for [`Access::Principal`], and it converted a grant over an *operation* into delivery of a
/// *credential* to a server the agent controls.
///
/// The suffix pin does not save it. `*.zendesk.com`, `*.atlassian.net`, `*.myshopify.com`,
/// `*.supabase.co` and `*.my.salesforce.com` are **self-service registrable namespaces**: a suffix
/// pin constrains which *vendor* a request reaches, never *whose account* at that vendor.
///
/// # Why the whole write surface, rather than the host-shaped fields alone
///
/// The narrower rule is available: [`host_pinning`] already says, per field, whether a value can
/// reach the authority, and only [`HostPinning::PinnedTo`] can — a value that lands in a path or a
/// query moves no request anywhere. Gating those alone would be the smallest change that closes the
/// measured path. It is not the one taken, for three reasons in the order they bite:
///
/// - **`PrincipalKind` already publishes this division of labour, and this reads it rather than
///   inventing one.** `User` is documented as the kind that *"manages connections, credentials and
///   grants"*; `Agent` as the kind for which *"humans sign in to wire things up"* while *"agents are
///   what call operations all day"*. Supplying a connector's per-connection value is wiring up, on
///   that published reading. Nothing here decides what an agent may *do* with a connection — that is
///   the grant model, and it is still X-13's.
/// - **A per-field rule would make a stated invariant depend on an approximation.**
///   `host_pinning`'s notion of "pins a suffix" is `suffix_of`'s two-label threshold, which
///   `exchange_host::settings` documents as a stand-in for a public-suffix list. It is the right
///   basis for *"may a tenant supply this at all"*, where it errs closed and its cost is four
///   refused connectors. It is the wrong basis for *"is this the invariant's boundary"*: one
///   template read as unpinned that is not, and the gate silently has a hole in it.
/// - **The gate has to be visible.** [`Access`] is declared as data so the whole surface is
///   enumerable — `super::tests::the_kind_gated_surface_is_only_what_was_declared` walks it, and
///   `a_declared_kind_is_what_decides_the_answer` proves the declaration is what the guard consults.
///   A rule that could only be applied inside the handler, once the field is known, is exactly the
///   "a route is guarded by its handler remembering to ask" that [`Access`] exists to refuse.
///
/// **What it costs, stated rather than discovered:** an agent cannot supply bitbucket's workspace
/// or contentful's space id either, and those move no request anywhere. Nothing shipped configures
/// a connection from an agent — there is no client for these routes at all yet — so the bound is
/// paid by nobody today, and widening it later is one kind added to this list with an argument
/// beside it. Narrowing it later, after something depends on it, is not.
///
/// `Service` is excluded on `agents::MAY_MINT`'s reasoning rather than a new one: it is a backend
/// acting for its own accounts, not a human of this tenant wiring one up, and the exfiltration is
/// identical whichever non-human kind writes the value.
///
/// # What it does *not* close
///
/// A **`User` of this tenant who did not supply the credential** can still do all of it. Credential
/// values are write-only on this surface by design, and this path makes one readable to anyone who
/// can name an origin. Closing that needs a place for an **operator** to pin an allowed host per
/// tenant — a surface that does not exist, with its own authorization question. See
/// `docs/designs/connection-settings.md` § 4.
pub(super) const MAY_CONFIGURE: &[PrincipalKind] = &[PrincipalKind::User];

/// **Who may put a credential value into this tenant's store: a `User`, and nothing else.**
///
/// The two routes that write a credential — `POST /api/connections/{connector}` and
/// `PUT /api/connections/{connector}/credentials/{credential}` — and X-54's decision, which is the
/// half X-47 ring-fenced when it gated the settings write and stopped there.
///
/// # Why this is inside the invariant, which does not say so in as many words
///
/// `AGENTS.md` reads *"an agent's token grants access to an operation, never to a credential"*, and
/// the obvious reading is about a credential travelling **out** to the agent. Neither route does
/// that; this host hands no value back on any route. What they do is the substitution in the other
/// direction, and the sentence covers it on any reading that is about authority rather than about
/// bytes: a caller that can decide **which** credential the tenant's operations run under has been
/// granted the credential position, whether or not it ever sees a value. The account those
/// operations then reach is the writer's, and every ticket, message and record the tenant's agents
/// create afterwards is created there.
///
/// # Why it is worth a gate when the `DELETE` beside it is not
///
/// X-40 left `DELETE /api/connections/{connector}` at [`Access::Principal`] deliberately, on a
/// stated test — *what does this outlive?* — and the same test is what puts these two on the other
/// side of it:
///
/// - **Nothing records who supplied a credential.** A connection is what the credential store says
///   it is (`docs/designs/connections.md`) — there is no record beside it, by design — so
///   `GET /api/connections` answers `held: true` for a value an agent planted exactly as it does
///   for the one a human did. A rotation is more invisible still: it replaces in place, with no
///   observable state in which anything is missing.
/// - **Revocation is not a remedy.** Revoking the agent's token stops the agent; it does not take
///   the value back out of the store, and nothing points an operator at the address to look at.
///   That is `agents::MAY_MINT`'s argument — an incomplete remedy an operator cannot see — reached
///   by a different route.
/// - **`DELETE` has neither property.** It is visible (`GET /api/connections` stops listing the
///   connection), the operator holds the plaintext this host never did and can reconnect, and no
///   authority survives it. It stays open to every kind, and
///   [`tests::an_agent_may_still_read_a_connection_and_disconnect_one`] is what keeps this story
///   from having quietly taken it.
///
/// # Why the gate is per method, and what that costs
///
/// `POST` shares its path with `GET` and `DELETE`, and [`Access`] is declared per [`Route`] — so
/// this module publishes `/api/connections/{connector}` **twice**, once for the two verbs that stay
/// open and once for the one that does not. The alternative was a rule applied inside [`create`],
/// which is the *"a route is guarded by its handler remembering to ask"* that [`Access`] exists to
/// refuse, and which `super::tests::the_kind_gated_surface_is_only_what_was_declared` could not
/// see. A duplicated path costs a second line in that enumeration; an invisible gate costs the
/// enumeration.
///
/// # `Service`, decided rather than deferred
///
/// Refused, now, on `agents::MAY_MINT`'s argument rather than a new one. A `Service` is a backend
/// acting for its own accounts, and nothing in this repository mints a service credential, verifies
/// one, lists one or revokes one — `PrincipalKind::Service` is a kind the identity port may return
/// and nothing else. A credential this host cannot attribute to a revocable caller, written at an
/// address nothing records the author of, is the same incomplete remedy one level further out of
/// sight.
///
/// It is worth naming what that costs, because it is real and it is coming: **credential rotation
/// is exactly what a service integration would want.** A provisioning backend that wires up a
/// tenant's connectors, or rotates them on a schedule, is a legitimate caller and it is refused
/// here. The decision is still to refuse today, on the direction the two mistakes point: admitting
/// a kind for which no revocation path exists is a hole nobody meets until a credential leaks,
/// while refusing one is a `403` met on the first attempt. Widening this is one kind added to this
/// list with an argument beside it and a line changed in `KIND_GATED`; narrowing it after something
/// depends on it is not. The story that wants `Service` here is the story that gives it a
/// revocation path.
///
/// # What it does *not* close: there is no operator kind
///
/// `User` is **every** signed-in human of the tenant. So this gate says *a human, not a bot*; it
/// does not and cannot say *the human who set this tenant up*. A `User` who did not supply a
/// credential can still replace it with one they control, and every operation the tenant runs
/// afterwards reaches their account.
///
/// That is the same within-tenant gap [`MAY_CONFIGURE`] records — and it is the same gap, not a
/// second one: `docs/designs/connection-settings.md` § *What this does not close* wants a surface
/// where an **operator** pins what a tenant may configure, and this wants a surface where an
/// operator says which humans manage connections. Both are the authorization question — *what may
/// this principal do* — which is X-13's grant model, and **no kind gate can answer it**. Inventing
/// an `Operator` variant on [`PrincipalKind`] here would put a policy model in the identity
/// vocabulary, where nothing mints it, nothing revokes it and no identity port knows how to return
/// it. It is written down rather than left to be inferred from the absence of a test.
pub(super) const MAY_SUPPLY_A_CREDENTIAL: &[PrincipalKind] = &[PrincipalKind::User];

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "connections",
    routes: &[
        Route {
            // Under `/api` for the reason the session route is: `vite dev` owns the origin and
            // proxies `/api` to this host, so anything outside that prefix is answered by the SPA
            // fallback instead.
            //
            // **Every kind, decided rather than defaulted (X-54).** The listing answers one entry
            // per connector this tenant holds, as addresses and a `held` boolean, and never a
            // value. An agent that can see *"this tenant has no zendesk connection"* is one that
            // can say so instead of failing an invocation for a reason nobody can act on — the
            // same argument the settings `GET` collection is open on.
            path: "/api/connections",
            access: Access::Principal,
            method_router: collection_route,
        },
        Route {
            // `{connector}` is a catalogue key, never an address. It selects *what* is being
            // connected; the tenant — the only part of the address a caller could want to move —
            // comes from the guard.
            //
            // **`GET` and `DELETE`, and every kind for both (X-54).** The read is the line above
            // narrowed to one connector and carries no more than it does. The `DELETE` is X-40's
            // own decision, restated rather than reopened: it destroys tenant data inside the
            // tenant the caller already belongs to, an operator can see it and undo it by
            // reconnecting, and nothing about it outlives revocation of the token that did it.
            // Whether an agent should reach a destructive route is the grant-shaped question,
            // which is X-13's. See `crate::routes::agents`.
            path: "/api/connections/{connector}",
            access: Access::Principal,
            method_router: connection_route,
        },
        Route {
            // **The same path, for the one verb that is not open: `POST`.** Declared separately
            // because [`Access`] is per route and the two halves of this path differ in it — a
            // check inside [`create`] would be the handler remembering to ask, and the enumeration
            // that walks this table could not see it. [`MAY_SUPPLY_A_CREDENTIAL`] carries the
            // argument: a caller that decides which credential this tenant's operations run under
            // has been granted the credential position, nothing records who supplied one, and
            // revoking the token that did it does not take the value back out.
            path: "/api/connections/{connector}",
            access: Access::PrincipalOfKind(MAY_SUPPLY_A_CREDENTIAL),
            method_router: create_route,
        },
        Route {
            // A path of its own, so replacing a credential is not a method away from creating one.
            // `{credential}` is the flat-namespace name the catalogue publishes —
            // `zendesk.api_token` — and it is a key into the connector's own declaration exactly
            // as `{connector}` is a key into the catalogue: refused when the connector declares no
            // such name, and never a path segment of the credential address. What the address
            // carries is the declared `leaf`, which the catalogue supplies and the request does
            // not; `tests::a_hostile_credential_name_cannot_reach_the_address` drives that
            // directly.
            //
            // **Only a `User` (X-54).** A rotation replaces the value in place, with no observable
            // state in which anything is missing, so an agent doing it is the most invisible form
            // of the substitution [`MAY_SUPPLY_A_CREDENTIAL`] describes — and rotation exists for
            // revoking a leaked secret, which is an operator's act rather than a caller's.
            path: "/api/connections/{connector}/credentials/{credential}",
            access: Access::PrincipalOfKind(MAY_SUPPLY_A_CREDENTIAL),
            method_router: credential_route,
        },
        Route {
            // What this connector needs configured, and which of them this tenant has supplied.
            // The answer is derived from the connector's own operations and from the resolved
            // principal, and carries no value.
            path: "/api/connections/{connector}/settings",
            access: Access::Principal,
            method_router: settings_route,
        },
        Route {
            // `{service}` and `{field}` are keys into what the connector declares, exactly as
            // `{connector}` is a key into the catalogue: refused when nothing declares them, and
            // never a segment of an address. The tenant — the only part a caller could want to move
            // — comes from the guard.
            path: "/api/connections/{connector}/settings/{service}/{field}",
            // **Only a `User`.** A tenant's value is substituted into the operation's own request,
            // so a caller that can write one chooses the origin this host sends that tenant's
            // credential to — and an agent's token grants access to an operation, never to a
            // credential. [`MAY_CONFIGURE`] carries the argument, including why the gate is the
            // whole write surface rather than the host-shaped fields alone.
            access: Access::PrincipalOfKind(MAY_CONFIGURE),
            method_router: setting_route,
        },
    ],
};

fn collection_route() -> MethodRouter<AppState> {
    get(list)
}

/// The two verbs on `/api/connections/{connector}` that answer every kind of principal.
///
/// `post` is **not** here, and its absence is the mechanism rather than an omission: it is declared
/// beside this one at the same path with its own [`Access`], and axum merges the two method routers
/// into one path with each verb carrying the guard its own declaration asked for.
fn connection_route() -> MethodRouter<AppState> {
    get(show).delete(remove)
}

/// The third verb on that path, gated to [`MAY_SUPPLY_A_CREDENTIAL`].
fn create_route() -> MethodRouter<AppState> {
    post(create)
}

fn credential_route() -> MethodRouter<AppState> {
    put(rotate)
}

fn settings_route() -> MethodRouter<AppState> {
    get(list_settings)
}

fn setting_route() -> MethodRouter<AppState> {
    put(set_setting).delete(clear_setting)
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

/// The one value a caller supplies when it rotates a credential.
///
/// **Deliberately not [`NewConnection`]'s shape**, and that is the point rather than a
/// consequence: the two bodies are incompatible, so a create body sent to the rotation route and a
/// rotation body sent to `POST` both fail to deserialise. Together with the separate path and the
/// separate method, replacing a credential takes three things being right at once, and none of
/// them is a default. See the module documentation for why an upsert is the thing being kept out.
///
/// A rotation replaces exactly one credential and names it in the path, so there is no map here
/// and nothing to say about the credentials it did not name — they are untouched.
///
/// **No `Debug`**, for [`NewConnection`]'s reason: this holds a credential value as a plain
/// `String`, so a derived formatter is one `debug!(?body)` away from logging it.
#[derive(Deserialize)]
struct RotatedCredential {
    /// The value to put at the credential's existing address.
    value: String,
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
    crate::audit::connection_created(&principal, provider.id);
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

/// Replace one credential's value, at the address the connection already uses.
///
/// The operation an operator reaches for when a secret has leaked, and the reason it exists is
/// that the alternative — `DELETE` then `POST` — has a window in it where the tenant has no
/// connection and everything relying on it fails. It is also the alternative that hands the
/// operator the partial-delete failure X-18 documents, on the one path where a live vendor
/// credential surviving matters most.
///
/// # Why there is no window here
///
/// The whole of the change is a single [`SecretStore::put`], and that is an **atomic whole-file
/// replace**: the address holds the old value until it holds the new one. There is no moment in
/// between and nothing for this host to sequence, so "no observable state where the tenant has no
/// connection" is a property of the operation rather than a promise about it. `rotate` issues no
/// `delete` at all, and [`tests::a_credential_is_rotated_in_place_and_the_connection_is_never_gone`]
/// asserts both halves: that a concurrent reader never sees the connection incomplete, and that
/// the store served zero deletes.
///
/// # Why it refuses where `create` writes
///
/// Everything before the `put` is a refusal, and they are ordered so that the destructive step is
/// last: an unknown connector, an undeclared credential, a value that is not a credential, a
/// connection that is not there, a credential that is not there, and the tenant's allowance. Only
/// after all of them does anything get written — so a refused rotation cannot have destroyed what
/// it failed to replace, because the only way this host could destroy the old value is by writing
/// over it, and it has not.
async fn rotate(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((connector, credential)): Path<(String, String)>,
    Json(body): Json<RotatedCredential>,
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

    // The same seam `create` writes through, in its single-value form: `write_of` resolves the
    // supplied name to an address and admits the value against the per-value bound, so the bound
    // is not a check this handler remembers to make and the name in the path is refused here if
    // the connector does not declare it. It is also what keeps `{credential}` from reaching the
    // address — the address is composed from the *declared* leaf this lookup returns.
    let (reference, secret) =
        match declaration.write_of(principal.tenant(), &credential, &body.value) {
            Ok(write) => write,
            Err(refusal) => return connection_refused(&refusal),
        };

    // The claim `create` and `remove` take, for the same reason and against the same neighbours: a
    // rotation deciding against a value a `DELETE` is in the middle of destroying would put a
    // fresh credential back at an address an operator has just revoked.
    let Some(_claim) = state.connections().claim(principal.tenant(), provider.id) else {
        return change_in_flight(provider);
    };

    // **Where a rotation stops being an upsert.** A create that finds nothing writes; a rotation
    // that finds nothing refuses. The probe is inside the claim, so what it read is still true
    // when the `put` below acts on it.
    let held_now = match held(store, &addresses).await {
        Err(error) => return store_failed(&error),
        Ok(held) if held.is_empty() => return not_connected(provider, &addresses),
        Ok(held) => held,
    };
    if !held_now.iter().any(|name| name == &credential) {
        return nothing_to_rotate(provider, &credential, &reference);
    }

    // The allowance, at the width it is decided at — see `create` for the whole argument. A
    // rotation reaches for it because a *larger* value spends allowance the tenant may not have.
    let Some(_allowance) = state.connections().claim_tenant(principal.tenant()) else {
        return allowance_change_in_flight(provider);
    };

    // A rotation is a replacement, so what it spends is the difference: the tenant's occupancy
    // with the value being replaced taken out of it, plus the value going in. Counting the whole
    // new value against an occupancy that already includes the old one would refuse rotations that
    // fit — an operator with a leaked secret told to disconnect something first.
    let replacing = match store.get(&reference).await {
        // The length and nothing else, exactly as `occupied` measures: no plaintext is bound to a
        // name here.
        Ok(current) => stored_bytes(&current),
        // The probe above saw a value at this address under this same claim, so a not-found here
        // is a store that changed underneath a claim rather than an empty address. Counted as
        // nothing, which is the strict reading — it can only make the decision below tighter.
        Err(error) if error.is_not_found() => 0,
        Err(error) => return store_failed(&error),
    };
    let held_bytes = match occupied(store, principal.tenant()).await {
        Ok(bytes) => bytes,
        Err(error) => return store_failed(&error),
    };
    if let Err(refusal) =
        admit_tenant_occupancy(held_bytes.saturating_sub(replacing), stored_bytes(&secret))
    {
        // Nothing has been written, so the value this refused to replace is untouched. That is the
        // guarantee, and it is structural: the `put` is below this line.
        return connection_refused(&refusal);
    }

    if let Err(error) = store.put(&reference, &secret).await {
        return rotation_failed(provider, &credential, &reference, &error);
    }
    crate::audit::credential_rotated(&principal, provider.id, &credential);

    // `200` and not `201`: nothing was created, and the connection is the one that was already
    // there. The answer is the same view every other route gives — addresses, and which
    // credentials are held — and the set of held credentials is unchanged, because a rotation
    // replaces one that was already among them.
    Json(view(provider, &addresses, &held_now)).into_response()
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

    crate::audit::connection_removed(&principal, provider.id);
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------------------------
// The settings half (X-47)
// ---------------------------------------------------------------------------------------------

/// The one value a caller supplies when it sets a connection setting.
///
/// Deliberately the same shape as [`RotatedCredential`] and deliberately a different type: they are
/// two bodies for two surfaces, and one type shared between them is how a change made for the
/// non-secret one lands on the secret one. A `Debug` is derived here — unlike its two neighbours —
/// because this really does hold a non-secret value, and that difference is exactly what this whole
/// half of the module exists to keep visible.
#[derive(Debug, Deserialize)]
struct SuppliedSetting {
    /// The value to put at the setting's derived address.
    value: String,
}

/// What this connector needs configured, and which of them this tenant has supplied.
///
/// Derived from the connector's own compiled operations, not from a list kept here: `zendesk` needs
/// `endpoint.subdomain` because its operations' Flux says so. See
/// `exchange_host::declared_settings` for why a `base_url` scan is not the same answer.
///
/// **No values.** A `set` boolean per field, so an operator can see what is left to supply without
/// this host handing back a customer's account identifiers.
async fn list_settings(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(connector): Path<String>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(store) = state.settings() else {
        return no_settings_store();
    };

    let declared = match declared_settings(provider) {
        Ok(declared) => declared,
        Err(refusal) => return settings_refused(&refusal),
    };

    // `suppliable` is the half that stops a refused connector reading as a broken one. Three
    // shipped connectors template their whole authority with nothing declared to pick from, so no
    // tenant may supply their host at all — and an operator staring at a connection that will not
    // work needs to be told that on purpose, with the connector's own template quoted, rather than
    // left to conclude this host is faulty.
    let settings: Vec<Value> = declared
        .iter()
        .map(|setting| {
            let pinning = host_pinning(provider, setting);
            let mut view = json!({
                "service": setting.service,
                "field": setting.binds(),
                "set": store.is_set(principal.tenant(), provider.id, setting),
                "suppliable": pinning.tenant_may_supply(),
            });

            if let HostPinning::WholeAuthority(template) = &pinning {
                view["reason"] = json!(format!(
                    "`{}` declares its host as `{template}`, which pins no vendor suffix — a value \
                     here would be the whole origin this host sends `{}`'s credential to, so no \
                     tenant may supply it and this connector cannot be invoked on this deployment",
                    provider.id, provider.id,
                ));
            }

            view
        })
        .collect();

    Json(json!({
        "connector": provider.id,
        "vendor": provider.vendor,
        // Whether this connector is usable at all once everything suppliable has been supplied.
        // Derived rather than asserted, so it cannot disagree with the rows above it.
        "configurable": declared
            .iter()
            .all(|setting| host_pinning(provider, setting).tenant_may_supply()),
        "settings": settings,
    }))
    .into_response()
}

/// Supply one non-secret per-connection value.
///
/// The address is `(resolved tenant, connector, service, field)` and not one segment of it comes
/// from the request in the sense that matters: `{connector}`, `{service}` and `{field}` are keys
/// looked up in what the connector declares, and the tenant is the guard's. A caller that names a
/// service or a field the connector does not declare is refused with `422` and told what it does
/// declare.
///
/// # What this route deliberately does not do
///
/// It does not decide whether the value is *safe to put in a URL*. `connector-pack` decides that at
/// the one substitution point it makes, holding the composed authority to an allow-list of host
/// characters — so `acme.zendesk.com@evil.example` is refused there, with the position of the value
/// in the request known, which is knowledge this route does not have. A second opinion here would be
/// a second spelling of one rule, and `AGENTS.md`'s "this host constructs no request of its own"
/// says which of the two spellings gets to exist.
async fn set_setting(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((connector, service, field)): Path<(String, String, String)>,
    Json(body): Json<SuppliedSetting>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(store) = state.settings() else {
        return no_settings_store();
    };
    let Some(setting) = DeclaredSetting::parse(&service, &field) else {
        return unreadable_field(provider, &service, &field);
    };

    // **Every decision is the store's, and there is deliberately no second copy of one here.**
    //
    // An earlier cut of this handler called `admit_tenant_settings(held, value.len())` before the
    // write, under a comment claiming it decided against what the write *replaces*. It did not
    // subtract the replaced value, so a tenant sitting on its allowance was refused a same-size
    // rotation that `SettingsStore::set` would have accepted — the check disagreed with both its own
    // comment and the store one line below it.
    //
    // The fix is to delete it rather than to correct it. The store decides the allowance under the
    // same write lock it reads the occupancy and performs the insert under, which is a *tighter*
    // read-decide-write than a route-level claim could be: there is no window between the read and
    // the write for another of this tenant's requests to land in. A route-level guard on top of that
    // would guard nothing and would be a second place for the rule to drift.
    //
    // So this handler resolves the address and hands over. The store applies, in order: that the
    // connector declares this service and this field, that the field is not the destination
    // authority, the per-value bound, and the tenant allowance.
    if let Err(refusal) = store.set(principal.tenant(), provider.id, &setting, &body.value) {
        return settings_refused(&refusal);
    }

    crate::audit::setting_set(&principal, provider.id, &setting.service, &setting.binds());
    Json(setting_view(provider, &setting, true)).into_response()
}

/// Unset one per-connection value.
///
/// `204` when something was there and `404` when nothing was, for [`remove`]'s reason: "unset
/// something that was not set" and "unset it" are different facts and a caller should be able to
/// tell. Nothing here touches a credential — a connector whose subdomain is cleared still holds its
/// token, and clearing the token is `DELETE /api/connections/{connector}`.
async fn clear_setting(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((connector, service, field)): Path<(String, String, String)>,
) -> Response {
    let Some(provider) = catalogued(&connector) else {
        return unknown_connector(&connector);
    };
    let Some(store) = state.settings() else {
        return no_settings_store();
    };
    let Some(setting) = DeclaredSetting::parse(&service, &field) else {
        return unreadable_field(provider, &service, &field);
    };

    // No claim, for [`set_setting`]'s reason: the store's own write lock spans the whole
    // read-decide-write, and clearing only ever frees allowance.
    match store.clear(principal.tenant(), provider.id, &setting) {
        Err(refusal) => settings_refused(&refusal),
        Ok(false) => nothing_to_clear(provider, &setting),
        Ok(true) => {
            crate::audit::setting_cleared(
                &principal,
                provider.id,
                &setting.service,
                &setting.binds(),
            );
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

/// One setting as a caller sees it: where it belongs and whether it is supplied. Never its value.
fn setting_view(provider: &'static Provider, setting: &DeclaredSetting, set: bool) -> Value {
    json!({
        "connector": provider.id,
        "service": setting.service,
        "field": setting.binds(),
        "set": set,
    })
}

/// This composition bound no connection-settings store.
///
/// Not a fallback and not an empty answer, exactly as [`no_store`] is not: a host that cannot hold a
/// connection setting says so and names the setting that would have given it one. The message says
/// what the file is *for*, because an operator who has just read the credential-store refusal will
/// otherwise assume this one is about secrets too.
fn no_settings_store() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "this host has no connection-settings store bound, so it cannot hold the per-connection \
             values a templated connector needs: set `{SETTINGS_SETTING}` to a path outside every \
             working tree. It holds no secrets — credentials stay in the credential store",
        ),
        json!({ "setting": SETTINGS_SETTING }),
    )
}

/// The `{field}` segment is not a `binds` target at all.
///
/// Distinct from "this connector does not declare it": that one is a name in the right vocabulary
/// naming the wrong thing, and this is a name in no vocabulary. The distinction earns its keep on
/// one case — `credential.zendesk.api_token` is a real row of the design's `binds` table and it is a
/// **secret**, so an operator reaching for it here has to be told it lives somewhere else rather
/// than that zendesk does not declare it.
fn unreadable_field(provider: &'static Provider, service: &str, field: &str) -> Response {
    refuse(
        StatusCode::UNPROCESSABLE_ENTITY,
        format!(
            "`{field}` is not a connection setting. A setting is spelled `endpoint.<variable>` or \
             `username.<credential>` — the same `binds` targets a refused invocation names. A \
             `credential.<name>` is a secret and belongs at `POST /api/connections/{}`, not here",
            provider.id,
        ),
        json!({ "connector": provider.id, "service": service, "field": field }),
    )
}

/// Nothing was set at that address, so there was nothing to unset.
///
/// Names the connector, the service and the field this host looked at — this tenant's own address
/// and never another's, the same rule [`not_connected`] follows.
fn nothing_to_clear(provider: &'static Provider, setting: &DeclaredSetting) -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        format!(
            "this tenant has supplied no `{}` for connector `{}` service `{}`, so there is nothing \
             to unset",
            setting.binds(),
            provider.id,
            setting.service,
        ),
        json!({
            "connector": provider.id,
            "service": setting.service,
            "field": setting.binds(),
        }),
    )
}

/// How a [`SettingsRefusal`] reaches a caller — **the one place**, so a variant added later cannot
/// be answered two different ways by the two handlers that can raise it.
///
/// The status is per variant, on the same reading [`connection_refused`] takes: a request that is
/// well formed and has no address is `422`, a value that is not a setting is `413`, and a tenant
/// whose own state conflicts with the request is `409`. The two host-side variants are `502` and
/// `503`, split the way [`store_failure`] splits them — a store that cannot be *read* is a defect
/// somebody has to repair, and a store that cannot be *written* may answer next time.
///
/// Every payload carries the bound or the alternatives it was decided against, and none carries a
/// value.
fn settings_refused(refusal: &SettingsRefusal) -> Response {
    let (status, extra) = match refusal {
        SettingsRefusal::NothingDeclared { connector } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "connector": connector }),
        ),
        SettingsRefusal::UndeclaredService {
            connector,
            service,
            services,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "connector": connector, "service": service, "declared": services }),
        ),
        SettingsRefusal::UndeclaredSetting {
            connector,
            service,
            setting,
            declared,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "connector": connector,
                "service": service,
                "field": setting,
                "declared": declared,
            }),
        ),
        // **No tenant may supply this, whatever the value.** `422` on the same reading as the
        // addressing refusals — the request is well formed and there is no address here this host
        // will accept — and deliberately not `403`, which would say "not you": nobody may write
        // here, on any deployment, and the remedy is a composition decision rather than a
        // permission. The template is echoed so the refusal shows its working.
        SettingsRefusal::WouldNameTheHost {
            connector,
            setting,
            template,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "connector": connector,
                "field": setting,
                "host_template": template,
                "suppliable": false,
            }),
        ),
        // **The value is not one the connector declares.** `422` for the same reason the
        // addressing refusals are: the request is well formed and there is no address here for
        // *that* value. The declared choices are echoed — they are the catalogue's own published
        // data, they are what a form would render, and without them the caller can only guess.
        SettingsRefusal::NotADeclaredChoice {
            connector,
            setting,
            choices,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "connector": connector,
                "field": setting,
                "choices": choices,
                "suppliable": true,
            }),
        ),
        SettingsRefusal::SettingTooLarge {
            connector,
            setting,
            bytes,
            limit,
        } => (
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({
                "connector": connector,
                "field": setting,
                "bound": "setting",
                "sent_bytes": bytes,
                "limit_bytes": limit,
            }),
        ),
        SettingsRefusal::TenantAllowanceExhausted {
            held,
            adding,
            limit,
        } => (
            StatusCode::CONFLICT,
            json!({
                "bound": "tenant_settings",
                "held_bytes": held,
                "adding_bytes": adding,
                "limit_bytes": limit,
            }),
        ),
        // A connector this host cannot read the surface of. `502`, and never a `422`: there is
        // nothing wrong with the request and nothing the caller can send instead.
        SettingsRefusal::Unreadable {
            connector,
            operation,
            ..
        } => (
            StatusCode::BAD_GATEWAY,
            json!({ "connector": connector, "operation": operation }),
        ),
        // The store did not take the write. `503` for `store_failure`'s reason — retrying may work
        // — and the *reason* stays in the log, because it names this host's own paths.
        SettingsRefusal::Unwritable { .. } => {
            error!(%refusal, "the connection-settings store failed");
            (StatusCode::SERVICE_UNAVAILABLE, json!({}))
        }
    };

    // The `Unwritable` reason names this host's own filesystem, so it is the one refusal whose
    // message does not travel. Every other variant's is written for the operator who has to act.
    let reason = match refusal {
        SettingsRefusal::Unwritable { .. } => {
            "the connection-settings store could not be written, \
                                               so nothing was changed. Retrying may work"
                .to_string()
        }
        other => other.to_string(),
    };

    refuse(status, reason, extra)
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
/// The address this host *derives* has no level at which two instances of one connector differ, so
/// accepting this would overwrite the first connection, answer `201`, and send every later call to
/// the wrong account while looking healthy. The refusal names the level that will replace it:
/// `@instances/<uuid>`, which landed in flux-connectors (C-406) and is published and pinned here
/// since X-11 — `connector_address::CredentialRef::for_instance` spells it. What is still missing
/// is this host's half: resolving a name the operator chooses to that uuid, and moving the
/// already-stored credential to the address it gains. That is X-14, and until it lands this refuses
/// rather than deriving an address the store was never written at.
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
                 `tenants/<tenant>/<authority>/@instances/<uuid>/<credential>` — which landed in \
                 flux-connectors (C-406) and is published; this host does not yet resolve a name \
                 you choose to that uuid, which is X-14. Until then, delete the existing \
                 connection before creating another",
        }),
    )
}

/// The connection is there, and this credential is not one of the values it holds.
///
/// A rotation replaces a value it expects to find; it does not create one. Writing here would be
/// the upsert [`already_connected`] refuses, arriving through the other door — a write that does
/// not know whether it is replacing something.
///
/// Distinct from [`not_connected`] because the two send an operator to different places: that one
/// says the tenant has no connection to this connector at all, and telling somebody whose
/// connection is fine that they have none is a false statement about their own state. A connector
/// may legitimately hold a subset of what it declares
/// ([`tests::a_connection_may_carry_a_subset_of_what_is_declared`]), so this is an ordinary case
/// rather than a damaged connection.
///
/// **The remedy named is the one that works.** Adding a credential to a connection that already
/// exists is not something this surface can do today — `POST` refuses with [`already_connected`]
/// and there is no other route — so the refusal says that plainly, including what the available
/// route costs, rather than naming a remedy that would answer `409`.
fn nothing_to_rotate(
    provider: &'static Provider,
    credential: &str,
    reference: &CredentialRef,
) -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        format!(
            "this tenant's `{}` connection holds no `{credential}` to rotate; nothing is stored at \
             the address below. A rotation replaces a value it expects to find and does not create \
             one, because a write that does not know whether it is replacing something is the \
             silent overwrite this surface refuses. Adding a credential to a connection that \
             already exists is not on this surface today: `DELETE /api/connections/{}` and `POST` \
             the whole set, which does destroy the credentials it already holds",
            provider.id, provider.id,
        ),
        json!({
            "connector": provider.id,
            "credential": credential,
            "address": address_path(reference),
        }),
    )
}

/// The store refused the one write a rotation makes.
///
/// The kind survives, as it does for every refusal on this surface: a `Denied` answered `503`
/// "retrying may work" sends an operator to retry, which is the one thing that cannot restore this
/// host's access to the store. [`store_failure`] is the shared mapping X-18 and X-20 established,
/// and this reads it rather than restating it.
///
/// # There is no partial state here, and that is why this is not a third `partly_*`
///
/// [`partly_written`] and [`partly_destroyed`] exist because their operations are **loops** over
/// several addresses that can stop in the middle: a create writes every supplied value, a delete
/// destroys every declared one, and each therefore owes the caller an account of which half
/// happened. A rotation is one [`SecretStore::put`] at one address, and that is an atomic
/// whole-file replace — it lands or it does not. So there is no `left_behind` to name, nothing
/// half-old and half-new to admit, and no rollback to attempt or to report the failure of.
///
/// What this says instead is the thing that is actually true, and it is the stronger statement:
/// **the value at that address is the one that was there before the request.** That is also this
/// refusal's half of "a refused rotation must not destroy what it failed to replace" — made true
/// by there being no `delete` on this path at all and by the `put` being last, and reported here
/// so an operator does not have to infer it.
fn rotation_failed(
    provider: &'static Provider,
    credential: &str,
    reference: &CredentialRef,
    error: &StoreError,
) -> Response {
    error!(%error, connector = provider.id, "a credential could not be rotated");

    let (status, _, advice) = store_failure(error);

    refuse(
        status,
        format!(
            "the credential store failed while rotating the `{}` credential `{credential}`, so \
             nothing was replaced: the value at the address below is the one it held before this \
             request. A rotation is one atomic write, so there is no half-old and half-new state \
             to account for and the credential you were replacing is still live. {advice}",
            provider.id,
        ),
        json!({
            "connector": provider.id,
            "credential": credential,
            "address": address_path(reference),
            "replaced": false,
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
        async_trait, ConnectionSettings as _, MAX_CREDENTIAL_VALUE_BYTES, MAX_TENANT_STORE_BYTES,
        TENANTS_ROOT,
    };
    use tower::Service;

    use crate::dev_identity::DevIdentity;

    /// Two tenants, and a third principal that is not a human.
    ///
    /// `alice` is `acme`; `bob` is `globex`. `triage-bot` is an **agent** of `acme`, and it is here
    /// because a kind gate cannot be tested against a roster with only one kind in it: every
    /// assertion that an agent is refused is worth nothing unless a caller of another kind reaches
    /// the same address and is admitted.
    const ROSTER: &str = "user:alice@acme,user:bob@globex,agent:triage-bot@acme";

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

        /// How many `delete`s this store has served.
        ///
        /// For the one assertion that cannot be made from the outside: a rotation must never make
        /// the address empty, and `delete` is the only operation that could. Counting them to zero
        /// is the property rather than a sample of it.
        fn deletes(&self) -> usize {
            *self.deletes.lock().expect("no test poisons this")
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

    /// An app with both tenants armed, a credential store **and** a settings store bound.
    ///
    /// Two stores, handed back separately, because the whole of X-47's placement argument is that
    /// they are two — a test that could not look at them independently could not tell a setting
    /// written into the credential store from one written where it belongs.
    fn configurable_app() -> (
        Router,
        Arc<TestStore>,
        Arc<exchange_host::SettingsStore>,
        Scratch,
    ) {
        let store = Arc::new(TestStore::default());
        let scratch = Scratch::new();
        let settings = Arc::new(
            exchange_host::SettingsStore::bind(scratch.join("settings"))
                .expect("a fresh settings store"),
        );

        let app = super::super::app(
            AppState::with_development_identity(Arc::new(
                DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
            ))
            .with_credentials(store.clone())
            .with_settings(settings.clone()),
        );

        (app, store, settings, scratch)
    }

    /// A scratch directory for a settings file, removed on drop.
    ///
    /// Hand-rolled rather than pulled in, for `exchange_host::credentials`' reason: a store's tests
    /// are the last place to add a dependency for four lines of `create_dir_all`.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);

            let path = std::env::temp_dir().join(format!(
                "flux-exchange-routes-settings-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Self(path.canonicalize().expect("a resolvable scratch directory"))
        }

        fn join(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An app with both stores bound **and an invoker**, so a refusal can be told from a dispatch.
    ///
    /// The invoker is this composition's real one — the same `crate::execution::invoker` the binary
    /// builds, holding the real transport. Nothing here fakes the egress, and that is deliberate:
    /// the claim is that no request is *made*, and a test that could only observe a fake transport
    /// staying quiet would be a weaker statement than the one this host answers with. What says a
    /// request was never sent is `sent`, which `exchange_host::Sent` decides from where the failure
    /// happened rather than from anything a test arranged.
    ///
    /// Since X-13 it also holds a real grant store, and `acme` is granted zendesk **widely** — the
    /// grant gate is not what these tests are about, and a tenant with no grant would refuse every
    /// invocation below with `403` before the settings refusal they exist to observe.
    fn dispatching_app() -> (
        Router,
        Arc<TestStore>,
        Arc<exchange_host::SettingsStore>,
        Scratch,
    ) {
        let store = Arc::new(TestStore::default());
        let scratch = Scratch::new();
        let settings = Arc::new(
            exchange_host::SettingsStore::bind(scratch.join("settings"))
                .expect("a fresh settings store"),
        );
        // The port, in the narrowest scope that can name it: `set` is a method on it, and this
        // helper is the only place in this module with a grant to store.
        use exchange_host::Grants as _;

        let grants = Arc::new(
            exchange_host::GrantStore::bind(scratch.join("grants")).expect("a fresh grant store"),
        );
        grants
            .set(
                &Tenant::new("acme").expect("a plain tenant id"),
                &[exchange_host::Grant::for_connector(
                    "zendesk",
                    exchange_host::Selector::any(),
                )],
            )
            .expect("a store outside a working tree takes a write");
        let invoker = Arc::new(
            crate::execution::invoker(
                exchange_host::Deployment::MultiTenant,
                store.clone(),
                settings.clone(),
                grants,
            )
            .expect("a usable workspace root"),
        );

        let app = super::super::app(
            AppState::with_development_identity(Arc::new(
                DevIdentity::from_roster(ROSTER).expect("a well-formed roster"),
            ))
            .with_credentials(store.clone())
            .with_settings(settings.clone())
            .with_invoker(invoker),
        );

        (app, store, settings, scratch)
    }

    /// Every kind-refusal this host logged while a test ran.
    ///
    /// Hand-rolled rather than pulled in, for [`Scratch`]'s reason: a capturing layer is thirty
    /// lines and a test dependency is forever. It records `WARN` and above only, which is the level
    /// the guard refuses at, and it records the event's **fields** — so what a test asserts is the
    /// line an operator would actually read, including which principal it names.
    #[derive(Clone, Default)]
    struct Warnings(Arc<Mutex<Vec<String>>>);

    impl Warnings {
        /// The recorded lines that are the guard's kind-refusal, in the order they were emitted.
        fn kind_refusals(&self) -> Vec<String> {
            self.0
                .lock()
                .expect("no test poisons this")
                .iter()
                .filter(|line| line.contains(super::super::KIND_REFUSED))
                .cloned()
                .collect()
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() > tracing::Level::WARN {
                return;
            }

            let mut line = String::new();
            event.record(&mut FieldsAsText(&mut line));
            self.0.lock().expect("no test poisons this").push(line);
        }
    }

    /// Every field of one event, rendered the way a subscriber would render it.
    struct FieldsAsText<'a>(&'a mut String);

    impl tracing::field::Visit for FieldsAsText<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write as _;
            let _ = write!(self.0, "{}={value:?} ", field.name());
        }
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

    /// **X-39's first Acceptance item.** A credential is replaced in place, and there is **no
    /// observable state in which the tenant has no connection** — asserted rather than argued.
    ///
    /// Two independent assertions, because either alone would be satisfiable by an implementation
    /// with a window in it:
    ///
    /// - A reader hammers `GET /api/connections/{connector}` for the whole duration of the
    ///   rotation, with the store's window widened so its reads really do interleave, and **every
    ///   one** of them must answer `200` with the credential `held`. A `DELETE`-then-`POST`
    ///   rotation fails this on the read that lands between the two.
    /// - The store served **no `delete` at all**. That is the structural half: `SecretStore::put`
    ///   is an atomic whole-file replace, so a rotation that only ever `put`s cannot have a window
    ///   — the address goes from the old value to the new one and is never empty. A `delete` is the
    ///   only operation that could make it empty, so counting them to zero is the property itself
    ///   rather than a sample of it.
    ///
    /// The rotated value is a second sentinel, so the answer is also checked not to hand back what
    /// it was just given.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_credential_is_rotated_in_place_and_the_connection_is_never_gone() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        /// What the rotation puts there. Distinct from [`SENTINEL`] so the assertion below is
        /// about the *new* value having landed, not merely about something being there.
        const ROTATED: &str = "ROTATED-NOT-A-REAL-SECRET-EITHER";

        let (app, store) = connected_app();
        let (status, _) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED);

        // Every `get` yields, so the reader below reliably lands inside the rotation's own
        // read-decide-write rather than only sometimes.
        store.widen_the_window();

        let stop = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = tokio::spawn({
            let app = app.clone();
            let stop = stop.clone();
            let reads = reads.clone();
            async move {
                let mut gone = Vec::new();

                while !stop.load(Ordering::Relaxed) {
                    let (status, body) =
                        call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
                    reads.fetch_add(1, Ordering::Relaxed);
                    if status != StatusCode::OK || body["credentials"][0]["held"] != true {
                        gone.push((status, body));
                    }
                }

                gone
            }
        });

        // The reader has to be running before the rotation starts, or the window it exists to
        // watch closes unobserved and the assertions below are about nothing.
        while reads.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }

        let (status, rotated) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": ROTATED })),
        )
        .await;

        stop.store(true, Ordering::Relaxed);
        let gone = reader.await.expect("the reader task must not panic");
        let reads = reads.load(Ordering::Relaxed);

        assert_eq!(
            status,
            StatusCode::OK,
            "a rotation replaces a value that is there, so it is not a creation: {rotated}",
        );
        assert!(
            reads > 1,
            "the reader made {reads} reads, so none of them can have been concurrent with the \
             rotation and this test proves nothing",
        );
        assert!(
            gone.is_empty(),
            "the connection was unreadable or incomplete during the rotation, in {reads} reads: \
             {gone:?}",
        );
        assert_eq!(
            store.deletes(),
            0,
            "a rotation that deletes has a window in which the tenant has no credential at that \
             address; the store's write is an atomic whole-file replace and that is what a \
             rotation is",
        );
        assert_eq!(
            store
                .at("tenants/acme/com.zendesk.api/api_token")
                .as_deref(),
            Some(ROTATED),
            "the new value must be the one at the address the connection was already using",
        );
        assert!(
            !rotated.to_string().contains(ROTATED),
            "an answer must not repeat the value it was given: {rotated}",
        );
    }

    /// **X-39's second Acceptance item.** A rotation names the connection it expects to exist and
    /// is refused when it does not — which is the whole of the difference between it and an
    /// upsert. An upsert writes where it finds nothing; this refuses there.
    ///
    /// Two refusals, because the two facts send an operator to different places: no connection to
    /// this connector at all, and a connection that does not hold *this* credential. The second is
    /// an ordinary case rather than damage — a connector may legally hold a subset of what it
    /// declares — and answering it by writing would be the create path's `409` undone through the
    /// other door.
    #[tokio::test]
    async fn a_rotation_is_refused_where_there_is_nothing_to_replace() {
        let (app, store) = connected_app();

        // Nothing connected at all.
        let (status, refusal) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": "ROTATED-NOT-A-REAL-SECRET-EITHER" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a rotation with nothing to replace must refuse, not create: {refusal}",
        );
        assert!(
            refusal.to_string().contains("holds no connection"),
            "the refusal must say the connection is missing, not the credential: {refusal}",
        );
        assert!(
            store.addresses().is_empty(),
            "a refused rotation must have written nothing: {:?}",
            store.addresses(),
        );

        // Connected, and this credential is not one of the values it holds. `slack` declares two
        // and this connection carries one.
        let (status, created) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({ "credentials": { "slack.bot_token": SENTINEL } })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");

        let (status, refusal) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/slack/credentials/slack.signing_secret",
            Some(json!({ "value": "ROTATED-NOT-A-REAL-SECRET-EITHER" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "rotation replaces a value it expects to find; it does not create one: {refusal}",
        );
        assert_eq!(refusal["credential"], "slack.signing_secret", "{refusal}");
        assert_eq!(
            refusal["address"], "tenants/acme/com.slack.api/signing_secret",
            "the refusal names the address this host looked at: {refusal}",
        );
        assert!(
            refusal.to_string().contains("does not create one"),
            "and says why, rather than reading as a broken connection: {refusal}",
        );

        assert_eq!(
            store.addresses(),
            vec!["tenants/acme/com.slack.api/bot_token".to_string()],
            "a refused rotation must not have created the credential it could not replace",
        );
        assert_eq!(
            store.at("tenants/acme/com.slack.api/bot_token"),
            Some(SENTINEL.to_string()),
            "nor touched the one the connection does hold",
        );
    }

    /// **The `409` on create is untouched, and rotation is not a slip away from it.**
    ///
    /// The two operations are kept apart by three independent things — a different path, a
    /// different method, and an incompatible body — so reaching a replacement from a create takes
    /// all three being deliberate. Each is driven here against a connection that already holds a
    /// value, and the value is asserted unchanged after every one: the failure this guards against
    /// is a mistyped request quietly overwriting a credential.
    #[tokio::test]
    async fn a_create_cannot_slip_into_a_rotation() {
        let (app, store) = connected_app();

        let (status, _) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED);

        let create_body = json!({ "credentials": { "zendesk.api_token": "SLIPPED-NOT-A-SECRET" } });
        let rotate_body = json!({ "value": "SLIPPED-NOT-A-SECRET" });

        for (method, path, body, why) in [
            (
                // The create path itself, which still refuses exactly as X-10 wrote it.
                Method::POST,
                "/api/connections/zendesk",
                create_body.clone(),
                "a second create is still the X-14 refusal and never an upsert",
            ),
            (
                // A create body at the rotation route: the shapes do not overlap.
                Method::PUT,
                "/api/connections/zendesk/credentials/zendesk.api_token",
                create_body.clone(),
                "a create body must not be a rotation",
            ),
            (
                // And the other way round.
                Method::POST,
                "/api/connections/zendesk",
                rotate_body.clone(),
                "a rotation body must not be a create",
            ),
            (
                // The rotation body at the connection route, where `PUT` is not answered at all —
                // so the collection has no whole-set replace hiding behind a method.
                Method::PUT,
                "/api/connections/zendesk",
                rotate_body.clone(),
                "there is no whole-set replace on the connection route",
            ),
            (
                Method::POST,
                "/api/connections/zendesk/credentials/zendesk.api_token",
                rotate_body.clone(),
                "and no create on the credential route",
            ),
        ] {
            let (status, body) = call(&app, "alice", method.clone(), path, Some(body)).await;

            assert!(
                status.is_client_error(),
                "{why}, so `{method} {path}` must be refused: {status} {body}",
            );
            assert_eq!(
                store.at("tenants/acme/com.zendesk.api/api_token"),
                Some(SENTINEL.to_string()),
                "{why}: `{method} {path}` changed the stored credential",
            );
        }

        // And the create refusal is the one X-10 wrote, unchanged — asserted here as well as in
        // `a_second_connection_to_one_connector_is_refused_rather_than_overwriting`, because this
        // is the test that would notice rotation having been wired into `POST`.
        let (status, refusal) = call(
            &app,
            "alice",
            Method::POST,
            "/api/connections/zendesk",
            Some(create_body),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
        assert!(
            refusal.to_string().contains("@instances/<uuid>"),
            "{refusal}"
        );
    }

    /// **X-39's third Acceptance item.** A rotation the store refuses reports what it did, in the
    /// vocabulary X-18 and X-20 established — and what it did is *nothing*.
    ///
    /// The kind survives out to the caller through the same [`store_failure`] mapping the two
    /// partial-failure refusals read, so a `Denied` is not answered "retrying may work". That half
    /// is asserted against the mapping itself rather than against copied strings, so a reworded
    /// advice cannot leave this test green while the refusal drifts.
    ///
    /// The other half is why there is no `partly_rotated` beside `partly_written` and
    /// `partly_destroyed`: those two loop over several addresses and can stop in the middle, and a
    /// rotation is **one** `put` at one address against a store whose write is an atomic whole-file
    /// replace. There is no half-old, half-new state to admit to — and the assertion that it is not
    /// merely unreported is the last one, against the store.
    #[tokio::test]
    async fn a_rotation_the_store_refuses_leaves_the_old_credential_in_place() {
        const ROTATED: &str = "ROTATED-NOT-A-REAL-SECRET-EITHER";

        for failure in [Failure::Unreachable, Failure::Denied, Failure::Backend] {
            let (app, store) = connected_app();
            let (status, _) = connect_zendesk(&app, "alice").await;
            assert_eq!(status, StatusCode::CREATED);

            // Every `get` still answers; it is the write that is refused, which is the only step a
            // rotation has.
            store.allow_only_failing_with(0, failure);

            let (status, refusal) = call(
                &app,
                "alice",
                Method::PUT,
                "/api/connections/zendesk/credentials/zendesk.api_token",
                Some(json!({ "value": ROTATED })),
            )
            .await;

            let (expected, _, advice) =
                store_failure(&failure.at("tenants/acme/com.zendesk.api/api_token".to_string()));
            assert_eq!(
                status, expected,
                "a rotation refused by the store keeps the failure's kind, exactly as a create and \
                 a delete do: {refusal}",
            );
            assert!(
                refusal["error"]
                    .as_str()
                    .is_some_and(|error| error.contains(advice)),
                "and carries that kind's advice rather than a generic one: {refusal}",
            );

            assert_eq!(
                refusal["replaced"], false,
                "the refusal must say plainly that nothing was replaced: {refusal}",
            );
            assert_eq!(
                refusal["address"], "tenants/acme/com.zendesk.api/api_token",
                "and name the address it failed at: {refusal}",
            );
            assert!(
                !refusal.to_string().contains(ROTATED) && !refusal.to_string().contains(SENTINEL),
                "and neither the value it was given nor the one already there: {refusal}",
            );

            // The statement the refusal makes, checked against the store rather than believed.
            assert_eq!(
                store.at("tenants/acme/com.zendesk.api/api_token"),
                Some(SENTINEL.to_string()),
                "a failed rotation must leave the value it could not replace exactly as it was",
            );

            // And the connection is still whole, which is the property the whole story is about.
            store.recovers();
            let (status, read) =
                call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
            assert_eq!(status, StatusCode::OK, "{read}");
            assert_eq!(read["credentials"][0]["held"], true, "{read}");
        }
    }

    /// **X-39's fourth Acceptance item, and X-22's bound.** A rotation to a larger value that would
    /// put the tenant past its allowance is refused, **and the old value survives**.
    ///
    /// This is the case where a refusal could most plausibly have destroyed something: the obvious
    /// implementation of "replace" is a delete followed by a write, and a bound checked between the
    /// two leaves the tenant with neither the old credential nor the new one. Here the decision is
    /// made before the only write there is, so the old value cannot have gone anywhere.
    ///
    /// The arithmetic is asserted too, because a rotation is a *replacement*: what it spends is the
    /// difference, so the occupancy it is decided against has the value being replaced taken out of
    /// it. Counting the whole new value against an occupancy that still includes the old one would
    /// refuse rotations that fit — and telling an operator with a leaked secret to go and
    /// disconnect something is the wrong instruction at the worst moment. The run ends by rotating
    /// to a value that does fit, so the bound is not passing by refusing everything.
    #[tokio::test]
    async fn a_rotation_past_the_tenant_allowance_is_refused_and_the_old_value_survives() {
        /// What this tenant occupies everywhere except `zendesk`. One byte more than "the
        /// allowance less a whole credential", so a rotation to a value at the per-value bound is
        /// past the allowance by exactly one byte — the per-value bound admits it and only the
        /// per-tenant one can refuse it.
        const ELSEWHERE: usize = MAX_TENANT_STORE_BYTES - MAX_CREDENTIAL_VALUE_BYTES + 1;

        let (app, store) = connected_app();
        let acme = Tenant::new("acme").expect("a plain tenant id");

        let (status, _) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED);
        occupy(&store, &acme, ELSEWHERE, &["zendesk"]);

        let too_large = "L".repeat(MAX_CREDENTIAL_VALUE_BYTES);
        let (status, refusal) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": too_large })),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "a rotation that would put the tenant past its allowance is refused: {refusal}",
        );
        assert_eq!(refusal["bound"], "tenant", "{refusal}");
        assert_eq!(
            refusal["held_bytes"],
            json!(ELSEWHERE),
            "the occupancy a replacement is decided against has the value being replaced taken \
             out of it, or a rotation that fits would be refused: {refusal}",
        );
        assert_eq!(
            refusal["adding_bytes"],
            json!(MAX_CREDENTIAL_VALUE_BYTES),
            "{refusal}",
        );
        assert_eq!(
            refusal["limit_bytes"],
            json!(MAX_TENANT_STORE_BYTES),
            "{refusal}",
        );

        // **The item.** A refused rotation must not destroy what it failed to replace.
        assert_eq!(
            store.at("tenants/acme/com.zendesk.api/api_token"),
            Some(SENTINEL.to_string()),
            "the value the rotation was refused for must still be there, and be the old one",
        );
        let (status, read) =
            call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
        assert_eq!(status, StatusCode::OK, "{read}");
        assert_eq!(read["credentials"][0]["held"], true, "{read}");

        // And the bound admits the rotation that fits, so it is a bound rather than a refusal of
        // everything. One byte less is exactly the allowance, which is inclusive.
        let fits = "F".repeat(MAX_TENANT_STORE_BYTES - ELSEWHERE);
        let (status, rotated) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": fits })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rotated}");
        assert_eq!(
            store.at("tenants/acme/com.zendesk.api/api_token"),
            Some(fits),
        );
        assert!(
            store.bytes_under("tenants/acme/") <= MAX_TENANT_STORE_BYTES,
            "one tenant occupies {} bytes, past the {MAX_TENANT_STORE_BYTES} it may hold",
            store.bytes_under("tenants/acme/"),
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
        assert_eq!(
            refusal["declared"],
            json!(["zendesk.api_token", "zendesk.messaging_key"])
        );
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
    /// refusals with their address lists, both size refusals, a store failure, and — since X-39 —
    /// a rotation that lands together with all four of its refusals ([`not_connected`],
    /// [`nothing_to_rotate`], the undeclared name, and [`rotation_failed`]). **Not driven:**
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
            // X-39's answer: a rotation that lands, which is handed a value and must not give one
            // back.
            (
                Method::PUT,
                "/api/connections/zendesk/credentials/zendesk.api_token",
                Some(json!({ "value": SENTINEL })),
            ),
            // And two of its refusals: an undeclared name, and a connector this tenant has not
            // connected at all. `slack` is connected further down, not here.
            (
                Method::PUT,
                "/api/connections/zendesk/credentials/zendesk.nope",
                Some(json!({ "value": SENTINEL })),
            ),
            (
                Method::PUT,
                "/api/connections/slack/credentials/slack.bot_token",
                Some(json!({ "value": SENTINEL })),
            ),
        ] {
            let (_, body) = call(&app, "alice", method, path, body).await;
            answers.push(body);
        }

        // X-39's other two refusals, each needing a store the requests above cannot be run through
        // afterwards: a connection holding a subset, and a store that refuses the write.
        let (subset_app, _) = connected_app();
        let (status, _) = call(
            &subset_app,
            "alice",
            Method::POST,
            "/api/connections/slack",
            Some(json!({ "credentials": { "slack.bot_token": SENTINEL } })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, nothing_to_rotate) = call(
            &subset_app,
            "alice",
            Method::PUT,
            "/api/connections/slack/credentials/slack.signing_secret",
            Some(json!({ "value": SENTINEL })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "this must be the nothing-to-rotate refusal, or the answer below proves nothing about \
             it: {nothing_to_rotate}",
        );
        answers.push(nothing_to_rotate);

        let (refused_app, refused_store) = connected_app();
        let (status, _) = connect_zendesk(&refused_app, "alice").await;
        assert_eq!(status, StatusCode::CREATED);
        refused_store.allow_only(0);
        let (status, rotation_failed) = call(
            &refused_app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": SENTINEL })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "this must be the failed-rotation refusal, or the answer below proves nothing about \
             it: {rotation_failed}",
        );
        assert_eq!(rotation_failed["replaced"], false, "likewise");
        answers.push(rotation_failed);

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

    // -----------------------------------------------------------------------------------------
    // The settings half (X-47)
    // -----------------------------------------------------------------------------------------

    /// **X-47's surface, end to end.** A tenant asks what a templated connector needs, supplies it,
    /// sees it as supplied, and unsets it.
    ///
    /// The listing is the part worth having beyond the write: X-12's finding was not that the value
    /// could not be stored, it was that an operator staring at *"needs `endpoint.subdomain`"* had
    /// nowhere to go. This is where to go, and it names the same `binds` targets the refusal does.
    #[tokio::test]
    async fn a_templated_connector_takes_the_setting_its_manifest_asks_for() {
        let (app, _store, _settings, _scratch) = configurable_app();

        let (status, needed) = call(
            &app,
            "alice",
            Method::GET,
            "/api/connections/zendesk/settings",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{needed}");
        assert_eq!(needed["connector"], "zendesk");
        assert_eq!(
            needed["settings"],
            json!([
                {
                    "service": "default",
                    "field": "endpoint.subdomain",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "default",
                    "field": "username.zendesk.api_token",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "default",
                    "field": "username.zendesk.messaging_key",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "help-center",
                    "field": "endpoint.subdomain",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "help-center",
                    "field": "username.zendesk.api_token",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "help-center",
                    "field": "username.zendesk.messaging_key",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "messaging",
                    "field": "endpoint.appId",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "messaging",
                    "field": "endpoint.subdomain",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "messaging",
                    "field": "username.zendesk.api_token",
                    "set": false,
                    "suppliable": true,
                },
                {
                    "service": "messaging",
                    "field": "username.zendesk.messaging_key",
                    "set": false,
                    "suppliable": true,
                },
            ]),
            "the listing names what to supply, and no value: {needed}",
        );
        assert_eq!(
            needed["configurable"], true,
            "zendesk's host is suffix-pinned, so a tenant can configure the whole of it: {needed}",
        );

        let (status, supplied) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            Some(json!({ "value": "acme" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{supplied}");
        assert_eq!(supplied["set"], true);
        assert!(
            !supplied.to_string().contains("acme\""),
            "the answer must not repeat the value: {supplied}",
        );

        let (_, needed) = call(
            &app,
            "alice",
            Method::GET,
            "/api/connections/zendesk/settings",
            None,
        )
        .await;
        assert_eq!(needed["settings"][0]["set"], true, "{needed}");
        assert_eq!(
            needed["settings"][1]["set"], false,
            "and the one nobody supplied is still unsupplied: {needed}",
        );

        let (status, _) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, gone) = call(
            &app,
            "alice",
            Method::DELETE,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "unsetting what is not set is a 404, not a 204: {gone}",
        );
    }

    /// **The Acceptance's fourth item, as a behaviour.** A connection setting is not written to the
    /// credential store, does not make a connection look `held`, and does not spend the credential
    /// allowance.
    ///
    /// This is the test the placement argument rests on. Everything else about "configuration is not
    /// a credential" is prose in `exchange_host::settings`; this is the part a diff can break.
    #[tokio::test]
    async fn a_setting_is_not_a_credential_and_does_not_land_in_the_credential_store() {
        let (app, store, settings, _scratch) = configurable_app();

        let (status, _) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            Some(json!({ "value": "acme" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        assert!(
            store.addresses().is_empty(),
            "a connection setting reached the credential store: {:?}",
            store.addresses(),
        );
        assert_eq!(
            store.bytes_under("tenants/acme/"),
            0,
            "a connection setting spent the credential allowance, whose whole argument is about \
             the latency of the one file every credential write rewrites",
        );

        // And the connection itself is still absent: a supplied subdomain is not a connection.
        let (status, read) =
            call(&app, "alice", Method::GET, "/api/connections/zendesk", None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a tenant that has supplied a subdomain and no token has no connection: {read}",
        );

        // The value is where it belongs, and the store that holds it is a different one.
        let subdomain = exchange_host::DeclaredSetting::parse("default", "endpoint.subdomain")
            .expect("a well-formed binds target");
        assert!(settings.is_set(
            &Tenant::new("acme").expect("a plain tenant id"),
            "zendesk",
            &subdomain,
        ));
    }

    /// **The exfiltration path, refused at the surface a caller can actually reach.**
    ///
    /// `connection_settings.rs::a_setting_cannot_become_the_destination_authority` proves the store
    /// refuses it; this proves the *route* does, over HTTP, as the principal any agent token
    /// resolves to. Both are needed: the host-level test says the value cannot be stored, and this
    /// says there is no request that stores it.
    ///
    /// The three connectors are the ones whose host template pins no vendor suffix **and** whose
    /// catalogue entry declares no closed set of values, so the tenant's value would be the whole
    /// origin this host sends their credential to. `newrelic` was a fourth until X-70 measured its
    /// `config_choices`; the route test for what replaced it is
    /// [`no_route_lets_a_tenant_supply_a_value_the_catalogue_does_not_declare`].
    #[tokio::test]
    async fn no_route_lets_a_tenant_supply_a_connectors_whole_authority() {
        let (app, store, settings, _scratch) = configurable_app();

        for (connector, field) in [
            ("okta", "endpoint.domain"),
            ("docusign", "endpoint.account_host"),
            ("freshdesk", "endpoint.domain"),
        ] {
            let (status, body) = call(
                &app,
                "alice",
                Method::PUT,
                &format!("/api/connections/{connector}/settings/default/{field}"),
                Some(json!({ "value": "evil.example" })),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "`{connector}` accepted a value that would be its whole destination host: {body}",
            );
            assert_eq!(body["suppliable"], false, "{body}");
            assert!(
                body["host_template"]
                    .as_str()
                    .is_some_and(|t| t.contains('{')),
                "the refusal must quote the template that pins nothing: {body}",
            );

            // And the listing says the same thing, so a connector refused on purpose does not read
            // as a broken one.
            let (_, listed) = call(
                &app,
                "alice",
                Method::GET,
                &format!("/api/connections/{connector}/settings"),
                None,
            )
            .await;
            assert_eq!(
                listed["configurable"], false,
                "`{connector}` must report itself as not configurable: {listed}",
            );
            let row = listed["settings"]
                .as_array()
                .expect("an array")
                .iter()
                .find(|row| row["field"] == field)
                .expect("the field is listed even though it cannot be supplied");
            assert_eq!(row["suppliable"], false, "{listed}");
            assert!(
                row["reason"]
                    .as_str()
                    .is_some_and(|r| r.contains("no tenant may supply")),
                "the listing must say why, not merely that: {listed}",
            );
        }

        assert!(store.addresses().is_empty(), "{:?}", store.addresses());
        assert_eq!(
            settings.held_bytes(&Tenant::new("acme").expect("a plain tenant id")),
            0,
            "nothing was stored for any of them",
        );
    }

    /// **The closed set, over HTTP** — a region a tenant may choose, and nothing that merely looks
    /// like one (X-70).
    ///
    /// `connection_settings.rs::only_an_exactly_declared_choice_may_be_supplied` proves the store
    /// decides this; this proves the route carries the decision out to a caller as a `422` that
    /// says what would have worked. The pairing matters more here than usual: a build that refused
    /// everything would satisfy the second half alone, which is exactly the state this story found.
    #[tokio::test]
    async fn no_route_lets_a_tenant_supply_a_value_the_catalogue_does_not_declare() {
        let (app, _store, _settings, _scratch) = configurable_app();
        let address = "/api/connections/intercom/settings/default/endpoint.host";

        let (status, body) = call(
            &app,
            "alice",
            Method::PUT,
            address,
            Some(json!({ "value": "api.eu.intercom.io" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a region intercom's own catalogue entry declares must be suppliable: {body}",
        );

        for refused in [
            "api.eu.intercom.io.evil.example",
            "API.EU.INTERCOM.IO",
            " api.eu.intercom.io",
            "evil.example",
        ] {
            let (status, body) = call(
                &app,
                "alice",
                Method::PUT,
                address,
                Some(json!({ "value": refused })),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "`{refused}` is not one of intercom's declared regions: {body}",
            );
            assert_eq!(
                body["choices"],
                json!([
                    "api.intercom.io",
                    "api.eu.intercom.io",
                    "api.au.intercom.io"
                ]),
                "the refusal must say what would have worked, out of the catalogue's own data: \
                 {body}",
            );
        }

        // And the listing says this address takes a value, so an operator is not told it is one of
        // the connectors nobody may configure.
        let (_, listed) = call(
            &app,
            "alice",
            Method::GET,
            "/api/connections/intercom/settings",
            None,
        )
        .await;
        assert_eq!(listed["configurable"], true, "{listed}");
    }

    /// A tenant sitting on its settings allowance can still replace a value with one the same size.
    ///
    /// **The regression the first cut shipped.** This handler ran its own
    /// `admit_tenant_settings(held, value.len())` before the store's, without subtracting what the
    /// write replaced — directly under a comment claiming it did. `SettingsStore::set` is
    /// replace-aware, so the two disagreed and the route's, being first, won. An operator whose
    /// subdomain had changed would have been told to *remove* a setting in order to change one.
    ///
    /// It is only observable near the bound, which is why this fills the allowance first: with a
    /// tenant holding 15 KiB of a 16 KiB allowance, `held + 1 KiB` exceeds it while
    /// `held - replaced + 1 KiB` does not, and those are exactly the two readings that disagreed.
    #[tokio::test]
    async fn a_tenant_at_its_settings_allowance_can_still_replace_a_value() {
        let (app, _store, settings, _scratch) = configurable_app();
        let acme = Tenant::new("acme").expect("a plain tenant id");
        let full = "x".repeat(exchange_host::MAX_SETTING_VALUE_BYTES);

        // Every address this host will actually accept **this** value at, across the whole
        // catalogue — one connector does not have enough of them to reach a per-tenant bound.
        // Asked with the value in hand rather than as `tenant_may_supply`, because a field whose
        // values are a catalogue-declared closed set is suppliable and still refuses a kilobyte of
        // `x` (X-70).
        let addresses: Vec<(String, String, String)> = connector_catalog::providers()
            .iter()
            .flat_map(|provider| {
                declared_settings(provider)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|setting| host_pinning(provider, setting).admits(&full))
                    .map(|setting| {
                        (
                            provider.id.to_owned(),
                            setting.service.clone(),
                            setting.binds(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        // Fill to within one value of the allowance, and remember the last address that landed.
        let mut last = None;
        for (connector, service, field) in &addresses {
            if settings.held_bytes(&acme) + full.len() > exchange_host::MAX_TENANT_SETTINGS_BYTES {
                break;
            }
            let (status, body) = call(
                &app,
                "alice",
                Method::PUT,
                &format!("/api/connections/{connector}/settings/{service}/{field}"),
                Some(json!({ "value": full })),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            last = Some((connector.clone(), service.clone(), field.clone()));
        }

        let held = settings.held_bytes(&acme);
        let (connector, service, field) = last.expect("the catalogue offers enough addresses");
        assert!(
            held + full.len() > exchange_host::MAX_TENANT_SETTINGS_BYTES,
            "the tenant must be near enough the bound for the two readings to disagree; held {held}",
        );

        // Replace one of them with a value the same size. Under the shipped bug this answered 409.
        let (status, body) = call(
            &app,
            "alice",
            Method::PUT,
            &format!("/api/connections/{connector}/settings/{service}/{field}"),
            Some(json!({ "value": "y".repeat(exchange_host::MAX_SETTING_VALUE_BYTES) })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "replacing a value with one the same size spends no allowance: {body}",
        );
        assert_eq!(
            settings.held_bytes(&acme),
            held,
            "and the tenant occupies exactly what it did before",
        );

        // The bound still binds: a *new* address past the allowance is refused, so deleting the
        // route's duplicate check did not delete the rule.
        let mut refused = None;
        for (connector, service, field) in &addresses {
            let (status, body) = call(
                &app,
                "alice",
                Method::PUT,
                &format!("/api/connections/{connector}/settings/{service}/{field}"),
                Some(json!({ "value": full })),
            )
            .await;
            if status == StatusCode::CONFLICT {
                refused = Some(body);
                break;
            }
        }
        let refused = refused.expect("the tenant allowance must still be reachable");
        assert_eq!(refused["bound"], "tenant_settings", "{refused}");
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|e| e.contains("remove a setting")),
            "and its remedy is about settings, not credentials: {refused}",
        );
    }

    /// The behaviour behind admitting `{service}` and `{field}` as path parameters: neither can
    /// steer where a value lands, and neither reaches anything by being shaped like an address.
    ///
    /// X-39's [`a_hostile_credential_name_cannot_reach_the_address`] is the shape this follows, and
    /// the reason is the same: widening the allowed-parameter list is an argument about *behaviour*,
    /// so it is paid for with a test rather than with a name on a list. Every value below is refused
    /// before anything is stored — by the declared-surface lookup itself, rather than by a filter
    /// somebody has to keep in step.
    #[tokio::test]
    async fn a_hostile_service_or_field_cannot_reach_the_settings_address() {
        let (app, store, settings, _scratch) = configurable_app();

        let hostile = [
            // Traversal, in each segment.
            ("..", "endpoint.subdomain"),
            ("default", "endpoint...%2F..%2Fetc"),
            // A rendered credential path, and the credential vocabulary itself — the row of the
            // `binds` table that is a *secret* and must not become storable here.
            ("default", "credential.zendesk.api_token"),
            ("default", "oauth.client_secret"),
            // Another connector's service, and a field of the right shape nothing declares.
            ("management", "endpoint.space_id"),
            ("default", "endpoint.base_url"),
        ];

        for (service, field) in hostile {
            let (status, body) = call(
                &app,
                "alice",
                Method::PUT,
                &format!("/api/connections/zendesk/settings/{service}/{field}"),
                Some(json!({ "value": "evil.example" })),
            )
            .await;

            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "`{service}/{field}` was not refused: {body}",
            );
        }

        // Nothing reached either store. The refusals above could each be right for the wrong
        // reason; this is the assertion that says nothing was written whatever the reason was.
        assert!(store.addresses().is_empty(), "{:?}", store.addresses());
        assert_eq!(
            settings.held_bytes(&Tenant::new("acme").expect("a plain tenant id")),
            0
        );
    }

    /// A setting lands under the **resolved principal's** tenant, and one tenant's is not another's.
    ///
    /// The settings half of `a_tenant_cannot_reach_another_tenants_connection`. There is deliberately
    /// no vector here by which `acme` could name `globex`'s value: no route takes a tenant, so the
    /// strongest thing either can do is ask for the same connector and see its own answer.
    #[tokio::test]
    async fn a_setting_belongs_to_the_resolved_principals_tenant() {
        let (app, _store, settings, _scratch) = configurable_app();

        let (status, _) = call(
            &app,
            "bob",
            Method::PUT,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            Some(json!({ "value": "globex" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (_, acme) = call(
            &app,
            "alice",
            Method::GET,
            "/api/connections/zendesk/settings",
            None,
        )
        .await;
        assert_eq!(
            acme["settings"][0]["set"], false,
            "globex's subdomain must not appear as acme's: {acme}",
        );

        let subdomain = exchange_host::DeclaredSetting::parse("default", "endpoint.subdomain")
            .expect("a well-formed binds target");
        assert!(settings.is_set(
            &Tenant::new("globex").expect("a plain tenant id"),
            "zendesk",
            &subdomain,
        ));
        assert!(!settings.is_set(
            &Tenant::new("acme").expect("a plain tenant id"),
            "zendesk",
            &subdomain,
        ));
    }

    /// **The reopened story's failing-first test.** An agent may not write a connection setting, and
    /// the refusal reaches an operator's log.
    ///
    /// `AGENTS.md` § Invariants, verbatim: *"An agent's token grants access to an operation, never
    /// to a credential."* The settings write route was [`Access::Principal`], which admits every
    /// kind, so an agent holding nothing but an operation grant could store the `{subdomain}` that
    /// composes zendesk's destination origin — and the tenant's credential was then dispatched to
    /// an origin the agent had chosen. A suffix pin keeps that origin inside `*.zendesk.com`, which
    /// is a **registrable namespace**: it constrains which vendor the request reaches, not whose
    /// account at that vendor.
    ///
    /// Three things are asserted, and the second is what makes the first mean anything:
    ///
    /// - every write verb at every field is refused for an agent — see [`MAY_CONFIGURE`] for why
    ///   the gate is the whole write surface rather than the host-shaped fields alone;
    /// - **a `User` reaches the same address and is admitted**, so what refused the agent is its
    ///   kind and not a route that refuses everyone;
    /// - the refusal is **logged**, by the guard's own `warn!` rather than by anything this module
    ///   added. An agent reaching for a route only a human may call is the shape of a leaked token
    ///   being used, and an operator who cannot see it happening has nothing to revoke.
    #[tokio::test]
    async fn an_agent_may_not_write_a_connection_setting_and_the_refusal_is_logged() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let (app, store, settings, _scratch) = configurable_app();
        let acme = Tenant::new("acme").expect("a plain tenant id");

        let warnings = Warnings::default();
        let _log =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));

        // Both kinds of field, and both verbs. `endpoint.subdomain` is the one that moves the
        // origin; `username.zendesk.api_token` moves nothing and is refused anyway, which is the
        // decision [`MAY_CONFIGURE`] records.
        let attempts = [
            (
                Method::PUT,
                "endpoint.subdomain",
                Some(json!({ "value": "attacker-controlled" })),
            ),
            (
                Method::PUT,
                "username.zendesk.api_token",
                Some(json!({ "value": "ops@acme.test" })),
            ),
            (Method::DELETE, "endpoint.subdomain", None),
        ];

        for (method, field, body) in attempts {
            let (status, answered) = call(
                &app,
                "triage-bot",
                method.clone(),
                &format!("/api/connections/zendesk/settings/default/{field}"),
                body,
            )
            .await;

            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "an agent reached `{method} {field}`, which is how a token for an operation becomes \
                 delivery of this tenant's credential to an origin the caller chose: {answered}",
            );
            assert!(
                answered["error"]
                    .as_str()
                    .is_some_and(|error| error.contains("user")),
                "the refusal must name the kind that would have worked: {answered}",
            );
            assert!(
                !answered.to_string().contains("attacker-controlled"),
                "and must never repeat the value it refused: {answered}",
            );
        }

        // Nothing reached either store, whatever the refusals said.
        assert!(store.addresses().is_empty(), "{:?}", store.addresses());
        assert_eq!(settings.held_bytes(&acme), 0);

        // **The control.** A `User` of the same tenant writes the same field at the same address.
        // Without this the three refusals above are satisfied by a route that refuses everybody.
        let (status, supplied) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            Some(json!({ "value": "acme" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "only the caller's kind may explain the difference: {supplied}",
        );

        // And the log an operator watches.
        let logged = warnings.kind_refusals();
        assert_eq!(
            logged.len(),
            3,
            "each refusal must be visible to an operator, not only to the caller: {logged:?}",
        );
        for line in &logged {
            assert!(
                line.contains("triage-bot") && line.contains("acme"),
                "the line must name the caller and its tenant, so there is something to revoke: \
                 {line}",
            );
            assert!(
                !line.contains("attacker-controlled"),
                "the caller's own id and tenant belong in the log; the value it offered does not: \
                 {line}",
            );
        }
    }

    /// **X-54's failing-first test.** An agent may not create a connection and may not rotate a
    /// credential, and each refusal reaches an operator's log.
    ///
    /// The neighbour X-47 ring-fenced. The settings write was gated because a value written there
    /// is substituted into the operation's own request; these two are the routes that write the
    /// **credential itself**, and they were left [`Access::Principal`] — so an agent holding
    /// nothing but an operation grant could put a value it controls at the address its tenant's
    /// operations then run under. Not the invariant's *"never to a credential"* read as reading one
    /// out; the substitution in the other direction, which the invariant's sentence does not name
    /// and which [`MAY_SUPPLY_A_CREDENTIAL`] argues is inside it anyway.
    ///
    /// **What makes it worse than the `DELETE` beside it, which stays open to every kind:** a
    /// planted or rotated credential leaves no trace of who put it there. `GET /api/connections`
    /// answers `held: true` either way, this module keeps no record beside the store, and revoking
    /// the agent's token does not take the value back out. A destroyed connection is visible and
    /// the operator holds the plaintext to restore it; a substituted one is invisible and the
    /// operator has nothing telling them to look.
    ///
    /// Four things are asserted, and the controls are what make the refusals mean anything:
    ///
    /// - `POST` is refused for an agent where the tenant holds no connection at all — which at the
    ///   base of this story answered `201` and left the agent's value at the tenant's address;
    /// - `PUT .../credentials/{credential}` is refused for an agent over a credential a **human**
    ///   supplied, and the value at the address afterwards is still the human's;
    /// - a `User` of the same tenant reaches both of the same addresses and is admitted, so what
    ///   refused the agent is its kind and not a route that refuses everyone;
    /// - both refusals are **logged**, by the guard's own `warn!`, because an agent reaching for a
    ///   route only a human may call is the shape of a leaked token being used.
    #[tokio::test]
    async fn an_agent_may_not_create_a_connection_or_rotate_a_credential_and_the_refusal_is_logged()
    {
        use tracing_subscriber::layer::SubscriberExt as _;

        /// What the agent offers. Distinct from [`SENTINEL`] so an assertion about the address can
        /// say **whose** value is at it, rather than only that something is.
        const SUBSTITUTED: &str = "SUBSTITUTED-NOT-THE-TENANTS-SECRET";
        /// What the human's own rotation puts there, for the second control.
        const ROTATED: &str = "ROTATED-NOT-A-REAL-SECRET-EITHER";

        const ADDRESS: &str = "tenants/acme/com.zendesk.api/api_token";

        let (app, store) = connected_app();

        let warnings = Warnings::default();
        let _log =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));

        // The tenant holds nothing, which is exactly where `create` writes.
        let (status, answered) = call(
            &app,
            "triage-bot",
            Method::POST,
            "/api/connections/zendesk",
            Some(json!({ "credentials": { "zendesk.api_token": SUBSTITUTED } })),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "an agent created this tenant's connection, so every operation the tenant runs against \
             zendesk now runs under a credential the agent chose: {answered}",
        );
        assert!(
            answered["error"]
                .as_str()
                .is_some_and(|error| error.contains("user")),
            "the refusal must name the kind that would have worked: {answered}",
        );
        assert!(
            !answered.to_string().contains(SUBSTITUTED),
            "and must never repeat the value it refused: {answered}",
        );
        assert!(store.addresses().is_empty(), "{:?}", store.addresses());

        // **The first control.** A `User` of the same tenant connects the same connector, which is
        // the same handler at the same address — so only the caller's kind differs.
        let (status, created) = connect_zendesk(&app, "alice").await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "only the caller's kind may explain the difference: {created}",
        );

        // And now the credential a human supplied is there to be replaced.
        let (status, answered) = call(
            &app,
            "triage-bot",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": SUBSTITUTED })),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "an agent replaced this tenant's credential in place, which `GET /api/connections` \
             cannot tell from the one the human supplied: {answered}",
        );
        assert!(
            answered["error"]
                .as_str()
                .is_some_and(|error| error.contains("user")),
            "the refusal must name the kind that would have worked: {answered}",
        );
        assert!(
            !answered.to_string().contains(SUBSTITUTED),
            "and must never repeat the value it refused: {answered}",
        );
        assert_eq!(
            store.at(ADDRESS).as_deref(),
            Some(SENTINEL),
            "the value this tenant's operations run under must be the one a human put there",
        );

        // **The second control**, for the rotation the way the first was for the create.
        let (status, rotated) = call(
            &app,
            "alice",
            Method::PUT,
            "/api/connections/zendesk/credentials/zendesk.api_token",
            Some(json!({ "value": ROTATED })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "only the caller's kind may explain the difference: {rotated}",
        );
        assert_eq!(store.at(ADDRESS).as_deref(), Some(ROTATED));

        // And the log an operator watches.
        let logged = warnings.kind_refusals();
        assert_eq!(
            logged.len(),
            2,
            "each refusal must be visible to an operator, not only to the caller: {logged:?}",
        );
        for line in &logged {
            assert!(
                line.contains("triage-bot") && line.contains("acme"),
                "the line must name the caller and its tenant, so there is something to revoke: \
                 {line}",
            );
            assert!(
                !line.contains(SUBSTITUTED),
                "the caller's own id and tenant belong in the log; the value it offered does not: \
                 {line}",
            );
        }
    }

    /// The other half of X-54's decision, and the reason the gate is declared per **method** rather
    /// than over the path: reading a connection and destroying one stay open to every kind.
    ///
    /// Without this, `an_agent_may_not_create_a_connection_or_rotate_a_credential_and_the_refusal_is_logged`
    /// is satisfied just as happily by gating `/api/connections/{connector}` whole — which would
    /// take `GET` and `DELETE` with it, silently reversing a decision X-40 wrote down and argued
    /// (`crate::routes::agents`, § *How far the argument reaches, and where it stops*).
    ///
    /// - **The two reads** answer addresses and a `held` boolean and no value at all, and an agent
    ///   that can see *"this tenant has no zendesk connection"* is one that can say so instead of
    ///   failing an invocation for a reason nobody can act on. Same argument the settings `GET`
    ///   collection is open on.
    /// - **`DELETE`** destroys tenant data inside the tenant the caller already belongs to, an
    ///   operator can see it and undo it by reconnecting, and nothing about it outlives revocation
    ///   of the token that did it. Whether an agent should reach a destructive route at all is the
    ///   grant-shaped question, which is X-13's.
    #[tokio::test]
    async fn an_agent_may_still_read_a_connection_and_disconnect_one() {
        let (app, store) = connected_app();

        let (status, created) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{created}");

        for path in ["/api/connections", "/api/connections/zendesk"] {
            let (status, read) = call(&app, "triage-bot", Method::GET, path, None).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "an agent must be able to see whether its tenant is connected: {read}",
            );
            assert!(
                !read.to_string().contains(SENTINEL),
                "and never the value, which is what makes the read safe to leave open: {read}",
            );
        }

        let (status, _) = call(
            &app,
            "triage-bot",
            Method::DELETE,
            "/api/connections/zendesk",
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "destroying a connection is the grant-shaped question (X-13), not the kind-shaped one",
        );
        assert!(store.addresses().is_empty(), "{:?}", store.addresses());
    }

    /// **The end to end.** An agent cannot cause a dispatch to an origin it named.
    ///
    /// This is the re-review's measured path, re-driven: an agent principal stores
    /// `endpoint.subdomain` and invokes zendesk, and the credential that was on the wire —
    /// `Basic …ops@acme.test/token:…` at `https://attacker-controlled.zendesk.com` — is what this
    /// asserts cannot happen.
    ///
    /// The whole chain runs against **one** composition, so nothing here is arranged: the credential
    /// a human connected, the store the write would have landed in, and the invoker that reads it
    /// are the same three objects. `sent` is the measurement rather than a fake transport's call
    /// count — it is `exchange_host::Sent`, decided from where the failure happened, and `"no"` is
    /// this host's own answer to *did anything go on the wire*.
    ///
    /// The write is asserted **before** the invocation on purpose. A run against code without the
    /// gate stops at the first assertion, having stored nothing and dispatched nothing — which is
    /// what keeps a red run of this test from sending a request to a host somebody could register.
    #[tokio::test]
    async fn an_agent_cannot_cause_a_dispatch_to_an_origin_it_named() {
        let (app, store, settings, _scratch) = dispatching_app();
        let acme = Tenant::new("acme").expect("a plain tenant id");

        // A human wires the connection up. This is the credential the path was after.
        let (status, created) = connect_zendesk(&app, "alice").await;
        assert_eq!(status, StatusCode::CREATED, "{created}");

        // The agent names the origin. Zendesk's template pins `.zendesk.com`, so the composed
        // authority stays at the vendor — and `attacker-controlled.zendesk.com` is a subdomain
        // anybody can sign up for, which is the whole reason a suffix pin is not a safety argument.
        let (status, refused) = call(
            &app,
            "triage-bot",
            Method::PUT,
            "/api/connections/zendesk/settings/default/endpoint.subdomain",
            Some(json!({ "value": "attacker-controlled" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "an agent stored the origin this host will send a credential to: {refused}",
        );
        assert_eq!(
            settings.held_bytes(&acme),
            0,
            "the value must not be in the store the invoker reads",
        );

        // And the invocation the agent *is* entitled to make reaches no origin at all. It refuses
        // by name, terminally, with nothing sent — X-12's behaviour, unchanged by any of this.
        let (status, answered) = call(
            &app,
            "triage-bot",
            Method::POST,
            "/api/operations/zendesk-ticket-show/invoke",
            Some(json!({ "ticket_id": "1" })),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answered}");
        assert_eq!(
            answered["sent"], "no",
            "a request went out for an unconfigured connection: {answered}",
        );
        assert_eq!(answered["retryable"], false, "{answered}");
        assert!(
            answered["message"]
                .as_str()
                .is_some_and(|message| message.contains("zendesk")),
            "the refusal must still name what an operator has to go and supply: {answered}",
        );
        assert!(
            !answered.to_string().contains(SENTINEL),
            "and must never repeat a credential: {answered}",
        );

        // The credential is exactly where the human put it, and nowhere else.
        assert_eq!(
            store.addresses(),
            vec!["tenants/acme/com.zendesk.api/api_token".to_owned()],
            "{:?}",
            store.addresses(),
        );
    }

    /// A composition that bound no settings store refuses and names the setting that would have
    /// given it one — it does not accept the value and drop it.
    ///
    /// X-09's rule at the surface that would have used it, and the message says what the file is
    /// *for*: an operator who has just read the credential-store refusal would otherwise assume this
    /// one is about secrets too.
    #[tokio::test]
    async fn no_settings_store_bound_refuses_and_names_the_setting() {
        let (app, _store) = connected_app();

        for (method, body) in [
            (Method::GET, None),
            (Method::PUT, Some(json!({ "value": "acme" }))),
        ] {
            let path = if method == Method::GET {
                "/api/connections/zendesk/settings".to_owned()
            } else {
                "/api/connections/zendesk/settings/default/endpoint.subdomain".to_owned()
            };

            let (status, body) = call(&app, "alice", method.clone(), &path, body).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
            assert_eq!(body["setting"], SETTINGS_SETTING, "{body}");
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|error| error.contains("no secrets")),
                "the refusal must say what this store is for: {body}",
            );
        }
    }

    /// An unknown connector is a `404` here too, and the settings routes refuse an anonymous caller
    /// the way every other route in this module does.
    #[tokio::test]
    async fn the_settings_routes_refuse_an_unknown_connector() {
        let (app, _store, _settings, _scratch) = configurable_app();

        let (status, body) = call(
            &app,
            "alice",
            Method::GET,
            "/api/connections/nosuchvendor/settings",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["connector"], "nosuchvendor");
    }

    /// The Acceptance's "no route accepts an address", stated over this module's own declaration.
    ///
    /// `super::super::tests::no_published_route_takes_a_tenant_in_its_path` covers the tenant
    /// segment over the whole surface — X-03 wrote it saying X-10 would inherit it, and this is
    /// that inheritance made explicit. This one covers the rest of an address: a path parameter
    /// that could carry an authority, a credential or a rendered store path.
    ///
    /// **X-39 widened the allowed set by one name and paid for it.** `{credential}` names the
    /// credential to rotate, and it is admitted on the same argument `{connector}` is: it is a key
    /// into a declaration this host compiled in, not a component of an address. The catalogue
    /// answers a lookup of it with the `leaf`, and the leaf is what the address carries. That is an
    /// argument about behaviour rather than about spelling, so it is not left to this list —
    /// [`a_hostile_credential_name_cannot_reach_the_address`] drives a name shaped like an address
    /// straight at the route and asserts it reaches nothing.
    ///
    /// **X-47 widened it by two more names and paid the same price.** `{service}` and `{field}`
    /// name a connection setting, and they are admitted on `{credential}`'s argument rather than on
    /// a new one: both are keys into what the connector's own operations declare — `declared_settings`
    /// derives the set, and a name outside it is refused before anything is stored — and neither
    /// reaches a path anywhere, because the settings store renders no filesystem path from either.
    /// The behavioural half is [`a_hostile_service_or_field_cannot_reach_the_settings_address`],
    /// which drives address-shaped and traversing values straight at the route.
    #[test]
    fn no_route_here_accepts_an_address() {
        /// What a path here may name, each because the catalogue is what resolves it.
        const KEYS: &[&str] = &["connector", "credential", "service", "field"];

        for route in MODULE.routes {
            for parameter in route
                .path
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{'))
                .filter_map(|segment| segment.strip_suffix('}'))
            {
                assert!(
                    KEYS.contains(&parameter),
                    "a path here may name only a catalogue key, and `{parameter}` is not one of \
                     {KEYS:?}: {}",
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

    /// The behaviour behind admitting `{credential}` as a path parameter: a caller cannot steer
    /// where a value lands by what it puts there, and cannot learn anything by trying.
    ///
    /// Each name below is one the catalogue does not declare, so each is refused before an address
    /// is composed — by the declaration lookup itself rather than by a filter somebody has to keep
    /// in step with the addressing scheme.
    ///
    /// **Two assertions, because "no refusal names another tenant's address" needs both.** The
    /// refusal does echo the undeclared name back, which is how `UndeclaredCredential` has always
    /// read and is the caller's own input rather than something this host looked up — so the
    /// property that actually matters is that the answer is *the same either way*: the whole run is
    /// made twice, once with the other tenant holding a `zendesk` connection and once without, and
    /// the refusals must be identical. A mirror is not an oracle. And the store is untouched in
    /// both, so none of these wrote anywhere, least of all at the address they spell.
    #[tokio::test]
    async fn a_hostile_credential_name_cannot_reach_the_address() {
        /// Names shaped like the thing a caller must not be able to reach.
        const HOSTILE: &[&str] = &[
            // A rendered address, for another tenant and for this one.
            "tenants%2Fglobex%2Fcom.zendesk.api%2Fapi_token",
            "tenants%2Facme%2Fcom.zendesk.api%2Fapi_token",
            // A traversal out of the leaf position.
            "..%2F..%2F..%2Fglobex%2Fcom.zendesk.api%2Fapi_token",
            // The leaf itself, which is what the *address* carries. A caller names the
            // flat-namespace name; the leaf alone is not one.
            "api_token",
        ];

        /// What the hostile rotations try to plant.
        const PLANTED: &str = "PLANTED-NOT-A-REAL-SECRET";

        /// Drive every hostile name as `alice`, optionally with the other tenant connected, and
        /// hand back what the caller saw and what the store holds afterwards.
        async fn attempt(globex_is_connected: bool) -> (Vec<(StatusCode, Value)>, Vec<String>) {
            let (app, store) = connected_app();

            let (status, _) = connect_zendesk(&app, "alice").await;
            assert_eq!(status, StatusCode::CREATED);
            if globex_is_connected {
                let (status, _) = connect_zendesk(&app, "bob").await;
                assert_eq!(status, StatusCode::CREATED);
            }

            let mut answers = Vec::new();
            for hostile in HOSTILE {
                answers.push(
                    call(
                        &app,
                        "alice",
                        Method::PUT,
                        &format!("/api/connections/zendesk/credentials/{hostile}"),
                        Some(json!({ "value": PLANTED })),
                    )
                    .await,
                );
            }

            (answers, store.addresses())
        }

        let (refused, occupied) = attempt(true).await;
        let (refused_alone, occupied_alone) = attempt(false).await;

        for (hostile, (status, refusal)) in HOSTILE.iter().zip(&refused) {
            assert_eq!(
                *status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "`{hostile}` is not a name zendesk declares, so it has no address: {refusal}",
            );
            assert!(
                !refusal.to_string().contains(PLANTED),
                "a refusal must never repeat the value it refused: {refusal}",
            );
        }

        assert_eq!(
            refused, refused_alone,
            "the refusals differ depending on whether another tenant holds a connection, which \
             makes this route an oracle for exactly the fact it must not disclose",
        );

        assert_eq!(
            occupied_alone,
            vec!["tenants/acme/com.zendesk.api/api_token".to_string()],
            "an undeclared credential name must have written nowhere at all",
        );
        assert_eq!(
            occupied,
            vec![
                "tenants/acme/com.zendesk.api/api_token".to_string(),
                "tenants/globex/com.zendesk.api/api_token".to_string(),
            ],
            "and must not have reached the address it spells, in either tenant",
        );
    }

    /// Every route requires a principal, and the ones that require a particular **kind** are named
    /// here beside the constant each is declared with.
    ///
    /// Asserted here as well as in the surface-wide enumeration, because that one compares against a
    /// list somebody edits and this one cannot be satisfied by editing a list: [`Access::Anonymous`]
    /// has no arm here at all, so a connection route that stopped requiring a principal is a failure
    /// rather than a new line somewhere.
    ///
    /// The gated routes are named rather than counted, in both directions. One appearing is a
    /// decision about who may reach a tenant's connections and should cost whoever makes it a line
    /// with a reason; one *disappearing* is the same decision undone, which is the direction that
    /// matters — see [`MAY_SUPPLY_A_CREDENTIAL`] and [`MAY_CONFIGURE`].
    ///
    /// **`/api/connections/{connector}` appears once, for its `POST`.** The entry declared beside
    /// it in [`MODULE`] carries `GET` and `DELETE` at [`Access::Principal`], which is X-40's
    /// decision left standing rather than swept up by X-54's;
    /// [`tests::an_agent_may_still_read_a_connection_and_disconnect_one`] is the behavioural half of
    /// that, since this test alone cannot tell a path gated for one verb from one gated whole.
    #[test]
    fn every_route_here_requires_a_principal_and_the_kind_gated_ones_are_named() {
        let gated: Vec<(&str, &[PrincipalKind])> = MODULE
            .routes
            .iter()
            .filter_map(|route| match route.access {
                Access::Principal => None,
                Access::PrincipalOfKind(kinds) => Some((route.path, kinds)),
                Access::Anonymous => panic!(
                    "a connection is tenant data and answers no caller this host cannot identify: \
                     {}",
                    route.path,
                ),
            })
            .collect();

        assert_eq!(
            gated,
            vec![
                ("/api/connections/{connector}", MAY_SUPPLY_A_CREDENTIAL),
                (
                    "/api/connections/{connector}/credentials/{credential}",
                    MAY_SUPPLY_A_CREDENTIAL,
                ),
                (
                    "/api/connections/{connector}/settings/{service}/{field}",
                    MAY_CONFIGURE,
                ),
            ],
            "who may reach a tenant's connections changed; every entry is a decision that belongs \
             beside the constant it names rather than only in a route table, and these are what \
             are gated: {gated:?}",
        );
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
