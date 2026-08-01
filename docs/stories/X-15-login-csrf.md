---
id: X-15
title: "A sign-in a victim did not start cannot become a session in their browser"
status: done
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
- [x] **Failing-first test** — a callback carrying a genuinely bound, unspent `state`, presented by a
      browser that did **not** open that sign-in, is refused and **issues no session**. Assert no
      `Set-Cookie` and nothing token-shaped in the body, the way X-04's state-mismatch test does.
- [x] The browser that opened the sign-in still completes it — asserted in the same test run, so the
      refusal cannot pass by breaking sign-in for everyone.
- [x] Whatever binds the authorization to the browser is itself a **`__Host-` cookie** with
      `Secure` + `HttpOnly` + `SameSite`, attributes asserted in one test. It is a pre-session
      credential and gets a pre-session credential's protections.
- [x] The binder is single-use and expires with the pending authorization, so a stale one cannot be
      replayed against a later sign-in.
- [x] A callback with no binder at all is refused — an attacker who simply omits the cookie must not
      fall through to the X-04 path that only checks `state`.
- [x] The refusal is distinguishable in logs from a forged-`state` refusal. An operator seeing these
      needs to know which one they are looking at; they mean different things about who is attacking.

## Progress
- Implemented in worktree `.claude/worktrees/agent-a1baed31de4a125b2` on branch `impl/X-15`.
  A session crash interrupted the run; the work itself survived and is **complete and green**.
- `cd97eb2` holds the failing-first test. The implementation that answers it is **still uncommitted**
  in that worktree — `oidc/flow.rs`, `oidc/mod.rs`, `routes/signin.rs`, `session.rs`.
- Recovery, 2026-08-01: the crash left a truncated `flux_exchange` link artifact in `target/`
  (`Exec format error`, `file` reported `data` not ELF) which failed the run before any test
  executed. Removed it and rebuilt; unrelated to the source. `cargo fmt` had also not yet run —
  applied.
- Gate green in the worktree: `cargo test --workspace` 137 passed / 0 failed,
  `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --all --check` clean.
- All six Acceptance items have tests:
  - walked-in callback refused, no session — `signin::tests::a_callback_from_a_browser_that_did_not_open_the_signin_issues_no_session`,
    which drives **both** victim shapes (holding nothing, and holding its own binder) and completes
    the real browser's sign-in in the same run so the refusal cannot pass by breaking sign-in.
  - `__Host-` cookie attributes — `signin::tests::the_binder_is_a_host_cookie_and_never_leaves_in_the_url`
  - single-use + expiry — `flow::tests::a_binder_is_spent_with_its_authorization_and_expires_with_it`
  - no binder at all — `signin::tests::a_callback_carrying_no_binder_is_refused_and_spends_nothing`
  - distinguishable in logs — `oidc::tests::a_walked_in_callback_reads_differently_in_the_log_from_a_forged_state`
- Design note worth keeping: `PendingAuthorizations::claim(state, binder)` deliberately replaces
  `take(state)` rather than sitting beside it, so no code path can complete a sign-in without asking
  which browser opened it. A binder mismatch leaves the authorization **unspent**, so a hostile
  callback cannot cancel a victim's in-flight sign-in.
- The binder is `SameSite=Lax` where the session cookie is `Strict` — deliberate, and documented at
  its definition in `flow.rs`: a `Strict` binder would never arrive on the provider's redirect back.
- **Next step: commit the worktree changes on `impl/X-15`, then merge to main** (CHANGELOG, board,
  `status: done`). Left uncommitted pending the user's instruction.

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
