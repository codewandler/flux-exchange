---
id: X-51
title: "A broken doc link fails the build instead of hiding among twenty others"
status: backlog
areas: [exchange-host, exchange-server]
note: "found by X-48, 2026-08-01: `cargo doc --workspace --no-deps` emits ~20 unresolved intra-doc link warnings and is not in the gate, so a genuinely broken link in new code is invisible"
---

# A broken doc link fails the build instead of hiding among twenty others

## Goal
`cargo doc --workspace --no-deps` is clean, and it is in the gate with `-D warnings`.

## Why this matters here more than in most repositories

This codebase carries an unusual amount of its argument in rustdoc. The safety envelope is explained
in module docs — `no_second_request_path.rs`'s four-mechanism section, `connections.rs`'s address
derivation, `settings.rs`'s template-not-value rule — and those docs link each other heavily. A
review reads them. A resuming agent reads them. **A link that silently resolves to nothing is an
argument that has quietly lost its referent**, and that is worse here than a dead link in a
convenience crate.

X-48 found roughly twenty already, from two habits:

- **Linking to test-function names**, e.g. ``[`tests::a_full_store_refuses…`]``. `cargo doc` does not
  build `#[cfg(test)]` items, so these never resolve.
- **Linking to private modules**, e.g. `bind` → `crate::paths`.

`cargo doc` is not in `AGENTS.md`'s gate and not in CI, so nothing catches any of it. The count is
the problem as much as the links are: twenty existing warnings mean the twenty-first — a real one, in
new code — arrives invisible.

Verified pre-existing rather than introduced: `crates/exchange-host/src/connections.rs:318`'s
``[`writes`]`` is one of them, in a file X-48's diff does not touch.

## Acceptance
- [ ] `cargo doc --workspace --no-deps` emits **zero** warnings.
- [ ] It runs in the gate with `-D warnings`, in `AGENTS.md` and in CI, like every other check here.
- [ ] **Failing-first**: add the check first and watch it go red on the existing twenty, before
      fixing any of them. The point is that the check has teeth, not that the tree is tidy.
- [ ] Links to test functions are resolved the way this repository resolves them elsewhere — naming
      the test in prose rather than linking a `#[cfg(test)]` item, since the link can never work. Do
      not silence them with `#[allow]`.

## Notes
- Cheap and mechanical, which is why it is worth doing before the doc surface grows further rather
  than after.
- Check whether `cargo doc` needs the same MSRV treatment the other jobs get — the gate reads
  `rust-version` from `Cargo.toml` (1.88).
