---
id: X-09
title: "A credential store, honest about what protects it"
status: ready
priority: 4
epic: connections
note: "no fallback to memory on a bad store value — a host that fell back would start, serve every route correctly, look exactly like a working one, and lose everything on restart"
---

# A credential store, honest about what protects it

## Goal
Persist credentials behind `SecretStore`, so a restart signs everyone out but leaves every connector
still wired.

## Acceptance
- [ ] A file-backed store: `0600` on the file and `0700` on the directory, set in the `open`/`mkdir`
      call rather than `chmod`-ed afterwards, and **re-checked at startup**.
- [ ] **Failing-first test** — a store whose mode has been widened is **refused, not repaired**. It
      was already exposed; quietly tightening it hides that.
- [ ] Writes are atomic — write a sibling temporary, `fsync`, `rename` — so a crash mid-write leaves
      the previous file whole rather than truncated.
- [ ] **Failing-first test** — a store path inside the repository working directory is refused. One
      `git add -A` from a committed credential is not a risk to leave to attention.
- [ ] A bad store configuration is a **startup error naming what would have worked**. There is no
      fallback to in-memory.
- [ ] `StoreError::NotFound` and `StoreError::Unreachable` never collapse into each other.
- [ ] The startup banner prints the exact path in use, assembled from the store that was actually
      bound so it cannot describe a different one.
- [ ] Whatever protection is *absent* (encryption, a keychain) is stated in the README in the same
      change, not left for a reader to infer.

## Progress
- (not started)

## Notes
- Deleting a credential must rewrite immediately, so a revoked credential does not return on restart.
- If the write is temp-file + rename, then `rm` on the file alone can leave a complete copy in the
  temporary. Document directory removal, not file removal.
