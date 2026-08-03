---
id: X-101
title: Serve generated connector channels
status: done
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
note: "persistent tenant-owned channels, explicit inbound grants, one multiplexed agent WebSocket"
---

# Serve generated connector channels

## Goal
Make inbound connector bindings the tenant-safe mirror of operation invocation: operators own the
vendor channel and agents receive only explicitly granted declared events.

## Acceptance
- [x] X-102 through X-105 are done and their combined Rust, console and documentation gates pass.
- [x] Flux and connector dependencies move together only after compatible releases are published.

## Progress
- 2026-08-02: epic opened from the accepted generated connector WebSocket program.
- 2026-08-03: Flux 0.54.2 and connectors 0.17.0 were published before Exchange resolved their
  compatible lines. Main CI 30788013022, Pages 30788013027 and the v0.15.0 publication gate
  30788162730 passed; the host crate is live on crates.io.

## Notes
- The family release order is Flux, connectors, then Exchange.
