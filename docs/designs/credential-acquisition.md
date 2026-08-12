# Design: the host acquires a credential, and a weak way of acquiring one is labelled

**Status:** accepted · **Epic:** `credential-acquisition` · **Stories:** X-72, X-73, X-74, X-75, X-76,
X-147

## Why

Every credential this service holds today arrived the same way: **a human pasted it in.**
`connector_catalog::Acquisition` has one shipped value, `Static` — *the stored secret, unchanged* —
and `providers/babelforce.toml`'s only `[[auth]]` block spells out what that means for the first-party
vendor in the catalogue:

> `description = "SSO-issued babelforce access token, minted outside flux and supplied through the
> environment"`

**Minted outside flux** is the whole problem. A babelforce user has an email address and a password.
They do not have an access token, and there is no supported way for them to get one by hand short of
running an authorization-code flow against a browser they may not have. So the connector with 389
catalogued operations is the one nobody can connect.

Owner-raised, 2026-08-01: babelforce supports the **OAuth2 password grant**, and it should be usable
here — with the weakness stated rather than hidden, and refused in production unless somebody opts in.

That request is right, and the reason it is right is principle 3 turned on authentication:

> **Grants select operations by declared metadata, not by name.**

A deployment that wants to forbid password-grant authentication should be able to say so **once**,
about a declared property, and have every connector that carries that property refuse — not maintain
a list of connector names that drifts the moment the catalogue grows a 55th provider.

## What this epic is *not* allowed to do

**An authentication endpoint is never a connector operation.** Owner-stated 2026-08-01, recorded in
flux-connectors' `AGENTS.md` § Authentication contract, and it is why `providers/babelforce.toml`
withholds `POST /oauth/token`, `GET /oauth/authorize` and `POST /oauth/revoke` by rule. babelforce's
canonical accounting is `389 emitted + 5 inexpressible + 3 auth-flow withheld = 397`, checked by
`crates/connector-spec/tests/babelforce_coverage.rs` in **both** directions.

Two consequences bind this design:

1. **Nothing here re-selects those three paths.** `expose = false` is not the mechanism —
   `connector_pack::resolve` admits any *selected* operation regardless of exposure (C-413). The
   operation must not exist.
2. **`produces_credential` is not the answer either.** C-432 measured this: `connector-flux` refuses
   to emit an operation declaring it, because an emitted module ends `response = http.request(…)` /
   `return response` and so binds the **raw token to a model-visible symbol**. The owner's amendment
   assumed flux 0.47.1's credential boundary would refuse an unmarked credential-shaped response;
   C-432 checked the vendored source and it does not — `PlatformSourcing` is an opt-**in** to
   refusal with no fourth value meaning *allow*, and the boundary sits on the plugin seam this
   family's artifacts do not travel.

So the grant is **an acquisition the host performs**, exactly as that contract says: *"The host
resolves the credential, performs effectful acquisition such as OAuth2, applies the placement scheme,
and registers values with its redactor."* This repository **is** that host. The manifest declares;
we perform.

## Approach

Four pieces, and the order is load-bearing.

### 1. `AuthHazard` — a hazard is a *kind*, and `Risk` is a *level*

```rust
/// A named weakness in **how a credential is obtained**, declared by the connector.
#[non_exhaustive]
pub enum AuthHazard {
    /// The resource owner's own password is presented to this host rather than to the
    /// authorization server.
    ///
    /// RFC 9700 §2.4 (Best Current Practice for OAuth 2.0 Security, 2025) — the resource owner
    /// password credentials grant **MUST NOT** be used: it exposes the resource owner's credentials
    /// to the client, widens where they can leak beyond the authorization server, and cannot carry
    /// two-factor or any multi-step authentication. RFC 6749 §4.3 requires the client discard them
    /// once a token is obtained. Nearest CWE: CWE-522, Insufficiently Protected Credentials.
    ResourceOwnerSecretShared,
}
```

**Not a fifth rung on `Risk`, and not a string.** Three reasons, each of which is a way this has been
got wrong elsewhere:

- `Risk` is an **ordered severity ladder** over what an operation does to vendor data, and
  `Selector::at_most` compares against that ordering. A hazard has no position on it: a password
  grant that buys a read-only token is `Risk::Low` **and** hazardous, and folding it in would make
  `at_most(Risk::High)` silently admit it.
