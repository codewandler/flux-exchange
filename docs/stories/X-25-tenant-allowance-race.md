---
id: X-25
title: "A tenant's allowance holds against its own concurrent creates"
status: ready
priority: 2
epic: connections
areas: [exchange-server]
note: "found by X-22's implementor in the bound it had just added, 2026-08-01: occupancy is read and written under a claim keyed per (tenant, connector), so one tenant issuing concurrent creates to different connectors can overshoot the allowance — closing it means reversing a property connection_guard deliberately pins"
---

# A tenant's allowance holds against its own concurrent creates

## Goal
`MAX_TENANT_STORE_BYTES` bounds what a tenant holds even when that tenant is issuing several
creates at once.

## The race

X-22 bounds a tenant's occupancy by reading what it already holds, deciding, then writing. That
read-decide-write runs under a `ConnectionGuard` claim keyed per **`(tenant, connector)`**, so two
creates by the same tenant to *different* connectors do not contend: each reads an occupancy the
other has not written yet, and both are admitted. The tenant ends up over its allowance.

**Reported by the implementor that introduced the bound**, in the same report as the bound itself,
rather than left for someone else to find.

## Why this is priority 2 and not priority 1

The overshoot is **bounded, not unbounded**. Every value still passes `MAX_CREDENTIAL_VALUE_BYTES`
independently, so the worst case degrades to the per-value ceiling X-22 was tightening —
roughly `addresses × 8 KiB`, about 480 KiB against today's 60 catalogued addresses — rather than to
no bound at all. It also needs deliberate concurrency from one authenticated tenant; ordinary use
does not reach it.

So the shared-fate property X-22 exists to protect still holds approximately. This story makes it
hold exactly.

## The hard part, which is why it was not fixed in X-22

Closing this means **serialising one tenant's connection changes across connectors** — and that
reverses a property `connection_guard` deliberately pins today:

> `claims_do_not_reach_across_tenants_or_connectors` — "the same tenant's connection to another
> connector is a different claim"

That test is not incidental; it is the guard's statement about how much contention it is willing to
impose. Reversing it is a decision *about the guard*, which is why X-22 documented the limit on
`occupied` rather than half-fixing it.

## Acceptance
- [ ] **Failing-first test** — two concurrent creates by one tenant to *different* connectors, each
      individually admissible, cannot leave the tenant over `MAX_TENANT_STORE_BYTES`. It must fail
      before the fix, which means it has to actually interleave rather than run sequentially; the
      existing `two_concurrent_creates_cannot_both_succeed` is the precedent for driving that.
- [ ] Two concurrent creates by **different tenants** still proceed without contending — the
      serialisation must not become a global lock. Asserted in the same run.
- [ ] `claims_do_not_reach_across_tenants_or_connectors` is either still green, or **replaced by a
      test stating the new property and saying why it changed**. Do not delete it quietly; it
      records a decision, and reversing that decision is the substance of this story.
- [ ] The tenant-scoped contention is documented where the guard's own doc argues its scope, so the
      next reader sees why one tenant serialises and two do not.
- [ ] `occupied`'s doc note about this known limit is removed, since the limit is gone.

## Notes
- Two shapes worth weighing, and the story does not mandate either: a second claim keyed on the
  tenant alone, taken around the occupancy decision only; or widening the existing claim's key.
  The first keeps the per-connector claim's meaning intact and adds contention only where the
  allowance is actually decided — probably better, but argue it.
- **Whatever shape: this is single-process only, like everything else on this surface.** Two
  replicas over one store race regardless, and `connection_guard.rs` already says so. Do not imply
  otherwise in the new doc.
- Measure what the added contention costs a tenant creating several connections in sequence. If it
  is meaningful, say so — a correct bound that makes ordinary use slow is a trade worth stating,
  not hiding.
