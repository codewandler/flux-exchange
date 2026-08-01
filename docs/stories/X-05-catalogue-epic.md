---
id: X-05
title: "The catalogue surface (epic)"
status: done
priority: 3
epic: catalogue
note: "connector-catalog has ZERO dependencies — no flux coupling, no IO, no runtime. This epic is unblocked by construction"
---

# The catalogue surface (epic)

## Goal
Serve what exists: the connectors, their operations, and the metadata a grant is written over — so
the console stops rendering fixtures and an agent can discover what it may call.

## Acceptance
- [x] X-06 — the catalogue is served from `connector-catalog`.
- [x] X-07 — the console reads it instead of `src/fixtures/catalog.ts`, and the fixture banner is
      removed **in the same change** that makes it untrue.

## Progress
- **Done.** Both children landed: X-06 serves the catalogue (53 connectors, 299 operations) and X-07
  reads it in the console, deleting `src/fixtures/catalog.ts` and its banner in the same change.
- The epic's Goal is met with one caveat worth carrying forward: an agent can now *discover* what
  exists, but not yet what it may call — every operation carries `admitted: null` because no identity
  provider binds until X-03 and no grant is evaluated until X-13. That is deliberate; the catalogue
  states what exists and never silently filters by grant.

## Notes
- `codewandler-connector-catalog` 0.8.0 has **no dependencies at all** — static data compiled in.
  Nothing about the engine-line blocker (X-11) touches this epic.
- Compiled-in is the right start and the wrong destination: the family intends bundles a host loads
  at runtime. Do not build anything that assumes the catalogue can only be static.
