---
id: X-93
title: "Production comes from a reviewed commit"
status: done
priority: 1
epic: remote-deployment
areas: [ci, deployment, supply-chain]
note: "v0.16.1 was built from protected main, scanned clean, deployed by immutable digest, verified live and retained with 90-day source/image/release evidence."
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
- [x] Publish or record source SHA, image digest, Fly release and machine identifiers without
      credential-shaped values.
- [x] After deployment, verify `/health` reports the release version and live console/API responses
      carry the security headers and `no-store` policy. A failed verification fails or rolls back
      the deployment visibly.
- [x] Produce a versioned Fly release through the new path, update the runbook and remove language
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
- 2026-08-03 — the first protected-main production run failed closed before deployment because its
  fresh Ubuntu runner did not carry `rg`. The production contract checker now uses portable POSIX
  `grep -E`; its failing run is the release-path test that exposed the undeclared local dependency.
- 2026-08-03 — the next run built the immutable image and failed closed before push because Grype
  identified its Debian 12 runtime as end-of-life, making vulnerability data incomplete. A local
  rebuild on supported Debian 13 still found more than a hundred distro findings in userland the
  service never invokes. Rather than waive them broadly, the runtime is now a static musl binary in
  `scratch` with only its CA bundle and console. A refused scan retains its JSON/SBOM and prints only
  vulnerability/package coordinates so the next diagnosis does not depend on runner disk. The exact
  rebuilt image ran as uid/gid 10001, served health 0.16.1 and the secured console, and passed Grype
  0.110.0 at the `negligible` threshold with no exception.
- 2026-08-03 — the scan-clean image deployed and Fly marked the machine healthy, but the post-deploy
  verifier found the same undeclared `rg` dependency before it could record header evidence. The
  visible rollback restored health 0.16.0. Both workflow-invoked verifiers now use portable
  `grep -E`; a repository-wide operational-script search found no remaining `rg` invocation.
- 2026-08-03 — protected-main production run `30833969572` completed every immutable-source,
  repository-gate, build, SBOM, zero-known-vulnerability scan, push, exact-digest deploy and live
  verification step for v0.16.1. Its sole non-expired artifact is retained for 90 days and contains
  the source/image/release evidence, SPDX document and digest sidecar, and Grype report. Source,
  version, image, release and machine references agree; both hashes recompute; the scan has zero
  matches; and an identifier-safe content scan found no credential-shaped value.
