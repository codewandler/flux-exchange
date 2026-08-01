---
id: X-57
title: "\"Sign-in is available\" stops meaning \"OIDC is configured\""
status: in-progress
priority: 0
epic: local-identity
design: docs/designs/local-identity.md
areas: [exchange-server, console]
note: "the shared prerequisite for every local-identity story: SignIn::available() returns true only for SignIn::Oidc, so a host with a working development identity tells the console it cannot sign anyone in"
---

# "Sign-in is available" stops meaning "OIDC is configured"

## Goal
`available()` answers whether this deployment can turn a caller into a principal.

## The whole finding, in one function

```rust
// crates/exchange-server/src/state.rs:103
pub fn available(&self) -> bool {
    match self {
        SignIn::Oidc(_) => true,
        SignIn::Unconfigured | SignIn::NoTokenExchange => false,
    }
}
```

A deployment with `DevIdentity` armed **can** turn a caller into a principal — that is what
`IdentityBinding::Development` means — and this reports `false`. So the console hides its sign-in
affordance on a host that would let it in, and the only way through is presenting a roster handle as
a bearer token by hand.

**Nothing else in this epic is reachable until this is fixed**, which is why it is priority 0 and why
it is separate: it is a small change with a large blast radius across published surfaces, and it
deserves to fail or pass on its own.

## The part that is not small

`sign_in_available` is published **anonymously, in two places**: `GET /api/onboarding` (X-42) and
`GET /api/signin/availability` (X-43). X-42's Acceptance asserts that a deployment with no identity
provider says sign-in is unavailable **rather than pretending**, and
`the_descriptor_says_whether_this_deployment_can_sign_anyone_in` holds it.

So this change must keep "no provider" answering `false` while letting "a local provider" answer
`true` — without letting a stranger learn **which kind** of provider a deployment runs.
`SignIn::available` already collapses three states for precisely that reason, and that property is
not to be lost while widening the enum.

**Ask whether a boolean still suffices** rather than widening it by reflex. If the console needs to
know *how* to sign somebody in — a redirect versus a form — then the answer a stranger gets and the
answer the console needs may not be the same value, and conflating them is how this function got
wrong in the first place.

## Acceptance
- [x] **Failing-first test** — a host with the development identity armed reports that a caller can
      sign in, and the console renders the affordance. It must fail before the change.
      → `routes::signin::tests::a_host_with_the_development_identity_reports_that_a_caller_can_sign_in`
      drives both anonymous surfaces; `state::tests::available_says_whether_a_caller_can_become_a_principal`
      pins the meaning. **Partial on its second clause, and the reason is a finding** — see Progress.
- [x] A host with **no** identity provider still reports `false`. X-42's and X-43's existing
      assertions keep passing **unmodified** — if one has to change, that is a finding, not a chore.
      → X-42's `the_descriptor_says_whether_this_deployment_can_sign_anyone_in` and
      `routes::tests::the_surface_serves_an_agent_descriptor_anonymously` are untouched and green.
      **One X-43 assertion had to change** — see Progress.
- [x] **Failing-first test** — nothing published anonymously reveals which kind of provider is
      configured. Drive two deployments with different providers and assert the anonymous answer is
      identical.
      → `routes::signin::tests::no_anonymous_surface_says_which_kind_of_provider_signs_people_in`
      compares a federated host and a locally-provisioned one byte for byte on both surfaces.
- [x] The descriptor (`GET /api/onboarding`) stays accurate. It derives from
      `console/src/onboarding.mts`, so a change here that does not reach it is a drift the route-table
      tests may not catch.
      → `routes::onboarding::tests::a_host_with_the_development_identity_says_a_caller_can_sign_in`
      asserts the served document is the derived artifact plus that one field, and nothing more.
      `console/test/descriptor.test.mjs` is green: the derivation did not move, so `onboarding.mts`
      needed no change.
