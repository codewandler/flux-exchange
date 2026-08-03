---
id: X-99
title: Serve generated connector channels
status: in-progress
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
note: "persistent tenant-owned channels, explicit inbound grants, one multiplexed agent WebSocket"
---

# Serve generated connector channels

## Goal
Make inbound connector bindings the tenant-safe mirror of operation invocation: operators own the
vendor channel and agents receive only explicitly granted declared events.

## Acceptance
- [ ] X-100 through X-103 are done and their combined Rust, console and documentation gates pass.
- [ ] Flux and connector dependencies move together only after compatible releases are published.

## Progress
- 2026-08-02: epic opened from the accepted generated connector WebSocket program.

## Notes
- The family release order is Flux, connectors, then Exchange.
