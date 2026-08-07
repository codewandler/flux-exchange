---
id: X-142
title: "Hosted single-org posture and member token minting"
pillar: "Core"
status: backlog
epic: hosted-single-org
design: hosted-single-org
note: "Decision 0019 rules 1-2: reachable SingleTenant behind OIDC is a named posture; members mint bounded Service Account tokens; operator authority stays deployment-declared"
---

# Hosted single-org posture and member token minting

## Goal

Hosted single-org becomes a named deployment posture (Decision 0019 rules 1–2): a reachable bind,
`SingleTenant` tenancy and OIDC identity together mean every principal the IdP admits under the
verified hosted-domain claim is a member of the one tenant. In this posture — and only in it — the
Service Account mint route accepts an authenticated member rather than requiring an operator, so a
signed-in engineer can mint the token their own client needs. Operator authority for every other
management surface stays deployment-declared: the existing subject allowlist, optionally extended
by a deployment-declared claim policy, never derived from sign-in alone.

## Acceptance

- [ ] The posture is explicit configuration, refused when its parts disagree (reachable bind
      without OIDC, or a tenant mismatch), and visible in startup diagnostics; local single-tenant
      and multi-tenant behavior is byte-for-byte unchanged, proven by the existing suites.
- [ ] In the posture, a member mints a Service Account token bound to their own subject under
      bounded lifetime and a per-principal live-token ceiling; outside it the mint route remains
      operator-only, proven by both test faces.
- [ ] Every mint writes an audit record binding token id to the minting subject; token values stay
      hash-stored and absent from responses beyond the one-time mint reply.
- [ ] A deployment-declared claim policy may grant operator to a bounded set; sign-in alone never
      does, and the refusal face is tested.
