---
id: X-103
title: Publish the live subscribe surface
status: in-progress
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
---

# Publish the live subscribe surface

## Goal
Expose operator channel management and mark subscribe live only after the route and end-to-end
behavior exist.

## Acceptance
- [ ] Operator-only GET/POST/PUT/DELETE `/api/channels` routes enforce tenant derivation.
- [ ] `GET /api/subscribe` is authenticated and end-to-end tested.
- [ ] Descriptor, console, README and public capability documentation agree that subscribe is live.
- [ ] Anonymous catalogue and logs expose no endpoint, credential, auth header or private payload.

## Progress

- 2026-08-02: operator CRUD and authenticated subscribe routes exist and are tested, but the built-in
  composition intentionally binds no runner until compatible Flux/connector releases arrive.
  Descriptor, console and public docs therefore remain non-live as this story requires.
