---
id: X-16
title: "A session ends when the identity behind it does"
status: done
epic: serve
areas: [exchange-server]
note: "deferred twice — X-03 left it to X-04 on the grounds that an id token has an `exp` to bind to, and X-04 deferred it again because no composition could produce an id token. X-04 now can, so the reason is gone"
---

# A session ends when the identity behind it does

## Goal
A session issued by a completed sign-in stops resolving, rather than outliving the identity that
justified it for as long as the process runs.

## Why now

This has been deferred twice, each time for a reason that has since expired:

- **X-03** left it to X-04, on the grounds that an id token carries an `exp` worth binding to.
- **X-04** deferred it again, because `TokenExchange` was an unbound port — there was no id token in
  the build to bind to, and machinery built against claims no composition could produce would have
  been untested by construction. `docs/designs/oidc-signin.md` says exactly this.

X-04 now binds a real exchange and `SignedClaims::expires_at` is populated on every sign-in, so the
stated blocker is gone. Left alone, the position is worse than it was before: the host is now
*capable* of knowing when an identity expires and discards that knowledge, so a session minted from
a five-minute token is as durable as the process.

## Acceptance
- [x] **Failing-first test** — a session opened from claims whose `exp` has passed no longer
      resolves, asserted through `SessionStore::resolve` rather than through a clock the test owns.
- [x] A session opened from claims that are still valid **does** resolve, in the same test run, so
      the expiry cannot pass by breaking sessions for everyone.
- [x] Expiry is taken from the **id token's `exp`**, not from a fixed lifetime this host invents. A
      provider that issues a five-minute token must not yield an eight-hour session here.
- [x] A session that has expired is indistinguishable, to the caller, from one that was never
      opened — the same refusal, naming nothing about why.
- [x] The expired entry is not merely unresolvable but **removed**, so `SessionStore`'s bound is not
      consumed by sessions nobody can use. `a_full_store_refuses_rather_than_evicting` states the
      store's policy; expiry must not become a back door through it.

## Notes
- `session.rs` already carries the store, its bound and its refusal policy; `oidc/flow.rs`'s
  `PendingAuthorizations` already sweeps on a TTL and is the nearest precedent for the shape.
- Decide explicitly what happens when a provider issues an `exp` in the past or absurdly far in the
  future. Clamping silently is the kind of repair this repository refuses; refusing the sign-in and
  saying so is more likely right.
- `Oidc::admit` already refuses an expired token at sign-in. This story is about the session that
  outlives it afterwards, which is a different moment and a different check.

## Progress
- **Done 2026-08-01.** Gate green: 149 tests, clippy clean under `-D warnings`, fmt clean.
- The `exp` crosses verbatim — a five-minute token yields a five-minute session. This host invents
  no lifetime, because one it invented would outlive the credential it was shown.
- **An `exp` already past, or beyond thirty days, refuses the sign-in rather than being clamped.**
  Clamping would issue a session neither the provider nor this host described, and would leave the
  misconfigured provider in place for the operator never to find.
- **One wall clock, not two.** `now()` moved to `session.rs`: `admit` decides whether a token has
  expired and the store decides how long a session may live, and two clocks could admit a token and
  then refuse it a session.
- The dev identity keeps `WhileTheProcessLives` — a roster handle carries no secret and no expiry,
  so any lifetime there would be invented, and that port already forces a loopback bind.
- Proof was six weakened-implementation runs rather than a merge-base run, which could only have
  produced a compile error; the implementor said so plainly rather than dressing it up.
- A fixture that used `expires_at: i64::MAX` had to state a realistic five minutes — a token that
  never expires is not a thing a provider issues, so that is the fixture being corrected.
- **Reviewed PASS** by an independent pass that re-derived the pins with five of its own weakenings
  rather than trusting the report — including `from_secs` -> `from_millis`, which the suite caught.
  It verified indistinguishability **structurally** (one expression, no branch separating expired
  from unknown) rather than only through the rendered-form comparison the tests use.
- **The hot-path cost is now measured, not guessed.** Release build, isolated target dir: `resolve`
  costs **105µs** with 4096 live entries and **17ns** once swept, and it does not scale with cores
  because the mutex serialises it. Reaching 4096 takes 4096 completed sign-ins or loopback dev
  access, so it is not an unauthenticated surface. Capacity retention is not a problem.
- **One wall clock is one *function*, not one *reading*.** `complete` reads `now()` for `admit` and
  `open` reads it again, so a token whose `exp` falls between the two is admitted and then refused
  `AlreadyExpired` — sub-second, safe direction (no session issued), but the caller sees `NoSession`'s
  503 rather than the 401 `Expired` would give. Corrected in the doc rather than claimed away;
  closing it means threading one reading through `complete`, and it belongs with X-17's refusal
  collapse.
