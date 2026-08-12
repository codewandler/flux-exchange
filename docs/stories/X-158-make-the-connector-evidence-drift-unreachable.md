---
id: X-158
title: "A connector bump that leaves the C-515 evidence behind fails in its own pull request"
status: done
priority: 0
areas: [build, release, tests, workflows, docs]
depends_on: [X-157]
note: "X-157 moved the 0.20 pin to 0.23 and unblocked the tag; this closes the hole that let it drift. CI ran the readiness checker's self-test and never pointed it at the tree, and the expected line was written down in two places. The evidence authority is now the single source, ordinary CI applies the real check, and a lock/authority disagreement is a test failure"
---

# A connector bump that leaves the C-515 evidence behind fails in its own pull request

## Goal

X-157 repaired the `0.20.0` pin the `v0.18.0` publish refused on, and the release path is open. This
story removes the reason it could drift unseen for two connector bumps. `native-evidence-v1.json`
binds the inherited C-515 obligations to one published artifact, so a `codewandler-connector-*` bump
that does not re-derive it publishes evidence about a `FileStore` that is not the one shipped —
and, until now, nothing said so until a tag had already spent a version number.

## Acceptance

- [x] The C-515 evidence prose that describes the *pinned* artifact names the 0.23 line. The prose
      that names the provider's own 0.19.1 → 0.20 migration boundary is verified against the
      published source rather than renumbered, and says why it does not move.
- [x] `ci.yml` runs `check-publication-readiness.sh` for real, after — never instead of — its
      self-test, and `check_wiring` refuses a tree where that step is missing.
- [x] The expected connector version and checksum live in one place. The readiness script reads them
      out of the evidence authority; its refusal messages stay as specific as they were, and its
      self-test proves the derivation rather than agreeing with it by coincidence.
- [x] A test in `crates/exchange-release` fails when `Cargo.lock`'s resolved
      `codewandler-connector-secrets` version or checksum disagrees with the authority's pinned
      package, naming both values and the re-derivation procedure. Proved red by mutation.
- [x] `AGENTS.md` carries the procedure, next to the Publishing contract: which fields move, where
      the checksum and released commit come from, and that CI now refuses the mismatch.
- [x] The whole gate is green, including `./scripts/check-publication-readiness.sh` in ordinary
      mode — which is the point of the story.

## Progress

- 2026-08-12: The failing-first proof is a mutation rather than a naturally red test, and
  deliberately so: X-157 had already repaired the drift on `main`, so the guard passes at the merge
  base by construction. It was instead run against a reconstruction of the tree that was actually
  tagged — the authority and its compiled pin put back to `0.20.0` with `Cargo.lock` left at
  `0.23.0` — where it fails naming both versions. A second mutation, version agreeing and checksum
  left behind, proves the checksum assertion separately.
- 2026-08-12: X-157's Notes said not to move this check into the ordinary gate, on the grounds that
  its cost model is release-only. That reason has expired and the note is superseded: the check
  reads files and reaches no network — its own tripwire fails the self-test if it executes `cargo`,
  `curl`, `wget`, `gh`, `rustup`, `npm`, `npx` or `pip` — and the one real reason it stayed off the
  ordinary gate, that ordinary mode refuses until X-138 and X-139 are `done`, no longer holds.

## Notes

- Write set: `crates/exchange-release/native-evidence-v1.json`,
  `crates/exchange-release/tests/upstream_authority_lock.rs`,
  `tests/fixtures/exchange-release-v2/fixture-set.json`,
  `scripts/check-publication-readiness.sh`, `.github/workflows/ci.yml`, `AGENTS.md`, `CHANGELOG.md`.
  No product code, no manifest, no lockfile.
- The authority edit and the fixture rebinding are separate commits because `contract.rs` requires
  `fixture-set.json`'s `exchange_commit` to be an ancestor of `HEAD` and never `HEAD` itself.
- Deliberately *not* collapsed into one source: `native_evidence.rs`'s `validate_upstream_package`
  still restates the four package fields at compile time. That is a second independent statement
  rather than a duplicate — it is what stops the JSON being edited alone — and the same is true of
  `engine_line.rs`'s index-derived checksums. The duplicate this story removed was the one nothing
  ran.
