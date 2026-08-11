---
id: X-147
title: The host performs the authorization code grant on behalf of a signed-in person
status: blocked
priority: 0
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
note: "the delegated half of X-72 — X-75 redeems a password and redeem_refresh renews, but nothing here can OBTAIN a token by user grant; blocked on the connector line that declares one reaching crates.io (X-146)"
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

**What does not exist.** None of it is published. The newest `codewandler-connector-*` release on
crates.io is **0.20.0**, and 0.20.0's `Acquisition` has exactly three variants — `Static`, `Minted`,
`BasicJoin`. There is no `OAuth2`, no `Subject`, no `providers/gitlab.toml` declaring
`authorization_code`. The flux-connectors working tree is still versioned `0.20.0`; the 0.21 line is
unreleased.

So the acceptance criterion below that composes an authorize URL *from released connector metadata*
cannot be satisfied against any crate this workspace may legitimately depend on. Closing that gap
with a `path` or `git` dependency on the sibling checkout is refused by
[`AGENTS.md`](../../AGENTS.md) § The dependency situation — it couples a shipped image to an
unreviewed working tree, and the family has already decided against it.

**What unblocks this story:** flux-connectors publishes the 0.21 line to crates.io, then [[X-146]]
moves both pin sets — every `codewandler-connector-*` to 0.21 and every `codewandler-flux-*` to the
engine line `connector-pack` 0.21 then requires — as one commit. This story starts when X-146 is
done, not before.

Note this needs **no `AuthHazard` opt-in**: the hazard gate exists for the resource-owner password
grant, and `authorization_code` carries none. A deployment with `FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS`
unset — the safe default — can run this flow.

## Acceptance

- [ ] `CredentialAcquirer` gains an authorization-code leg beside `redeem_password`, returning the
      same `AcquiredCredential`. The host crate keeps only the secret-in/secret-out port; HTTP,
      endpoint composition and vendor quirks stay in the composing binary, exactly as X-75 arranged.
- [ ] The authorize URL is composed **from the connector's own declaration** — the `OAuth2`
      acquisition's endpoint reference resolved against that service's base URL, plus its declared
      `authorize_path` and `scopes`. A caller cannot name a host, a path or a scope; a scope absent
      from the connector's list is one this host does not request. *(Requires the 0.21 line.)*
- [ ] **PKCE is mandatory and `state` is single-use.** The verifier and the state are minted here,
      bound to the initiating session and tenant, expire, and are consumed on first callback. A
      callback whose `state` is unknown, expired, already used, or bound to a different session is
      refused without contacting the vendor. Failing-first tests for each.
- [ ] The redirect URI is **deployment configuration, not connector data**, and is compared exactly.
      The connector declares no redirect for a hosted deployment — upstream `OAuthRedirect` models a
      loopback port and path only, which is the local-development shape.
- [ ] The resulting credential is stored under the **initiating principal**, not the tenant at large.
      A user-subject credential — `catalog::Subject::User`, which GitLab's delegated token declares —
      kept at a tenant-wide address would let one member act as another. A failing-first test asserts
      one member cannot resolve another's.
- [ ] Refusals are typed and value-free, reusing `AcquisitionRefusal`. No authorization code, token,
      verifier or state appears in an error, a log or a `Debug`.
- [ ] Production composes a **non-empty** `AcquisitionBindings` derived from the released catalogue,
      and the comment quoted above is replaced by what is actually true. *(Requires the 0.21 line.)*
- [ ] A connector declaring a grant this host cannot perform is refused at composition, naming the
      grant — never attempted and never silently downgraded to another grant in the list.

## Progress

- 2026-08-11: Filed on a branch as X-123. That ID was already taken on `main` by *Production refuses
  an operatorless deployment*; renumbered to X-147 on integration.
- 2026-08-12: Verified the upstream premise and corrected it. The `OAuth2`/`Subject` metadata is real
  but **unreleased** — crates.io's newest connector line is 0.20.0 and carries none of it. Status set
  to `blocked` behind [[X-146]]. Priority kept at 0: this is still the first thing to do once the
  dependency is available.

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
