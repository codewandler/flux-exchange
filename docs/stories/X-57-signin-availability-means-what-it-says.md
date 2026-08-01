---
id: X-57
title: "\"Sign-in is available\" stops meaning \"OIDC is configured\""
status: ready
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
- [ ] **Failing-first test** — a host with the development identity armed reports that a caller can
      sign in, and the console renders the affordance. It must fail before the change.
- [ ] A host with **no** identity provider still reports `false`. X-42's and X-43's existing
      assertions keep passing **unmodified** — if one has to change, that is a finding, not a chore.
- [ ] **Failing-first test** — nothing published anonymously reveals which kind of provider is
      configured. Drive two deployments with different providers and assert the anonymous answer is
      identical.
- [ ] The descriptor (`GET /api/onboarding`) stays accurate. It derives from
      `console/src/onboarding.mts`, so a change here that does not reach it is a drift the route-table
      tests may not catch.
- [ ] Whatever `available()` now means is written on it, replacing the comment that describes the old
      meaning.

## Notes
- `DevIdentity` is **loopback-only** and stays that way. This story does not make it reachable; it
  makes the console able to use it where it is already legitimately armed.
- Read `docs/designs/local-identity.md` § 1 before choosing a shape.
