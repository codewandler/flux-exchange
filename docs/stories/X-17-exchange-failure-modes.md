---
id: X-17
title: "An operator can tell their own misconfiguration from a refused credential"
status: ready
priority: 2
epic: serve
areas: [exchange-server]
note: "found by X-04's two reviewers, 2026-08-01: `ExchangeError::Rejected` collapses four causes, one of which is this host's own client secret being wrong — and it is logged as 'the provider refused the authorization code'"
---

# An operator can tell their own misconfiguration from a refused credential

## Goal
A failure that an operator must fix reads differently, in the log, from one where a caller's
authorization code was simply refused.

## What is wrong

`AGENTS.md` requires that failures an operator responds to differently be distinguishable. X-04's
token exchange does not meet that bar on the back channel. `ExchangeError::Rejected` currently
collapses at least four distinct causes:

1. The provider genuinely refused the authorization code — the caller's problem, and correctly opaque.
2. **`401 invalid_client` — this host's `FLUX_EXCHANGE_OIDC_CLIENT_SECRET` is wrong.** The
   operator's problem, and it is reported as "the provider refused the authorization code".
3. The token named a `kid` the JWKS does not publish — very often a wrong `FLUX_EXCHANGE_OIDC_JWKS_URI`.
4. The response carried no `id_token`.

An operator debugging (2) or (3) sees the line they would see if everything were configured
correctly and a user had simply mistyped something. The caller-facing answer must stay opaque —
that part is right and must not regress — but the **log** is where these separate.

## Also in scope

Three smaller findings from the same reviews, all in `oidc/http_exchange.rs`:

- **The unknown-`kid` rate limit lapses exactly when it matters.** `fetch_keys` writes `fetched`
  only after a successful parse, so while the JWKS endpoint is down a hostile `kid` provokes one
  outbound fetch **per callback request** rather than one per `UNKNOWN_KID_REFETCH_FLOOR`.
- **The rotation branch is untested.** Every existing test starts with a cold cache, so
  "cache is fresh but lacks this `kid`" and the rate-limited refetch are unexercised. A provider
  that rotates early could plausibly refuse valid sign-ins for up to `JWKS_TTL`.
- **Nothing constrains the scheme** of `FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT` or `_JWKS_URI`. An
  `http://` token endpoint sends this host's client secret as Basic credentials in cleartext, with
  no refusal, while the design says that POST goes over TLS.

## Acceptance
- [ ] **Failing-first test** — a `401 invalid_client` from the token endpoint produces a log line
      distinguishable from a refused authorization code, and **the caller-facing answer is
      byte-identical** to the refused-code case. Both halves in one test: the split must not become
      a disclosure.
- [ ] An unpublished `kid` is likewise distinguishable in the log from a refused code.
- [ ] A `http://` token endpoint or JWKS URI is **refused at startup**, naming the variable — not at
      the first sign-in. Follow the existing config refusal's shape.
- [ ] The unknown-`kid` refetch is rate-limited **even when the JWKS endpoint is failing**, proven
      by a test that counts requests reaching a stub that only ever errors.
- [ ] A key rotation is picked up: a token signed by a key published *after* the cache was filled
      verifies, without waiting out `JWKS_TTL`.

## Notes
- `SignInRefusal` already models exactly this split for the front channel — `UnknownState`,
  `NoBinder` and `AnotherBrowser` are three log lines and one caller-facing answer, added by X-15.
  This story is the same move applied to `ExchangeError`, and that precedent is the shape to copy.
- Do **not** widen what reaches the caller. `a_refusal_tells_the_caller_nothing_about_the_provider`
  is the existing guard and must stay green.
- Whether cleartext should be refused outright or permitted for loopback (a local test IdP is a real
  workflow) is a judgment call this story has to make and record.
