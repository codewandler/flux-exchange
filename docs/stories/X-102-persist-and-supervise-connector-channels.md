---
id: X-102
title: Persist and supervise connector channels
status: in-progress
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
---

# Persist and supervise connector channels

## Goal
Keep one tenant-owned vendor channel alive independently of subscribers and restore it after an
Exchange restart.

## Acceptance
- [ ] Failing-first tests prove tenant-derived storage, restoration and transient reconnect.
- [ ] A channel references an existing connection and declared binding; request bodies cannot set
      tenant, endpoint, credential or placement.
- [ ] Credential or connection-setting rotation restarts the affected supervisor.
- [ ] Placement is resolved by an operator-owned port and refuses before credentials are read.

## Progress

- 2026-08-03: Exchange now binds `connector_pack::channel_plan` to the released 0.17 pack and runs
  its plan through Flux 0.54's guarded channel runtime. Restoration, reconnect, rotation restart and
  operator-owned placement tests pass; multi-tenant composition still refuses without remote
  placement before any credential is read.
- 2026-08-02: persistent tenant-scoped store, independent supervisor restoration/reconnect, rotation
  restart hooks and placement-before-runner ordering are implemented. The released typed-placement
  and connector runner bindings remain dependency-ordered follow-up work.
- 2026-08-02: story opened; implementation starts behind host-owned store and runner ports so the
  unpublished connector/Flux release line is not bypassed with a sibling dependency.
