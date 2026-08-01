---
id: X-26
title: "A sign-in refusal carries its own status"
status: done
epic: serve
areas: [exchange-server]
note: "found by X-24's implementor, 2026-08-01: the refusal-to-status table lives inline in routes::signin::callback and is unreachable from any other module, so a test that wants to state 'this refusal, and this status' has to be written at the route instead of beside the refusal"
---

# A sign-in refusal carries its own status

## Goal
A test can state "this refusal answers with this status" in one assertion, beside the refusal.

## What is wrong

`SignInRefusal` has, by now, ten-odd variants and a carefully argued mapping to HTTP statuses —
`Expired` is a `401` because the caller's credential was rejected, `NoSession` a `503` because this
host could not do its job, and the four back-channel refusals share one arm so the caller cannot tell
them apart. Every one of those decisions is deliberate and several were argued across X-15, X-17 and
X-24.

**That mapping lives inline in `routes::signin::callback` and is reachable from nowhere else.** The
consequence showed up in X-24: proving that an expired token answers `401` rather than `503` needed a
*second, route-level* test, because the first test — the one that actually knows which refusal was
produced — had no way to ask what status it carries.

So the argument and its proof are in different files, and a future change to the mapping breaks a
test that does not mention the refusal it broke.

## Acceptance
- [x] **Failing-first test** — one assertion states both the refusal and its status, from beside the
      refusal rather than through the router. It cannot compile or pass before the mapping is
      reachable, so say plainly which kind of proof you are giving.
- [x] Every existing status is **unchanged**, asserted exhaustively: a test that names each variant
      and its status, so a future edit to the mapping has to change a test that says what it changed.
- [x] The four back-channel refusals still share one status **and one caller-facing string**;
      `a_refusal_tells_the_caller_nothing_about_the_provider` stays green.
- [x] The route still decides how to *render* a refusal. Moving the status must not drag the page,
      the headers or the logging out of the route with it.

## Notes
- The obvious shape is `SignInRefusal::status()`, next to `caller_facing()`, which already lives on
  the type for exactly this reason. Check whether the argument comments move with it or stay at the
  route — they are the valuable part, and splitting them from the code they explain is the failure
  mode to avoid.
- X-24's implementor flagged this deliberately rather than doing it, because moving a
  carefully-argued match out of the route is a change with a real downside: the route is where a
  reader looks to see what a caller receives. If you conclude the current shape is right after all,
  **say so and stop** — a story that ends in a written-down "no" is a good outcome here.

## Progress
- **Done 2026-08-01.** Gate green: 43 + 176 tests, clippy clean, fmt clean. Compile-failure proof,
  which this story's Acceptance explicitly sanctions — the mapping was unreachable, so the assertion
  could not be written at all. Backed by a mutation proof that the exhaustive test bites: flipping
  one arm to `NOT_FOUND` failed it.
- **Dispatched with permission to conclude "no", and it argued the opposite — better than I had.**
  The split already existed: `caller_facing`'s doc says three separate times that a group of
  refusals shares "one phrase and one status", and the route's comments deferred to it, so the
  reasoning sat beside `caller_facing` while the code it justified sat at the route pointing back.
  Moving `status()` next to `caller_facing()` puts them together for the first time.
- **The downside I cited was already conceded.** `refused()` took the caller-facing *string* from the
  refusal, so a reader at the route could never see the phrase — having the status inline was half a
  picture, not a whole one. A pointer at the `Err(refusal)` arm names the two facts a reader most
  wants.
- **Beyond the story, flagged not hidden:** `refused()` lost its status parameter. Four call sites
  were each restating a value the refusal already implied, and
  `refused(&SignInRefusal::CodeRejected, StatusCode::UNAUTHORIZED)` is a place where a typo compiles.
  Statuses on the wire are identical.
- **Carried forward:** the exhaustive test guards the refusal→status edge, not the **error→refusal**
  edge. A new `ExchangeError` folded into an existing refusal would silently inherit its status and
  undo X-17's split without touching `status()`.
- **Filed as adjacent, not done:** `routes/connections.rs`'s `connection_refused` keeps exactly the
  shape removed here — an inline match producing a status at the route — so the repo now has two
  shapes for one job. Deliberately untouched: that match produces a status *and* a per-variant JSON
  body together, for a JSON API rather than an HTML page, so it is not obviously the same call.
