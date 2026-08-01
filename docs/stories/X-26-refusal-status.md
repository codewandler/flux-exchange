---
id: X-26
title: "A sign-in refusal carries its own status"
status: ready
priority: 3
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
- [ ] **Failing-first test** — one assertion states both the refusal and its status, from beside the
      refusal rather than through the router. It cannot compile or pass before the mapping is
      reachable, so say plainly which kind of proof you are giving.
- [ ] Every existing status is **unchanged**, asserted exhaustively: a test that names each variant
      and its status, so a future edit to the mapping has to change a test that says what it changed.
- [ ] The four back-channel refusals still share one status **and one caller-facing string**;
      `a_refusal_tells_the_caller_nothing_about_the_provider` stays green.
- [ ] The route still decides how to *render* a refusal. Moving the status must not drag the page,
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
