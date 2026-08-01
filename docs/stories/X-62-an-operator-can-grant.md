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
- [ ] A route reads the grants a tenant holds, and a route edits them. Both declare an access, and
      the write is at least as narrow as [[X-54]]'s `MAY_SUPPLY_A_CREDENTIAL`.
- [ ] **Failing-first test** — a grant written through the surface admits exactly what the gate
      admits. Assert it against `admit_grant`, not against a copy of its rules.
- [ ] The surface expresses selectors, never operation ids. **Failing-first test** — a request naming
      an operation id is refused.
- [ ] The console shows which operations a proposed grant would admit, derived from the same facts
      the gate uses, before it is saved.
- [ ] **Nothing tenant-specific leaks anonymously.** What a tenant is granted is tenant data; the
      catalogue and the descriptor must not learn it.
- [ ] A deployment with no grants file bound still serves everything else, and says which setting is
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
