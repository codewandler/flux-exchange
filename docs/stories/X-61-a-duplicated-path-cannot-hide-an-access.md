---
id: X-61
title: "A second declaration at one path cannot hide from the anonymous enumeration"
status: in-progress
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
- [x] **Failing-first test** — an `Access::Anonymous` on the *second* declaration at a duplicated
      path is caught by the surface-wide guard. Demonstrate with the exact mutation above, green
      today.
- [x] The guard probes each declaration with **a method that declaration actually serves**, or it
      states plainly that it probes paths and not declarations and names what covers the rest.
- [x] `KIND_GATED`'s check is examined the same way — it compares a `Vec` built from `published()`,
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

## Progress

**Done, in `crates/exchange-server/src/routes/mod.rs` only.** No production code changed — the
declared surface is byte-identical, and the whole diff is in `mod tests`. The guard was the thing
that was wrong, not the routes.

### What changed

The probe stopped being a path probe and became a declaration probe.

- `methods_served(route)` asks **one declaration's own method router** which verbs it answers, by
  sending it `TRACE` (the `UNSERVED` sentinel) and reading the `Allow` header off the `405`. No
  handler runs during discovery — one request per declaration, refused before dispatch. It is
  deliberately the *unguarded* method router: `route_layer` wraps the `405` fallback too, so a
  guarded route answers `401` before the fallback could name anything.
- `anonymously_reachable(modules, assemble)` walks each declaration, drives every method that
  declaration serves through the **assembled** app, and reports the declaration if any answer is
  not `401`. Driving the assembled app rather than the isolated route is deliberate: what is
  measured is what the merged router really hands a caller.
- `assembled(modules)` is `app` for an arbitrary module set, so a spy module goes through a real
  merge — the only place a duplicated path behaves as it does in production.
- `anonymous_get` is now `anonymous_request`'s `GET` case; every other caller is unchanged.

`ANONYMOUS` is unchanged: nine entries, same order. The surface did not move, only what can see it.

### Failing-first

`a_second_declaration_at_one_path_cannot_hide_from_the_enumeration` — one spy path declared twice,
`get` at `Access::Principal` and `post` at `Access::Anonymous`, walked by the same
`anonymously_reachable` the real surface is. Committed red first (`f2d7793`) with the probe still
`GET`-only, where it reported `left: []`.

And the story's own mutation, both ways round:

| tree | `Access::Anonymous` on `connections.rs:431` | `the_anonymous_surface_…` |
| --- | --- | --- |
| merge base `195a6bc` | applied | `test result: ok. 1 passed` — blind |
| this branch | applied | FAILED, naming `("connections", "/api/connections/{connector}")` |

### `KIND_GATED`, examined by mutation and not by reading

Both directions were driven against the pristine base and both turned
`the_kind_gated_surface_is_only_what_was_declared` red, so it genuinely sees both declarations:

- `POST` entry → `Access::Anonymous` (the mutation the anonymous guard could not see): its
  `("connections", "/api/connections/{connector}", [User])` line disappears from the built `Vec`.
- `GET`/`DELETE` entry at the same path → `Access::PrincipalOfKind(MAY_CONFIGURE)`: a line appears.

Worth knowing before reading a failure there, and now written on the test: two declarations at one
path produce **byte-identical** `(module, path, kinds)` tuples, so the failure message tells you the
count is wrong without telling you which declaration did it. The assertion still fails — a `Vec`
keeps count and order — but the message has to be read next to the table.

### What is still not covered, stated rather than left to be found

A method **no** declaration serves. On a duplicated path the merged router's `405` fallback belongs
to whichever declaration merged second, which is the note above — confirmed by reading
`axum-0.8.9`'s `Fallback::merge`, which picks `other` when both sides are defaults. Still not a hole
in either order, and still unpinned; this story records it on the guard's doc comment and does not
fix it.
