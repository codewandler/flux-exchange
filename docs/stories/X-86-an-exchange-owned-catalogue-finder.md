---
id: X-86
title: One search bar over the catalogue this exchange actually serves
status: done
priority: 0
design: docs/designs/catalogue-finder.md
note: "owner-directed 2026-08-02: retire the copied explorer; exchange owns one search bar with connector, service and operation tabs, and adds channels only when it has real channel data"
---

# One search bar over the catalogue this exchange actually serves

## Goal
Make the catalogue fast to browse and search without maintaining a second copy of
flux-connectors' documentation explorer. The console presents only facts this exchange serves and
owns the UI that renders them.

## Acceptance
- [x] `GET /api/catalogue/connectors` publishes each connector's catalogue-declared vendor and
      description without publishing tenant state or a credential value.
- [x] A failing-first console test proves one search bar searches every visible fact and separates
      results into Connectors, Services and Operations tabs.
- [x] Connectors are the default browse view; a query persists across tabs; exact and prefix names
      rank before metadata matches; connector and service results can narrow the finder to their
      operations.
- [x] The finder state is shareable in the explorer fragment, changes replace history, and stale or
      unknown state widens rather than rendering an apparently empty catalogue.
- [x] There is no Channels tab until this exchange receives real channel metadata. There is no
      placeholder data and no change to flux-connectors.
- [x] The copied fifteen-component contract is retired. Current guidance, tests and console docs
      say that flux-exchange owns its catalogue UI while preserving one token vocabulary and the
      data-down/no-fetch component boundary.
- [x] Empty results, keyboard-accessible tabs, light/dark rendering and a phone-width layout are
      deliberate states rather than incidental output.
- [x] The Cargo and console gates pass, a CHANGELOG entry records the ownership and API change, and
      the generated story board is current.

## Progress
- 2026-08-02 — owner chose exchange-only ownership, a search-first tabbed finder, Connectors as the
  default browse view, relevance ordering, and Channels only when real metadata reaches this host.
- 2026-08-02 — shipped the additive connector facts, exchange-native model and views, shareable
  finder state, keyboard/drill-down/empty-state coverage, responsive token-only CSS, and retired the
  copied explorer. Workspace build/test/Clippy/format and console test/build gates pass.

## Notes
- The current Rust catalogue dependency exposes providers and operations, but no events or channel
  declarations. The console adapter therefore fills both with empty arrays today; an empty Channels
  tab would be a false affordance.
- The existing service already returns the operation facts a grant is written over. The missing
  connector facts are `vendor` and `description`, both static catalogue data.
