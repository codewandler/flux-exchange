---
id: X-46
title: "A connector's declared credentials are published, not discovered by provoking a refusal"
status: done
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
- [x] **Failing-first test** — a caller can read a connector's declared credentials from the
      catalogue surface, and it fails before the field exists.
- [x] The console's `loadDeclaration` reads **that**, and the empty-`POST` workaround is deleted. A
      test asserts the console issues **no `POST`** in order to render a connect form.
- [x] **What is published is the declaration, not a tenant's state** — the names and whatever the
      catalogue already knows, never whether anyone holds them. That distinction is what keeps this
      on the anonymous-safe catalogue rather than the per-tenant surface.
- [x] The `422` on an empty credential map is **unchanged** — it is a real refusal with its own
      argument in `docs/designs/connections.md`, and this story removes the *need* to provoke it, not
      the refusal itself.
- [x] Existing catalogue answers are unchanged for every caller that does not ask for the new field.

## Notes
- Decide whether this belongs on the connector entry, alongside `operations`, or on its own path, and
  say why. `connector-catalog` is a published upstream crate — check what it already carries before
  adding anything here; the declaration may already exist and simply not be served.
- If the fact genuinely lives upstream and this host cannot publish it without a crates change, that
  is a finding to report, not a reason to widen the workaround.

## Progress
- **Done 2026-08-01.** Gate green: 48 + 3 + 10 + 5 host, 231 server, 51 console. Genuine merge-base
  failure on both halves — Rust `404` vs `200`, console `POST` vs `GET`.
- **The fact already existed.** `connector-catalog` 0.9 has carried `Provider::auth` all along, and
  `routes/connections.rs` was already reading it to build the very `422` the console was provoking.
  So this was purely *serve a fact that exists* — no dependency change, no upstream request.
  **That is the second story in a row where checking upstream first turned a supposed limitation
  into a lookup**; X-12 found the same with C-405 and `Provider::runtime`.
- **Its own path rather than a field**, argued on shape: a credential is declared at *provider* level
  upstream, so nesting it in an operations answer would make a client fetch 299 operations to learn
  two names, and putting it on the 53-entry listing would turn a directory into a payload. It also
  makes "existing answers unchanged" true by construction. X-43 chose the same shape.
- **`place` and `acquire` are deliberately not published** — they describe how this host composes a
  request at invoke time, not what an operator must store. `authority` and `leaf` **are**, because
  together they are why a declaration may be unaddressable, which today is only learnable by being
  refused.
- **This widens the anonymous surface** with per-connector credential *names*. Byte-identical vendor
  data anyone can `cargo add`, naming no tenant — but it is a real widening and the line a reviewer
  should check.
- **Behaviour change:** a connector declaring nothing now yields `{status:'ready', credentials:[]}`
  rather than a refusal. `Connect.mts` already had the branch for it.
- **Carried forward:** the wire shape is pinned in two places that cannot see each other — a Rust
  contract test and a console fixture. Both go green independently if the shape changes while the
  real console breaks.
- **Reviewed PASS.** The anonymous-surface question was settled **structurally rather than by
  probing**: the handler is `async fn credentials(Path(connector)) -> Response` — no `State`, no
  identity extractor — so it **cannot observe a caller**, and `Access::Anonymous` attaches no layer.
  A two-tenant probe could only confirm what the signature already forces.
- **The disclosure guard was proved non-decorative**, not assumed: the reviewer added `held: bool` to
  `CredentialView` and three tests failed, including the wire-contract one.
- Upstream `Credential` is `{name, leaf, acquire, place}` — **there is no value field to leak**, so
  the withholding of `place`/`acquire` is about wire-contract minimalism rather than secrecy.
- **Two coverage gaps found, filed as [X-49](X-49-pin-the-branches-x46-opened.md):** the
  `declares-nothing` render path this story newly made reachable has no test, and
  `the_existing_catalogue_answers_gained_no_field` lacks the non-vacuity assertion its sibling
  twenty lines earlier has.
