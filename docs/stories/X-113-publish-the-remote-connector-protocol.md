---
id: X-113
title: "Publish the complete remote connector protocol"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "one versioned contract covers invoke, subscribe, streamed output, cancellation, terminal status and lease frames under the same tenant/grant derivation"
---

# Publish the complete remote connector protocol

## Goal

Define the stable client/server contract Flux needs to use any Exchange-hosted connector without
learning its credential, endpoint or runtime placement.

## Acceptance

- [ ] HTTP request/response covers catalogue, management and one-shot invoke; one authenticated
      WebSocket covers events, streams, cancellation and lease liveness.
- [ ] Every frame is versioned, request-correlated, size-bounded and scoped to the resolved Service
      Account and grants; no caller-supplied tenant exists.
- [ ] Refused, unknown, unreachable, interrupted, expired and runtime-failed outcomes remain distinct.
- [ ] Reconnect and replay semantics are explicit and default to no replay; cursors exist only when a
      connector declares them.
- [ ] Contract tests are consumable by Flux C-503 and cover compatibility negotiation and malformed
      or unauthorized frames failing closed.

## Progress

- (not started)

## Notes

- X-101…X-105's `/api/subscribe` framing is the delivered starting point, not a second socket.
