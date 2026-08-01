---
story: X-10
status: accepted
---

# Connections: an address the caller cannot name, and a refusal where the address is incomplete

How a tenant's connection to a connector is created, listed and destroyed, and — the part that took
the most argument — what happens at the one place the address scheme does not yet reach.

Extends [`identity-and-session.md`](identity-and-session.md), which made a request carry a resolved
`Principal`, and builds on X-09's `CredentialStore`. The rule everything below answers to is
`AGENTS.md` § Invariants, first entry:

> **The tenant comes from the resolved principal and from nothing a caller controls.** Not a path
> segment, not a body field, not a header.

and its last:

> **Refuse; never repair.** … each refuses and names the address, never the value.

## The address is derived, and there is no route that could accept one

**Decision: the credential address is a function of `(resolved tenant, connector's declared
authority, connector's declared credential leaf)`, and no route takes an address, a tenant or a
path.**

```text
tenants/<tenant>/<authority>/<credential>
        ^^^^^^^^ ^^^^^^^^^^^ ^^^^^^^^^^^^
        |        |           `Credential::leaf`, from the catalogue
        |        `Provider::authority`, from the catalogue
        `Principal::tenant(), from the guard
```

None of the three comes from the request. The surface is:

| Route | Method | What the caller supplies |
| --- | --- | --- |
| `/api/connections` | `GET` | nothing |
| `/api/connections/{connector}` | `GET`, `POST`, `DELETE` | a **connector id**, and on `POST` a body of credential *values* |

`{connector}` is a catalogue key — `zendesk`, `slack` — looked up in the compiled-in
`connector_catalog` and refused with `404` when nothing declares it. It is not an address and it
cannot become one: it never reaches a path segment of the credential address, only the
`authority`/`leaf` pair the catalogue answers with.

Two structural tests already on the surface do the enforcement, and X-10 adds nothing to either
because it does not need to:
`routes::tests::no_published_route_takes_a_tenant_in_its_path` walks `published()` (X-03 wrote it
saying X-10 would inherit it, and X-10 does), and
`the_anonymous_surface_is_only_what_was_declared_anonymous` needs no new `ANONYMOUS` entry, because
both routes here are `Access::Principal`. A connection is tenant data; there is no version of it
that answers a caller this host has not identified.

### Rendering the address once

`CredentialRef` is the addressing type, and `TenantLayout` is the one thing that renders it into a
path. Both come from `connector-spec` by way of `connector-secrets`, and `exchange_host` now
re-exports `CredentialRef`, `Secret`, `SecretStore` and `StoreError` for the same reason it already
re-exports `async_trait`: a composition should not have to name a second crate, at a version it has
to guess, to spell one address. `FileStore` renders with `TenantLayout` too, so the path a refusal
quotes is the path the store actually looked at — not a second spelling of it.

## The gap this story does not close, and turns into a refusal

**Decision: a second connection to a connector the tenant already has is refused with `409`, naming
X-14 and the instance dimension that would have made it work.**

This is the load-bearing decision of the story, and it is a decision *against* shipping the
Acceptance's address as though it were complete.

`tenants/<tenant>/<authority>/<credential>` has no place to say *which* Zendesk. A tenant with a
sandbox subdomain and a production one renders **one** address for both, because nothing in the
address varies per connection. Left alone, the second `POST` would overwrite the first, return
`201`, and every later call would reach the wrong account while looking entirely healthy. That is
the exact failure mode `AGENTS.md` § Invariants describes — "a store that falls back to memory, or a
mode that is quietly tightened, hides the thing you needed to know" — with the value silently
replaced instead of the mode silently widened.

The obvious local fix is to add an instance segment here. **It is refused**, and X-14 says why: the
address scheme is one scheme shared with `connector-spec`, `connector-secrets` and every other
consumer of `CredentialRef`. Two spellings of an address is how two components stop agreeing where a
credential lives, and this repository would be the one that forked it.

### The dimension has landed upstream, and is not published

flux-connectors **C-406** merged the instance level (`14b5dc7` on its `main`):

```text
tenants/<tenant>/<authority>[/@instances/<uuid>][/<service>]/<credential>
```

`InstanceId` is a validated newtype over a canonical lowercase hyphenated uuid, and the nil uuid is
refused because "no instance" is already spelled by omitting the level. The `@instances` marker
carries the `@` deliberately: it is unspellable in every component grammar, so the level cannot be
forged and no service or credential name is reserved away — a bare uuid segment would have been
ambiguous with a service, a uuid being a well-formed service name. Existing single-instance
addresses are byte-identical, so nothing in this story's rendering changes when the pin moves.

**It is not published.** crates.io serves `codewandler-connector-spec` 0.8.0, this workspace pins
`"0.8"` from the registry, and the pin is fenced. So the level cannot be spelled here yet — but the
refusal can name it, and the derivation can leave it one insertion away.

So the gap becomes a refusal that says what is coming:

```json
{
  "error": "this tenant already has a connection to connector `zendesk`, and the credential address has no instance dimension to tell two of them apart",
  "connector": "zendesk",
  "address": "tenants/acme/com.zendesk.api/api_token",
  "would_have_worked": "an instance level on the address — `tenants/<tenant>/<authority>/@instances/<uuid>/<credential>` — which has landed in flux-connectors (C-406) and is not published yet; this host pins connector-spec 0.8. Wiring it up, including resolving a name you choose to that uuid, is X-14. Until then, delete the existing connection before creating another"
}
```

> **This refusal is the placeholder for the `@instances` level.** Whoever lands X-14 replaces
> `Refusal::AlreadyConnected` in `routes::connections` with an address that carries the instance,
> extends `ConnectorDeclaration::address_of_declared` in `exchange-host` — the single place an
> address is composed, and the seam left for exactly this — and deletes
> `a_second_connection_to_one_connector_is_refused_rather_than_overwriting` along with it. Grep for
> `@instances`.

Upstream's `TenantInstances` states the rule the host then has to satisfy — elide at one, the named
one at several, refuse when several and none is named, refuse a uuid the tenant does not hold — and
records that the label → uuid mapping is the host's job. That is X-14's, and it belongs in
`address_of_declared` rather than beside it, which is why nothing here models a uuid or an instance
today.

The single-instance tenant — the case that works today — works fully: create, list, read and delete
are all unaffected by the refusal, and the tests cover them end to end.

### What a refusal names

The address, never the value. Every refusal in this module quotes the rendered path and never the
secret, and `no_refusal_carries_a_credential_value` drives the whole module's failure paths with a
sentinel value stored and asserts the sentinel appears in no response body. The refusal that matters
most is the cross-tenant one, and it is worth being precise about what it can say: a caller asking
for a connection its tenant does not have is answered with **its own** derived address, because that
is where this host looked. It never names the other tenant, the other tenant's address, or the fact
that somebody else has one.

## A connection is what the credential store says it is

**Decision: there is no second record. A connection exists exactly when the store holds a value at
one of the addresses derived for that tenant and connector.**

The alternative — a `connections` table beside the credential store — is one more thing that can
disagree with the store, and the disagreement is not symmetric: a record with no credential is a
connection that `401`s at the vendor, and a credential with no record is a live credential nothing
lists. `DELETE` destroying the credential is then not a step that could be forgotten; it is the
whole of what deleting means. That is the Acceptance item "deleting a connection destroys its
credential", satisfied by construction rather than by remembering.

The cost is stated rather than hidden. `SecretStore` is `get`/`put`/`delete` and has no listing
operation — deliberately, since Vault's does not either — so `GET /api/connections` derives an
address for every connector in the compiled-in catalogue and probes it. That is ~50 `get` calls per
listing against the file store, each of which reads the store file. It is fine at this size and it
is the first thing to change if the catalogue grows an order of magnitude; the answer then is an
index *derived from* the store, not a second source of truth beside it. A probe reads the value and
drops it without exposing it, because `SecretStore` has no `exists`.

### Which credentials a connection carries

A connector declares its credentials at provider level: `zendesk` declares one (`zendesk.api_token`),
`slack` declares two (`slack.bot_token` for outbound calls, `slack.signing_secret` for inbound
webhook verification). A connection carries a value for **any non-empty subset** of them, named by
the flat-namespace name the catalogue publishes:

```json
POST /api/connections/slack
{"credentials": {"slack.bot_token": "…"}}
```

Requiring all of them would force an operator to invent a signing secret they do not use; allowing
none would let an empty `POST` create a connection with nothing behind it. A name the connector does
not declare is refused, listing the ones it does — a typo that stored a value at an address no
operation reads is a credential nobody will find and nobody will rotate.

Two connectors are refused by the catalogue's own data rather than by anything here, and both
refusals name which fact is missing:

- **No declared authority** — `Provider::authority` is `Option`, and `connector_spec` returns
  `Ok(None)` from `credential_ref_for` for exactly this case. Without an authority no address
  renders at all, so there is nowhere to put the value. It is **not** defaulted to anything: a
  guessed authority is a credential written to an address no operation will ever read from, which is
  a silent loss dressed as a success. Every connector in catalogue 0.8.0 does declare one, so the
  test for this drives the derivation directly with a declaration that does not.
- **No declared credential** — `freshdesk` declares none, which flux-connectors' `AGENTS.md` records
  as an intentional gap (C-16: the IR cannot yet say that an API key occupies the *username*
  position). There is nothing to address, so `POST` refuses and names the connector.

## Where the store is bound, and what happens when it is not

**Decision: `AppState` carries `Option<Arc<dyn SecretStore>>` — the port, not the concrete store —
and an unbound store makes every connection route answer `503` naming
`FLUX_EXCHANGE_CREDENTIALS`.**

`AppState`'s own documentation says it carries the *ports* a composition bound, and `CredentialStore`
is a concrete binding of one. Holding `Arc<dyn SecretStore>` keeps that true, keeps the deployment
that wants Vault able to bind it, and — not incidentally — keeps `exchange-server` off the
`#[cfg(unix)]` that `CredentialStore` carries, since only `FileStore` is unix-only.

