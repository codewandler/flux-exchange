---
id: X-61
title: "A second declaration at one path cannot hide from the anonymous enumeration"
status: ready
priority: 1
epic: invoke
areas: [exchange-server]
note: "found by X-54's review, 2026-08-01: setting the POST entry at a duplicated path to Access::Anonymous leaves the_anonymous_surface_is_only_what_was_declared_anonymous green — the guard that exists to make widening deliberate cannot see it"
---

# A second declaration at one path cannot hide from the anonymous enumeration

## Goal
The guard that makes widening the anonymous surface a deliberate, tested act sees every declaration.

## What was found

X-54 gated `POST /api/connections/{connector}` without reversing X-40's DELETE decision by declaring
the path **twice** in `connections::MODULE` — `get(show).delete(remove)` at `Access::Principal`, and
`post(create)` at `PrincipalOfKind`. axum merges the two method routers and each verb keeps its own
`route_layer`. The review confirmed that works, with a full verb matrix.

**It also demonstrated that the surface-wide guard is now blind to the second entry:**

> Setting `connections.rs:431`'s entry to `Access::Anonymous` and running
> `the_anonymous_surface_is_only_what_was_declared_anonymous` (`routes/mod.rs:464`) →
> `test result: ok. 1 passed`.

The guard probes `anonymous_get(probe_path(route.path))` at `mod.rs:529-531`. Both entries resolve to
the same **GET**, which is served by the `Principal` declaration — so the POST entry's declared access
is **unobservable** to the test whose entire job is to notice it.

## Why this is priority 1 despite not being exploitable

Nothing is wrong today. A module-local test catches it
(`every_route_here_requires_a_principal_and_the_kind_gated_ones_are_named`), and an anonymous POST in
that state `500`s on the missing `Extension<Principal>` rather than writing.

**The problem is that the surface-wide guard is the one that generalises and the local one is not.**
`the_anonymous_surface_is_only_what_was_declared_anonymous` exists so that widening the anonymous
surface of a credential-holding service cannot happen by accident, in any module. The next module to
copy X-54's duplicated-path pattern will not have a hand-written local backstop, and nothing will
tell its author that the general guard stopped covering them.

This repository has corrected the same class four times this week: **a guard whose reach is narrower
than its name.**

## Acceptance
- [ ] **Failing-first test** — an `Access::Anonymous` on the *second* declaration at a duplicated
      path is caught by the surface-wide guard. Demonstrate with the exact mutation above, green
      today.
- [ ] The guard probes each declaration with **a method that declaration actually serves**, or it
      states plainly that it probes paths and not declarations and names what covers the rest.
- [ ] `KIND_GATED`'s check is examined the same way — it compares a `Vec` built from `published()`,
      which does see both entries, but confirm that by mutation rather than by reading.

## Notes
- **Do not fix this by forbidding duplicated paths.** X-54's use of one is well argued and the
  alternatives were worse — gating the whole path reverses X-40 and closes the per-connector read, and
  a check inside the handler is invisible to the enumeration, which is precisely what `Access` exists
  to refuse.
- Also unrecorded and worth capturing while here: **the merged router's 405 fallback takes the second
  declaration's guard**, so reordering the two entries changes what an agent receives for `PATCH` and
  `OPTIONS` (403 in the current order, 405 if swapped). Not a hole in either order — an unresolvable
  caller still gets 401 and no handler is reached — but nothing pins the order and nothing documents
  that it matters.
