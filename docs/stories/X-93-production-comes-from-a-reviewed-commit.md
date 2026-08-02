---
id: X-93
title: "Production comes from a reviewed commit"
status: ready
priority: 1
epic: remote-deployment
areas: [ci, deployment, supply-chain]
note: "v0.13.0 was manually built from a dirty working tree; the live version is known, but the image is not reproducibly attributable to a reviewed SHA."
---

# Production comes from a reviewed commit

## Goal
Make every production image traceable to one reviewed, gated commit and leave machine-readable
evidence of what was built, scanned and deployed.

## Acceptance
- [ ] Replace working-tree `fly deploy` with a GitHub workflow that accepts or derives an immutable
      commit SHA, checks out exactly it, runs the complete repository gate and deploys only that
      checkout. Every third-party action is pinned to a full SHA.
- [ ] Use a protected GitHub production environment and a production-scoped Fly token with the
      narrowest workable permission; forks and pull requests cannot receive it.
- [ ] Pin every container base by digest while retaining a readable version comment, run Cargo with
      `--locked` in image builds and fail if a lockfile would change.
- [ ] Scan the built image for known vulnerabilities and emit an SBOM tied to the image digest and
      source SHA. Exceptions are narrow, inline and owned like the existing RustSec exceptions.
- [ ] Publish or record source SHA, image digest, Fly release and machine identifiers without
      credential-shaped values.
- [ ] After deployment, verify `/health` reports the release version and live console/API responses
      carry the security headers and `no-store` policy. A failed verification fails or rolls back
      the deployment visibly.
- [ ] Produce a versioned Fly release through the new path, update the runbook and remove language
      that describes dirty-worktree deployment as normal.
