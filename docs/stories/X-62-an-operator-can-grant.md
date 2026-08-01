---
id: X-62
title: "An operator can grant something without editing a file by hand"
status: in-progress
priority: 0
epic: agent-access
areas: [exchange-server, console]
note: "X-13 landed the grant gate fail-closed and no surface edits a grant, so a deployment now runs nothing until somebody hand-writes FLUX_EXCHANGE_GRANTS. Priority 0 alongside X-57: together they are what stands between this platform and being usable"
---

# An operator can grant something without editing a file by hand

## Goal
An operator can see what their tenant is allowed to run, and change it, from the console.

## Why this is priority 0

X-13 closed a real exposure — before it, any principal this host resolved could run any operation in
the catalogue against its tenant's connections, and `GET /api/onboarding` published that fact to
strangers. It closed it **fail-closed**, which is the right posture.

**The cost is that a deployment now runs nothing at all until somebody hand-writes a grants file**,
and there is no route, no screen and no command that writes one. Invoke is reachable, correct, gated —
and unusable through the product.

Paired with [[X-57]], this is the whole of what stands between this platform and somebody actually
using it: X-57 is *you cannot sign in*, and this is *once you have signed in, nothing will run*.

## What it must not become

**Not a list of operation ids.** X-13's own Goal is *"decided from the operation's declared metadata,
not from a list of names"*, and that is not a style preference — the same reasoning is why X-47's
host rule reads the catalogue instead of a hand-written list, and why it caught four vulnerable
connectors where a list would have caught two. A UI that writes ids back into a grant would undo the
property the gate was built around.

So the surface has to express a **selector** — this connector, at most this risk, these effects — and
show the operator *which operations that currently admits*, from the same projection the gate decides
on (`OperationFacts::of`). That preview is most of the value: a grant nobody can evaluate before
saving is a grant somebody sets too wide.

**Not silently self-granting.** Whoever may edit a grant decides what the tenant can run, which is
strictly more authority than supplying a credential. That is the same kind-shaped question X-54
answered for credentials, and the answer here should not be weaker.

## Acceptance
- [x] A route reads the grants a tenant holds, and a route edits them. Both declare an access, and
      the write is at least as narrow as [[X-54]]'s `MAY_SUPPLY_A_CREDENTIAL`.
- [x] **Failing-first test** — a grant written through the surface admits exactly what the gate
      admits. Assert it against `admit_grant`, not against a copy of its rules.
- [x] The surface expresses selectors, never operation ids. **Failing-first test** — a request naming
      an operation id is refused.
- [ ] The console shows which operations a proposed grant would admit, derived from the same facts
      the gate uses, before it is saved.
- [x] **Nothing tenant-specific leaks anonymously.** What a tenant is granted is tenant data; the
      catalogue and the descriptor must not learn it.
- [x] A deployment with no grants file bound still serves everything else, and says which setting is
      missing — X-13 already answers `503` naming both stores; keep that true.

## Notes
- The descriptor's `warn` for `invoke` currently ends *"there is no route that edits one, so a tenant
  nobody has granted anything runs nothing at all: ask whoever operates this deployment."* **That
  sentence is this story's Acceptance in prose** — when this lands, it changes.
- X-13's grants are per **tenant**, not per principal, and `grant.rs`'s module doc states the
  narrowing this build does not make. Do not quietly add per-principal scope here; if it is wanted,
  it is its own story with its own argument.
- The grant file is created `0600` and an existing file's mode is **not** verified — X-13 states this
  rather than implying it. Somebody who can write that file decides what this host runs. If this
  story adds a second writer, the mode question gets sharper.

## Progress

**The service half is done; the console screen is not.** Five of six Acceptance items are satisfied
and committed on `impl/X-62`.

What landed:

