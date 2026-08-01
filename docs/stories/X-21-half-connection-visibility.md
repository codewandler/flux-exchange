---
id: X-21
title: "A half connection is distinguishable from a deliberately partial one"
status: backlog
epic: connections
areas: [exchange-server]
note: "raised by X-18's implementor, 2026-08-01: GET answers 200 for a connection whose delete failed half way, which reads as 'connected' — but a connector may legitimately hold a subset of what it declares, so the two render identically and telling them apart needs a record this module deliberately does not keep"
---

# A half connection is distinguishable from a deliberately partial one

## Goal
An operator reading `GET /api/connections/{connector}` can tell a connection that was damaged from
one that was always partial.

## Why this is not a one-line change

X-18 made a *failed* delete report honestly in its own answer. It deliberately did not change `GET`,
and the reason is worth keeping:

- A connector may **legitimately** hold a subset of what it declares —
  `a_connection_may_carry_a_subset_of_what_is_declared` asserts exactly that.
- So "half destroyed by a failed delete" and "deliberately partial" render identically today, and
  no amount of inspecting the store distinguishes them.
- Telling them apart requires a **record beside the store** saying what was intended. `list`'s doc
  states this module deliberately keeps no second source of truth.

So this is a design question — whether this surface acquires that record, and what it costs — not a
status-code change. It sits in `backlog` rather than `ready` because it needs a design doc first.

## Also in this neighbourhood
- **A crash mid-loop leaves the same state and nothing reconciles at startup.** X-18 made an
  *answered* partial delete honest; a process that dies inside the loop still leaves a half
  connection with nobody told. That needs the same record, which is why it belongs here rather than
  as its own story.

## Notes
- Write a design doc first (`/track:design`). The question to answer is whether a second source of
  truth earns its cost here, and if not, what an operator is supposed to do instead.
- `TestStore::recovers()` resets `puts_allowed` and `fails` but not `deletes_fail` or
  `deletes_allowed`. Harmless today; a future test calling it and expecting deletes to work again
  will be surprised. Worth tidying whenever this area is next touched.
