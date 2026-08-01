---
id: X-10
title: "Connections addressed by a tenant the caller cannot name"
status: done
epic: connections
design: docs/designs/connections.md
note: "Tenant::new already refuses a traversing spelling at construction; this story is where that validated value becomes the ONLY way an address is built"
---

# Connections addressed by a tenant the caller cannot name

## Goal
A connection — a connector plus its credential and configuration — belongs to a tenant, and is
reachable only through a principal resolved for that tenant.

## Acceptance
- [x] Create, list and delete a connection, scoped to the caller's tenant.
- [x] **Failing-first test** — tenant A's authenticated principal cannot read, use or delete tenant
      B's connection, and the refusal names the address rather than the value.
- [x] The credential address is **derived**: `tenants/<tenant>/<authority>/<credential>`, where the
      tenant comes from the principal and the authority from the connector's declaration. No route
      accepts an address.
- [x] **Failing-first test** — a connector with no declared authority is refused rather than stored
      at a guessed address.
- [x] Deleting a connection destroys its credential.

## Progress
- **Done.** Merged from `impl/X-10`; gate green (167 tests).
- Cross-tenant isolation held under everything a review could build: 18 hostile connector ids across
  3 methods — traversal, percent-encoded traversal, a NUL, unicode homoglyphs, a 5000-char id — all
  refused, with the store empty afterwards. The connector id is a **catalogue key only**; all three
  address components come from `principal.tenant()` and `&'static` catalogue data.
- **A review broke round one on concurrency, and it was the story's own point.** The 409 was a
  check-then-write with nothing between the halves: two concurrent `POST`s from one tenant both got
  `201` on **attempt 0 of 500**, one value silently overwritten, and the loser told it had succeeded
  — verbatim the failure this story exists to prevent. Closed by `ConnectionGuard`, which claims
  `(tenant, connector)` across the whole probe-decide-write of `POST` and `DELETE`. Per-connection
  rather than global, because one lock over the surface would make one tenant's writes wait on
  another's — shared fate in the repository whose point is that tenants share nothing. It **refuses
  rather than queues**: waiting needs a lock held across an `await`, and the queued caller would wake
  to find the value and answer `409` anyway.
- **The limit, written down rather than implied:** this is single-process only. Two replicas over one
  store would race again and nothing here would notice, because `SecretStore` has no
  compare-and-swap. Recorded in the guard's docs, the route module, and the design.
- A partial `create` now rolls back what it wrote, so a store failure part-way through no longer
  leaves a connection that could never be completed without a `DELETE`.
- Store failures keep their kind out to the caller: `Unreachable` → 503 "retrying may work";
  `Denied`/`Backend` → 502 "retrying will not help". Collapsing them would have told an operator to
  wait out a problem that waiting cannot fix.
- **A load-bearing invariant nobody had asserted:** no two connectors render the same address for one
  tenant — measured at 60 addresses across 52 addressable connectors. A collision would make
  connecting one read as connecting the other.
- **The second-connection refusal is a placeholder, and says so**, quoting `@instances/<uuid>` and
  naming X-14 and upstream C-406. Per-connection *configuration* is deferred for the same reason: a
  Zendesk subdomain is exactly the per-instance fact with no home until the address can tell
  instances apart.
- **The address above is incomplete, raised by the owner 2026-08-01.** A tenant with two connections
  to the *same* connector — two Zendesk subdomains, a sandbox and a production account — collides:
  `tenants/<tenant>/<authority>/<credential>` has no instance dimension, so the second connection
  overwrites the first. See [X-14](X-14-two-instances-of-one-connector.md). Do not land this story's
  address scheme as written; either X-14 lands first, or this story carries the instance dimension.

## Notes
- `Tenant::new` (`crates/exchange-host/src/principal.rs`) already refuses empty, over-long and
  traversing spellings at construction, with tests. Build on it; do not re-validate ad hoc.
- Design first: this story is non-trivial and has no design doc yet. Write one under
  `docs/designs/` (`/track:design`) before implementing. The cross-repo reasoning it builds on is
  flux's [`docs/designs/ecosystem.md`](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md).
