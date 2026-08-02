---
id: X-97
title: "Public credentials leave the file store"
status: ready
priority: 2
epic: remote-deployment
areas: [exchange-host, exchange-server, deployment]
note: "The file store is honest and mode-safe but application-plaintext; the existing SecretStore port is the seam for a managed Vault-class backend."
---

# Public credentials leave the file store

## Goal
Bind public deployments to a managed secret backend so compromise, access policy, encryption keys
and auditability no longer reduce to one plaintext file and its platform volume.

## Acceptance
- [ ] Bind the existing `SecretStore` port to a managed Vault-class backend; do not introduce a
      second credential API or request-building path into `exchange-host`.
- [ ] Authenticate the workload without a long-lived bootstrap secret in repository files, image
      layers or ordinary configuration. Backend policy limits this deployment to its own credential
      prefix and required operations.
- [ ] Keep non-secret settings and grants outside the secret backend. Their existing stores and
      meanings do not change merely because credentials moved.
- [ ] Migrate from the file store atomically with a measured inventory, read-after-write
      verification and an explicit cutover that never silently falls back to the old store.
- [ ] Rotate every migrated vendor credential after cutover. Remove old credential-store
      directories including sibling temporaries, then destroy old volumes/copies and wait for all
      retained snapshots to expire under the recorded decommission timeline.
- [ ] Failing-first integration tests cover unavailable/denied backend errors, tenant-prefix
      isolation, atomic migration and restart persistence without exposing a value in logs or
      diagnostics.
- [ ] Update the no-second-request-path dependency allow-list with a sentence explaining why the
      backend client is a store transport rather than an operation transport; keep its feature and
      dependency surface narrow.
- [ ] Produce a versioned Fly release and live-verify managed reads/writes/rotation, refusal when the
      backend is unavailable, and completion of old-store decommissioning.
