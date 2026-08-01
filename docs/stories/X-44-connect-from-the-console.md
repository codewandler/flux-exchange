---
id: X-44
title: "An operator can connect a connector without reaching for curl"
status: done
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
- [x] **Failing-first test** — a connect form renders for a connector, with an input per credential
      that connector **declares**, and fails before it exists. Server-rendered, following
      `shell.test.mjs`'s precedent.
- [x] The inputs come from the **catalogue's declaration**, not from a list the console maintains. A
      connector that gains a credential must gain an input without anyone editing the console.
- [x] **No credential value is ever rendered back.** The form writes; it does not read values, and
      after a successful connect the view shows addresses and `held`, exactly as it does now. Assert
      it — this is the north star at the UI layer.
- [x] A refusal from the service is shown **as the service worded it**, not re-worded by the console.
      Several stories went to trouble over those sentences; the console must not invent its own.
- [x] The `409` on an already-connected connector is surfaced as what it is — the connection exists —
      and points at rotation rather than at delete. X-39 shipped rotation for exactly this reason.
- [x] Nothing under `console/src/components/` is modified.

## Notes
- Credential inputs are secrets. Use `type="password"`, do not persist them anywhere, and do not put
  them in the URL. A value in a query string is a value in an access log.
- Read X-39's story before deciding what to do about `409` — an upsert is deliberately refused, and
  the console must not offer one by stitching delete-then-create together behind a button. That would
  reintroduce the window X-39 exists to remove.

## Progress
- **Done 2026-08-01.** Console 42 -> 50 tests; Rust 45 + 220; build clean. Genuine merge-base
  failure.
- **Inputs derive from the connector's own declaration, asserted twice** — a credential this console
  has never heard of still renders an input, and a scan of every console source finds no real
  credential name in it. So a connector that gains a credential gains an input with nobody editing
  the console.
- **No value is rendered back:** success renders through `connectionCard`, *the same function the
  read-only listing uses*, so the page after a write shows addresses and `held` exactly as before.
- **The `409` points at rotation and never at delete** — the page names `PUT .../credentials/{cred}`
  per declared credential, and a test asserts the page contains no `DELETE` and the module issues
  none. Stitching delete-then-create behind a button would have reintroduced the window X-39 removed.
- **The form clears on success only.** A `413` or a typo would otherwise cost the operator two long
  secrets, and the value is in a `type="password"` element either way.
- **The decision worth reviewing, and filed as [X-46](X-46-catalogue-publishes-declarations.md):**
  nothing on this host publishes what a connector declares, so the declaration is read out of the
  `422` that a deliberately-empty `POST` returns. It writes nothing — the refusal precedes the claim,
  the probe and any `put`, and that ordering is load-bearing in the design — and the implementor
  wrote the argument out rather than hiding it. But **discovering a declaration by provoking a
  refusal couples the console to an error body**, and it is the same lesson X-43 had just established
  one layer down: a fact should be a *field*, not inferred from a refusal.
- **Carried forward:** the service's own `409` `would_have_worked` still says "delete the existing
  connection before creating another", which predates X-39. The console does not render that field,
  but the API still tells a `curl` user to do the thing X-39 removed.
