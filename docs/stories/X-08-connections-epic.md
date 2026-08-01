---
id: X-08
title: "Connections and credentials (epic)"
status: ready
priority: 4
epic: connections
note: "connector-secrets and connector-spec carry NO flux dependency, so this epic is unblocked too — only invoke waits on the engine line"
---

# Connections and credentials (epic)

## Goal
Let an operator connect a provider: deposit a credential, bind whatever configuration the connector
requires, and have both reachable only by that tenant.

## Acceptance
- [ ] X-09 — a credential store that is honest about what protects it.
- [ ] X-10 — connections addressed by a tenant the caller cannot name.

## Progress
- (not started)

## Notes
- `codewandler-connector-secrets` 0.8.0 depends on `connector-spec`, `async-trait`, `reqwest`,
  `serde_json`, `thiserror` — **no flux crate**. This epic does not wait on X-11.
