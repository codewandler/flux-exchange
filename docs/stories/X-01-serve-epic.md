---
id: X-01
title: "The HTTP surface (epic)"
status: ready
priority: 1
epic: serve
note: "EPIC — turn the binary that prints a matrix into a service. Nothing here is blocked: the surface needs no flux-coupled crate"
---

# The HTTP surface (epic)

## Goal
Turn `crates/exchange-server` from a binary that prints a deployment matrix into a service that
serves authenticated requests, without ever offering a reachable bind to an unauthenticated caller.

## Acceptance
- [ ] X-02 — an HTTP server with health, and a **refusal** rather than a default when a reachable
      bind has no way to authenticate.
- [ ] X-03 — the `Identity` port bound, with a session a request carries.
- [ ] X-04 — OIDC sign-in behind that port.
- [ ] No route reads the tenant from anything a caller controls. Asserted, not intended.

## Progress
- (not started)

## Notes
- `crates/exchange-host/src/lib.rs` already defines the `Identity` port and states the tenant rule.
- Deliberately no framework choice yet — X-02 makes it and records why.
