---
id: X-15
title: "A sign-in a victim did not start cannot become a session in their browser"
status: ready
priority: 6
epic: serve
note: "found by X-04's implementor, 2026-08-01: server-side `state` does NOT close login-CSRF. An attacker who legitimately starts a sign-in here, authenticates as themselves, then walks a victim into the callback with that genuinely-bound state has the victim's browser holding the attacker's session"
---

# A sign-in a victim did not start cannot become a session in their browser

## Goal
A callback can only complete in the **same browser** that opened the sign-in it belongs to.

## The attack this closes

X-04 validates `state` server-side: the callback must present a `state` this host bound at
`/api/signin` and has not yet spent. That closes a *forged* state — a value the host never issued —
and there is a test proving it.

It does **not** close login-CSRF, because the attacker can obtain a genuinely-bound state honestly:

1. The attacker visits `/api/signin` here. The host binds a real `state`, and it is unspent.
2. The attacker authenticates at the IdP **as themselves**, and stops at the redirect rather than
   following it — they now hold a valid `code` plus the matching, still-unspent `state`.
3. The attacker walks a victim into that callback URL.
4. Every server-side check passes, because every value is genuine. The victim's browser is issued a
   session **as the attacker**.

The victim is now working inside the attacker's tenant. Anything they connect, paste or configure —
a credential, most obviously — lands in an account the attacker controls. That inverts this
repository's north star from the other end: the credential does not cross the boundary, the *victim*
does.

This is not a defect in X-04's implementation. Server-side `state` is doing exactly what it can do;
the missing half is that nothing ties the pending authorization to the browser that opened it.

## Acceptance
- [ ] **Failing-first test** — a callback carrying a genuinely bound, unspent `state`, presented by a
      browser that did **not** open that sign-in, is refused and **issues no session**. Assert no
      `Set-Cookie` and nothing token-shaped in the body, the way X-04's state-mismatch test does.
- [ ] The browser that opened the sign-in still completes it — asserted in the same test run, so the
      refusal cannot pass by breaking sign-in for everyone.
- [ ] Whatever binds the authorization to the browser is itself a **`__Host-` cookie** with
      `Secure` + `HttpOnly` + `SameSite`, attributes asserted in one test. It is a pre-session
      credential and gets a pre-session credential's protections.
- [ ] The binder is single-use and expires with the pending authorization, so a stale one cannot be
      replayed against a later sign-in.
- [ ] A callback with no binder at all is refused — an attacker who simply omits the cookie must not
      fall through to the X-04 path that only checks `state`.
- [ ] The refusal is distinguishable in logs from a forged-`state` refusal. An operator seeing these
      needs to know which one they are looking at; they mean different things about who is attacking.

## Progress
- (not started)

## Notes
- **Found by X-04's implementor and reported rather than quietly patched**, which is why it is a story
  and not a surprise. It sits outside X-04's Acceptance, which asks only that a *mismatched* state is
  refused.
- The conventional shape is a second cookie set at `/api/signin` holding a value (or its hash) that
  the callback must present alongside `state` — the OAuth 2.0 Security BCP's binding requirement.
  `nonce` does not substitute: it binds the **id token** to the authorization request, not the
  request to the browser, and X-04 already validates it.
- `SameSite=Strict` does not close this either. The victim follows a link, and X-04 already answers
  the callback with a meta-refresh page precisely because `Strict` withholds cookies on a
  cross-site-initiated redirect chain — so the attack arrives through a path `Strict` permits.
- Build on what X-04 already has: `entropy.rs` (one `/dev/urandom` path), `flow.rs`'s
  `PendingAuthorizations` with its single-use `take()` and TTL, and `session.rs`'s cookie shape.
  Do not add a second entropy source or a second cookie-building path.
- X-04 is `PARTIAL` — the token exchange is behind an unbound port for want of an HTTP client and a
  JOSE library. **This story does not need that half**: the binding is entirely between `/api/signin`
  and the callback, both of which exist and are tested.
