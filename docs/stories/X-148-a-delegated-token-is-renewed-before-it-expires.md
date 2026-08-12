---
id: X-148
title: A delegated token is renewed before it expires, so an unattended run outlives it
status: blocked
priority: 1
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
note: "a GitLab OAuth access token expires in about two hours and an autonomous fleet run outlives that; redeem_refresh exists and nothing calls it on a schedule — blocked behind X-147, which first produces a credential to renew"
---

# A delegated token is renewed before it expires, so an unattended run outlives it

## Goal

Keep a delegated connection usable across a long unattended run by renewing its access token before
expiry, without a human present and without a mid-run failure that looks like a permissions problem.

## Why now

A GitLab OAuth access token expires in **about two hours** by default. The consuming case is an
autonomous delivery fleet whose runs routinely outlive that, so "the connection worked when the run
started" is not enough. `redeem_refresh` is implemented and `AcquiredCredential` already carries
`expires_at`; what is missing is anything that *calls* it before a vendor returns 401.

The failure this prevents is specifically nasty: an expired token and a revoked grant both surface as
`401`, so without renewal an operator debugging a stalled run cannot tell "this needed refreshing"
from "this person lost access".

**Blocked behind [[X-147]]**, which is itself blocked behind [[X-146]]: there is nothing to renew
until the authorization-code leg produces a delegated credential. The scheduling, single-flight and
atomicity work here reads `expires_at` and calls the existing `redeem_refresh`, so it depends on no
connector metadata of its own — only on a credential existing.

## Acceptance

- [ ] `expires_at` is **persisted with the credential** and survives a host restart. A token whose
      expiry is unknown is treated as expiring immediately rather than as valid indefinitely.
- [ ] Renewal happens **before** expiry, on a declared skew, rather than in response to a `401`.
      Reacting to a `401` cannot distinguish an expired token from a revoked grant and turns every
      renewal into a failed vendor call first.
- [ ] Renewal is **single-flight per credential**: concurrent operations on one connection produce
      one refresh, and the losers wait for its outcome rather than each redeeming the same refresh
      token. A failing-first test drives it concurrently — a vendor that rotates refresh tokens
      invalidates the old one, so a double redeem revokes the connection.
- [ ] The write is atomic and recoverable, through the existing prepared-transaction store. A crash
      between "vendor issued a new pair" and "the store holds it" must not leave the connection with
      a refresh token the vendor has already rotated away.
- [ ] A refresh that fails **permanently** — the grant was revoked, the refresh token rotated away —
      marks the connection as needing re-authorization and says which person must re-authorize.
      Distinct from a transient failure, which retries with backoff. Failing-first tests for both.
- [ ] No token, refresh token or expiry-derived secret appears in a log, an error or a `Debug`.
- [ ] An operator can see, per connection, whether it is currently valid and when it was last
      renewed — without any surface exposing a value.

## Progress

- 2026-08-11: Filed on a branch as X-124 alongside what was then X-123. That ID was already taken on
  `main` by *Adopt exchange-only integration execution*; renumbered to X-148 on integration.
- 2026-08-12: Status set to `blocked` behind [[X-147]]. Nothing here waits on connector metadata —
  only on a delegated credential existing to renew.

## Notes

Renewal is the reason this work belongs in Exchange rather than in a consuming product. A caller that
holds a token and renews it is a caller that holds a refresh token — the longer-lived and more
dangerous of the two. Keeping both here is what lets a consumer hold neither.

The **storage posture is a separate decision** and deliberately not in this story's scope. The bound
store is `connector_secrets::FileStore` — a `0600` file, application-plaintext at rest — which is
adequate for a demo and is what [[X-97]] is about. This story makes the *number* of long-lived
secrets grow, one refresh token per person per connection, which is what raises the priority of that
decision rather than answering it.