- A hazard is a property of the **credential's acquisition**, which happens once per connection —
  not of an operation, which happens per call. Putting it on `OperationFacts` would restate one fact
  on 389 rows.
- A free-form `hazard = "..."` string makes the filter a string match, so a typo reads as *no hazard
  declared* and admits. A closed set makes an unknown value a refusal at load. The cost is that
  every new hazard is a deliberate edit here, which is the point.

The name is a citation, not a coinage. It states the property — *the resource owner's secret was
shared* — which is what a filter is written against and what an auditor can check.

### 2. `AuthPosture` — the filter, opt-in and fail-closed

Modelled on `Deployment::admits`, which is the precedent in this repository for *a value read at
startup that decides admission, with no per-request override*:

```
FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS=resource_owner_secret_shared
```

- **Unset — every declared hazard is refused.** That is the production default and it needs no
  configuration to be safe, which is the property `Deployment` and the bind rule both already have.
- **Set and naming an unknown hazard — the process refuses to start and names the value.** It does
  not skip the unrecognised entry and arm the rest, following `DevIdentity`'s roster exactly: a list
  that silently lost an entry is a list whose operator is debugging the wrong thing.
- **The refusal names the hazard and the connector, never a value.** Principle 5.

Where it is enforced matters. The refusal belongs at **connection time** — the moment somebody tries
to acquire through the hazardous path — and not only at startup, because a catalogue update can
introduce a hazard into a running deployment that started clean.

### 3. The acquisition — a port in the host, a binding in the binary

This host constructs no request of its own (principle 6), and
`crates/exchange-host/Cargo.toml`'s `[dependencies]` is an allow-list read by
`tests/no_second_request_path.rs`. **An OAuth token request is a request somebody constructs**, so
the design has to say where — and the answer already exists in this tree.

`crates/exchange-server/src/oidc/` performs an OAuth2 authorization-code exchange today: `TokenExchange`
is a trait, `http_exchange.rs` is the concrete binding, and `reqwest` appears in
`crates/exchange-server/Cargo.toml` and **in no other manifest in the workspace**. Sign-in already
does the thing this epic needs, one seam over.

So: **a port in `exchange-host`, its HTTP binding in `exchange-server`.** The published crate gains a
trait and no transport; the composition gains one more implementation beside the one it already has.
That is the same argument that keeps `DevIdentity` out of the published crate — a product embedding
this host gets the vocabulary and chooses its own performer.

What the acquisition does, in order:

