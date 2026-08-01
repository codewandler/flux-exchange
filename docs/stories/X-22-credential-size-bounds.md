---
id: X-22
title: "One tenant cannot make every other tenant's writes slow"
status: ready
priority: 1
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
- [ ] **Failing-first test** — a credential value beyond the bound is refused, naming the bound and
      the credential, and **nothing is written**. Assert the store is untouched, not merely that the
      status is 4xx.
- [ ] A connection carrying more credentials than the connector declares is already refused; a
      connection at or under the bound still succeeds, asserted in the same run so the refusal
      cannot pass by refusing everything.
- [ ] The bound is **stated once** and named in the refusal, so an operator reading it learns the
      limit rather than guessing.
- [ ] The refusal carries **no credential value** — it names the credential and the bound, never
      what was sent. The existing disclosure guarantees stay green.
- [ ] A test pins that the total a single tenant can occupy is bounded, not just each value —
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
