---
id: X-93
title: "Production comes from a reviewed commit"
status: in-progress
priority: 1
epic: remote-deployment
areas: [ci, deployment, supply-chain]
note: "Wave #1 implements the immutable-SHA full-gate workflow, digest-pinned image, SBOM/scan, exact-digest deploy, evidence and rollback verifier; production environment configuration and the first workflow release remain live operations."
---

# Production comes from a reviewed commit

## Goal
Make every production image traceable to one reviewed, gated commit and leave machine-readable
evidence of what was built, scanned and deployed.

## Acceptance
- [x] Replace working-tree `fly deploy` with a GitHub workflow that accepts or derives an immutable
      commit SHA, checks out exactly it, runs the complete repository gate and deploys only that
      checkout. Every third-party action is pinned to a full SHA.
- [x] Use a protected GitHub production environment and a production-scoped Fly token with the
      narrowest workable permission; forks and pull requests cannot receive it.
- [x] Pin every container base by digest while retaining a readable version comment, run Cargo with
      `--locked` in image builds and fail if a lockfile would change.
- [x] Scan the built image for known vulnerabilities and emit an SBOM tied to the image digest and
      source SHA. Exceptions are narrow, inline and owned like the existing RustSec exceptions.
- [ ] Publish or record source SHA, image digest, Fly release and machine identifiers without
      credential-shaped values.
- [x] After deployment, verify `/health` reports the release version and live console/API responses
      carry the security headers and `no-store` policy. A failed verification fails or rolls back
      the deployment visibly.
- [ ] Produce a versioned Fly release through the new path, update the runbook and remove language
      that describes dirty-worktree deployment as normal.

## Evidence

- 2026-08-03 — failing-first `scripts/check-production-operations.sh` refused the unpinned,
  unlocked Dockerfile and absent workflow. Its self-test now proves it rejects a movable base; the
  repository contract and the global action-pin checker both pass.
- 2026-08-03 — `.github/workflows/production.yml` selects only a full SHA reachable from protected
  `main`, reruns Rust/MSRV/console/site/audit/repository gates, builds one labelled image, emits SPDX,
  refuses Grype findings at every severity, pushes once and deploys the resolved digest. Verification
  records release/machine/source/digest and redeploys the prior digest before leaving a failed run red.
- 2026-08-03 — the read-only verifier passed against current Fly release v4: health reported 0.16.0,
  console/API security policy was present, API policy was `no-store`, exactly one machine ran the
  expected digest, and release/machine identifiers were obtainable without reading a credential.
  This diagnoses the verifier only; it is not the required new-path release.
- 2026-08-03 — the GitHub `production` environment was read back with protected-branch-only policy,
  no circular required reviewer for the repository's sole maintainer, and an app-scoped 90-day Fly
  deploy token stored only as its environment `FLY_API_TOKEN`. Pull requests and forks do not run
  either production workflow and cannot receive the environment secret.
- Remaining live work: merge the workflow and retain the first green production artifact.
