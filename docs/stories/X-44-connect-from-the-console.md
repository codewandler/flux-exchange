---
id: X-44
title: "An operator can connect a connector without reaching for curl"
status: ready
priority: 2
epic: connections
areas: [console]
note: "X-34 shipped a read-only Connections view and said plainly that a connect form belongs in its own story. The console's first stated job is to wire things up, and it currently cannot"
---

# An operator can connect a connector without reaching for curl

## Goal
A signed-in operator can create a connection from the console.

## Why this is the console's most conspicuous gap

`docs/vision.md` gives the console two jobs: **wire things up** and **see what happened**. X-34 built
the surface that shows what is wired, and was explicit about what it did not build:

> **No connect form** — `POST` needs per-connector credential inputs and belongs in its own story.

That was the right call for that story and it leaves the console unable to do the first of its two
jobs. Today an operator reads their connections in a browser and creates them with `curl`.

## Acceptance
- [ ] **Failing-first test** — a connect form renders for a connector, with an input per credential
      that connector **declares**, and fails before it exists. Server-rendered, following
      `shell.test.mjs`'s precedent.
- [ ] The inputs come from the **catalogue's declaration**, not from a list the console maintains. A
      connector that gains a credential must gain an input without anyone editing the console.
- [ ] **No credential value is ever rendered back.** The form writes; it does not read values, and
      after a successful connect the view shows addresses and `held`, exactly as it does now. Assert
      it — this is the north star at the UI layer.
- [ ] A refusal from the service is shown **as the service worded it**, not re-worded by the console.
      Several stories went to trouble over those sentences; the console must not invent its own.
- [ ] The `409` on an already-connected connector is surfaced as what it is — the connection exists —
      and points at rotation rather than at delete. X-39 shipped rotation for exactly this reason.
- [ ] Nothing under `console/src/components/` is modified.

## Notes
- Credential inputs are secrets. Use `type="password"`, do not persist them anywhere, and do not put
  them in the URL. A value in a query string is a value in an access log.
- Read X-39's story before deciding what to do about `409` — an upsert is deliberately refused, and
  the console must not offer one by stitching delete-then-create together behind a button. That would
  reintroduce the window X-39 exists to remove.
