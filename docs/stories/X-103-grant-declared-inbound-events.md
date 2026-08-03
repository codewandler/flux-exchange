---
id: X-103
title: Grant declared inbound events
status: in-progress
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
---

# Grant declared inbound events

## Goal
Extend tenant grants with explicit connector, binding and declared-event subsets, defaulting every
existing grant document to no inbound access.

## Acceptance
- [ ] Old grant documents deserialize with an empty inbound grant set.
- [ ] Every granted and requested event must belong to the binding's closed declaration.
- [ ] Cross-tenant channel identifiers and ungranted event subsets are refused.

## Progress

- 2026-08-03: grant writes and previews now resolve bindings from the catalogue, accept only
  non-empty declared event subsets, and faithfully report inbound authority. The console refuses an
  unreadable inbound response rather than dropping part of a whole-set grant on the next save.
- 2026-08-02: old grants default to no inbound access and closed connector/binding/event subsets are
  enforced at the subscription gate; cross-tenant opaque ids share the not-found response.
