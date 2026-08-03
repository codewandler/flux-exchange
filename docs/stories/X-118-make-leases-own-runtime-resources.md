---
id: X-118
title: "Make leases own rich runtime resources"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "Milestone 3 — acquire/renew/release ownership for stateful connector resources; no dependency from the effective catalogue or one-shot HTTP invoke"
---

# Make leases own runtime resources

## Goal

Bind every stateful connector resource to a caller's still-valid grant and liveness, with deterministic
cleanup on release, expiry, revocation, disconnect or runtime failure. Lease lifecycle follows the
independently shippable X-113 HTTP contract.

## Acceptance

- [ ] Acquire returns an opaque lease id, never a credential/handle meaningful outside Exchange; use
      and release derive the same principal, connection and operation scope.
- [ ] TTL, renewal bounds and maximum held resources are enforced server-side and declared in the
      X-117/X-118 lifecycle protocol extension.
- [ ] WebSocket loss, grant revocation, connection rotation, worker death and normal release each run
      idempotent cleanup and record a value-free terminal event.
- [ ] Restart recovery either reattaches through a declared runtime mechanism or expires/refuses; it
      never pretends an unknown resource survived.
- [ ] Failing-first tests cover a database handle, port-forward or equivalent resource and prove one
      tenant/principal cannot use another's lease id.

## Progress

- (not started)

## Notes

- Depends on X-114, X-115 and X-117. X-113 deliberately owns no lease acceptance.
