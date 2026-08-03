---
id: X-110
title: Edit inbound channel grants in the console
status: done
priority: 0
epic: generated-connector-channels
areas: [exchange-server, console]
design: docs/designs/generated-connector-websocket-channels.md
note: "channel creation without an inbound grant editor leaves the operator unable to complete create → grant → subscribe"
---

# Edit inbound channel grants in the console

## Goal
An operator can grant a channel's closed declared event subset without hand-editing the grant file.

## Acceptance
- [x] `PUT /api/grants` accepts inbound binding/event subsets, resolves their connector from the
      enclosing grant, and refuses unknown bindings, empty sets and undeclared events.
- [x] `GET` and preview return inbound authority so whole-set replacement cannot silently drop it.
- [x] The Grants screen derives checkboxes from anonymous channel declarations and preserves inbound
      authority whenever declarations cannot be read.
- [x] Held and preview cards show inbound consequences beside admitted outbound operations.
- [x] Failing-first route coverage, console tests/build and the full release gate pass.

## Progress
- 2026-08-03: filed during the X-101 release UX audit after the Channels screen was runnable but
  the only way to authorize `/api/subscribe` was still a hand-edited grant file.
- 2026-08-03: route and console tests now cover declared subsets, all invalid subset shapes,
  round-trip preview/write, declaration-driven checkboxes, consequence rendering and fail-closed
  handling when declarations or an inbound grant response cannot be read.
- 2026-08-03: the complete main and tag gates passed and the inbound grant editor shipped in
  v0.15.0.
