---
id: X-07
title: "The console reads the live catalogue"
status: done
priority: 7
epic: catalogue
note: "the fixture banner must come out in the SAME change that makes it untrue — a banner that outlives its condition teaches readers to ignore banners"
---

# The console reads the live catalogue

## Goal
Replace `console/src/fixtures/catalog.ts` with the served catalogue.

## Acceptance
- [x] The console fetches the catalogue from the service.
- [x] The fixture banner is removed in the same change, and `console/README.md` § Status is updated
      in the same change.
- [x] The 15 carried components are **not** modified. If one needs a change, that is a finding worth
      reporting, not a patch to apply quietly — they are shared with flux-connectors.
- [x] `console/test/components.test.mjs` stays green, including the import-invariant test.
- [x] An unreachable service renders an explicit error naming the endpoint, not an empty catalogue.
      "Zero connectors" and "cannot reach the server" must not look the same.

## Progress
- **Done.** Merged from `impl/X-07`; console gate green (13 tests, `npm run build` clean).
- `console/src/fixtures/catalog.ts` is deleted and the banner went with it, in the same change.
- All 15 carried components are **byte-identical** — verified with `git diff --exit-code`. The one
  file touched under `components/` is its README, which documented the import invariant by pointing
  at the now-deleted `src/fixtures/`; leaving it would have been the very failure this story is about.
- **Four findings were reported upstream rather than patched**, per `AGENTS.md`: the components
  cannot distinguish *absent* from *not published by this source*, so this service's thinner document
  makes `ProviderCard` render "not configured" in the danger colour on every card. Filed as
  flux-connectors **C-408**; `catalog.mts`'s `Operation` also lacks `effects`/`admitted`, so the
  console carries them alongside for now.
- Known limits, stated rather than discovered later: the console and API must share an origin (no
  CORS story), the catalogue loads N+1 requests, and one failing connector fails the whole load by
  design — a partial catalogue rendered as complete is the same lie as an empty one.

## Notes
- The components take data as props and resolve paths through an injected `PathResolver`; nothing in
  them fetches. Keep it that way.
