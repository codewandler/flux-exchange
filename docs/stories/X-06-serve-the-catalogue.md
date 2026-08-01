---
id: X-06
title: "Serve the connector catalogue"
status: ready
priority: 3
epic: catalogue
note: "the operation metadata a grant is written over — risk, effects, idempotency — must be in the response, or Selector cannot be evaluated by anything but the server"
---

# Serve the connector catalogue

## Goal
Expose the compiled-in catalogue over HTTP: connectors, their operations, and each operation's
declared `risk`, `effects` and `idempotency`.

## Acceptance
- [ ] A route lists connectors and a route returns one connector's operations with their declared
      metadata.
- [ ] **Failing-first test** — the response carries `risk`, `effects` and `idempotency` for every
      operation. Without them a client cannot predict what a `Selector` admits, and the grant model
      becomes server-only folklore.
- [ ] The response distinguishes *what exists* from *what this principal may call*. Do not filter the
      catalogue by grant silently — an agent that cannot see an operation it lacks cannot report that
      it was refused.
- [ ] Adding a connector to the catalogue requires no change to this route.

## Progress
- (not started)

## Notes
- `use catalog::…` — the crate's lib name is `catalog`, not `connector_catalog`.
- `ConnectorSurface` in `crates/exchange-host/src/lib.rs` is the host-side view; it is a *view* of
  the catalogue, never a second model of one.
