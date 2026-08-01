---
id: X-07
title: "The console reads the live catalogue"
status: ready
priority: 7
epic: catalogue
note: "the fixture banner must come out in the SAME change that makes it untrue — a banner that outlives its condition teaches readers to ignore banners"
---

# The console reads the live catalogue

## Goal
Replace `console/src/fixtures/catalog.ts` with the served catalogue.

## Acceptance
- [ ] The console fetches the catalogue from the service.
- [ ] The fixture banner is removed in the same change, and `console/README.md` § Status is updated
      in the same change.
- [ ] The 15 carried components are **not** modified. If one needs a change, that is a finding worth
      reporting, not a patch to apply quietly — they are shared with flux-connectors.
- [ ] `console/test/components.test.mjs` stays green, including the import-invariant test.
- [ ] An unreachable service renders an explicit error naming the endpoint, not an empty catalogue.
      "Zero connectors" and "cannot reach the server" must not look the same.

## Progress
- (not started — X-06 first)

## Notes
- The components take data as props and resolve paths through an injected `PathResolver`; nothing in
  them fetches. Keep it that way.
