---
id: X-104
title: Multiplex agent channel subscriptions
status: done
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
---

# Multiplex agent channel subscriptions

## Goal
Serve one authenticated agent WebSocket that multiplexes opaque channel subscriptions and live
at-most-once events without coupling vendor-channel lifetime to subscribers.

## Acceptance
- [x] Subscribe and unsubscribe commands return request-correlated acknowledgements or refusals.
- [x] One vendor stream fans out through bounded 32-event subscriber queues.
- [x] A slow subscriber alone is closed and counted; no replay or cursors are implied.
- [x] Events carry connector, binding, declared event, receive time and raw typed payload.

## Progress

- 2026-08-03: an assembled-server test now starts a real TCP listener, authenticates with a
  development agent principal, upgrades `/api/subscribe`, and proves correlated subscribe/event/
  unsubscribe frames over the actual WebSocket route.
- 2026-08-02: authenticated request-correlated subscribe/unsubscribe, live at-most-once fan-out and
  isolated 32-event subscriber queues are implemented behind the optional channel supervisor.
- 2026-08-03: the complete main and tag gates passed and multiplexed subscriptions shipped in
  v0.15.0.
