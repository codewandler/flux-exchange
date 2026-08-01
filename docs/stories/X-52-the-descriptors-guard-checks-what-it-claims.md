---
id: X-52
title: "The descriptor's guard checks what its name claims"
status: in-progress
priority: 1
epic: agent-onboarding
areas: [exchange-server, console]
note: "found by X-42's review, 2026-08-01: two published fields are pinned by nothing — the method, and which route a live capability points at. Demonstrated: republishing be-minted at /api/session keeps all 251 Rust tests green"
---

# The descriptor's guard checks what its name claims

## Goal
Every field `GET /api/onboarding` publishes is held by something, or is documented as unheld.

## What the review found

X-42 passed. Its central product — `a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`
and `every_published_route_is_a_capability_or_is_argued_not_to_be` — genuinely closes the round-1
defect in **both** directions, demonstrated by mutation. These are the two fields it does not reach,
and both were demonstrated rather than argued.

### 1. The test does not check the endpoint it is named for

`crates/exchange-server/src/routes/onboarding.rs:604-655` compares `capability.live` against *does
the `SERVED_BY` path exist*. It **never compares `SERVED_BY`'s path with the capability's own
`call.endpoint`**.

> Republishing `be-minted` at `/api/session` — another real route — keeps all 251 Rust tests green.

It was caught only by a hand-written console assertion in another story's file
(`console/test/onboarding.test.mjs:290`). All three live endpoints happen to be pinned that way, so
nothing is wrong today. **The test's name claims more than it checks**, which is the failure this
repository has now corrected in four separate stories.

### 2. `call.method` is pinned by nothing

> Changing `read-the-catalogue`'s `method: 'GET'` to `'DELETE'` in `console/src/onboarding.mts:196`
> and regenerating leaves the whole gate green — console 72 pass, Rust 251 pass.

Correct today (`catalogue/mod.rs:82` `get`, `agents.rs:154` `post`, `invoke.rs:85` `post`). The
obstacle is real and worth stating: `Route` carries a `fn() -> MethodRouter`, so **the method is not
statically readable** from the route table. That is why this is a story and not a line.

Two shapes to weigh, and neither is obviously right: have `Route` declare its method beside its
`method_router` (a field that could disagree with the router it sits next to), or drive each
published endpoint with its published method and assert the answer is not `405`. The second tests
the real thing and needs a request per capability.

### 3. A sentence that is not true of every deployment

`crates/exchange-server/src/routes/onboarding.json:52` ends the `authenticate` withholding with *"The
only principals a deployment resolves today are humans who signed in through its identity provider."*
A development-identity deployment also resolves `agent:` and `service:` roster handles
(`dev_identity.rs:45-49`, resolve at `:151-168`).

The operative claim — a minted agent token resolves nowhere — is true, and the error is conservative.
It is still a published sentence that is false on a real composition, on the document whose whole
argument is honesty.

## Acceptance
- [x] **Failing-first test** — republishing a capability at a route that exists but does not serve it
      is refused. Demonstrate with `be-minted` at `/api/session`, which is green today.
- [x] `call.method` is either held by something or the document says it is not. **If it cannot be
      held, say so where it is published**, rather than leaving a reader to assume every field is
      guarded because most are.
- [x] The `authenticate` sentence is true of every composition this repository ships, including the
      development identity.
- [x] No test's name claims more than the test checks — reread the two new ones against their names.

## Progress

**2026-08-01 — done, all four items.** Both defects reproduced by mutation at the merge base
(`1225dd2`) before anything was written, and both go red against the fix.

1. **The endpoint.** `a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`
   (`crates/exchange-server/src/routes/onboarding.rs:641`, the `match` at `:682`) now compares each capability's own
   `call.endpoint` against the `SERVED_BY` path as well as measuring liveness against it, over a
   total `match` so `(served, no call)` and `(unserved, has call)` panic rather than passing
   silently. Kept in that test rather than split out, because it is that test's *name* that
   over-claimed. Base: republishing `be-minted` at `/api/session` left all 253 Rust tests green;
   now it fails there, naming both spellings.

2. **The method.** New `every_published_call_reaches_a_handler_for_the_method_it_names`
   (`onboarding.rs:768`): drives every published `method endpoint` through the assembled app and
   asserts the answer is neither `405` nor `404`, plus a per-endpoint control (`PATCH`) that must
   be `405`. The second of the two shapes the story weighed — a `method` field on `Route` would
   have pinned the document to a declaration and left the declaration pinned to nothing. **Two
   things a reader has to know**, both written on `Call::method` and in the design: it proves the
   method reaches *a* handler and not that it is the only one the route serves, and it must drive a
   **resolved** caller — on a guarded route the `route_layer` runs before the method router, so an
   anonymous probe answers `401` for every method. The first spelling of this test was anonymous
   and would have passed for `DELETE /api/agents`; its own control is what caught that.

3. **The sentence.** `console/src/onboarding.mts` `authenticate.pending` no longer claims humans are
   the only principals a deployment resolves; the artifact is regenerated. The fact that made the
   old sentence false is already driven by
   `dev_identity::tests::a_handle_resolves_to_the_principal_the_roster_armed`, which resolves an
   `agent:` roster handle. **This one is not held by a test and the design says so** — a wrong
   argument inside a withholding is invisible to the gate, which is the risk the Notes name.

4. **Names.** Both X-42 tests reread. The first is now accurate rather than over-claiming, and the
   second (`every_published_route_is_a_capability_or_is_argued_not_to_be`) is strengthened for free:
   its "is a capability" now means "is the route a published capability names", not merely "is a
   path some `SERVED_BY` row mentions".

No existing test was weakened. Gate: `cargo build/test/clippy/fmt` green (254 server tests, up one),
console 72 pass and `npm run build` clean.

## Notes
- Do not weaken the existing tests to make room. They are the reason X-42 passed.
- The review's own summary of the risk is the thing to keep in mind: `SERVED_BY` and
  `NOT_A_CAPABILITY` are lists somebody maintains, mechanically checked in both directions so the
  maintenance is forced rather than remembered — **but a wrong *argument* in a `NOT_A_CAPABILITY`
  line is invisible to a test.**
