---
id: X-97
title: "Public credentials leave the file store"
status: in-progress
priority: 2
epic: remote-deployment
areas: [exchange-host, exchange-server, deployment]
note: "The file store is honest and mode-safe but application-plaintext; the SecretStore port is the seam. The shipped connector-secrets VaultStore is NOT that backend — it refuses references, apply and every prepared transaction."
design: docs/designs/managed-secret-backend.md
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

## Progress

- 2026-08-03 — Design filed. Nothing built.
- 2026-08-12 — Status audited. `in-progress` means *design filed, zero implementation*: all eight
  acceptance boxes are unchecked and no branch, local or remote, carries any X-97 code. The two
  commits mentioning X-97 are documentation only.

  **"Bind the existing `VaultStore`; don't build one" was checked and does not hold.** The type is
  real — `connector_secrets::vault::VaultStore`, package `codewandler-connector-secrets` 0.20.0,
  `src/vault.rs:143`, a HashiCorp Vault KV v2 client present in every published version — but it
  cannot satisfy this story's acceptance, for four independent reasons:

  1. `references()` returns `StoreError::Unsupported` (`vault.rs:384`) — *"Vault KV v2 listing and
     policy semantics are not implemented"*. Acceptance requires a **measured inventory**.
  2. `apply()` returns `StoreError::Unsupported` (`vault.rs:391`). Acceptance requires **atomic**
     migration, and the live connection routes already depend on `apply` — the tests at
     `crates/exchange-server/src/routes/connections.rs:10341` and `:10393` fail against it today.
  3. `impl PreparedSecretStore for VaultStore {}` is empty (`vault.rs:401`), so all five methods keep
     the trait defaults that return `PreparedSecretError::Unsupported` (`transaction.rs:170-205`).
  4. It authenticates with a **static long-lived token only** (`vault.rs:31-38`, "expiry, refresh and
     rotation are out of scope for this crate by instruction") — which the second acceptance bullet
     explicitly forbids.

  It is also not compiled here: the `vault` feature is non-default and this workspace does not enable
  it, as `crates/exchange-host/tests/no_second_request_path.rs:114-119` states in so many words.

  So the design's choice stands and the note below is still the governing constraint: the adapter has
  to be built and **released upstream in `connector-secrets`** before Exchange can consume it. No AWS
  Secrets Manager implementation exists in any of the ten `connector-secrets` versions on disk.
  **This story is upstream-blocked in the same way [[X-146]] and [[X-147]] are** — kept
  `in-progress` rather than `blocked` only because the design work inside it is genuinely live.

## Notes

- 2026-08-03 — Design selected AWS Secrets Manager behind the existing `SecretStore` port, with Fly
  Machine OIDC exchanged through STS web identity, app-scoped audience/subject trust, a
  deployment-scoped IAM/KMS policy, and a versioned scope manifest as the atomic commit point.
  Implementation, migration, vendor rotation, Fly verification, and decommissioning remain open.
- The current compatible Rust line is designed around exact `aws-sdk-secretsmanager` 1.99.0 and
  `aws-config` 1.8.13 pins. The implementation must prove their complete locked graph on Rust 1.88;
  it must not raise the workspace MSRV to admit the current SDK release.
- `SecretBatch` keeps its checked mutations private to `connector-secrets`, so the atomic adapter
  must first ship there as a crates.io release. Exchange will propagate that optional store feature;
  it will not add a second mutation API or use a sibling path/git dependency.
