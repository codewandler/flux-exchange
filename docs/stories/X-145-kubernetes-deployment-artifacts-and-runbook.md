---
id: X-145
title: "Kubernetes deployment artifacts and runbook"
pillar: "Core"
status: backlog
epic: hosted-single-org
design: hosted-single-org
note: "Decision 0019 rule 6: manifests and runbook encode the delivered constraints - one writer, PVC audit journal, store modes, same-origin console"
---

# Kubernetes deployment artifacts and runbook

## Goal

Published Kubernetes manifests and a runbook encode what the code already enforces, so the first
cluster deployment is configuration rather than discovery. The delivered container image is a
static `FROM scratch` build; the constraints the manifests must honor are all measured behavior:
a reachable bind refuses to start without the durable audit journal, store parents refuse modes
wider than 0700, exactly one writer may own the file stores, sessions are in-memory so a rollout
signs everyone out, and the console must be served same-origin with the API.

## Acceptance

- [ ] Manifests deploy the released image with `replicas: 1`, a recreate strategy, a persistent
      volume for the audit journal and stores, and mode/ownership handling that survives volume
      mounting; a kind/minikube walkthrough in the runbook reproduces a green `/health`.
- [ ] The hosted single-org posture (X-142), destination allowlist (X-143) and declarative
      provisioning (X-144) each appear in the manifests as commented, value-free examples.
- [ ] One Ingress serves console and API same-origin; the runbook states the rollout sign-out
      behavior and the single-writer rule with their reasons.
- [ ] Nothing organization-specific appears anywhere: tenant, hosted domain, operator subjects and
      destinations are placeholders the deployment fills.
