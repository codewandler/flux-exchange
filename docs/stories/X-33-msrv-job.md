---
id: X-33
title: "CI proves the MSRV the crate promises"
status: ready
priority: 2
epic: serve
areas: [ci]
note: "split from X-30, 2026-08-01: the job could not land while rust-version was false. The number is now 1.88 (observed, not chosen), so the job can be written against a promise that holds"
---

# CI proves the MSRV the crate promises

## Goal
`rust-version` cannot silently become false again.

## Why this is a separate story

X-30 was going to add this job and found the promise it would check was **already false**:
`cargo +1.87 build --workspace --locked` refuses, because `jsonwebtoken`, `time`, `time-core` and
`time-macros` each declare `rust-version = 1.88.0`. Cargo refuses before compiling anything, so 1.87
had never built this tree since X-04 brought `jsonwebtoken` in — **and `rust-version = "1.87"`
shipped in `v0.1.0`, `v0.2.0` and `v0.3.0`.**

Landing a knowingly-red job is what X-28 exists to prevent, so X-30 reported and stopped. The number
was then corrected to `1.88` at integration. This story adds the job that keeps it honest.

## Acceptance
- [ ] A job builds the workspace on the toolchain named by `rust-version`, and **reads that number
      out of `Cargo.toml`** rather than repeating it. A job with the version hardcoded is a fourth
      place holding one number, which is the shape X-27 and X-30 both just removed.
- [ ] **Failing-first evidence** — show the job's build command failing on a toolchain below the
      declared MSRV and passing on it, with real output. That is the whole check, so it must be
      demonstrated rather than asserted.
- [ ] `cargo build --workspace --locked`, not the full gate — an MSRV job answers "does it compile
      for a consumer", not "are the tests green".
- [ ] The action is SHA-pinned with a trailing version comment, and the job runs under
      `permissions: contents: read`. `scripts/check-action-pins.sh` will fail CI otherwise, which is
      the point of X-30.
- [ ] Keep it cheap — it is one build, and it must not become a second full gate.

## Notes
- The toolchain installer already used by both workflows takes a version string, so reading the
  number out of `Cargo.toml` into that input is the whole trick.
- **If the MSRV is ever raised again, it should be because this job went red and someone decided** —
  not because a number was edited to match reality after the fact. Say that in the job's comment.
- `AGENTS.md` § Build / test / run now says "Rust 1.88 or newer — that is the floor `jsonwebtoken`
  and `time` impose, not a number we chose." Keep it true.
