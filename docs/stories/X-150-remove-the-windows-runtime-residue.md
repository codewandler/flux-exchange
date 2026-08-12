---
id: X-150
title: "Remove the Windows runtime residue the Linux contraction left behind"
status: ready
priority: 2
areas: [exchange-server, exchange-host, tests, build]
note: "X-137 made Exchange Linux-only but left ~9.7k lines of Windows code, including 3,444 lines in three source files no mod declaration compiles and two dangling references that only build.rs's panic keeps hidden"
---

# Remove the Windows runtime residue the Linux contraction left behind

## Goal

Finish what [[X-137]] started: make the tree say what Decision 0012 decided. Exchange is a
Linux-only runtime, and code that no longer compiles anywhere should not still be sitting in
`crates/`.

## Why now

X-137 removed Windows from the *release contract* and from the module graph, but not from the
repository. What is left is not dormant-but-valid code — it is **source no `mod` declaration
reaches**, plus two references to things that do not exist. Measured on `main` at
`4d9cc97`:

| file | lines | `mod` declaration |
|---|---|---|
| `crates/exchange-server/src/local_helper_windows.rs` | 1767 | **none** |
| `crates/exchange-server/src/local_management/windows.rs` | 1611 | **none** |
| `crates/exchange-server/src/windows_handle.rs` | 66 | **none** |

Plus 11 `crates/exchange-server/tests/*windows*` files, and ~9,658 lines in total across everything
matching `crates/**/*windows*`.

**Two of these are outright dangling and would not compile if the platform were reachable:**

- `crates/exchange-server/src/supervisor.rs:899` —
  `#[cfg(windows)] use crate::windows_handle::validate_supervisor_handle …`, but there is no
  `mod windows_handle;` anywhere in the crate.
- `crates/exchange-server/src/local_management/windows.rs:709` and `:749` are gated on
  `feature = "native-fxha-identity-test-seam"`, and
  `crates/exchange-server/tests/local_management_windows_fxha_identity.rs:9` passes that feature to
  Cargo — but **the feature is declared in no `Cargo.toml`**.

Both are invisible today for exactly one reason: `crates/exchange-server/build.rs` panics on any
non-Linux target, so nothing ever tries. That is a build script standing in for a compiler, and it
is the kind of hidden breakage the repository's own `Refuse; never repair` rule exists to surface.
`crates/exchange-server/tests/linux_only_runtime.rs` already asserts `"local_management::windows"`
is a forbidden *string*, which is the intent — this story makes the tree match it.

Owner-confirmed 2026-08-12: **Windows is not needed.**

## Acceptance

- [ ] The three unreferenced source files are deleted, not `#[cfg]`-ed out. A file the module graph
      does not reach is not conditional compilation, it is dead weight that reads as supported.
- [ ] The dangling `windows_handle` import in `supervisor.rs` is gone along with whatever `#[cfg(windows)]`
      branch reached for it, and no `#[cfg(windows)]` arm remains in `crates/exchange-server/src`
      that names a module the crate does not declare.
- [ ] `native-fxha-identity-test-seam` is gone from every source gate and every test argv, or — if
      any of it survives — it is **declared** in `crates/exchange-server/Cargo.toml` with the same
      sentence-explaining-why the other process-only seams carry.
- [ ] Windows-only integration tests are removed with the code they cover. A test whose subject is
      deleted is not evidence.
- [ ] `crates/exchange-host/src/private_fs/windows.rs` is assessed **separately and explicitly**:
      `exchange-host` is the *published* crate, so removing a platform module from it is a
      consumer-visible change and belongs in the changelog as a decision, not a cleanup. Keep it if
      the published crate is still meant to build on Windows; say so in the story either way.
- [ ] Failing first: a test asserts the crate declares no module it does not contain and contains no
      source file the module graph does not reach — so the next contraction cannot leave the same
      residue. `linux_only_runtime.rs` is the natural home.
- [ ] `build.rs`'s non-Linux panic stays. It is the deliberate boundary; this story removes what it
      was accidentally hiding, not the boundary itself.
- [ ] The full gate passes: Rust workspace, console, and public-site builds and tests.
- [ ] The changelog records the removal, and `docs/decisions/0012` is referenced rather than restated.

## Progress

- 2026-08-12: Filed after the residue surfaced during an unrelated branch audit. The two dangling
  references were found by reading, not by a failing build — which is the argument for the
  failing-first test above.

## Notes

- This is **not** the place to reconsider Decision 0012. The decision stands; this is bookkeeping the
  decision implied.
- `story/X-137-fxha-native` must survive this story. The X-137 story preserves that branch head by
  exact SHA (`a92161ed90f620628f1c77e627763557b91e9fa1`) as historical evidence and says it must
  never be merged. Deleting the code here does not delete that record, and the branch is deliberately
  excluded from branch cleanup.
