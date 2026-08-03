---
id: X-100
title: The development command keeps a real browser session
status: done
priority: 0
epic: local-identity
areas: [exchange-server, supply-chain]
note: "a route fixture covered --dev, but no repeatable test spawned cargo run; meanwhile the explicit-roster page rendered <handle> as an empty HTML element"
---

# The development command keeps a real browser session

## Goal
`cargo run --locked -- --dev` remains an executable local-development contract: it derives
`user:${USER}@dev`, offers the one-click sign-in action, exchanges it for an HttpOnly cookie and
resolves that cookie back to the same `dev` principal.

## Acceptance
- [x] A failing-first page test reproduces the invisible bearer placeholder from the explicit-roster
      instructions and the rendered page visibly says `Authorization: Bearer <handle>`.
- [x] A repository test starts the real binary through the documented Cargo command with an isolated
      startup user and ephemeral loopback port, then drives the sign-in page, POST, cookie and
      authenticated session over HTTP.
- [x] Both ordinary CI and the tag-triggered publication gate run that exact process test, so a route
      fixture cannot stand in for a broken CLI composition.
- [x] The complete repository gate passes and the repair is included in the next release.

## Progress
- 2026-08-03: The existing tests cover `Startup::select`, the assembled router and a real socket,
  separately. X-99 also recorded one manual process walk, but no committed test kept those pieces
  together. The reported page exposed a second bug: raw `<handle>` in host-authored HTML is parsed as
  an element, leaving the visible instruction as `Authorization: Bearer `.
- 2026-08-03: the first CI process run exposed ANSI-coloured tracing under `CARGO_TERM_COLOR=always`;
  the harness now gives the spawned server a machine-readable log environment. Main run 30788013022
  and tag/publish run 30788162730 both passed the exact Cargo command browser round trip in v0.15.0.

## Notes
- Cargo's first `--` is the argument-forwarding boundary. `cargo run --dev` does not pass `--dev` to
  the binary; the supported spelling is `cargo run -- --dev` (or the locked form used by the test).
- An explicitly configured `FLUX_EXCHANGE_DEV_IDENTITY` roster remains deliberately different: it
  may contain several principals, so it cannot offer the shorthand's automatic one-person action.
