---
id: X-127
title: "Persist local Exchange state safely on every Flux platform"
status: ready
priority: 0
epic: remote-deployment
areas: [ci, exchange-host, exchange-server, persistence, windows]
note: "Milestone 1 — a five-target binary is support only when every credential-bearing local workflow persists safely and survives restart on that target"
---

# Persist local Exchange state safely on every Flux platform

## Goal

Make the local Exchange composition genuinely runnable on all five Flux platforms, not merely
cross-compilable. Credentials, settings, grants, labelled connections and Service Accounts remain
durable and owner-only on Windows as well as Unix, with every unsafe or unavailable store refusing
instead of falling back to memory.

## Why this is Milestone 1 work

X-126 proposes a Windows release asset while the file-backed composition and its permission proof
are Unix-only. A binary that compiles for Windows but cannot bind the stores needed to connect,
grant and invoke is not a supported local Exchange. Replacing the missing binding with an in-memory
store would be worse: startup would look healthy, the workflow could appear to succeed, and every
credential and grant would disappear on restart.

Decision 0004 therefore defines platform support by a native persisted workflow, not by a compiler
target or `/health` response. Windows needs the same fail-closed owner-only property that Unix
currently states with `0700` directories and `0600` files, expressed and tested through Windows
ownership and DACLs rather than emulated mode bits.

## Acceptance

- [ ] The Exchange server and the complete local storage composition compile for the exact closed
      Flux target set: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
      `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu` and
      `x86_64-pc-windows-msvc`. Native Linux, macOS and Windows jobs exercise their platform
      bindings; a cross-check alone, a skipped store module or a server that only answers health is
      not evidence of support.
- [ ] One portable composition binds the existing credential, connection-settings, grant,
      connection-registry and Service Account ports to explicit durable paths. It preserves each
      store's current address, tenant, atomic-replacement, bounded-write and refusal semantics; no
      route or connector runtime learns a platform-specific storage API.
- [ ] On Unix, newly created state roots are owner-only `0700` directories and state files are
      owner-only `0600` files. An existing object with a different owner, wider mode, wrong object
      kind or uninspectable metadata refuses without chmod, replacement or another repair.
- [ ] On Windows, the state root and every credential, settings, grant, connection-registry and
      Service Account file are owned by the SID of the process identity and carry a protected DACL
      that grants access only to that SID. Inherited allow entries, an allow entry for another SID,
      a mismatched owner, reparse point, wrong object kind or unreadable security descriptor refuses
      before any state is read or written. Exchange never silently rewrites or narrows an unsafe
      DACL.
- [ ] **Failing-first permission fixtures:** widening one Unix mode and, on a native Windows runner,
      planting each of a broad DACL, inherited allow entry and foreign owner makes startup refuse
      with the affected store/path and no value. Restoring the owner-only metadata is the only path
      to a successful reopen; the test proves the refusal did not modify the planted metadata.
- [ ] A requested persistent local composition is all-or-nothing. A missing, denied, malformed or
      unsafe credential, settings, grant, connection-registry or Service Account path refuses
      startup and names the store. No production or supervised-local branch substitutes a memory
      store, creates an unprotected sibling file, or continues with only the stores that opened.
- [ ] **Native Windows restart proof:** from a clean owner-only state root, start the real server,
      create a labelled connection with every required credential and setting, write a grant, mint
      a Service Account, and invoke a harmless released fixture successfully. Stop the process,
      start a new process over the same root, authenticate with the existing Service Account and
      invoke through the same labelled connection and grant again. The proof inspects only
      value-free state and also demonstrates that no secret entered stdout, stderr or a response.
- [ ] Equivalent native restart coverage runs on one Unix target, while format fixtures prove that
      platform permission metadata does not change the logical store schema. Documentation names
      the five supported targets and their owner-only policy without claiming that Windows ACLs are
      Unix modes or that administrator access is application encryption.

## Progress

- 2026-08-04: Filed from cross-repository Decision 0004 after the local-release audit found that
  X-126 named a Windows artifact before the credential-bearing server composition could safely run
  and persist there.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0004-flux-manages-a-verified-local-exchange.md` at the supervision
  amendment (`71fea6c`).
- X-126 depends on this story before any target may enter the signed release manifest. A target
  compiling is necessary and deliberately insufficient.
- Reuse the existing ports and file formats. This is a portable binding and permission contract,
  not a second persistence model and not permission to expose a credential value for migration.
- X-97 may later select a managed credential backend for a public deployment. It does not replace
  the owner-only local file contract needed by a separately managed desktop/local process.