The binding is additive: `AppState::with_credentials` is a builder method rather than a fourth
constructor or a widened signature on the existing three, so a composition that binds no store is
still spelled the way it was and X-04's identity work does not collide with this.

`main` binds a store when `FLUX_EXCHANGE_CREDENTIALS` names one, and refuses to start when it names
one that cannot be opened. **Unset binds nothing**, which is the same shape as
`FLUX_EXCHANGE_DEV_IDENTITY`: the default is inert, nothing has to be turned off to be safe, and a
value that is set and wrong stops the process rather than being worked around. This is not the
in-memory fallback X-09 refuses — there is no fallback, there is a `503` that names the setting and
the example path. A host with no store bound cannot hold a credential and says so, every time it is
asked.

## What this deliberately does not do

- **No configuration on a connection.** The Goal names "a connector plus its credential and
  configuration", and configuration is exactly where the per-instance facts live — the Zendesk
  subdomain that `base_url` templates. It has no home until the address can tell two instances
  apart, so it lands with X-14 rather than being stored now against an address that would collide.
  The Acceptance does not ask for it.
- **No update or rotation.** `POST` on an existing connection is the X-14 refusal, not an upsert,
  because an upsert is precisely the silent overwrite this story exists to prevent. Rotation needs a
  shape that can say "replace *this* instance's value", which is X-14's.
- **No use of a connection.** Invoking an operation is X-12 and is blocked on the engine line
  (X-11). Nothing here reads a credential value back out to a caller — `GET` answers with addresses
  and never with values, and there is no route that returns one.
- **No listing straight from the store.** See the cost note above; `FileStore::paths()` exists but
  is not on the `SecretStore` port, and reaching past the port for it would tie this surface to one
  store implementation.
