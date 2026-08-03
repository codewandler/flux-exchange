---
id: X-96
title: "Traffic controls are fair as well as bounded"
status: in-progress
priority: 1
epic: remote-deployment
areas: [exchange-server, observability, operations]
note: "The process-wide X-87 limiter bounds memory and concurrency but lets one caller spend the shared invocation budget and supplies no saturation metric."
---

# Traffic controls are fair as well as bounded

## Goal
Keep the process-wide safety backstop while preventing one identity or an anonymous edge flood from
consuming the service's entire useful budget without visibility.

## Acceptance
- [x] Preserve the process-wide sign-in, invocation-rate and invocation-concurrency limits as the
      final application backstop; saturation still refuses immediately with `429` and
      `Retry-After`, without an unbounded queue.
- [x] Add per-principal invocation budgets after identity resolution and before vendor dispatch.
      Tenant/principal keys come only from the resolved principal, never a caller header or body.
- [x] Put anonymous flood controls at the trusted edge for sign-in and other anonymous endpoints.
      Document the proxy trust boundary and do not read `X-Forwarded-For` or another forwarding
      header unless the immediate proxy is explicitly authenticated/configured.
- [x] Expose bounded metrics for admitted/refused work, saturation and active invocations, labelled
      without principal identifiers or attacker-controlled high-cardinality values.
- [x] Failing-first tests prove one principal cannot exhaust another's budget, the global ceiling
      still holds across principals, spoofed forwarding headers do not select buckets, and health
      remains responsive under saturation.
- [x] Alert on sustained anonymous and authenticated saturation without logging tokens or request
      bodies.
- [ ] Produce a versioned Fly release and live-verify edge refusal, per-principal fairness, metrics
      and the unchanged process ceiling.

## Evidence

- 2026-08-03 — v0.16.1 served 40 concurrent anonymous sign-in requests without following the
  provider redirects: 30 were admitted and 10 refused immediately with `429`; every refusal carried
  `Retry-After`. Health remained `200` after saturation. `/metrics` then exposed exactly the seven
  fixed series, recording 30 admitted and 10 globally refused sign-ins with no caller-derived label.
  An authenticated two-principal live comparison remains before the release acceptance is complete.
