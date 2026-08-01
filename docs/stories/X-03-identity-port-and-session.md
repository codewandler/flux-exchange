---
id: X-03
title: "Bind the Identity port, with a dev identity and a session"
status: ready
priority: 2
epic: serve
note: "the tenant must come from the resolved principal and from NOTHING a caller controls — not a path segment, not a body field, not a header"
---

# Bind the Identity port, with a dev identity and a session

## Goal
Make `Identity` real: a request carries something, the host resolves it to a `Principal`, and every
downstream port is constructed from *that* principal's tenant.

## Acceptance
- [ ] An `Identity` implementation for local development, explicitly opt-in and named as such at
      startup, plus a session a browser or an agent can carry.
- [ ] **Failing-first test** — a request that supplies a tenant in a path segment, a body field or a
      header does **not** influence which tenant is used. This is the story's whole point; assert it
      three times, once per vector.
- [ ] `IdentityError::Rejected` and `IdentityError::Unreachable` are distinguishable end to end. A
      caller that collapses them cannot tell "not signed in" from "the IdP is down", and neither can
      an operator.
- [ ] A session cookie, if used, is `Secure` + `HttpOnly` + `SameSite`, with the attributes asserted
      in one test rather than spread across the code.

## Progress
- (not started)

## Notes
- The rule is already written on the `Identity` trait in `crates/exchange-host/src/lib.rs`; this
  story is where it becomes enforced rather than documented.
- A dev identity is a hole if it can be reached in production. Decide how it is armed, and prefer a
  refusal over a default.
