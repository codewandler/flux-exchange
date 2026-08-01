---
id: X-32
title: "A console test in a subdirectory is not silently skipped"
status: done
epic: catalogue
areas: [console]
note: "found by X-28's implementor while wiring the console into CI, 2026-08-01: `node --test test/*.test.mjs` matches one directory level, so a test added under test/<subdir>/ never runs and CI stays green"
---

# A console test in a subdirectory is not silently skipped

## Goal
Adding a console test makes it run, wherever it is put.

## What is wrong

`console/package.json`'s test script is `node --test test/*.test.mjs`. That glob matches **one
directory level**. A test added at `test/routing/fragment.test.mjs` is never executed, and both the
local run and CI report success.

Not a defect today — X-28's implementor confirmed all three current files run (components 3 +
routing 5 + service 10 = 18). It is a trap laid for whoever next organises the tests into folders,
and it is exactly the failure mode this repository has spent several stories eliminating elsewhere:
a check that looks like it covers something and does not.

Now that CI runs the console suite (X-28), a silently-skipped test is a silently-green pipeline.

## Acceptance
- [x] **Failing-first test** — a test file placed in a subdirectory of `test/` is executed. Add one
      (it can be trivial) and show it **not** running before the fix and running after; the proof is
      the count changing, so quote both counts.
- [x] The current 18 tests still run — asserted by count, so a fix that changed discovery in some
      other way cannot pass unnoticed.
- [x] `npm test` keeps working with no arguments and needs no new dependency. Node's own test runner
      already has recursive discovery; use it rather than adding a glob library.
- [x] `AGENTS.md`'s console section and any doc naming the test command stay accurate.

## Notes
- Node 22's `--test` discovers recursively when pointed at a directory rather than a glob. Check the
  behaviour on the pinned Node major before relying on it, and say what you verified.
- `console/test/components.test.mjs` is the guard on the 15 carried components and is itself guarded
  by a scanner self-test. Do not disturb it — verify it still runs after any discovery change,
  by name.

## Progress
- **Done 2026-08-01.** Console 18 -> 21 tests; Rust unchanged at 43 + 182.
- **This story's own Note was wrong, and the implementor measured rather than following it.** It
  said Node 22 recurses when pointed at a directory. On the pinned 22.23.1, `node --test test/`
  tries to load `test` as a *module* and dies `MODULE_NOT_FOUND`. Bare `node --test` does recurse
  but collects every `.mjs` under `test/`, so a helper module becomes a test. The working form is
  the quoted glob `'test/**/*.test.mjs'`.
- **The quoting is not cosmetic.** Unquoted, POSIX `sh` collapses `**` to `*` and the suite drops
  from 21 tests to 1 **while still printing `# fail 0`** — the same silent-green shape this story
  exists to remove.
- **A top-level alarm was added beyond the Acceptance and earns its place:** a lone subdirectory
  canary is exactly the defect being fixed, since narrowing the pattern again would stop running it
  silently. The alarm sits where a narrowed pattern still finds it, and was mutation-tested against
  all four spellings.
- **An honest limit the implementor stated:** under the unquoted regression the alarm is itself
  top-level, so it is skipped and cannot fire. No file inside a suite can detect a pattern that
  excludes it — only a **minimum-count assertion outside** the suite would. Worth a story.
- `components.test.mjs`, the guard on the 15 carried components, was verified still running **by
  name**, not merely by count.
