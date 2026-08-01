---
id: X-04
title: "OIDC sign-in"
status: in-progress
priority: 6
epic: serve
note: "signing in proves who the operator is; it mints no token for any vendor operation. Connecting a provider is a different flow with a different consent screen"
---

# OIDC sign-in

## Goal
A human can sign in with an OIDC provider, and the resulting principal names a tenant.

## Acceptance
- [ ] Authorization-code flow with PKCE, state and nonce validated.
- [ ] **Failing-first test** — a callback whose `state` does not match the one bound at `/signin` is
      refused with no session issued.
- [ ] The client secret comes from the environment and from nowhere else, and its `Debug` prints a
      redaction rather than the value.
- [ ] Missing configuration produces a **startup message naming the unset variables** and a service
      that serves an explanatory page — not a panic, and not a broken login that fails at the callback.

## Progress
- (not started — X-03 first)

## Notes
- **Signing in is not connecting.** Sign-in asks for `openid`/`email`/`profile` and mints nothing for
  a vendor. Provider connection is its own flow; do not conflate the consent screens.
- Design first: this story is non-trivial and has no design doc yet. Write one under
  `docs/designs/` (`/track:design`) before implementing. The cross-repo reasoning it builds on is
  flux's [`docs/designs/ecosystem.md`](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md).
