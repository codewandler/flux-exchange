---
id: X-102
title: Multiplex agent channel subscriptions
status: in-progress
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
---

# Multiplex agent channel subscriptions

## Goal
Serve one authenticated agent WebSocket that multiplexes opaque channel subscriptions and live
at-most-once events without coupling vendor-channel lifetime to subscribers.

## Acceptance
- [ ] Subscribe and unsubscribe commands return request-correlated acknowledgements or refusals.
- [ ] One vendor stream fans out through bounded 32-event subscriber queues.
- [ ] A slow subscriber alone is closed and counted; no replay or cursors are implied.
- [ ] Events carry connector, binding, declared event, receive time and raw typed payload.

## Progress

- 2026-08-02: authenticated request-correlated subscribe/unsubscribe, live at-most-once fan-out and
  isolated 32-event subscriber queues are implemented behind the optional channel supervisor.
