---
id: X-02
title: "Serve HTTP, and refuse a reachable bind with no way to authenticate"
status: done
epic: serve
design: docs/designs/http-surface.md
note: "flux-server's precedent: a non-loopback bind without a token is refused AT STARTUP, because a daemon that auto-approves behind an open listener is RCE"
---

# Serve HTTP, and refuse a reachable bind with no way to authenticate

## Goal
An HTTP server with a health route, bound to loopback by default, which **refuses to start** on a
reachable address unless a principal can be resolved.

## Acceptance
- [x] `GET /health` answers on `127.0.0.1` by default.
      → `main::tests::health_answers_over_a_socket_on_the_default_interface` (a real socket on the
      default interface), `bind::tests::the_default_bind_is_loopback`,
      `main::tests::the_default_is_used_when_nothing_is_configured`.
- [x] **Failing-first test** — startup on a non-loopback bind with no identity configured is
      **refused**, and the error names what would have worked. It must not start-and-warn: an
      operator who misses a warning has an open credential-holding service.
      → `bind::tests::a_reachable_bind_without_identity_is_refused` (failed at `71c4014` with
      `E0425: cannot find function admit_bind`), `bind::tests::the_refusal_names_what_would_have_worked`,
      `bind::tests::the_unspecified_addresses_are_reachable`. The check runs before the socket opens
      (`main.rs` `serve`), and the process exits non-zero.
- [x] Health is the only route reachable without a principal, and that is asserted by a test that
      enumerates routes rather than by inspection.
      → `routes::tests::health_is_the_only_route_reachable_without_a_principal` walks
      `routes::published()` and drives a real anonymous request per route through the **assembled**
      app. `routes::tests::the_declared_access_is_what_decides_the_answer` is the guard's guard.
- [x] The framework choice (axum, or otherwise) is recorded in a design note with its reason.
      → [`docs/designs/http-surface.md`](../designs/http-surface.md).

## Progress
- **Done.** Merged from `impl/X-02`; gate green on the integration branch after merge.
- Independently reviewed: a 23-address sweep found no reachable bind that is admitted (IPv4-mapped
  IPv6 and unparseable hostnames both fail closed), 18 request shapes were driven at a guarded spy
  route and none reached the handler, and the enumeration test was proven live by flipping a spy
  route to `Anonymous` and watching it fail by name.
- The surface lives in `crates/exchange-server/src/routes/`, one module per feature area, assembled
  at a single merge site (`MODULES` in `routes/mod.rs`). A module declares its routes **as data** and
  its `Router` is derived from them — axum's `Router` cannot be introspected, so an opaque
  per-module `Router` would make the enumeration test unable to see a future module. X-03 and X-06
  each add a file plus one line in `MODULES`.
- `AppState::without_identity()` is the only constructor today. **X-03 adds the `with_identity`
  counterpart**; `admit_bind` already keys off `AppState::identity_binding()`, so binding a provider
  is all that is needed to make a reachable bind legal.
- `Access::Principal` carries a narrow `#[allow(dead_code)]`: no route needs a principal until X-03,
  and inventing one to satisfy the lint would have been worse. **X-03 should remove that attribute.**
- Not done here, deliberately: a `WWW-Authenticate` challenge on the `401`, and TLS. Both noted in
  the design note's closing section.

## Notes
- Precedent worth copying, not inventing: flux's own HTTP server requires a bearer on every route
  except health and the discovery card, and **rejects a non-loopback bind without a token at
  startup**. The reasoning there — the daemon auto-approves tools, so an open listener is RCE —
  applies here with credentials in place of tools.
- No flux-coupled dependency is needed for this story.
