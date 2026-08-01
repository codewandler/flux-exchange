---
id: X-24
title: "A sign-in reads the clock once"
status: ready
priority: 3
epic: serve
areas: [exchange-server]
note: "found by X-16's reviewer, 2026-08-01: `complete` reads now() for `admit` and `open` reads it again, so a token whose exp falls between the two readings is admitted and then refused — the caller gets NoSession's 503 rather than the 401 Expired would give"
---

# A sign-in reads the clock once

## Goal
One sign-in decides against one moment in time.

## What is wrong

X-16 consolidated the wall clock to a single function, `session::now()`, on the argument that two
clocks could admit a token and then refuse it a session. **One function is not one reading**, and the
code does read it twice:

- `Oidc::complete` calls `admit(..., now())`, which decides whether the id token has expired.
- It then reaches `SessionStore::open`, whose `deadline()` calls `now()` again.

A token whose `exp` falls between the two readings is admitted by the first and refused
`AlreadyExpired` by the second. The window is sub-second and it fails in the **safe** direction — no
session is issued — so this is not a hole. What is wrong is the answer: the caller sees the `503`
that `NoSession` carries rather than the `401` that `SignInRefusal::Expired` would have given, and
the operator's log line says the store could not open a session when in fact the token had expired.

X-16's own doc now records this honestly rather than claiming otherwise. This story closes it.

## Acceptance
- [ ] **Failing-first test** — a token whose `exp` sits exactly on the boundary produces the
      `Expired` refusal and its status, not `NoSession`'s. Drive it with an injected instant rather
      than by racing a real clock; `Oidc::admit` already takes `now` as an argument for exactly this
      reason and is the precedent.
- [ ] One reading is threaded through `complete`, so no code path between admission and session
      opening can consult the clock a second time.
- [ ] `session.rs`'s doc comment loses the paragraph describing the window, because the window is
      gone — and does not regain an absolute claim it cannot support.
- [ ] Every existing session and sign-in test stays green, unmodified. This is a plumbing change and
      must not alter what any of them assert.

## Notes
- The obvious shape is for `SessionStore::open` to take the instant rather than read it. Check what
  else calls `open` — the development identity does, with `Expiry::WhileTheProcessLives` — and keep
  that path honest rather than making it invent a timestamp it has no use for.
- Do not widen this into a clock-injection abstraction. One argument threaded through one call path
  is the whole change; a `Clock` trait would be a larger claim than the defect justifies.
