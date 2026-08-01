---
id: X-28
title: "The gate runs on every push, not only at a release"
status: done
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
- [x] `.github/workflows/ci.yml` runs on push and on pull request, and runs the **whole** gate as
      `AGENTS.md` § Build / test / run states it: `cargo build --workspace`,
      `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo fmt --all -- --check`.
- [x] **The console is built and tested too** — `cd console && npm install && npm test &&
      npm run build`. It is a separate Node build; give it its own job rather than bolting it onto
      the Rust one.
- [x] Actions are **pinned by commit SHA** with the version in a trailing comment, matching
      `crates-io.yml` and the sibling repositories. An unpinned action is a supply-chain hole.
- [x] `permissions:` is least-privilege — `contents: read` unless something genuinely needs more.
- [x] The toolchain matches `crates-io.yml`'s pin, so a green CI run and a green release build are
      the same cargo. If they must differ, say why in a comment.
- [x] `crates-io.yml` still gates before publishing. **Do not delete its gate** on the grounds that
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

## Progress
- **Done 2026-08-01.** Local gate green: 43 + 171 Rust tests, clippy, fmt, and the console's
  `npm ci && npm test && npm run build` (18 tests, build clean) — every command the workflow runs,
  executed locally with output.
- **No failing-first test was invented**, and the implementor said so plainly. The base state is a
  fact rather than a test result: one workflow at `9ec774d`, triggered on a version tag only.
- **`actionlint` was proved to be a real check before being used as evidence** — run against a
  deliberately corrupted copy (`reff`, `ubunut-latest`), which it caught, then against the committed
  file, which is clean. That is the nearest thing to a failing-first proof a workflow admits.
- **`crates-io.yml` keeps its inline gate.** A tag can be pushed at a commit no CI run ever covered
  — an unmerged branch, a run cancelled by the next push, a window where Actions were disabled — so
  deferring would mean trusting a run that may not exist, on the one irreversible path.
- `npm ci` rather than `npm install`, following the sibling's argued convention: it installs the
  committed lockfile and fails when lockfile and `package.json` disagree.
- `concurrency` cancels in-progress runs on **pull requests only**, never on `main`, so no `main`
  commit is left without a completed run.
- **The workflow had never executed when this was written.** What local runs could not cover: that
  the pinned SHAs resolve upstream, `rust-cache` on a cold cache, `setup-node`'s npm cache, and the
  `cancel-in-progress` expression at runtime.
- **Filed as adjacent, not done here:** `../flux` has an `action-pins` job that fails CI when an
  unpinned action reappears, and a `crate-versions` job checking versions at PR time rather than at
  release time — the same "too late" shape X-28 exists to fix. The MSRV (`rust-version = 1.87`) is
  still a promise to consumers that nothing verifies, since CI and release both build on 1.97.0.
