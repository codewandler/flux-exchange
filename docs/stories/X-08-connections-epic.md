---
id: X-08
title: "Connections and credentials (epic)"
status: done
epic: connections
note: "connector-secrets and connector-spec carry NO flux dependency, so this epic is unblocked too — only invoke waits on the engine line"
---

# Connections and credentials (epic)

## Goal
Let an operator connect a provider: deposit a credential, bind whatever configuration the connector
requires, and have both reachable only by that tenant.

## Acceptance
- [x] X-09 — a credential store that is honest about what protects it.
- [x] X-10 — connections addressed by a tenant the caller cannot name.

## Progress
- **Done.** Both children landed: X-09's credential store and X-10's tenant-addressed connections.
- **One half of the Goal is deliberately not met, and it is not a gap in the children.** "Bind
  whatever configuration the connector requires" is unbuilt: a per-connection value like a Zendesk
  subdomain is exactly the per-instance fact that has no home until the credential address can tell
  two instances of one connector apart. That is [X-14](X-14-two-instances-of-one-connector.md),
  blocked on the upstream publish. Connecting works today for one connection per connector per
  tenant; a second is refused rather than silently overwriting the first.

## Notes
- `codewandler-connector-secrets` 0.8.0 depends on `connector-spec`, `async-trait`, `reqwest`,
  `serde_json`, `thiserror` — **no flux crate**. This epic does not wait on X-11.
