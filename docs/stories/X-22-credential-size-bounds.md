---
id: X-22
title: "One tenant cannot make every other tenant's writes slow"
status: done
epic: connections
areas: [exchange-server, exchange-host]
note: "found by the standing credential audit, 2026-08-01: nothing bounds a credential's size or a connection's count, and every put/delete rewrites and fsyncs the whole store under one mutex — so one authenticated tenant sets the cost of every other tenant's write"
---

# One tenant cannot make every other tenant's writes slow

## Goal
What one tenant stores cannot decide what another tenant's write costs.

## What is wrong

`NewConnection.credentials` is a `BTreeMap<String, String>` with **no length check on either side**.
A tenant may declare up to the connector's full credential set, and axum's default body limit is
2 MB, so one authenticated tenant can occupy roughly sixty addresses at 2 MB each.

That would be merely wasteful if the store were per-tenant. It is not: `FileStore` holds **one file**,
and every `put` and `delete` rewrites and `fsync`s the whole of it under **one mutex**. So the size
of tenant A's credentials sets the latency of tenant B's every write, and a tenant who fills their
allowance degrades a surface they do not share anything else with.

This is shared fate between tenants in the repository whose central claim is that tenants share
nothing. That is what makes it worth fixing rather than filing as a capacity note — it is the
invariant, not the performance.

## Acceptance
- [x] **Failing-first test** — a credential value beyond the bound is refused, naming the bound and
      the credential, and **nothing is written**. Assert the store is untouched, not merely that the
      status is 4xx.
- [x] A connection carrying more credentials than the connector declares is already refused; a
      connection at or under the bound still succeeds, asserted in the same run so the refusal
      cannot pass by refusing everything.
- [x] The bound is **stated once** and named in the refusal, so an operator reading it learns the
      limit rather than guessing.
- [x] The refusal carries **no credential value** — it names the credential and the bound, never
      what was sent. The existing disclosure guarantees stay green.
- [x] A test pins that the total a single tenant can occupy is bounded, not just each value —
      per-value limits alone do not bound the whole.

## Notes
- Two bounds, and they are different questions: **per value** (a credential is a token, not a file)
  and **per tenant** (how much of one store one tenant may hold). Decide both and say why each
  number is what it is; a number with no argument beside it is one the next person will change
  without knowing what it protected.
- The refusal belongs at the route, before anything is written — `routes/connections.rs` already
  refuses an undeclared credential there and that is the precedent for shape.
- **Do not** solve this by raising axum's body limit or by adding a second store. The design's
  single-process, single-file limit is recorded and out of scope here; this story is about the
  bound, not the storage shape.
- Consider whether `exchange-host`'s `SecretStore` port should carry the bound instead, so a second
  implementation cannot forget it. That is a real design question — if the port is the right home,
  say so and put it there.

## Progress
- **Done 2026-08-01.** Gate green: 43 + 171 tests (both crates gained tests), clippy clean, fmt
  clean. Genuine merge-base failure — a 64 KiB credential was accepted and stored at the base, and
  the quoted body's `held: true` states that directly.
- **The two numbers, with the argument beside each.** `MAX_CREDENTIAL_VALUE_BYTES` = **8 KiB**, about
  *kind*: a credential is a token, a signing secret, or at the largest an RSA-4096 PEM at ~3.2 KiB,
  so a value that does not fit is not a credential that grew. `MAX_TENANT_STORE_BYTES` = **64 KiB**,
  the one that protects the neighbours — per-value alone leaves a ceiling of `addresses x 8 KiB`,
  ~480 KiB against today's 60 addresses and **growing every time upstream adds a connector**. Two
  `const _: () = assert!(...)` compile-time checks pin that the tenant bound stays the tighter.
- **The port question was checked, not assumed.** `SecretStore` is `connector_secrets`', re-exported
  by `exchange-host` deliberately as "a doorway to it, not a second copy", so a bound there means
  editing an upstream crate or declaring a second port locally. Instead the per-value bound sits in
  `ConnectorDeclaration::writes` — the only way supplied values become writes, so it is the step
  that produces the thing to be written rather than a check the route must remember. What that does
  not reach is a different `SecretStore` fed by some other path, and the module doc says so rather
  than glossing it.
- `413` answers an oversized value; `409` an exhausted allowance, following this module's existing
  sense of `409` — the tenant's own state conflicts and the remedy is a `DELETE`. A `413` there
  would tell an operator to send less when they have to disconnect something.
- **The implementor found a hole in the bound it had just added, and reported it in the same
  breath:** occupancy is read and written under a claim keyed per `(tenant, connector)`, so one
  tenant's concurrent creates to *different* connectors can overshoot. Bounded — every value still
  passes the per-value bound, so the worst case degrades to the ~480 KiB ceiling rather than to
  nothing — and closing it means reversing a property `connection_guard` deliberately pins. Filed as
  [X-25](X-25-tenant-allowance-race.md) rather than half-fixed.
- **Carried forward:** `create` now walks the tenant's catalogued addresses (~60 `get`s) to measure
  occupancy — the same walk `GET /api/connections` already makes, paid on the rarer `POST`, and map
  lookups against `FileStore`. First place to look if creates get slow. The refusals quote the
  caller's **own** byte counts, never another tenant's and never a value; that is the only new
  information crossing the boundary.
- Unchanged and out of scope: axum's 2 MB body limit, so a large body is still read and parsed
  before the per-value bound refuses it. The refusal is before any *store* write, which is what the
  Acceptance asks and what the test asserts.
