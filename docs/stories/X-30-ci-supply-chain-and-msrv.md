---
id: X-30
title: "CI proves the pins and the MSRV, not just the tests"
status: ready
priority: 2
epic: serve
areas: [ci]
note: "found by X-28's implementor, 2026-08-01: ../flux fails CI when an unpinned action reappears and checks crate versions at PR time; this repo enforces both by review only, and `rust-version = 1.87` is a promise to consumers that nothing verifies"
---

# CI proves the pins and the MSRV, not just the tests

## Goal
The three promises this repository makes outside its test suite are checked by machine.

## What is unproven

X-28 gave this repository CI, and its implementor immediately listed what CI still does not cover.
Each is a claim we currently make on trust:

1. **Every action is pinned by commit SHA.** Both workflows do pin, and `AGENTS.md` treats it as an
   invariant — but nothing fails when someone adds `uses: foo/bar@v3`. `../flux` has an
   `action-pins` job running `scripts/check-action-pins.sh --self-test`, so the check is itself
   checked. Here it is enforced by review only, which is to say by whoever happens to look.
2. **`rust-version = 1.87` is the MSRV.** It is a promise to every consumer of
   `codewandler-flux-exchange-host`, and **both** CI and the release build on 1.97.0. Nothing has
   ever compiled this workspace on 1.87. The promise may already be false.
3. **The tag matches the workspace version.** Checked inside `crates-io.yml`, i.e. at release time —
   which is the same "too late" shape X-28 exists to fix. `../flux` checks it at PR time with
   `scripts/check-crate-versions.sh`.

## Acceptance
- [ ] An `action-pins` job fails CI when any workflow references an action by tag or branch rather
      than by 40-hex SHA. **The checker self-tests** — it must prove it catches an unpinned
      reference before it is trusted to say there are none, following `../flux`'s
      `--self-test` precedent.
- [ ] An **MSRV job** builds the workspace on the toolchain named by `rust-version`. If it does not
      compile on 1.87, that is a finding to report, not a number to quietly raise — say so and stop,
      because raising the MSRV is a decision about consumers.
- [ ] A **version-consistency check at PR time**: `[workspace.package].version` and the
      `exchange-host` pin in `[workspace.dependencies]` agree. Those are two places holding one
      number and a release is where they hurt.
- [ ] Every new action is itself SHA-pinned, and the new jobs run under `permissions: contents: read`.
- [ ] `crates-io.yml`'s existing tag-vs-version check stays. A PR-time check does not cover a tag
      pushed at a commit no PR touched.

## Notes
- **Read `../flux/scripts/check-action-pins.sh` and `../flux/scripts/check-crate-versions.sh` first**
  and follow them rather than inventing. The family already solved this twice.
- X-28's implementor noted a trap worth inheriting: a comment containing the literal `uses:` keyword
  will trip a naive grep-based scanner. Whatever you write must not be fooled by its own repository's
  comments — and the self-test is where you prove that.
- Keep the jobs cheap. A pin check is a grep; an MSRV build is a `cargo build`, not the full gate.
