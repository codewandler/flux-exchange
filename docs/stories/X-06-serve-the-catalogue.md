---
id: X-06
title: "Serve the connector catalogue"
status: in-progress
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
- **The import is `connector_catalog`, not `catalog`** — corrected 2026-08-01 after it cost an
  implementor time. The crate does set `[lib] name = "catalog"`, but this workspace declares it under
  the dependency key `connector-catalog`, and a renamed key is what Cargo links it as. Either alias
  it (`use connector_catalog as catalog;`) or spell it out.
- `ConnectorSurface` in `crates/exchange-host/src/lib.rs` is the host-side view; it is a *view* of
  the catalogue, never a second model of one.
- **`catalog::Operation` declares `risk` and `idempotency` but has no `effects` field** (checked
  against 0.8.0). Acceptance asks for all three, so the third has to be *derived* and the derivation
  documented where a reader will find it — an operation whose `hosts` are non-empty reaches the
  network, and nothing in the catalogue speaks to `WorkspaceWrite` or `Process` today. Derive
  honestly and say it is derived; do not present an inferred effect as a declared one, and do not
  add an effect the catalogue cannot support.
- The two enum pairs are near-mirrors, not the same type: `catalog::Risk` matches `host::Risk`
  variant for variant, but `catalog::Idempotency::NonIdempotent` is `host::Idempotency::NotIdempotent`.
  Map with an exhaustive `match`, so a variant added upstream is a compile error rather than a
  silently wrong answer.
- `catalog::providers()` / `operations_of()` are the whole iteration surface, which is what makes
  "adding a connector requires no change to this route" achievable — read the catalogue, never a
  list maintained here.
