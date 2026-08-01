---
id: X-02
title: "Serve HTTP, and refuse a reachable bind with no way to authenticate"
status: in-progress
priority: 1
epic: serve
note: "flux-server's precedent: a non-loopback bind without a token is refused AT STARTUP, because a daemon that auto-approves behind an open listener is RCE"
---

# Serve HTTP, and refuse a reachable bind with no way to authenticate

## Goal
An HTTP server with a health route, bound to loopback by default, which **refuses to start** on a
reachable address unless a principal can be resolved.

## Acceptance
- [ ] `GET /health` answers on `127.0.0.1` by default.
- [ ] **Failing-first test** — startup on a non-loopback bind with no identity configured is
      **refused**, and the error names what would have worked. It must not start-and-warn: an
      operator who misses a warning has an open credential-holding service.
- [ ] Health is the only route reachable without a principal, and that is asserted by a test that
      enumerates routes rather than by inspection.
- [ ] The framework choice (axum, or otherwise) is recorded in a design note with its reason.

## Progress
- (not started)

## Notes
- Precedent worth copying, not inventing: flux's own HTTP server requires a bearer on every route
  except health and the discovery card, and **rejects a non-loopback bind without a token at
  startup**. The reasoning there — the daemon auto-approves tools, so an open listener is RCE —
  applies here with credentials in place of tools.
- No flux-coupled dependency is needed for this story.
