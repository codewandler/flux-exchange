---
id: X-30
title: "CI proves the pins and the MSRV, not just the tests"
status: done
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
- [x] An `action-pins` job fails CI when any workflow references an action by tag or branch rather
      than by 40-hex SHA. **The checker self-tests** — it must prove it catches an unpinned
      reference before it is trusted to say there are none, following `../flux`'s
      `--self-test` precedent.
- [ ] An **MSRV job** builds the workspace on the toolchain named by `rust-version`. If it does not
      compile on 1.87, that is a finding to report, not a number to quietly raise — say so and stop,
      because raising the MSRV is a decision about consumers.
- [x] A **version-consistency check at PR time**: `[workspace.package].version` and the
      `exchange-host` pin in `[workspace.dependencies]` agree. Those are two places holding one
      number and a release is where they hurt.
- [x] Every new action is itself SHA-pinned, and the new jobs run under `permissions: contents: read`.
- [x] `crates-io.yml`'s existing tag-vs-version check stays. A PR-time check does not cover a tag
      pushed at a commit no PR touched.

## Notes
- **Read `../flux/scripts/check-action-pins.sh` and `../flux/scripts/check-crate-versions.sh` first**
  and follow them rather than inventing. The family already solved this twice.
- X-28's implementor noted a trap worth inheriting: a comment containing the literal `uses:` keyword
  will trip a naive grep-based scanner. Whatever you write must not be fooled by its own repository's
  comments — and the self-test is where you prove that.
- Keep the jobs cheap. A pin check is a grep; an MSRV build is a `cargo build`, not the full gate.

## Progress
- **Done 2026-08-01**, except the MSRV job — split to [X-33](X-33-msrv-job.md), because it could not
  land green. Gate: 43 + 178 tests, clippy clean, fmt clean.
- **THE HEADLINE: `rust-version = "1.87"` was false, and shipped in three published versions.**
  `cargo +1.87 build --workspace --locked` refuses — `jsonwebtoken@10.4.0`, `time@0.3.54`,
  `time-core@0.1.9` and `time-macros@0.2.32` each declare `rust-version = 1.88.0`. Cargo refuses
  before compiling anything, so **1.87 has never built this tree since X-04 brought `jsonwebtoken`
  in, on the same day**. Verified independently by the coordinator before acting.
- **The implementor reported it and stopped rather than raising the number**, exactly as the story
  asked. Raising an MSRV is a decision about consumers, not a way to make a job green.
- **Resolved at integration: `rust-version` is now `1.88`.** The MSRV is *observed*, not chosen —
  the alternative was pinning `jsonwebtoken` and `time` backwards, which would downgrade the JOSE
  library doing signature verification to preserve a number nobody had ever verified.
- **Both checkers self-test before they scan**, so a checker that has stopped catching violations
  cannot report there are none. The pin scanner goes beyond `../flux`'s line-wise grep: an awk
  classifier tracks block-scalar indentation and drops comments, because flux's own error hint
  contains an example pin one paste away from a workflow. Both decoys — a commented-out reference
  and a real unpinned step — were proved against the real tree, not just in the self-test.
- **Carried forward:** `crates-io.yml` reads the workspace version with `grep -m1 '^version = '` over
  the whole of `Cargo.toml`, correct today only because `[workspace.package]` happens to come first.
  `check-crate-versions.sh`'s section-scoped reader could replace it.
