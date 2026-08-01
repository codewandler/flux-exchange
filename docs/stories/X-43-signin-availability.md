---
id: X-43
title: "Whether a human can sign in here is a fact the console reads, not prose it parses"
status: ready
priority: 1
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
- [ ] **Failing-first test** — a caller can determine, from a **field rather than prose**, whether
      sign-in is available on this deployment. It must fail before the field exists.
- [ ] Correct in all three compositions this host already has, asserted for each: OIDC bound, OIDC
      configured but the token exchange unbuildable, and nothing configured at all. `state.rs`'s
      `SignIn` enum already models exactly these three — do not invent a fourth vocabulary.
- [ ] **It discloses nothing about the configuration.** Not the issuer, not the client id, not an
      endpoint, not which variables are unset. Whether sign-in works is a fact about the *service*;
      what it is configured with is not. Assert this adversarially — the startup log names unset
      variables deliberately, and a caller must not learn them.
- [ ] Reachable **without a session**, since the whole point is to be read by someone who has none.
      Widening the anonymous surface is a deliberate act — `the_anonymous_surface_is_only_what_was_declared_anonymous`
      is the guard, and the new route must appear in it.
- [ ] `/api/signin`'s existing behaviour is unchanged: still `303` when it can, still an explanatory
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
