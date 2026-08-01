---
id: X-05
title: "The catalogue surface (epic)"
status: ready
priority: 3
epic: catalogue
note: "connector-catalog has ZERO dependencies — no flux coupling, no IO, no runtime. This epic is unblocked by construction"
---

# The catalogue surface (epic)

## Goal
Serve what exists: the connectors, their operations, and the metadata a grant is written over — so
the console stops rendering fixtures and an agent can discover what it may call.

## Acceptance
- [ ] X-06 — the catalogue is served from `connector-catalog`.
- [ ] X-07 — the console reads it instead of `src/fixtures/catalog.ts`, and the fixture banner is
      removed **in the same change** that makes it untrue.

## Progress
- (not started)

## Notes
- `codewandler-connector-catalog` 0.8.0 has **no dependencies at all** — static data compiled in.
  Nothing about the engine-line blocker (X-11) touches this epic.
- Compiled-in is the right start and the wrong destination: the family intends bundles a host loads
  at runtime. Do not build anything that assumes the catalogue can only be static.
