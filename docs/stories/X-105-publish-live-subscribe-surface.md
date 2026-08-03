---
id: X-105
title: Publish the live subscribe surface
status: done
epic: generated-connector-channels
design: docs/designs/generated-connector-websocket-channels.md
---

# Publish the live subscribe surface

## Goal
Expose operator channel management and mark subscribe live only after the route and end-to-end
behavior exist.

## Acceptance
- [x] Operator-only GET/POST/PUT/DELETE `/api/channels` routes enforce tenant derivation.
- [x] `GET /api/subscribe` is authenticated and end-to-end tested.
- [x] Descriptor, console, README and public capability documentation agree that subscribe is live.
- [x] Anonymous catalogue and logs expose no endpoint, credential, auth header or private payload.

## Progress

- 2026-08-03: the released connector/Flux runner is bound, channel CRUD and subscribe are live, the
  descriptor and public documentation agree, and the console manages tenant channels without ever
  receiving endpoint or authentication material.
- 2026-08-02: operator CRUD and authenticated subscribe routes exist and are tested, but the built-in
  composition intentionally binds no runner until compatible Flux/connector releases arrive.
  Descriptor, console and public docs therefore remain non-live as this story requires.
- 2026-08-03: the main Pages gate and complete tag gate passed and the live surface shipped in
  v0.15.0.
