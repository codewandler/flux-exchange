---
id: X-25
title: "A tenant's allowance holds against its own concurrent creates"
status: done
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
- [x] **Failing-first test** — two concurrent creates by one tenant to *different* connectors, each
      individually admissible, cannot leave the tenant over `MAX_TENANT_STORE_BYTES`. It must fail
      before the fix, which means it has to actually interleave rather than run sequentially; the
      existing `two_concurrent_creates_cannot_both_succeed` is the precedent for driving that.
- [x] Two concurrent creates by **different tenants** still proceed without contending — the
      serialisation must not become a global lock. Asserted in the same run.
- [x] `claims_do_not_reach_across_tenants_or_connectors` is either still green, or **replaced by a
      test stating the new property and saying why it changed**. Do not delete it quietly; it
      records a decision, and reversing that decision is the substance of this story.
- [x] The tenant-scoped contention is documented where the guard's own doc argues its scope, so the
      next reader sees why one tenant serialises and two do not.
- [x] `occupied`'s doc note about this known limit is removed, since the limit is gone.

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

## Progress
- **Done 2026-08-01.** Gate green: 43 + 176 tests, clippy clean, fmt clean.
- **Shape: a second claim keyed on the tenant alone**, held from before the occupancy read to the
  end of `create` — not merely around the decision, because it is the `put` loop that makes the read
  stale, so releasing after the decision would have closed nothing.
- **`DELETE` deliberately does not take the tenant claim.** A delete only frees allowance and cannot
  cause an overshoot, and the case a `DELETE` exists for is revoking a leaked secret, which must not
  wait behind another connector's create.
- **`claims_do_not_reach_across_tenants_or_connectors` was kept green, not deleted.** The
  per-connection claim is unchanged and still what makes `POST` and `DELETE` to one connection
  exclude each other; a new test states the width that *did* change and why.
- **The different-tenants half was mutation-checked**, not assumed: keying the tenant claim on a
  constant makes it fail on the first attempt, so it is not passing by construction.
- **The proof is a looped race, stated as such** — 200 attempts with a widened window, failing 10/10
  runs at the base within the first ten attempts, clean 8/8 after. A single attempt is not certain
  to interleave, and the report said so rather than implying a deterministic schedule.
- **Measured, as the story asked, rather than reassured:** the claim costs **80 ns**; a create is
  already ~300 µs dominated by the occupancy walk, so it is ~0.03% and not meaningful for sequential
  use. The cost lands on a client firing several creates for one tenant in parallel, which now gets
  a retryable `409` where it previously got a `201` and an allowance that did not hold.
- **Knowingly accepted:** a `DELETE` racing a same-tenant `POST` to another connector can free space
  the create already read past, so the create can refuse when there was room. Retryable, errs toward
  the bound rather than past it, and closing it would mean putting `DELETE` inside the tenant claim —
  the thing that must not wait. First place to look at an unexplained `409` on create.
- Also carried: `change_in_flight` and `allowance_change_in_flight` are not machine-distinguishable
  — same status, same JSON shape, only the prose differs.
