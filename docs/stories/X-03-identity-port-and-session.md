---
id: X-03
title: "Bind the Identity port, with a dev identity and a session"
status: done
priority: 2
epic: serve
design: docs/designs/identity-and-session.md
note: "the tenant must come from the resolved principal and from NOTHING a caller controls — not a path segment, not a body field, not a header"
---

# Bind the Identity port, with a dev identity and a session

## Goal
Make `Identity` real: a request carries something, the host resolves it to a `Principal`, and every
downstream port is constructed from *that* principal's tenant.

## Acceptance
- [x] An `Identity` implementation for local development, explicitly opt-in and named as such at
      startup, plus a session a browser or an agent can carry.
- [x] **Failing-first test** — a request that supplies a tenant in a path segment, a body field or a
      header does **not** influence which tenant is used. This is the story's whole point; assert it
      three times, once per vector.
- [x] `IdentityError::Rejected` and `IdentityError::Unreachable` are distinguishable end to end. A
      caller that collapses them cannot tell "not signed in" from "the IdP is down", and neither can
      an operator.
- [x] A session cookie, if used, is `Secure` + `HttpOnly` + `SameSite`, with the attributes asserted
      in one test rather than spread across the code.

## Progress
- **Done.** Merged from `impl/X-03`; gate green (90 tests). Design: `docs/designs/identity-and-session.md`.
- **The load-bearing decision: `IdentityBinding::Development` is a third bind state, not `Bound`.** A
  dev identity resolves principals, so treating it as bound would have made `0.0.0.0` legal — but a
  roster handle is a credential with *no secret in it*, which is worse than an unauthenticated port,
  because everything downstream believes the principal. A review swept 20 addresses and found none
  admitted while it is armed, with all four loopback spellings still admitted.
- **A review broke round one twice.**
  - `HttpOnly` was a security control that only appeared to exist: `POST /api/session` accepted the
    *cookie* as credential material and returned a fresh bearer token in the body, so script never
    needed to read the cookie — it POSTed with the ambient cookie and read an equally powerful,
    never-expiring token out of the response. Fixed by an invariant rather than a special case: **a
    session token is returned only to a caller that presented a readable credential**, so this route
    can never turn an unreadable credential into a readable one. The test matches the 64-hex *shape*
    rather than a field named `token`, and also asserts nothing token-shaped is planted in a fresh
    `Set-Cookie`.
  - The seam enforcing the decision above had **zero** tests: mutating `state.rs`'s
    `Development => IdentityBinding::Development` to `Bound` left all 36 tests green while
    `FLUX_EXCHANGE_DEV_IDENTITY` + `0.0.0.0` would serve a credential-free identity on the network.
    Now pinned through the real path, plus a full constructor table so `Unbound` and `Bound` are
    covered too.
- No fourth tenant vector found: a probe combining query string, `Host`, `X-Forwarded-Host`, a
  repeated `X-Tenant`, a second cookie named `tenant`, form encoding, and nested *and* duplicate JSON
  `tenant` keys still resolved the principal's tenant.
- Sessions are bounded at 4096 and **refuse at the bound rather than evicting** — evicting would sign
  out a caller who did nothing wrong and could not tell that from a bug. Still no expiry and no
  cross-restart store; both are X-04's, and documented rather than implied.

## Notes
- The rule is already written on the `Identity` trait in `crates/exchange-host/src/lib.rs`; this
  story is where it becomes enforced rather than documented.
- A dev identity is a hole if it can be reached in production. Decide how it is armed, and prefer a
  refusal over a default.