1. Take the resource owner's username and password **borrowed, not owned** — `Redemption<'_>` is the
   shape, and it redacts when printed for the same reason.
2. `POST /oauth/token` with `grant_type=password`.
3. Store **the access token** at the connection's ordinary credential address. By the time anything
   places it on a request it is a value in the store like any other — which is precisely what
   upstream's `Acquisition::Minted` documentation says, and why no placement code changes.
4. **Discard the password.** Never written, never logged, never in an error body. RFC 6749 §4.3
   makes this a MUST for the client, and here the client is us.
5. Record expiry from the response's `expires_in`, and the `refresh_token` if one was issued —
   babelforce rotates refresh tokens on every use, so the stored one is replaced each refresh or the
   next refresh fails with `invalid_grant`.

### 4. A behaviour no document declares is a quirk of one endpoint

**Owner-decided 2026-08-02**, and it is the rule that keeps this vocabulary honest:

> if this isn't spec it should not be a general thing — make it a quirk of that endpoint.

The occasion was the token lifetime. The owner said babelforce accepts a TTL parameter; the vendored
`specs/babelforce/auth-2026-06-25.openapi.yaml` declares eleven request properties and none is a
lifetime. **Both were true.** `AuthController.token()` reads `expires_in` straight out of `params`,
which is precisely why no generated document could show it — and it reads it **differently for every
grant**:

| Grant | `expires_in` on the request | Semantics |
|---|---|---|
| `client_credentials` | read, defaulting to `-1` | `-1` means *never expires* |
| `password` | read when present | otherwise the service default |
| `refresh_token` | read, passed into the refresh | — |
| `link` | read, then clamped to **at most 60s** | a fifth grant, also undeclared |
| `authorization_code` | **not read** | only `access_type`, default `offline` |

Plus the case the owner named as the precedent: on the `refresh_token` grant, **`account_id` switches
the account** the new token belongs to — commented as an account switch in the vendor's own source, and
outside anything RFC 6749 describes.

That table is the whole argument against a general field. A `requested_ttl` on the acquisition
vocabulary would be a hard cap on one grant, silently ignored on another, and the difference between
an hour and forever on a third — against a **single vendor**. Against the other fifty-three it would
be a field nobody declares and everybody is assumed to honour, which is the failure this repository
already has a sentence for: *a marking nothing reads is worse than none.*

So it is a **quirk**: confined to one connector's auth surface, named, per-grant, and attributed to a
dated measurement of the vendor's implementation rather than to a document that does not say it.
flux-connectors already carries that word and its discipline — `quirks.pagination`, `quirks.rate_limit`,
*declarations, not behavior*. This is the same shape one seam over, on `[[auth]]` rather than on an
operation. **X-76** holds the measurement and the rule; **C-440** is where the declaration lands.

### 5. The manifest side, upstream

flux-connectors declares the acquisition and the hazard on the `[[auth]]` block. That is
**C-440** in that repository, and this epic reads what it publishes rather than inventing a local
copy. Until it lands, nothing here has a hazard to filter on and the filter is exercised against a
fixture.

## Alternatives considered

- **Ship `babelforce-token` as an operation with a high `risk`.** Rejected on the owner's own rule:
  an authentication endpoint is never a connector operation, and a token endpoint's response body is
  a credential, which fails a second and independent test. It would also convert a *named exclusion
  with a reason* into a selection, breaking the `389 + 5 + 3` accounting that
  `babelforce_coverage.rs` enforces in both directions.
- **A fifth `Risk` value.** Rejected: conflates level with kind — see §1.
- **A free-form hazard string with citation URIs** (`"rfc9700#section-2.4"`, `"cwe-522"`). Maximally
  checkable and genuinely tempting, but it makes the filter a string match where a typo admits. The
  citations go in the doc comment on the closed variant instead, which is where a reader looks and
  where a reviewer can check them.
- **Perform the grant in `exchange-host` directly.** Rejected: it puts a transport in the published
  crate and lands on `no_second_request_path.rs`, which is the test that exists to make exactly this
  decision expensive.

## Risks & open questions

- **The requested token TTL is real, undeclared, and deliberately not general.** See §4 — it is a
  quirk of one endpoint, and X-76 is where the measurement and the rule live.
- **The password grant cannot carry 2FA** (RFC 9700 §2.4). A tenant with MFA enabled cannot use this
  path at all, and the vendor's refusal will look like a wrong password. The refusal must distinguish
  them or an operator debugs the wrong thing — the same defect class X-17 and X-20 were filed for.
- **The password transits this process.** Discarding it is necessary and not sufficient: it must be
  registered with the redactor before the request is built, not after, or the first `?` that
  propagates a transport error carries it into a log.
- **A hazard is only as good as the declaration.** If upstream marks nothing, the filter admits
  everything and reads as safety. C-432's closing line is the standing warning: *a marking flux does
  not read is worse than none.* The guard is that this repository's filter refuses an **undeclared**
acquisition kind it does not recognise, rather than defaulting it to hazard-free.

## Delivery seam while C-440 is unreleased (2026-08-03)

The released catalogue still has no acquisition declaration to map. That does **not** permit this
repository to infer one from the connector name: doing so would make X-74's property gate a name
gate in disguise. The local delivery seam is therefore explicit and composition-owned:

- `AppState` may be given an acquisition binding registry. Each entry fixes the connector,
  declared hazard, target credential and performer before a request arrives. The HTTP body selects
  neither endpoint nor hazard.
- The production composition binds an empty registry until C-440 is released. Tests inject a
  babelforce-shaped fixture entry and drive the real connection route. This is honest executable
  coverage of the seam, not a claim that the released catalogue declares it.
- The existing connection paths are reused. `?acquire=password` and `?acquire=refresh` select the
  request form and make the value-free audit vocabulary distinguish vendor acquisition from a
  human-supplied credential; the fixed registry entry still decides whether either form exists.
- The access token remains at the connector's ordinary declared credential address. Returned
  expiry and refresh token live at reserved companion addresses in the same credential scope and
  move through one `SecretBatch`. A refresh that rotates its refresh token therefore cannot commit
  half of the pair. Inventory projects only declared credentials, and removal/migration includes
  companions so the internal state cannot be orphaned.
- A successful acquisition is recorded as acquired/initiated-by, never as supplied-by. X-60's
  question remains answerable without pretending the operator pasted a token the vendor minted.

### The HTTP request shape, and where the quirks stop

`exchange-host` owns `Redemption<'_>`, `Refresh<'_>`, the acquired token result and the async
performer port. None has a requested lifetime, account id, URL, HTTP method or form vocabulary.

`exchange-server` owns the concrete HTTP performer. Its ordinary form sends only the OAuth grant
fields. A `BabelforceTokenEndpointQuirks` value, stored on that one performer instance, may add
`expires_in` to password and refresh requests and `account_id` to refresh. Its documentation carries
the complete measured table, including authorization-code ignoring `expires_in`; no caller field
and no generic acquisition type can carry any of them. Applying the babelforce configuration to a
second connector is the failing-first test.

The vendor response's `expires_in` is different: it is observed state, not a requested policy. The
performer turns it into an absolute expiry and the connection stores it. No default TTL is invented
when the response omits it.

## The delegated lane (X-147, 2026-08-12)

The password grant above exists because a vendor that offers nothing else offers this or nothing. The
**authorization code** grant is the one it is a fallback for, and it is the half X-72 filed and never
built: the person authorizes at the vendor, in their own browser, with their own account, and this
host never sees a resource-owner secret at all. That is why it declares **no hazard** — RFC 9700 §2.4
objects to the password grant precisely because the secret crosses the client, and here it does not.

The seam is X-75's, unchanged: a port in `exchange-host`, its HTTP binding in `exchange-server`. The
port gains `CredentialAcquirer::redeem_authorization_code`, taking an `AuthorizationCodeRedemption`
of two secrets — the code and the PKCE verifier — and returning the same `AcquiredCredential`. It
carries no authorization endpoint, no redirect URI, no client id and no scopes: those are deployment
configuration or connector declaration, and a port that carried them would be the published crate
describing a browser flow it does not perform.

**The method has a default body that refuses.** `codewandler-flux-exchange-host` is published, so a
required method would break every downstream performer at the version that added it — including the
ones bound to connectors that carry no delegated grant and would have to write the same refusal by
hand. The default is `AcquisitionRefusal::GrantNotPerformed`: a refusal like every other variant,
decided before anything is sent.

### Hazard-free must be expressible, and not as a variant

`AcquisitionBinding`'s hazard becomes `Option<AuthHazard>`, and `AcquisitionBinding::admit` is the
one place the absence is decided. `AuthHazard::None` was the obvious alternative and is refused for
the reason §1 gives for the type existing at all: it is *a named weakness*, each variant a citation,
and a member meaning "there is nothing wrong with this" would put a no-op inside a closed set that
every exhaustive match in `exchange_host::acquisition` would then need a skip arm for.

The cost is stated rather than waved at: a `None` **admits unconditionally**, so a binding that
forgot its hazard is one a fail-closed deployment now performs. What holds it is that there is no
default — `AcquisitionBinding::new` takes the option positionally, so every composition site says
which it meant.

### Addressing a delegated credential

**The decision: a reserved service segment per principal.**

A delegated token is `catalog::Subject::User` — it acts *as the person*. One kept at the connection's
ordinary tenant-wide address would let any member of the tenant act as any other, which is the
failure this whole story exists to remove rather than relocate. `CredentialRef` has no principal
component and `connector-address` is upstream, so the principal has to go in a segment that already
exists.

It goes in the **service**, which is the segment this host already reserves for state a connector did
not declare — X-75's `exchange-acquisition`, holding a connection's refresh token, expiry and managed
marker. A delegated credential lives at:

```text
tenants/<tenant>/<authority>/exchange-delegated-<digest>/<leaf>
```

`<digest>` is 128 bits of SHA-256 over a length-prefixed, domain-separated encoding of the
principal's kind, id and tenant. A digest and not the id itself because the service grammar
`connector-address` enforces is lowercase ASCII letters, digits and `-`, and a principal id is an
OIDC `sub`, an email address or a roster handle — refusing every principal whose id is not already an
address segment would refuse most real deployments, and rewriting one into the grammar is a lossy
transform, which is a collision manufactured by the encoding. It is **derived at every use** from the
resolved principal, so there is nothing stored to go stale and nothing for a caller to name.

The three companion leaves sit under the *same* per-principal service rather than under
`exchange-acquisition`, which is the point: one shared companion address would put every member's
refresh token in one slot and the last person to authorize would silently overwrite the rest. A
connector declaring a credential named like one of them is refused by name rather than accommodated.

**The rejected alternative: `@instances/<uuid>`.**

`connector-address` already has a level below the authority, and a delegated credential could have
been filed as one of the tenant's several connections with a UUID minted per principal. It was
rejected because that level is X-14's *labelled connection* namespace and is read as such by four
things at once: the connection registry's label overlay, `GET /api/connections`, the plan surface,
and the first-to-second migration. A per-person credential filed there would appear to an operator as
a connection they could rename or delete, and the UUID would have to be minted once and remembered —
a second registry keyed by principal, with its own staleness. The reserved-service level needs no
registry: a person who never authorized simply has no address.

What that costs, recorded rather than discovered:

- The delegated address is **not** projected by `GET /api/connections`, exactly as
  `exchange-acquisition`'s companions are not, so an operator cannot yet see which members hold one.
- `DELETE /api/connections/{connector}` walks the connector's declared addresses and their
  `exchange-acquisition` companions, so it does **not** destroy a delegated credential and answers
  `204` while one survives. `DELETE /api/acquisitions/{connector}` closes the half with an obvious
  answer — a person may always revoke their own, at an address nothing else can reach. What a
  *tenant-level* disconnect should do to every member's delegated credentials is open, and is
  entangled with the addressing question below.
- The tenant-occupancy sum is narrower on this route than on `POST /api/connections/{connector}`: it
  counts one connector's scope rather than every connector. The tenant claim is held across the read
  and the write, so the X-25 race is closed; what remains is that the total is an under-count.

### Deferred: an allocated UUID instead of a derived digest

The owner proposed replacing the derived digest with an **allocated UUID plus a companion mapping
committed in the same `SecretBatch`**, with a read-side index so an operator can see which members
hold one. A UUID fits `validate_service_name`'s `[a-z0-9-]+` grammar with no upstream change, and the
upstream `TenantLayout` doc already cites the pattern approvingly — action-proxy's
`customer/<uuid>/integrations/<uuid>`, *"the same idea with the vendor identity replaced by an opaque
row id, so nothing about the path says which API it opens."*

**Decided 2026-08-12: keep the derivation for now, and revisit it with [[X-97]].** Three reasons, in
order:

1. **The rename risk it solves does not exist for the case that matters.** `Principal::id` is the
   immutable OIDC `sub` for a federated human, not an email or a display name, so the digest is
   already stable across a rename. The exposure is roster and local-user handles, which are the
   loopback development paths.
2. **The visibility gap does not require a UUID.** The digest is computable *forward* from a
   principal, and a deployment knows its principals, so "does this member hold one" and "which
   members hold one" are both answerable by iteration. What a UUID adds is a reverse lookup, which
   is a convenience rather than a capability.
3. **It would introduce a second source of truth that `SecretBatch` cannot atomically span.** A batch
   is scoped to one `(tenant, authority)` and admits nothing outside it, so a mapping row and a
   credential write have no shared commit point unless the mapping is itself a companion in the same
   batch — at which point the "companion DB" is the secret store, and the DB is a rebuildable index.

The sequencing is what decides it. **Vault KV v2 has a native `/metadata/` facility**, and X-97 is
now committed to a Vault-class backend. When that lands, the metadata store exists as a property of
the backend rather than as bespoke infrastructure, and the UUID scheme becomes cheap and consistent
instead of a second thing to keep in step. Building it before then means inventing the companion
store twice.

The derivation is deliberately confined to `delegated_acquisition::delegated_service`, so swapping it
is one function plus a migration. **It is free to change until the first delegated credential is
stored; after that it is a migration** — which is the fact that should force the decision, not this
document.

### `state`, PKCE, and what the callback may be

PKCE is mandatory and `S256`-only, reusing `oidc::pkce` rather than growing a second one. The pending
store is a **sibling** of `oidc::flow::PendingAuthorizations` rather than a generalisation of it: that
one carries an OIDC `nonce` a connector grant has nothing to echo, and this one carries a `Principal`
sign-in cannot have. `Binder`, its entropy source, its redaction, its emptiness rule and the
`Set-Cookie` formatter are all shared. The binder cookie has a **distinct name** —
`__Host-flux_exchange_acquire` — because a person connecting a connector is by construction already
signed in, so both flows can be live in one browser and one name would have the second silently
invalidate the first.

`state` is bound to the **`Principal`** and not to a session handle. There is no session handle every
caller has: a person signed in through `LocalUsers` or a service-account bearer holds no
`SessionToken`, so a store keyed on one would work for the OIDC deployment and silently exclude the
others. Every guarded route has `Extension<Principal>` from `routes::require_principal`.

The callback is **`Access::Anonymous`**, and it has to be: the browser arrives mid-redirect from the
vendor's origin and the session cookie is `SameSite=Strict`, so it is not sent on that navigation. A
guarded callback would refuse every real vendor redirect and pass only in a test that forged the
cookie. What makes an anonymous route that mints tenant authority safe is that it reads no caller
identity and cannot: the principal comes from the pending delegation, and the claim happens **first**,
before any vendor is contacted and before the store is touched — the ordering `oidc::complete_admission`
records, and the reason a callback this host did not open costs one map lookup.

### The redirect URI, and what "compared exactly" had to mean

A new variable, `FLUX_EXCHANGE_ACQUISITION_REDIRECT_URI`, and deliberately not
`FLUX_EXCHANGE_OIDC_REDIRECT_URI`: that one points at `/api/signin/callback` and is registered with
the identity provider, while this one points at `/api/acquisitions/callback` and is registered with
each vendor.

The first implementation validated that variable thoroughly and then **sent a different string to the
vendor** — the route read the configured value only to check it was present and composed the
authorize URL from a composition argument checked for being non-empty. Every rule in
`acquisition_redirect` guarded a string that never left the process, and no test could see it because
one fixture spelled both the same. That is a control that only appears to exist, and it is worth
recording because the shape is generic: *a check on one copy of a value is not a check on the copy
that travels.*

Five things now make it true, and none is a comment:

1. **`AcquisitionRedirect` is a newtype with one constructor**, which runs the canonical check. There
   is no `String` path into a redirect field anywhere in the composition.
2. **Startup refuses any spelling a URL parser would normalise**, so there is one spelling per
   deployment and byte equality is a usable comparison.
3. **`AcquisitionBindings::new` takes the deployment's redirect and refuses** a bound grant that is
   not byte-equal to it. Every registry passes through it, so a binding that disagrees with its
   deployment cannot be constructed.
4. **`AcquisitionBinding::delegating` sets the browser-facing half and the back-channel half from one
   `Arc`**, so the performer that re-presents the redirect at the token endpoint holds the same value
   the authorization URL carried, rather than a second argument that happens to match.
5. **The registry is the only holder, and `AppState::acquisition_redirect` reads through to it.** The
   first fix left the state with its own redirect field and its own builder, which reproduced the
   same defect one level up: a composition could pass A to the registry and B to the state, and the
   authorize URL would carry B while the token request re-presented A. There is now one wiring.

The value the route sends is `AppState::acquisition_redirect` — the deployment's own — and it is
never derived from a request's `Host` header, which a caller controls. Because a registry holding a
grant necessarily holds a redirect, the route's "no redirect configured" refusal is now unreachable
by construction; it is kept as a fail-closed backstop rather than an `unwrap`, and is documented as
uncovered rather than claimed as tested.

### The generalisation worth keeping

Both rounds of this defect were the same shape, and it is the shape worth checking for elsewhere: **a
validated copy and a travelling copy of the same value, kept in step by convention.** The validation
looks rigorous, the tests pass, and the thing that reaches the outside world was never checked. The
repair is always the same — make it one value, or make the divergence refuse at composition.

### ~~What still waits on the 0.21 connector line~~ — superseded by X-154

*Recorded as it stood: the authorization URL was composed from an **injected** `DelegatedGrant`
rather than from the connector's own `Acquisition::OAuth2` declaration; production composed an empty
`AcquisitionBindings`; and refusing a connector that declares an unperformable grant needed the
declaration to exist. Closing the gap with a `path` or `git` dependency on the sibling checkout was
refused by [`AGENTS.md`](../../AGENTS.md) § The dependency situation, and still is.*

## Composing an acquisition from the declaration (X-154, 2026-08-12)

The declaration exists — connector 0.23, `catalog::Acquisition::OAuth2` — and
`crate::credential_acquisition` now reads it. What each value is decided by is the whole design, and
it is one sentence per axis:

| Value | Decided by | Why |
|---|---|---|
| authorize path, token path, scopes, permitted grants | the connector's declaration | vendor truth; a caller names none of them, and a scope absent from the list is one this host does not request |
| the endpoint's base URL | the served catalogue's document, filled from the connector's declared default | `OAuth2::endpoint` names a *service*, and only that service's document entry says what host it is — see below |
| declared acquisition hazard | the connector's declaration | X-74's gate is a *property* filter, and babelforce is the first released connector to carry one |
| `client_id`, `client_secret` | this deployment | Decision 0022, amended 2026-08-12: *"the artifact publishes the registration **requirement**, never a value"*, and upstream C-536 refuses to emit one |
| redirect URI | this deployment | X-147's rule, unchanged; upstream's `OAuthRedirect` models a loopback port and path, which is the desktop shape |
| which connectors are acquired for at all | this deployment | `FLUX_EXCHANGE_ACQUISITION_CONNECTORS`; a registry derived from every connector that *declares* an OAuth2 acquisition would offer an authorization for a vendor nobody registered an application with |

**Every refusal is at composition and names the connector.** A declared grant this composition does
not perform is refused *naming that grant* and is never downgraded to another entry in the same list
— the pairing that would otherwise silently work is babelforce's `[password, refresh_token]`, where
falling back to refresh means renewing a credential nothing ever obtained. `password` itself is the
interesting case: this host *has* a performer for it, and it is still not derivable from a
declaration, because X-75's lane needs a stated endpoint, an explicit hazard opt-in and a resource
owner typing a secret — none of which is in a catalogue, and inventing them would stand up the exact
grant RFC 9700 §2.4 says MUST NOT be used out of vendor metadata.

### Resolving the endpoint — measured in round 1, closed in round 2

`catalog::OAuth2::endpoint` is a **service name**. The base URL it resolves against is in the
connector's canonical document (`services[].base_url`), and the generated `&'static` tables carry
only `Provider::base_url` — the *default* service's. GitLab's OAuth2 declaration names `login`,
whose document base URL is `{origin}`; `catalog::Provider::base_url` is `{origin}/api/v4`, the API
service. They are different services and the second is not a substitute for the first.

