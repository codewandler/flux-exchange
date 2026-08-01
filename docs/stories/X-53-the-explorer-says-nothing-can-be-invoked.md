---
id: X-53
title: "The explorer stops badging operations this service runs as \"not live yet\""
status: ready
priority: 2
epic: catalogue
areas: [console]
note: "found by X-42's review, 2026-08-01: the fourth rendering of the invoke falsehood. service.mts sets works: false for every operation with the comment \"nothing in flux-exchange can be invoked yet\""
---

# The explorer stops badging operations this service runs as "not live yet"

## Goal
The catalogue explorer says what is true about invocation.

## Why this is a separate story and not part of X-42

X-42 found that `invoke` shipped in v0.7.0 while three console renderings said it had not, and
corrected all three. **This is the fourth**, and X-42 was right to leave it.

`console/src/service.mts:1075` reads *"nothing in flux-exchange can be invoked yet"* and `:1159` sets
`works: false` for **every** operation, so `ProviderCard.vue:37` renders "not live yet" on operations
this service will run.

It is out of X-42's reach for a real reason: the badge comes from components carried from
`flux-connectors`, and `AGENTS.md` § The console forbids editing those locally. So the fix is either
upstream or in what this repository feeds them.

## The question to answer before writing code

**What should `works` mean?** It is one boolean and there are at least four candidate readings, which
is why this is not a flag flip:

- the operation's connector is in the catalogue (always true here);
- the tenant has a connection to it;
- the tenant has supplied every setting that connector needs (X-47);
- the caller's principal could actually invoke it (there is no grant model, so: any principal).

The last two are **tenant-specific**, and the explorer is reachable anonymously. Whatever is chosen
must not turn a public page into a report on a tenant's connections.

## Acceptance
- [ ] `works`' meaning is written down where it is set, not inferred from its name.
- [ ] **Failing-first test** — an operation of a connector this service can run is not badged as
      unrunnable.
- [ ] Nothing tenant-specific reaches an anonymously-reachable surface. Assert it, the way X-42's
      `the_document_is_identical_with_two_tenants_connected` does.
- [ ] The comment at `service.mts:1075` is true when the story closes.

## Notes
- If the honest answer needs a component change, that is an upstream `flux-connectors` story and this
  one records the hand-off rather than editing a carried component locally.
- `console/test/shell.test.mjs:10` still states "`invoke`, `subscribe` and execution records do not
  exist". Stale comment, same falsehood, free to fix here.
