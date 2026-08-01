---
id: X-28
title: "The gate runs on every push, not only at a release"
status: ready
priority: 1
epic: serve
areas: [ci]
note: "the crates.io workflow runs the gate inline because there is no ci.yml at all — so a red main is only discovered when someone tries to release, and the console's Node build is never run by CI"
---

# The gate runs on every push, not only at a release

## Goal
A change that breaks the gate is caught when it lands, not when someone tries to cut a release.

## What is missing

This repository has **one** workflow, `crates-io.yml`, and it fires on a version tag. It runs the
gate inline — deliberately, because publishing an artifact nobody tested is the worst thing to make
permanent — but that means:

- **A red `main` is invisible until release time.** Every merge between tags is unverified by CI.
- **The console is never built by CI at all.** `console/` is a separate Node build that does not
  participate in the Cargo workspace, and `npm test` / `npm run build` have never run outside a
  developer's machine.
- The release workflow carries a gate that is really CI's job, and says so in its own comment.

Both sibling repositories — `../flux` and `../flux-connectors` — have a `ci.yml`. This one does not.

## Acceptance
- [ ] `.github/workflows/ci.yml` runs on push and on pull request, and runs the **whole** gate as
      `AGENTS.md` § Build / test / run states it: `cargo build --workspace`,
      `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo fmt --all -- --check`.
- [ ] **The console is built and tested too** — `cd console && npm install && npm test &&
      npm run build`. It is a separate Node build; give it its own job rather than bolting it onto
      the Rust one.
- [ ] Actions are **pinned by commit SHA** with the version in a trailing comment, matching
      `crates-io.yml` and the sibling repositories. An unpinned action is a supply-chain hole.
- [ ] `permissions:` is least-privilege — `contents: read` unless something genuinely needs more.
- [ ] The toolchain matches `crates-io.yml`'s pin, so a green CI run and a green release build are
      the same cargo. If they must differ, say why in a comment.
- [ ] `crates-io.yml` still gates before publishing. **Do not delete its gate** on the grounds that
      CI now covers it — a tag can be pushed at a commit CI never ran, and the release is the
      irreversible one. If you make it defer, the deferral must be airtight and argued.

## Notes
- Follow `../flux-connectors/.github/workflows/ci.yml` for shape and for the caching strategy; do
  not invent a different one. Read it before writing.
- Rust `rust-version` is `1.87` (the MSRV a consumer needs) while the workflows pin `1.97.0` (what
  we build with). Those are different numbers for different reasons — if CI should also prove the
  MSRV builds, that is a separate job and worth proposing, not assuming.
- Keep it fast enough that people do not learn to ignore it. Cache the cargo registry and the target
  directory the way the sibling does.
