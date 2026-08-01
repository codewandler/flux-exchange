---
id: X-59
title: "A deployment can hold one tenant and stop asking which"
status: ready
priority: 2
epic: local-identity
design: docs/designs/local-identity.md
areas: [exchange-server, exchange-host]
note: "the tenancy axis, orthogonal to authentication: Deployment::SingleTenant already exists for the runtime gate and this extends it rather than inventing it"
---

# A deployment can hold one tenant and stop asking which

## Goal
Running this for yourself does not mean inventing a tenant id.

## Why this is separate from [[X-58]], and second

The request that prompted this epic framed static users and "no multi-tenancy" as alternatives. They
are **two axes**, and all four combinations are legitimate — local users on a multi-tenant host is a
self-hosted team; OIDC on a single-tenant host is one company's deployment.

**Authentication is what blocks console use. Tenancy is a convenience.** So this is second, and it is
not a prerequisite for anything.

## What it must not become

`Deployment::SingleTenant` already exists — `admit_runtime` takes it, `runtime.rs` distinguishes it
from `MultiTenant`. This extends that; it does not invent a mode.

**Single-tenant is one tenant, not no tenant.** Every credential address is
`tenants/<tenant>/<authority>/<credential>`, and a mode that omitted the segment would write
credentials where nothing else looks for them — the same stranding upstream's instance elision exists
to avoid, and a one-way door for anybody who later grows a second tenant.

So: one tenant, **named once at startup**, and every principal is of it. The address is unchanged.

## Acceptance
- [ ] A deployment declares one tenant at startup and every principal it resolves is of that tenant.
- [ ] **The credential address is byte-identical** to what a multi-tenant deployment renders for the
      same tenant. Assert it literally, the way `tests/engine_line.rs` asserts the rendered address —
      this is the property that keeps the door open.
- [ ] **Failing-first test** — nothing in a request can name a tenant, in this mode or any other. This
      already holds; pin it here too, because a single-tenant mode is where somebody would be tempted
      to accept one "since there is only one".
- [ ] Moving a single-tenant deployment to multi-tenant does not strand a stored credential. State how
      in the design, and test it if it is testable.
- [ ] The console does not show a tenant column that always says the same word, and the descriptor
      does not publish the tenant's name — it is tenant-specific and `GET /api/onboarding` is
      anonymous.

## Notes
- The honest gain is small and worth saying so: it removes a made-up id from the getting-started path.
  It does not remove tenancy from the model, and it must not.
