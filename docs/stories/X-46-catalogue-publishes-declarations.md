---
id: X-46
title: "A connector's declared credentials are published, not discovered by provoking a refusal"
status: ready
priority: 1
epic: catalogue
areas: [exchange-server]
note: "found by X-44's implementor in the workaround it had to write, 2026-08-01: nothing publishes what a connector declares, so the console reads it out of the 422 that a deliberately-empty POST returns"
---

# A connector's declared credentials are published, not discovered by provoking a refusal

## Goal
A caller can ask what a connector requires, without asking to do something it will be refused.

## What X-44 had to do, and why it is not sustainable

X-44 built the connect form and needed one fact: **which credentials does this connector declare?**
Nothing on this host publishes it.

- `GET /api/catalogue/connectors/{id}/operations` serves `OperationFacts` and no credentials.
- `GET /api/connections` lists only connectors the tenant **already holds** — useless for connecting
  a new one.

The only place the service states a declaration is the **`422` refusal** to
`POST /api/connections/{connector}` with `{"credentials": {}}`, whose body carries
`{"declared": [...]}`. So the console discovers a declaration by asking to create a connection it
knows will be refused.

The implementor verified it writes nothing — the refusal precedes the connection claim, the store
probe and any `put`, and that ordering is load-bearing in `docs/designs/connections.md` — and wrote
the argument out rather than hiding it. It is correct today. It is still the wrong shape:

- **The console is coupled to an error body.** Reword that refusal, or change its shape, and every
  connector renders "what this connector declares could not be read" instead of a form.
- **X-43 established the opposite principle one week earlier**: a capability fact should be a
  *field*, not something inferred from a refusal. This is the same lesson at a different layer.
- It reads as an unwanted write to anyone auditing traffic, and the reviewer of a future story will
  reasonably ask why the console POSTs before every render.

## Acceptance
- [ ] **Failing-first test** — a caller can read a connector's declared credentials from the
      catalogue surface, and it fails before the field exists.
- [ ] The console's `loadDeclaration` reads **that**, and the empty-`POST` workaround is deleted. A
      test asserts the console issues **no `POST`** in order to render a connect form.
- [ ] **What is published is the declaration, not a tenant's state** — the names and whatever the
      catalogue already knows, never whether anyone holds them. That distinction is what keeps this
      on the anonymous-safe catalogue rather than the per-tenant surface.
- [ ] The `422` on an empty credential map is **unchanged** — it is a real refusal with its own
      argument in `docs/designs/connections.md`, and this story removes the *need* to provoke it, not
      the refusal itself.
- [ ] Existing catalogue answers are unchanged for every caller that does not ask for the new field.

## Notes
- Decide whether this belongs on the connector entry, alongside `operations`, or on its own path, and
  say why. `connector-catalog` is a published upstream crate — check what it already carries before
  adding anything here; the declaration may already exist and simply not be served.
- If the fact genuinely lives upstream and this host cannot publish it without a crates change, that
  is a finding to report, not a reason to widen the workaround.
