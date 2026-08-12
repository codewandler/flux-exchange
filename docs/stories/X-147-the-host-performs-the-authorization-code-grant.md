---
id: X-147
title: The host performs the authorization code grant on behalf of a signed-in person
status: in-progress
priority: 0
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
note: "the delegated half of X-72 — the PKCE/state machinery, the callback, the principal-scoped address and the hazard-free binding are delivered against an injected binding; the three criteria that read the connector's own OAuth2 declaration still wait on the 0.21 line (X-146)"
---

# The host performs the authorization code grant on behalf of a signed-in person

## Goal

Let a signed-in person authorize a connection with their own vendor account, so operations run with
that person's permissions rather than a shared credential's.

This is the half of [[X-72]] that is missing. `CredentialAcquirer` already declares
`redeem_password` and `redeem_refresh`, and `AcquiredCredential` already carries
`(access_token, refresh_token, expires_at)` — so this host can *renew* a token it holds and cannot
*obtain* one. The browser-redirect leg has no implementation at all: no authorize URL is composed, no
`state` or PKCE verifier is minted or checked, and no callback is served.

## Why now — and what is not yet true

The upstream blocker recorded in `crates/exchange-server/src/credential_acquisition.rs` is moving,
but **it has not moved yet**, and this story is filed `blocked` for that reason.

> *"Until upstream C-440 ships a connector declaration, production composes an empty
> `AcquisitionBindings`."*

**What exists.** flux-connectors' `main` carries the metadata this story reads: `catalog::Acquisition`
gained an `OAuth2(&'static OAuth2)` variant under C-525, carrying the endpoint reference, authorize
path, client id, scopes and permitted grants; `catalog::Subject` says whether a credential acts as
the integration or as a person; C-531 lets a hosted deployment declare its redirect URI. Verified at
flux-connectors commit `428938cd`, `crates/catalog/src/lib.rs:398-496`.

**What did not exist when this was filed, and now does.** On 2026-08-11 the newest
`codewandler-connector-*` release was 0.20.0, whose `Acquisition` had exactly three variants —
`Static`, `Minted`, `BasicJoin`. So the criteria below that read connector metadata could not be
satisfied against any crate this workspace may legitimately depend on, and closing that gap with a
`path` or `git` dependency on the sibling checkout is refused by [`AGENTS.md`](../../AGENTS.md)
§ The dependency situation.

**flux-connectors released `v0.21.0` on 2026-08-12** and all four connector crates are on crates.io.
The released `codewandler-connector-catalog` 0.21.0 carries `Acquisition::OAuth2`, `OAuth2`,
`OAuthGrant`, `OAuthRedirect`, `Subject` and `Credential.subject` — verified against the published
crate, not the working tree.

**What remains before the metadata-dependent criteria can be ticked:** [[X-146]] moves the four
`codewandler-connector-*` pins to 0.21. Note that story's own premise needed correcting —
`connector-pack` 0.21.0 requires `codewandler-flux-runtime ^0.54`, exactly as 0.20.0 did, so the
**engine line does not move** and no `codewandler-flux-*` pin changes. This is a connector-only bump.

The slice delivered here deliberately depends on none of that: it composes the authorize URL from an
**injected** grant, which is the same seam X-75 established for the password grant.

Note this needs **no `AuthHazard` opt-in**: the hazard gate exists for the resource-owner password
grant, and `authorization_code` carries none. A deployment with `FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS`
unset — the safe default — can run this flow.

## Acceptance

- [x] `CredentialAcquirer` gains an authorization-code leg beside `redeem_password`, returning the
      same `AcquiredCredential`. The host crate keeps only the secret-in/secret-out port; HTTP,
      endpoint composition and vendor quirks stay in the composing binary, exactly as X-75 arranged.
- [ ] The authorize URL is composed **from the connector's own declaration** — the `OAuth2`
      acquisition's endpoint reference resolved against that service's base URL, plus its declared
      `authorize_path` and `scopes`. A caller cannot name a host, a path or a scope; a scope absent
      from the connector's list is one this host does not request. *(Requires the 0.21 line.)*
- [x] **PKCE is mandatory and `state` is single-use.** The verifier and the state are minted here,
      bound to the initiating session and tenant, expire, and are consumed on first callback. A
      callback whose `state` is unknown, expired, already used, or bound to a different session is
      refused without contacting the vendor. Failing-first tests for each.
- [x] The redirect URI is **deployment configuration, not connector data**, and is compared exactly.
      The connector declares no redirect for a hosted deployment — upstream `OAuthRedirect` models a
      loopback port and path only, which is the local-development shape.
