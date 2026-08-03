---
id: X-118
title: "Make leases own rich runtime resources"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "turn the existing lease vocabulary into acquire/renew/release ownership for database handles, port-forwards, exec sessions and other stateful connector resources"
---

# Make leases own runtime resources

## Goal

Bind every stateful connector resource to a caller's still-valid grant and liveness, with deterministic
cleanup on release, expiry, revocation, disconnect or runtime failure.

## Acceptance

- [ ] Acquire returns an opaque lease id, never a credential/handle meaningful outside Exchange; use
      and release derive the same principal, connection and operation scope.
- [ ] TTL, renewal bounds and maximum held resources are enforced server-side and declared in the
      remote connector protocol.
- [ ] WebSocket loss, grant revocation, connection rotation, worker death and normal release each run
      idempotent cleanup and record a value-free terminal event.
- [ ] Restart recovery either reattaches through a declared runtime mechanism or expires/refuses; it
      never pretends an unknown resource survived.
- [ ] Failing-first tests cover a database handle, port-forward or equivalent resource and prove one
      tenant/principal cannot use another's lease id.

## Progress

- (not started)
