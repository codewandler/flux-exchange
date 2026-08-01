---
id: X-10
title: "Connections addressed by a tenant the caller cannot name"
status: ready
priority: 5
epic: connections
note: "Tenant::new already refuses a traversing spelling at construction; this story is where that validated value becomes the ONLY way an address is built"
---

# Connections addressed by a tenant the caller cannot name

## Goal
A connection — a connector plus its credential and configuration — belongs to a tenant, and is
reachable only through a principal resolved for that tenant.

## Acceptance
- [ ] Create, list and delete a connection, scoped to the caller's tenant.
- [ ] **Failing-first test** — tenant A's authenticated principal cannot read, use or delete tenant
      B's connection, and the refusal names the address rather than the value.
- [ ] The credential address is **derived**: `tenants/<tenant>/<authority>/<credential>`, where the
      tenant comes from the principal and the authority from the connector's declaration. No route
      accepts an address.
- [ ] **Failing-first test** — a connector with no declared authority is refused rather than stored
      at a guessed address.
- [ ] Deleting a connection destroys its credential.

## Progress
- (not started — X-03 and X-09 first)
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