- [x] The resulting credential is stored under the **initiating principal**, not the tenant at large.
      A user-subject credential — `catalog::Subject::User`, which GitLab's delegated token declares —
      kept at a tenant-wide address would let one member act as another. A failing-first test asserts
      one member cannot resolve another's.
- [x] Refusals are typed and value-free, reusing `AcquisitionRefusal`. No authorization code, token,
      verifier or state appears in an error, a log or a `Debug`.
- [ ] Production composes a **non-empty** `AcquisitionBindings` derived from the released catalogue,
      and the comment quoted above is replaced by what is actually true. *(Requires the 0.21 line.)*
- [ ] A connector declaring a grant this host cannot perform is refused at composition, naming the
      grant — never attempted and never silently downgraded to another grant in the list.
      *(Requires the 0.21 line: there is no declared grant list to compare a performer against.)*

## Progress

- 2026-08-11: Filed on a branch as X-123. That ID was already taken on `main` by *Production refuses
  an operatorless deployment*; renumbered to X-147 on integration.
- 2026-08-12: Verified the upstream premise and corrected it. The `OAuth2`/`Subject` metadata is real
  but **unreleased** — crates.io's newest connector line is 0.20.0 and carries none of it. Status set
  to `blocked` behind [[X-146]]. Priority kept at 0: this is still the first thing to do once the
  dependency is available.

