---
id: X-13
title: "Grants gate invocation"
status: in-progress
epic: invoke
priority: 2
areas: [exchange-host, exchange-server]
note: "Selector and Grant are already tested types in exchange-host; this is where they become the thing standing between a principal and an effect"
---

# Grants gate invocation

## Goal
An operation runs only if a grant the caller holds admits it — decided from the operation's declared
metadata, not from a list of names.

## Acceptance
- [x] Invocation consults the caller's grants and refuses with `Error::NotGranted`, naming the
      principal and the operation.
- [x] **Failing-first test** — a read-only grant (`risk <= low`) admits the reads of a connector and
      refuses its writes, with no operation named anywhere in the grant.
- [x] **Failing-first test** — a grant for one connector does not reach another.
- [x] An explicit `deny` beats an explicit `allow`, end to end and not merely in the type.
- [x] Grants are storable and editable per tenant.

## Progress

**Landed, 2026-08-01.** `Invoker::invoke` step 3 is the gate the design reserved a slot for.

- **The gate is a chain the compiler checks, not two calls to remember.** `admit_runtime` still
  returns `Admitted`; `admit_grant` *consumes* it and returns `Granted`; `Granted::resolve` is the
  only route from this crate to `connector_pack::resolve` — `Admitted::resolve` is gone. Skipping
  either gate is a type error, which is X-48's pattern applied to X-13's own version of the same
  mistake.
- **Decided from the catalogue, not from a list.** `OperationFacts::of(&catalog::Operation)` is the
  one projection, and `routes::catalogue::view` now publishes *it* rather than a second copy — so
  what a client reads off `/api/catalogue` is what the gate decides on. Its three mapping tests
  moved into `grant.rs` with the derivation.
- **Grants are per tenant**, held in a file store (`FLUX_EXCHANGE_GRANTS`) behind a `Grants` port,
  bound by the binary. Per-principal grants are a narrowing this build does not make, and
  `grant.rs`'s module documentation says so rather than implying otherwise.
- **Fail-closed, including at composition.** No grant store bound → no invoker → `503` naming both
  settings. A tenant holding nothing runs nothing. The alternative (absent store admits everything)
  is the exposure this story closed, and it is refused as a default.
- **The published claim changed with the code.** The onboarding descriptor's `invoke` `warn` said
  *"gated by identity alone… any principal may run any operation in the catalogue"*; it now states
  the grant rule, the `403 not_granted` refusal, and that a tenant nobody has granted anything runs
  nothing. `console/src/minting.mts` and the README moved with it.

**What this does not do**, and what should be a story rather than a footnote:

1. **No surface edits a grant.** The store is a file an operator writes; there is no route and no
   console screen. Adding one means a new route module and a `routes/mod.rs` edit.
2. **A `Granted` carries the operation, not the grant that admitted it.** An execution record wants
   the second, and `docs/designs/invoke.md` §6 now says so where it used to defer the whole subject.
3. **`effects` is still derived from `hosts`.** Exact for all 53 shipped connectors, and a selector
   written on effects would under-report the day upstream ships a connector whose Flux declares
   more. Asserted rather than assumed, in `grant.rs`.

## Notes
- The unit tests in `crates/exchange-host/src/grant.rs` already pin the semantics, including the
  deny-beats-allow asymmetry and subset-not-intersection on effects. This story wires them to a route.

## Unblocked, 2026-08-01

X-11 removed the engine-line blocker, so this is no longer waiting on upstream. It **is** ordered
behind X-12 — there is nothing to gate until something invokes — and that is an ordering edge inside
the epic rather than a block.

**Two stories have been shipped on the explicit promise that this one closes their gap**, and both
said so rather than pretending otherwise:

- `docs/designs/agent-access.md`: an agent token "authenticates and authorises nothing beyond what
  any principal may do", which X-40 narrowed to *except that it may not create a principal*.
- X-04's design records that `invoke` is gated by identity alone until this lands, and calls it "a
  stated gap rather than a position".

So this is not a new capability bolted on; it is the thing two shipped stories are waiting for.