- `crates/exchange-server/src/routes/grants.rs` — `GET`/`PUT /api/grants` (one path, one `Route`,
  one declaration: X-61's duplicated-path blindness is avoided rather than inherited) and
  `POST /api/grants/preview`. All three are `Access::PrincipalOfKind(MAY_GRANT)`, and `MAY_GRANT`
  is `&[PrincipalKind::User]` — the same kinds as X-54's `MAY_SUPPLY_A_CREDENTIAL`, pinned to that
  constant by the enumeration test rather than written out twice.
- **The read is gated too**, which was not obvious going in and is the one decision worth arguing
  with. `admit_grant` deliberately withholds a tenant's policy from a refused caller so an agent
  cannot enumerate it one call at a time; a read open to every kind would hand it over in one
  request. Cost, stated: an agent cannot discover in advance what it may run.
- `exchange_host::Invoker::grants()` — a new public accessor, which is how the surface reaches the
  store without a second port on `AppState` (fenced this wave, and the wrong shape anyway):
  `Grants`' own documentation says two stores that disagree is the failure to avoid, and one `Arc`
  makes that structural.
- The wire body expresses three axes and nothing else. `Selector`'s `allow_ids`/`deny_ids` stay for
  the hand-edited file: a stored grant carrying one is **shown** by the read, marked
  `expressible: false`, and a `PUT` that would replace it is refused `409` rather than dropping it.
- The descriptor's `invoke` warn no longer says nothing edits a grant; artifact regenerated.

**What is not done, and why.** The console screen. It needs a loader in `console/src/service.mts` —
the only module in this console that knows a network exists — which is another agent's file this
wave. Nothing was worked around: the *derivation* the Acceptance asks for is served by
`POST /api/grants/preview`, from `OperationFacts::of` through `ConnectorSurface::admitted`, so the
screen has an authoritative answer to render and no rule of its own to reimplement. The next agent
adds `loadGrants`/`replaceGrants`/`previewGrant` to `service.mts`, a `grants` surface to
`surfaces.mts`, and the screen.

## Progress 2026-08-01 — the service surface is in; the screen is not

Merged as `f29f3c2`. Gate green: 378 Rust, 76 console. **Story stays `in-progress`** — one Acceptance
item is genuinely unbuilt.

**Built:** `GET/PUT /api/grants` and `POST /api/grants/preview`, all
`Access::PrincipalOfKind(MAY_GRANT)`, asserted against `connections::MAY_SUPPLY_A_CREDENTIAL` itself
rather than against a copy of it. A grant naming an operation id is refused by a **recursive key scan
that runs before serde**, plus `deny_unknown_fields` on the selector — two mechanisms, because the
whole property is that ids cannot get in. The equivalence test calls `exchange_host::admit_grant` over
every operation of a connector and is bracketed both ways, so it cannot pass on an empty set or on a
grant that admits everything.

**Not built: the console screen.** It needs a loader in `console/src/service.mts` — the only module in
this console that knows a network exists, enforced by `descriptor.test.mjs` — which was fenced to
X-57 in the same wave. The implementor **refused to work around it** by giving a new module its own
`fetch`, which was the right call: the invariant is worth more than the story. `service.mts` is free
now.

**Two refusals the story did not ask for, both kept:**
- A `PUT` is refused `409` when the tenant already holds a grant carrying `allow_ids`/`deny_ids` —
  this surface cannot express one, and writing would drop it silently. The read shows it and marks
  `expressible: false`. ⚠ This blocks the primary flow for *exactly today's population*: anyone who
  already hand-wrote a grants file with a deny. Right refusal, first surprise.
- Two grants for one connector in a set are refused `422` rather than resolved by an unstated
  precedence.

**The read is gated to `User` too**, narrower than the Acceptance required. `admit_grant` withholds
the axis that refused, so an agent cannot enumerate a tenant's policy call by call — but an open read
hands the whole policy over in one request. Cost stated rather than discovered: an agent cannot
discover in advance what it may run.

**Carried:** the whole-set `PUT` has no read-modify-write guard, so two concurrent writers race and
the second's stated set wins entire. That is `Grants::set`'s documented whole-set semantics and the end
state is always one caller's intent — but there is no `ConnectionGuard` equivalent. First thing to
look at if a grant "reverts".
