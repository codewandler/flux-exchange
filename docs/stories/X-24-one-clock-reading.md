---
id: X-24
title: "A sign-in reads the clock once"
status: done
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
- [x] **Failing-first test** — a token whose `exp` sits exactly on the boundary produces the
      `Expired` refusal and its status, not `NoSession`'s. Drive it with an injected instant rather
      than by racing a real clock; `Oidc::admit` already takes `now` as an argument for exactly this
      reason and is the precedent.
- [x] One reading is threaded through `complete`, so no code path between admission and session
      opening can consult the clock a second time.
- [x] `session.rs`'s doc comment loses the paragraph describing the window, because the window is
      gone — and does not regain an absolute claim it cannot support.
- [x] Every existing session and sign-in test stays green, and **none of them changes what it
      asserts**. *(Corrected at integration: this item originally said "unmodified", which was
      over-specified and not satisfiable alongside the Note below — see Progress.)*

## Notes
- The obvious shape is for `SessionStore::open` to take the instant rather than read it. Check what
  else calls `open` — the development identity does, with `Expiry::WhileTheProcessLives` — and keep
  that path honest rather than making it invent a timestamp it has no use for.
- Do not widen this into a clock-injection abstraction. One argument threaded through one call path
  is the whole change; a `Clock` trait would be a larger claim than the defect justifies.

## Progress
- **Done 2026-08-01.** Gate green: 39 + 163 tests, clippy clean, fmt clean.
- **Acceptance item 4 was wrong as I wrote it, and the implementor said so rather than working
  around it.** "Unmodified" is not satisfiable alongside this story's own Note, which asks that
  `open` *take* the instant and that `WhileTheProcessLives` not be made to invent one. The only
  shape satisfying both puts the reading on the arm that uses it —
  `Expiry::Credential { expires_at, as_of }` — and a struct-variant literal must then gain the field
  at every construction. Six sites, mechanical; **zero assertions changed, no test renamed or
  deleted**, verified by diff before merging. The item is corrected above to say what it meant.
- The rejected alternative was a second entry point (`open` + `open_as_of`), which touches no test
  and is worse: it leaves the `now()` read alive inside `deadline()`, so "the window is gone" would
  be true only of the path `complete` happens to take.
- **The reading stays after the token exchange, not at the top of `complete`.** Moving it up reads
  plainer and fails **open**: the deadline is `Instant::now() + (exp - now)`, so a reading taken
  before a slow token endpoint would let the session outlive the token by the round-trip.
- `deadline()` is now a pure function and reads no clock; `oidc/mod.rs:161` is the sole production
  read of `session::now()` in the binary.
- **The replacement doc bounds itself** rather than claiming an absolute, which is the third time
  this run a doc has had to: it says the property holds of *this call path* and "is not a property
  of the type system. A future caller of `SessionStore::open` that read this for itself would be
  taking its own reading again."
- **Carried forward:** `as_of` is caller-supplied, so a caller passing a stale or far-future value
  silently moves the deadline. Only `complete` passes one today, and only from `now()`.
- **Filed as adjacent, not done here:** the refusal-to-status table lives inline in
  `routes::signin::callback` and is unreachable from other modules, which is why the status needed a
  second route-level test rather than one assertion beside the first. A `SignInRefusal::status()`
  would fix that but moves a carefully-argued match out of the route — its own story.