`ConfigField::also_services` is the near miss worth naming, because it looks like the answer: it says
the `origin` *variable* is shared with `login`, which is not the same as saying `login`'s base URL is
that variable. Deriving one from the other happens to be right for GitLab and is wrong for `default`
in the same declaration — a guess that reads as a rule.

Round 1 therefore refused a named endpoint by name rather than guessing. [[X-153]] then landed
`ServedCatalogue`, and `endpoint_base` now reads the service's own `base_url` out of the document
through `ServedCatalogue::provider_document` — **the catalogue this deployment serves**, threaded in
from the composition rather than reached for, so a deployment that loaded a newer pack composes the
acquisitions that pack declares.

`also_services` is consulted after all, for the job upstream documents it for: filling `login`'s
`{origin}` from the answer declared on `default`.

### What fills a template, and the question that leaves open

A startup composition has no tenant. So the only value it may substitute into a base URL template is
the **connector's own declared default** — GitLab declares `https://gitlab.com` on the `origin`
field — and a variable with no declared default is refused, naming the connector and the variable.
Zendesk is the shipped case: `https://{subdomain}.zendesk.com`, `required`, no default, because
there is no such thing as a default Zendesk.

**The open question, stated rather than solved.** GitLab's `origin` carries `approval = operator`
precisely so a deployment can point a connection at a self-managed instance — and a tenant that has
done so would still be sent to `https://gitlab.com` to authorize, because this registry is composed
once at startup and that setting is per connection. Nothing here is wrong for the default
deployment and nothing here is right for that one.

Closing it means composing the authorize URL **per request**, from the resolved principal's tenant
settings, at the point `routes::acquisitions::authorize` runs — which is a different lifetime from a
startup registry, needs the settings store the registry does not hold, and has its own question about
what an operator-approved origin means for a credential already acquired against another one. That
is a story, not a patch, and it is filed as this one's successor rather than guessed at here.

## Acceptance / done

An operator connects babelforce with a username and a password on a deployment that has explicitly
opted in; the stored credential is a token with an expiry, the password appears nowhere on disk or in
any log; and the same connection attempt on a deployment that has **not** opted in is refused by name,
with the hazard cited, before any request leaves the process.

For the delegated lane: a signed-in person opens an authorization, authorizes at the vendor in their
own browser, and the token lands at an address derived from *their* principal — one another member of
the same tenant cannot resolve. A callback whose `state` is unknown, expired, already spent, or
presented by a different browser is refused with no vendor contacted and nobody else's authorization
consumed.
