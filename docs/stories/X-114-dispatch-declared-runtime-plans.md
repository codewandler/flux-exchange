---
id: X-114
title: "Dispatch declared connector runtime plans through one host seam"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "generalize the admitted/granted invoke chain beyond HttpRequestTool while keeping connector-pack as the only compiled behavior path and runtime caller-immutable"
---

# Dispatch declared connector runtime plans through one host seam

## Goal

Consume flux-connectors' zero-IO runtime plan after the existing runtime and grant gates, then hand it
to a closed runtime registry without teaching Exchange how any vendor request or command is built.

## Acceptance

- [ ] The compiler-enforced admitted → granted chain is required before a runtime plan can dispatch.
- [ ] The runtime registry exhaustively covers `http`, `socket`, `process`, `container`, `plugin` and
      `remote`; a new upstream variant is a compile error.
- [ ] The connector plan, not caller input, fixes runtime, artifact, authority, credential/config
      addresses and lifecycle.
- [ ] `exchange-host` retains no transport and no second request/process/handshake construction path;
      `exchange-server` only binds generic runtime implementations.
- [ ] Failing-first tests exercise a synthetic non-HTTP catalogue operation through invoke and prove
      hardcoding `Http` or bypassing either gate cannot satisfy the dispatch API.

## Progress

- (not started)

## Notes

- Depends on flux-connectors C-504. Read `docs/designs/invoke.md`, “Where the locks stop”, before
  changing dependency or source fences.
