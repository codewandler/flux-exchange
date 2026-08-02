---
id: X-95
title: "Audit evidence survives the process"
status: ready
priority: 1
areas: [exchange-server, observability, operations]
note: "X-87 emits structured success events, but they have no correlation id, durable sink, retention target or alert policy."
---

# Audit evidence survives the process

## Goal
Retain enough non-secret evidence to explain authentication, authorization and administrative
activity after the process that observed it is gone.

## Acceptance
- [ ] Emit JSON audit records with request and event identifiers, timestamp, stable action/outcome,
      actor tenant/kind/id and the minimum non-secret target. Preserve X-87's narrow APIs: tokens,
      credential/setting values, OIDC material and request bodies are unrepresentable as fields.
- [ ] Cover successful and refused authentication, repeated authorization failures, agent lifecycle,
      connection/credential/settings changes, grant changes and invocation outcomes without turning
      refusal detail into a caller oracle.
- [ ] Drain audit records to access-controlled storage with at least 30 days' retention and a tested
      query by event id, actor and target. Document who can read or delete the evidence.
- [ ] Alert on authentication floods, repeated authorization failures and credential/grant changes;
      alerts carry identifiers and counts, never material.
- [ ] Connect credential-supplier provenance to [X-60](X-60-who-supplied-this-credential.md) and keep
      operational audit distinct from the future per-invocation execution-record model.
- [ ] Failing-first tests parse emitted JSON, correlate a request across refusal/success paths and
      scan every field/value for prohibited material sent by a sentinel fixture.
- [ ] Produce a versioned Fly release and verify a live event reaches retained storage and can be
      queried without any credential-shaped field.
