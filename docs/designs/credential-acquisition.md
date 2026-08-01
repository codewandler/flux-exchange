# Design: the host acquires a credential, and a weak way of acquiring one is labelled

**Status:** proposed · **Epic:** `credential-acquisition` · **Stories:** X-72, X-73, X-74, X-75, X-76

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

## Acceptance / done

An operator connects babelforce with a username and a password on a deployment that has explicitly
opted in; the stored credential is a token with an expiry, the password appears nowhere on disk or in
any log; and the same connection attempt on a deployment that has **not** opted in is refused by name,
with the hazard cited, before any request leaves the process.