- [x] Whatever `available()` now means is written on it, replacing the comment that describes the old
      meaning. → `crates/exchange-server/src/state.rs`, `SignIn::available`.

## Notes
- `DevIdentity` is **loopback-only** and stays that way. This story does not make it reachable; it
  makes the console able to use it where it is already legitimately armed.
- Read `docs/designs/local-identity.md` § 1 before choosing a shape.

## Free while you are in there — two sentences that stopped being true when OIDC landed

Found by X-53 and deliberately not folded into its diff:

- `console/src/service.mts:1070` — the `CONSOLE-NO-PRINCIPAL` banner still says *"this console has
  no sign-in yet"*.
- `console/src/service.mts:125` — the `ServedOperation` doc says *"There is no sign-in yet"*.

Both are about sign-in, both are false, and both are in the surface this story is changing the
meaning of. Fix them here rather than leaving a sixth and seventh rendering of a stale claim — this
epic has now corrected the same class of falsehood in five places across X-42, X-52 and X-53.

Both corrected. Neither swapped one cause for another: `admitted` is `null` **whoever is reading**,
because nothing in this build decides what a principal may run. That is X-13, and it is the event
these two sentences should key on rather than sign-in.

## Progress

`SignIn::Development` is the new variant — a unit variant, because `/api/signin` has nothing to do
with a port on this composition and `AppState::development_identity` already hands the concrete one
to `POST /api/session`. `with_development_identity` now sets the identity port and the sign-in state
together, for `with_oidc`'s reason. `available()` is `true` for `Oidc | Development`.

**The shape decision, which the story asked for rather than assumed: the boolean still suffices.**
Publishing *how* to sign in — redirect versus form — anonymously would be publishing which kind of
provider a deployment runs, which is the property `available` collapsed states to protect. And it is
not needed: the affordance is a link to `/api/signin`, and `/api/signin` is where "how" is answered,
one request later, to a caller who came to sign in. That route was already the operator's channel —
it tells an anonymous caller which of two misconfigurations a host has. So `/api/signin` gained a
third answer for `SignIn::Development`: a `200` with a page saying how, **not** a `503`, because a
`503` would be the console's affordance leading to a page denying what the field had just asserted.
The whole argument is in `docs/designs/local-identity.md` §1 and on `SignIn::available` itself.

The callback answers `SignIn::Development` with `SignInRefusal::UnknownState` — this host planted no
`state` and never will, and that is the same `400` and the same phrase a forged `state` gets on a
federated host, so the route stays an oracle for nothing.

### Two findings

1. **The console never consumed `sign_in_available`.** X-43 published it so the console would stop
   rendering a *Sign in* link into a `503`, and nothing under `console/src` reads it —
   `ConsoleShell.mts:140,149`, `App.vue:252` and `Agents.mts:397` all render the anchor
   unconditionally. So the console never *hid* the affordance; it always showed it, and on a
   development host it led to "This host has no identity provider configured". This story fixed where
   it leads. **Gating the affordance on the field is still unbuilt** and belongs with X-58's form.
2. **One X-43 assertion had to change, and it encoded the same conflation one layer up.**
   `asking_whether_sign_in_is_available_does_not_change_what_signin_answers` asserted that an
   available composition answers `/api/signin` with a `303` — *available means a redirect*. Its
   available arm is now split: a redirect when a provider is bound, a page when the identity is
   local. The claim worth keeping survives both — the caller is not told this host can sign nobody
   in. X-43's other two assertions are untouched; `every_composition()` gained a fourth row and its
   "there is no fourth" comment was corrected.

### What a resuming agent would look at next

`AGENTS.md`'s status section still carries the warning this story removes the cause of ("`SignIn::available()`
reports `false` for it, so the console hides its sign-in affordance"). It was left alone deliberately —
it reads like a ledger the integrator owns, and it is now wrong in two ways rather than one, since the
console does not hide anything.
