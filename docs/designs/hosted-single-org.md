# Design — Hosted single-org

## Why

One organization operating Exchange on its own shared infrastructure — a dev Kubernetes cluster is
the motivating case — sits between the two delivered postures. It is single-tenant, but reachable;
its members are IdP-verified, but not operators; its destinations include the organization's own
private services, but destination authority belongs to whoever authors the deployment, never to a
signed-in member. Decision 0019 names this posture and draws its one distinction precisely:
authority, not reachability. The infra layer holds the cluster's network and filesystem bindings;
an authenticated member only ever exercises governed grants.

## Approach

Everything rides delivered machinery. OIDC with the verified hosted-domain claim and exact-match
`SingleTenant` tenancy are configuration; the posture only composes them with a reachable bind and
names the composition. Member minting relaxes one route's access check inside the posture, bounded
and audited. The destination aperture reuses Decision 0008 rule 4's destination-pinning at the two
existing egress composition points. Declarative provisioning reuses the connector-declared forms
and the file-shaped secret kind, reconciled idempotently at startup by Exchange-owned input — the
Decision 0007 boundary holds with the deployment operator as a second owner. Manifests document
constraints the code already enforces.

## Stories

- X-142 — hosted single-org posture and member token minting
- X-143 — deployment-declared destination aperture
- X-144 — declarative provisioning from file-shaped secrets
- X-145 — Kubernetes deployment artifacts and runbook
- Flux side: `flux/C-656` (SSO member login over a named binding, Decision 0019 rule 5)
