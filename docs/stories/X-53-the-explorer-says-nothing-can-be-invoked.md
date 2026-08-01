---
id: X-53
title: "The explorer stops badging operations this service runs as \"not live yet\""
status: in-progress
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
- [x] `works`' meaning is written down where it is set, not inferred from its name.
      → `console/src/service.mts:1149-1198`, the doc block on `SERVICE_RUNS_OPERATIONS`, which is
      what `adapt` puts on the shared `Status` at `:1216`.
- [x] **Failing-first test** — an operation of a connector this service can run is not badged as
      unrunnable. → `console/test/service.test.mjs:315`
      `an_operation_this_service_runs_is_not_badged_as_unrunnable`, with
      `the_cards_still_badge_from_works` at `:349` keeping it from guarding a rule the carried cards
      have stopped using.
- [x] Nothing tenant-specific reaches an anonymously-reachable surface. Assert it, the way X-42's
      `the_document_is_identical_with_two_tenants_connected` does.
      → `console/test/service.test.mjs:396` `the_explorer_is_the_same_document_for_two_tenants`:
      two tenants whose connections really differ, a comparison on the whole serialised document
      rather than on a field somebody thought to check, and the second half X-42's test does not
      need — the console must also never have *asked*, so every URL the catalogue path requested is
      asserted to be one of the anonymous catalogue routes.
- [x] The comment at `service.mts:1075` is true when the story closes. It is gone: the sentence it
      argued for is gone with it, and what stands there now is why `works` is what it is.

## What `works` was decided to mean

**This service runs this operation.** `POST /api/operations/{operation}/invoke` is in the published
surface, it is compiled against the same catalogue the console read, and dispatch ends in the
operation's own compiled Flux. It is not a claim that the reader may call it, that anyone holds the
credential, or that a call would succeed — the catalogue-wide `CONSOLE-NO-PRINCIPAL` banner is what
draws that line, and it is what keeps the badge from reading as a promise to the visitor.

The two tenant-specific readings (*has a connection*, *has every setting*) are rejected because this
explorer is reachable anonymously: a badge derived from either would turn a public page into a report
on somebody's connections, and would need `GET /api/connections`, which the catalogue path never
calls. The fourth (*this principal could invoke it*) is `admitted`, which is three-valued and whose
`null` is not `false`; collapsing it into a boolean destroys the distinction `OperationFacts` renders
and makes a public badge move with who is looking.

It is **derived, not asserted**: `surfaces.mts` says whether this service serves `invoke`, and
`routes::onboarding::tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`
measures that field against `routes::MODULES`. That is X-42's lesson applied a fourth time — derive,
and derive from the construct that answers the question being asked. Renaming that surface away makes
this read `false`, which under-claims rather than over-claims.

## Hand-offs this story did not make locally

- **`flux-connectors`, documentation only.** Six carried comments state that `works` is `false` for
  every operation and that nothing can make a live call — `ProviderCard.vue:34-42`,
  `StatusBadge.vue:4`, `OperationRow.vue:6`, `OperationList.vue:12-16`, `CatalogExplorer.vue:5` and
  `catalog.mts:311`. They are true of the documentation site and false of this host. **No behaviour
  needs to change**: the components compute the badge from the props they are given and render this
  document correctly. `AGENTS.md` § The console forbids editing them here, so this is the hand-off.
- **This repository, a published gap.** The served catalogue publishes no runtime per connector, so
  the console cannot see a connector whose declared runtime a multi-tenant deployment would refuse
  (`Deployment::admits`). It costs nothing today —
  `exchange_host::invoke::tests::the_whole_catalogue_declares_http` holds that every shipped
  connector declares `Runtime::Http`, so the gate refuses none of them — and it is the one fact that
  would make this badge wrong per connector. Worth a story before a non-HTTP connector ships.

## Notes
- If the honest answer needs a component change, that is an upstream `flux-connectors` story and this
  one records the hand-off rather than editing a carried component locally.
- `console/test/shell.test.mjs:10` still states "`invoke`, `subscribe` and execution records do not
  exist". Stale comment, same falsehood, free to fix here. **Done**, and it now names which of the
  two gaps `invoke` has.
- `console/README.md:59` carried the same falsehood in a fifth place — *"There is no invoke route
  yet, so every operation reads 'not live yet', which is true"* — describing the very badge this
  story changed. Corrected here rather than left to contradict the code beside it.

## Progress

**2026-08-01 — implemented on `impl/X-53`.** Console-only; nothing under `crates/` was touched.

- `console/src/service.mts` reads `SURFACES` from `surfaces.mts` (the one thing in that file that is
  not the served document) and sets `works` from whether this build's service serves `invoke`.
- `console/test/service.test.mjs` carries the three new tests above; the badge test failed at the
  merge base with *"`zendesk-ticket-show` … is marked as something this service cannot run"*.
- `console/README.md` and `console/test/shell.test.mjs`' header corrected.
- Gate green in the worktree: `npm test` (75), `npm run build`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`.

What a resuming agent would look at next: the `CONSOLE-NO-PRINCIPAL` summary at
`console/src/service.mts:1070` (and the same claim on `ServedOperation` at `:125`) still says *"this console has no sign-in yet"*, which stopped being
true when OIDC landed. It is a different falsehood on the same banner and was left alone rather than
folded into this diff.
