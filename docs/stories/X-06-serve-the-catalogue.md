---
id: X-06
title: "Serve the connector catalogue"
status: done
epic: catalogue
note: "the operation metadata a grant is written over — risk, effects, idempotency — must be in the response, or Selector cannot be evaluated by anything but the server"
---

# Serve the connector catalogue

## Goal
Expose the compiled-in catalogue over HTTP: connectors, their operations, and each operation's
declared `risk`, `effects` and `idempotency`.

## Acceptance
- [x] A route lists connectors and a route returns one connector's operations with their declared
      metadata.
- [x] **Failing-first test** — the response carries `risk`, `effects` and `idempotency` for every
      operation. Without them a client cannot predict what a `Selector` admits, and the grant model
      becomes server-only folklore.
- [x] The response distinguishes *what exists* from *what this principal may call*. Do not filter the
      catalogue by grant silently — an agent that cannot see an operation it lacks cannot report that
      it was refused.
- [x] Adding a connector to the catalogue requires no change to this route.

## Progress
- **Done.** Merged from `impl/X-06`; gate green (61 tests).
- Serves 53 connectors / 299 operations, read from `catalog::providers()` — no connector name appears
  in non-test code, so a new connector needs no change here.
- **`effects` is derived, not declared, and says so on the wire.** `catalog::Operation` has no
  `effects` field, so the rule is `network` iff `hosts` is non-empty, and `effects_derived: true`
  travels beside it. Known gap: all 299 operations currently have hosts, so a review confirmed the
  `iff` is unpinned — replacing the body with an unconditional `Network` keeps the suite green. A
  synthetic `catalog::Operation` with empty `hosts` is what would close it.
- **The anonymous surface widened, deliberately.** X-02's `health_is_the_only_route…` became
  `the_anonymous_surface_is_only_what_was_declared_anonymous`, comparing against a `const ANONYMOUS`
  with the argument for each entry beside it. A review confirmed it got *stronger*: leaving every
  route declared `Anonymous` but making the anonymous arm apply the guard still fails the test, which
  a static table comparison could not do. The catalogue is anonymous because it is `&'static` data
  from a published crate, names no tenant or credential, and never reads a grant.

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
