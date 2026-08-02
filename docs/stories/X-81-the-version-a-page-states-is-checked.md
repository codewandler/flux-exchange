---
id: X-81
title: "Four places state this project's version and three of them are wrong"
status: ready
priority: 1
areas: [ci, exchange-host]
note: "found by X-73's implementor, 2026-08-02: lib.rs says v0.7.0, AGENTS.md and README.md say v0.9.0, the manifest says 0.11.0. lib.rs's is the published crate's front-page doc comment, so docs.rs is serving the wrong one — and nothing in the gate compares any of them to the manifest"
---

# Four places state this project's version and three of them are wrong

## Goal
A version stated in prose either matches `[workspace.package].version` or fails the gate.

## The measurement, 2026-08-02

| Where | Says | Actual |
|---|---|---|
| `crates/exchange-host/src/lib.rs:34` | **v0.7.0** | 0.11.0 |
| `AGENTS.md:38` | **v0.9.0** | 0.11.0 |
| `README.md:11` | **v0.9.0** | 0.11.0 |
| `Cargo.toml:6` | 0.11.0 | — |

`lib.rs:34` is the **published crate's front-page doc comment** — the first paragraph on docs.rs. It
has been wrong for four releases, and two paragraphs below it the same comment warns against a claim
outliving what it describes.

## Why a story rather than four edits

Because four edits is what has already been done, twice, and here we are. X-63's implementor found
three stale claims on the README and corrected them; X-30 corrected `rust-version` after it was false
through three releases. Each was a repair, none left anything behind that would catch the next one.

**This repository already has the pattern for the fix**: `scripts/check-crate-versions.sh` compares
`[workspace.package].version` against the `exchange-host` pin in `[workspace.dependencies]`, in CI, at
PR time — because two places holding one number is where a publish first hurts. There are five places
holding this number, and the script knows about two.

## Acceptance
- [ ] **Failing-first test** — a version stated in prose that does not match the manifest fails the
      gate. Watch it fail against the tree as it stands today, which is three failures, then fix all
      three.
- [ ] The check reads the manifest rather than restating the number. A checker with the version
      written into it is the fifth place to be wrong.
- [ ] It runs in CI alongside `check-crate-versions.sh`, and self-tests before it scans — both existing
      checkers run `--self-test` first, following `../flux`, because a checker that has not just proved
      it catches a violation is not evidence there are none.
- [ ] The scanned set is stated, and so is what is deliberately outside it. A CHANGELOG heading names
      a version that was true when written and must **not** be rewritten; so must a sentence like
      *"X-57, v0.9.0+"* that dates a change rather than claiming a current state. Distinguishing those
      from a stale status line is the whole difficulty, and getting it wrong makes the check either
      useless or unbearable.

## Notes
- Found by [[X-73]]'s implementor while reading `lib.rs`, and independently visible in `AGENTS.md`
  during this session's wave.
- Ordered before the next release deliberately: the release commit will correct the three numbers, but
  correcting them is a repair and this story is the thing that stops the fourth.
