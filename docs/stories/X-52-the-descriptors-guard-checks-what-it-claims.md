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
- [ ] **Failing-first test** — republishing a capability at a route that exists but does not serve it
      is refused. Demonstrate with `be-minted` at `/api/session`, which is green today.
- [ ] `call.method` is either held by something or the document says it is not. **If it cannot be
      held, say so where it is published**, rather than leaving a reader to assume every field is
      guarded because most are.
- [ ] The `authenticate` sentence is true of every composition this repository ships, including the
      development identity.
- [ ] No test's name claims more than the test checks — reread the two new ones against their names.

## Notes
- Do not weaken the existing tests to make room. They are the reason X-42 passed.
- The review's own summary of the risk is the thing to keep in mind: `SERVED_BY` and
  `NOT_A_CAPABILITY` are lists somebody maintains, mechanically checked in both directions so the
  maintenance is forced rather than remembered — **but a wrong *argument* in a `NOT_A_CAPABILITY`
  line is invisible to a test.**
