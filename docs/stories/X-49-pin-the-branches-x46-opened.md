---
id: X-49
title: "The branches X-46 opened are pinned"
status: in-progress
epic: catalogue
areas: [console, exchange-server]
note: "found by X-46's review, 2026-08-01: publishing declarations changed how a connector that declares nothing renders, and nothing tests the branch it now takes"
---

# The branches X-46 opened are pinned

## Goal
The behaviour X-46 changed is held by a test, not by having been noticed once.

## The two findings

### 1. A newly-reachable render path has no test

Before X-46, a connector declaring nothing arrived at the console as `refused`. It now arrives as
`ready` with `credentials: []`, and renders through the `declares-nothing` branch in `Connect.mts`.

`grep -rn "declares-nothing" console/test console/src` returns **only the source line**. X-46's own
Notes call this out as a behaviour change, and nothing exercises the branch it now takes.
`freshdesk` is the one connector that reaches it, so the fixture is free.

### 2. A guard without the non-vacuity check its sibling has

`the_existing_catalogue_answers_gained_no_field` walks the catalogue and pins exact key sets, but
carries no `seen > 0` assertion. Its sibling twenty lines earlier has exactly that, for exactly this
reason. It is **non-vacuous in fact** — the reviewer's run walked all 53 connectors — so this is
inconsistency with the file's own discipline rather than a hole. It is also the cheapest possible fix
and the kind of thing that stops being true silently.

## Acceptance
- [x] **Failing-first test** — the `declares-nothing` branch is exercised, and fails before it is
      written (assert against a connector that declares nothing; delete the branch and watch it go
      red, or drive it and show the count change).
- [x] `the_existing_catalogue_answers_gained_no_field` gains a non-vacuity assertion matching its
      sibling's.
- [x] Nothing else changes. This is a coverage story, not a behaviour story — if either test forces a
      production change, that is a finding to report.

## Progress

**Both findings closed; no production file changed.** The diff is two test files.

1. `console/test/connect.test.mjs` gains
   `a_connector_that_declares_nothing_says_so_rather_than_rendering_an_empty_form`, with a
   `freshdesk` declaration fixture (`credentials: []`, `authority: "com.freshdesk.api"`). It drives
   `loadDeclaration` through the stub transport first — so the `ready`-not-`refused` half of X-46's
   change is pinned too, rather than being assumed by a hand-built state — and then renders the
   form. Proved by deletion: with the `declares-nothing` branch removed from `Connect.mts`,
   `credentialInputs` returns `<div class="connect__fields"></div>` and the test fails on the missing
   note. Restored, and the branch is untouched in the committed diff.
2. `the_existing_catalogue_answers_gained_no_field` counts the operation objects whose key set it
   compares and ends with `assert!(seen > 0, "an empty catalogue would vacuously pass")` — the same
   line and the same sentence as `every_connector_publishes_exactly_what_it_declares`. Shown capable
   of firing: with the walk's provider loop emptied it panics on that assertion, and the same
   emptied walk *passed* before the counter existed.

The counter sits on the innermost (operation) loop rather than on the listing loop above it: the
listing's own emptiness is already caught by `the_wire_shape_of_the_listing_is_the_agreed_contract`,
which looks `zendesk` up in it, and one counter is what the sibling has.

## Notes
- Also flagged by the same review and **deliberately not in scope**: the console's source-scan guard
  matches one literal spelling, so it catches the workaround that was removed rather than an
  equivalently-shaped reintroduction. The behavioural assertion beside it is what actually holds the
  property, and the reviewer said so — worth reading before adding a second regex.
- The wire shape being pinned in two places that cannot see each other (a Rust contract test and a
  console fixture) is **pre-existing and repo-wide** — the same gap already exists for
  `ZENDESK_CONNECTION`. Not this story's to solve, and not X-46's fault.
