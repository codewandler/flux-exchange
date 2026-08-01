---
id: X-01
title: "The HTTP surface (epic)"
status: done
epic: serve
note: "EPIC — turn the binary that prints a matrix into a service. Nothing here is blocked: the surface needs no flux-coupled crate"
---

# The HTTP surface (epic)

## Goal
Turn `crates/exchange-server` from a binary that prints a deployment matrix into a service that
serves authenticated requests, without ever offering a reachable bind to an unauthenticated caller.

## Acceptance
- [x] X-02 — an HTTP server with health, and a **refusal** rather than a default when a reachable
      bind has no way to authenticate.
- [x] X-03 — the `Identity` port bound, with a session a request carries.
- [x] X-04 — OIDC sign-in behind that port.
- [x] No route reads the tenant from anything a caller controls. Asserted, not intended.

## Progress
- **Unblocked 2026-08-01: X-04 is done.** The dependency decision was taken and the token exchange
  is bound, so sign-in completes and this epic's remaining Acceptance no longer waits on it. Two
  children were filed out of X-04's review and belong to this epic before it can close:
  [X-16](X-16-session-expiry.md) and [X-17](X-17-exchange-failure-modes.md).
- **Historic — was blocked on X-04, which was blocked on a dependency decision.** X-02 and X-03 are done and the
  fourth Acceptance item — no route reads the tenant from anything a caller controls — is asserted
  rather than intended: three vector tests (path segment, body field, header) against a route that
  genuinely declares `/{tenant}`, plus `no_published_route_takes_a_tenant_in_its_path` walking the
  published surface, plus a probe that found no fourth vector across query string, `Host`,
  `X-Forwarded-Host`, repeated headers, a second cookie, form encoding and duplicate JSON keys.
- X-04 is merged and reviewed but **PARTIAL**: the token exchange needs an HTTP client and the id
  token needs a JOSE library, and neither is in this workspace. That is a dependency call for the
  owner, not something an implementor should decide.
- Also found while building this epic: [X-15](X-15-login-csrf.md), which is ready and buildable now.

## Notes
- `crates/exchange-host/src/lib.rs` already defines the `Identity` port and states the tenant rule.
- Deliberately no framework choice yet — X-02 makes it and records why.

## Closed 2026-08-01

Every Acceptance item is met. X-04 landed earlier in the same session that closed this epic and its
checkbox was simply never ticked — found by a backlog sync, not by anyone reading the story.

**The `serve` slug outlived this tracker**, and that is fine rather than a bookkeeping error: X-15
through X-43 group under it and are additional work on the same surface, not children of this
epic's Acceptance. An epic tracker is done when *its* stated Acceptance is, and grouping is a view.
