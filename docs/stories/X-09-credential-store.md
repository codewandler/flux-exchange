---
id: X-09
title: "A credential store, honest about what protects it"
status: done
epic: connections
note: "no fallback to memory on a bad store value — a host that fell back would start, serve every route correctly, look exactly like a working one, and lose everything on restart"
---

# A credential store, honest about what protects it

## Goal
Persist credentials behind `SecretStore`, so a restart signs everyone out but leaves every connector
still wired.

## Acceptance
- [x] A file-backed store: `0600` on the file and `0700` on the directory, set in the `open`/`mkdir`
      call rather than `chmod`-ed afterwards, and **re-checked at startup**.
- [x] **Failing-first test** — a store whose mode has been widened is **refused, not repaired**. It
      was already exposed; quietly tightening it hides that.
- [x] Writes are atomic — write a sibling temporary, `fsync`, `rename` — so a crash mid-write leaves
      the previous file whole rather than truncated.
- [x] **Failing-first test** — a store path inside the repository working directory is refused. One
      `git add -A` from a committed credential is not a risk to leave to attention.
- [x] A bad store configuration is a **startup error naming what would have worked**. There is no
      fallback to in-memory.
- [x] `StoreError::NotFound` and `StoreError::Unreachable` never collapse into each other.
- [x] The startup banner prints the exact path in use, assembled from the store that was actually
      bound so it cannot describe a different one.
- [x] Whatever protection is *absent* (encryption, a keychain) is stated in the README in the same
      change, not left for a reader to infer.

## Progress
- **Done.** Merged from `impl/X-09`; gate green on the integration branch after merge.
- **Composed, not reimplemented.** `connector-secrets` 0.8.0 already ships a `FileStore` that sets
  `0600`/`0700` in the create call, refuses a widened mode rather than tightening it, and writes
  through temp + `fsync` + `rename(2)`. This story binds that store; it does not build a second one.
- **Consequence worth stating: two Acceptance items were not proven failing-first.** The mode-refusal
  tests pass at the merge base, because the behaviour is upstream's. They were kept as regression
  guards, and a review confirmed they are live — inserting a `set_permissions` repair before
  `FileStore::open` makes both fail. The genuinely new safety logic here is the working-tree refusal,
  and that is what the base proof targets.
- **A review broke the first attempt.** `resolve` returned an un-canonicalised path when the walk-up
  hit a `..` after a missing directory, so a symlink + `..` spelling bypassed the working-tree guard
  and `FileStore` then created the credential file *inside the checkout*. Fixed by resolving
  downward from the root — `..` is only ever applied to an already-resolved prefix, which is what the
  kernel does — and lexical normalisation was rejected because it is wrong under a symlink.
- **Wired into the binary by X-10.** `bind_configured` + `banner()` were left unbound here
  deliberately; X-10 bound them, and chose *unset means bound to nothing* rather than a startup
  refusal — the connection routes then refuse and name the setting. That keeps this story's "no
  fallback to memory" rule (nothing else is ever selected) without making `cargo run` refuse for a
  reader who only wants `/health`.

## Notes
- Deleting a credential must rewrite immediately, so a revoked credential does not return on restart.
- If the write is temp-file + rename, then `rm` on the file alone can leave a complete copy in the
  temporary. Document directory removal, not file removal.