- 2026-08-12: **Split on the seam the Notes below name, and the half that needs no connector metadata
  is delivered.** Status `in-progress`, against an *injected* binding — the same seam X-75 already
  established, so nothing here claims the released catalogue declares anything it does not.

  Delivered, with tests:

  - `CredentialAcquirer::redeem_authorization_code`, taking an `AuthorizationCodeRedemption` of two
    secrets. **A default method, not a required one** — the host crate is published and a required
    method is a breaking change — whose body refuses `AcquisitionRefusal::GrantNotPerformed`.
  - `crates/exchange-server/src/delegated_acquisition.rs`: a sibling of `oidc::flow`'s pending store,
    reusing `oidc::pkce` and `oidc::flow::Binder` and restating the *there is deliberately no
    `take(state)`* rule. `state` is bound to the initiating **`Principal`**, single-use, TTL'd,
    bounded, and a browser binder is planted under a **distinct** cookie name.
  - `POST /api/acquisitions/{connector}/authorize` (`Access::User`) and
    `GET /api/acquisitions/callback` (`Access::Anonymous`, with its argument written beside the entry
    in `routes::mod`'s `ANONYMOUS` list). The callback claims the `state` **before** any vendor is
    contacted.
  - `FLUX_EXCHANGE_ACQUISITION_REDIRECT_URI`, validated at startup and compared exactly — any
    spelling a URL parser would rewrite is refused rather than rewritten.
  - `AcquisitionBinding`'s hazard is `Option<AuthHazard>` with `AcquisitionBinding::admit` as the one
    unconditional-admit path. **No `AuthHazard::None` variant**, on `acquisition.rs`'s own argument.
  - The credential is addressed under a reserved per-principal service segment. The decision and its
    rejected alternative (`@instances/<uuid>`) are recorded in the design doc.

  **Remaining, and every one of them waits on the 0.21 connector line reaching crates.io — that is
  [[X-146]], and nothing else blocks them:**

  - composing the authorize URL from the connector's own `Acquisition::OAuth2` declaration;
  - production composing a non-empty `AcquisitionBindings` and replacing the C-440 comment in
    `crates/exchange-server/src/credential_acquisition.rs`;
  - refusing a connector that declares a grant this host cannot perform — there is no declared grant
    list to compare a performer against until the declaration exists.

  Two edges found and deliberately left, because closing either belongs with the criteria above: the
  delegated address is not projected by `GET /api/connections` (as `exchange-acquisition`'s
  companions are not), and this route's tenant-occupancy check sums one connector's scope rather than
  every connector.

- 2026-08-12: **Reworked after independent review.** One blocking finding and five minors.

  **B1, blocking — the redirect URI had three uncompared sources, and the ticked criterion was not
  true.** The route read the startup-validated deployment value only to check it was *present* and
  then sent a composition argument to the vendor, so everything `acquisition_redirect` checks guarded
  a string that never left the process; the string that did left after one `is_empty` test. The
  claim that one grant in two places made this structural was also wrong — they were two independent
  `Option<DelegatedGrant>` fields set by two calls with no equality check. Fixed so divergence cannot
  be constructed rather than being unlikely: `AcquisitionRedirect` is a newtype with one checking
  constructor; `AcquisitionBindings::new` takes the deployment's redirect and **refuses** a bound
  grant that is not byte-equal to it; `AcquisitionBinding::delegating` sets the browser-facing and
  back-channel halves from one `Arc`; and the route composes the authorize URL from
  `AppState::acquisition_redirect`. Three tests pin it, two of them proved by mutation.

  **M1 — a delegated credential could not be revoked at all.** Worse than this story recorded:
  `DELETE /api/connections/{connector}` walks declared addresses and `exchange-acquisition`
  companions, so it answered `204` while the delegated credential and a live refresh token survived.
  Added `DELETE /api/acquisitions/{connector}` (`Access::User`), which destroys the caller's **own**
  in one batch — it takes no target and cannot be pointed at anybody else's. **Still open and
  recorded rather than guessed at:** what a *tenant-level* disconnect should do to every member's
  delegated credentials. That is entangled with the addressing question below.

  **M2** — the callback's read-decide-write now holds the `ConnectionGuard` tenant claim, taken after
  the vendor answers so one tenant's members do not serialise on a network round trip. **M3** — the
  pending bound is per tenant (64) with a global memory ceiling (4096), so one signed-in person can
  no longer lock every other tenant out for the TTL. **M4** — `DelegatedGrant`'s transport rule is
  keyed to a loopback literal instead of `cfg!(test)`, so the rule is reachable from a test.
  **M5** — `TraceLayer`'s default span recorded the whole URI, so the authorization code and `state`
  reached a `DEBUG` span from the layer rather than from any handler; the span now records the path
  only, which also closes the same leak on `/api/signin/callback`. **M6** — board regenerated.

  **Open, and deliberately not implemented:** the owner's proposal to replace the derived digest in
  the service segment with an allocated UUID plus a companion mapping and a read-side index. The
  derivation stays confined to one function so swapping it is one function and a migration.

- 2026-08-12: **Re-review returned APPROVE WITH NITS; three closed.**

  **The M5 pin was not a pin.** `no_query_string_reaches_a_request_span` exercised a `traced_path`
  helper, so reverting the span to `request.uri()` left it green while `acquisitions.rs` claimed that
  test "is what keeps it stopped" — the same class as B1, a claim stronger than the code, in the fix
  for it. The test now runs real requests through `app()` under a real `tracing` subscriber and
  asserts on the fields the span actually recorded; the helper is deleted. Verified by mutation: the
  revert now fails with `a query string reached a span field: path=/api/acquisitions/callback?state=
  abc123&code=SECRET-CODE-NOT-A-REAL-TOKEN`.

  **`!cfg!(test)` survived at `HttpCredentialAcquirer::new`**, one screen below M4's argument against
  exactly that construct. Now keyed to a loopback literal like `DelegatedGrant::new`, with a test
  driving the refusal. This endpoint carries resource-owner passwords and refresh tokens in a POST
  body, so the rule was worth having and worth being able to run.

  **Nit 3 taken rather than left as a comment.** The registry and the state were two independent
  wirings of the redirect, which is B1's own shape one level up. `AcquisitionBindings` now holds it
  and `AppState::acquisition_redirect` reads through; `with_acquisition_redirect` is gone. A
  consequence worth recording: the route's `NoRedirectUri` refusal is now **unreachable by
  construction**, so its test was rewritten to pin the composition refusal that replaced it and the
  arm is documented as an uncovered fail-closed backstop rather than claimed as tested.

  Nits 4-6 (span level, tenant-wide channel restart on revoke, no guard claim on revoke) needed no
  code change; the span's level change is now named in its doc comment.

## Notes

- **Refresh already exists and is not in scope**, but this story is what makes it reachable:
  `redeem_refresh` handles rotation (`require_rotated_refresh`, `RefreshOutcomeUnusable`) and has had
  no credential to renew. The scheduling of refresh — *when* a token is renewed before expiry — is
  [[X-148]].
- GitLab is the first connector to exercise this and the natural conformance target. Its origin is
  operator-approved and may be an internal, VPN-only host: the upstream origin grammar admits a
  private DNS name, a single label, a bare IPv4 literal and a non-default port. **It requires HTTPS**
  — an internal instance serving plain HTTP is refused, deliberately, because a token would cross the
  wire in cleartext.
- The PKCE/`state` machinery, the callback route, the session binding and principal-scoped credential
  storage depend on **no** connector metadata. If this story is ever split to make progress ahead of
  the release, that is the seam to split on — the two criteria marked *(Requires the 0.21 line.)*
  are the only ones that genuinely wait.
