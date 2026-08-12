---
id: X-158
title: "Acquisition tests place their credential store outside a working tree"
status: ready
priority: 3
areas: [exchange-server, tests]
note: "connector-secrets 0.23 refuses to bind a credential store inside a git working tree (InsideWorkingTree); the acquisitions route tests' scratch helper uses TMPDIR, so they fail in any dev environment where TMPDIR (bare /tmp, or $HOME/.cache under a dotfiles .git) resolves under a working tree. Green in CI, red on common dev machines"
---

# Acquisition tests place their credential store outside a working tree

## Goal

`routes::acquisitions::tests`' scratch-store helper (`acquisitions.rs:~793`,
`CredentialStore::bind(scratch/state/credentials)`) binds under the process `TMPDIR`. Since
connector-secrets 0.23, `bind` refuses a store whose path is inside a git working tree
(`InsideWorkingTree { root }`). On a developer machine where `TMPDIR` resolves under a working
tree — bare `/tmp` on some setups, or `$HOME/.cache` when `$HOME` holds a dotfiles `.git` — the
five-to-eleven acquisition tests fail with a store-placement panic that has nothing to do with
what they test. They pass in CI (its runner temp is outside any tree) and under `/dev/shm`,
which is what proves the diagnosis.

## Acceptance

- [ ] The helper binds its store in a location guaranteed outside any working tree — an explicit
      scratch root the test controls (not inherited `TMPDIR`), or a documented `/dev/shm`-style
      location, or a marker that opts the scratch tree out of working-tree detection. Whichever:
      the tests pass regardless of where `TMPDIR` points.
- [ ] A note in the test module states the constraint (connector-secrets refuses a store inside a
      working tree) so the next helper does not reintroduce it.
- [ ] The full `cargo test -p flux-exchange` passes with `TMPDIR` set to a path under a working
      tree (the reproduction), proving the fix.

## Progress

- 2026-08-12: Filed during the v0.18.1 release cut, when the local gate's only failures were these
  tests under a working-tree TMPDIR. CI green (X-157 PR), /dev/shm green (11/11) — environmental,
  not a code regression, so it did not block the release; but it fails on common dev machines.

## Notes

- Write set: `crates/exchange-server/src/routes/acquisitions.rs` (test module only). No product code.
