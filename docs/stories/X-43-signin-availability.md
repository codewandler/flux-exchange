---
id: X-43
title: "Whether a human can sign in here is a fact the console reads, not prose it parses"
status: done
epic: serve
areas: [exchange-server]
note: "found by X-34's implementor, 2026-08-01: the console links to /api/signin unconditionally, so on a host with no identity provider the Sign in button leads to a 503 page. The 401 body distinguishes the cases in prose, and matching on prose is fragile"
---

# Whether a human can sign in here is a fact the console reads, not prose it parses

## Goal
A caller can learn whether this deployment can sign anyone in, without clicking a button to find out.

## What is wrong

The console's header renders a **Sign in** link unconditionally. On a deployment with no identity
provider configured — which is a supported, tested state that still serves `/health`, the catalogue
and an explanatory page — that button leads to a `503`. The operator learns the platform cannot sign
them in by being refused.

X-34's implementor found this and did not paper over it:

> Arguably the header should be able to say "this host cannot sign anyone in" **before** the click —
> the `401` body distinguishes "no provider configured" from "none presented" — but matching on error
> prose is fragile and it needs a contract change to do properly.

That is right. The distinction exists today only in a human-readable sentence, and a console that
branches on the wording of a refusal is a console that breaks when someone improves the wording.

## Acceptance
- [x] **Failing-first test** — a caller can determine, from a **field rather than prose**, whether
      sign-in is available on this deployment. It must fail before the field exists.
- [x] Correct in all three compositions this host already has, asserted for each: OIDC bound, OIDC
      configured but the token exchange unbuildable, and nothing configured at all. `state.rs`'s
      `SignIn` enum already models exactly these three — do not invent a fourth vocabulary.
- [x] **It discloses nothing about the configuration.** Not the issuer, not the client id, not an
      endpoint, not which variables are unset. Whether sign-in works is a fact about the *service*;
      what it is configured with is not. Assert this adversarially — the startup log names unset
      variables deliberately, and a caller must not learn them.
- [x] Reachable **without a session**, since the whole point is to be read by someone who has none.
      Widening the anonymous surface is a deliberate act — `the_anonymous_surface_is_only_what_was_declared_anonymous`
      is the guard, and the new route must appear in it.
- [x] `/api/signin`'s existing behaviour is unchanged: still `303` when it can, still an explanatory
      page when it cannot. This story adds a way to ask beforehand; it does not alter the answer.

## Notes
- The obvious home is the existing session endpoint — a caller asking "who am I" and "can I sign in"
  in one round trip is the shape the console actually wants, and it avoids a second anonymous route.
  Weigh that against `/api/session`'s current contract and say which you chose and why.
- This is a **disclosure decision** on a credential-holding service, like X-42. The field is a
  boolean-shaped fact about capability. Resist adding "and here is the issuer" — that is exactly the
  helpfulness this repository refuses elsewhere.
- The console half is not this story. A field nothing reads is still an improvement over prose
  nothing can safely read.

## Progress
- **Done 2026-08-01.** Gate green: 44 + 216, clippy clean, fmt clean. Runtime merge-base failure —
  the tests were written and run with the worktree still at the base, before any non-test file moved.
- **The story suggested `/api/session` to save a round trip. The implementor refused, with a better
  argument than the suggestion.** `Access` is per **route**, not per method, so `/api/session`'s
  single `Access::Principal` declaration covers `GET` whoami, `POST` — **which mints a session** —
  and `DELETE`. Letting an unauthenticated caller read a field there means making that route
  `Anonymous`, which **unguards the minting route** and pushes its guard out of the enumerable route
  table into handler bodies. `routes/mod.rs` says the surface is enumerable precisely because *"a
  route is not guarded by its handler remembering to ask"*. Trading a structural guard on a
  session-minting route for one saved round trip is the wrong trade here.
- **A boolean, not the three states — and the collapse is asserted byte for byte, status included.**
  A three-valued field would tell an anonymous caller whether this host's eight `FLUX_EXCHANGE_OIDC_*`
  variables are set, which is a piece of the deployment's shape. The three-way distinction stays
  where it is useful and safe: the explanatory pages a human meets at `/api/signin`, unchanged.
- **The disclosure assertions were mutation-checked**, not merely written: adding a `reason` field
  trips the one-key clause, and returning a different status for `Unconfigured` trips the
  byte-identical clause.
- `SignIn::available` is an exhaustive match with **no wildcard arm**, so a fourth variant is a
  compile error at the decision rather than a silent `false`.
- **Carried forward:** the console is the consumer and does not read this yet. The contract is
  exactly `200` with `{"sign_in_available": true|false}`, one key.
- **Filed as adjacent:** `require_principal`'s `401` body still distinguishes "no identity provider
  configured" from "none presented" **in prose** — the same hazard X-34 named, one layer down. This
  route makes it avoidable; it does not remove it.
