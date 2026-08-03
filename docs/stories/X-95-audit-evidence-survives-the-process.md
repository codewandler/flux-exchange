---
id: X-95
title: "Audit evidence survives the process"
status: done
priority: 1
areas: [exchange-server, observability, operations]
note: "v0.16.0 retains correlated, value-free operational evidence for 30 days; Fly release v4 proved one event survives restart and remains queryable."
design: ../designs/durable-audit-evidence.md
---

# Audit evidence survives the process

## Goal
Retain enough non-secret evidence to explain authentication, authorization and administrative
activity after the process that observed it is gone.

## Acceptance
- [x] Emit JSON audit records with request and event identifiers, timestamp, stable action/outcome,
      actor tenant/kind/id and the minimum non-secret target. Preserve X-87's narrow APIs: tokens,
      credential/setting values, OIDC material and request bodies are unrepresentable as fields.
- [x] Cover successful and refused authentication, repeated authorization failures, agent lifecycle,
      connection/credential/settings changes, grant changes and invocation outcomes without turning
      refusal detail into a caller oracle.
- [x] Drain audit records to access-controlled storage with at least 30 days' retention and a tested
      query by event id, actor and target. Document who can read or delete the evidence.
- [x] Alert on authentication floods, repeated authorization failures and credential/grant changes;
      alerts carry identifiers and counts, never material.
- [x] Connect credential-supplier provenance to [X-60](X-60-who-supplied-this-credential.md) and keep
      operational audit distinct from the future per-invocation execution-record model.
- [x] Failing-first tests parse emitted JSON, correlate a request across refusal/success paths and
      scan every field/value for prohibited material sent by a sentinel fixture.
- [x] Produce a versioned Fly release and verify a live event reaches retained storage and can be
      queried without any credential-shaped field.

## Notes

- 2026-08-03 — Fly release v4 (`deployment-01KZ3XM2SYK50GJFV4DWN0YNMJ`) serves application version
  `0.16.0` on machine `2862de4a43e058`, with the audit directory at `0700` and database at `0600`.
  The image was built from this dirty feature tree; X-93 remains the owner of reviewed-commit
  attribution for production images.
- Anonymous request `257595bb43da6de0eab80744eaf3d044` produced retained authentication-refusal
  event `c13fc966277d458cc4ac4c4052c8efcc`. Querying that event before and after restarting the sole
  machine returned the same closed JSON record, with no actor or value field and no
  credential-shaped material.
- The completed gate was `cargo build --workspace`, `cargo test --workspace` (including 324 server
  tests), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, the
  console test/build, and the public-site build/test. This host's system temporary directory was
  quota-exhausted, so Rust and site scratch files used `/home/timo/.cache/flux-exchange-tests`.
