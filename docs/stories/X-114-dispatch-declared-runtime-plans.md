---
id: X-114
title: "Dispatch declared connector runtime plans through one host seam"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "Exchange dispatches connector-declared plans after admission and grants; Flux supplies guarded substrate but no second official execution placement"
---

# Dispatch declared connector runtime plans through one host seam

## Goal

Exchange dispatches the connector-declared runtime plan after the existing runtime and grant gates,
through a closed registry, without teaching Exchange how any vendor request or command is built.
Flux contributes guarded runtime substrate, not a second official-integration execution placement.

## Acceptance

- [ ] The compiler-enforced admitted → granted chain is required before a runtime plan can dispatch.
- [ ] The runtime registry exhaustively covers `http`, `socket`, `process`, `container`, `plugin` and
      `remote`; a new upstream variant is a compile error.
- [ ] The connector plan, not caller input, fixes runtime, artifact, authority, credential/config
      addresses and lifecycle.
- [ ] `exchange-host` retains no transport and no second request/process/handshake construction path;
      `exchange-server` only binds generic runtime implementations.
- [ ] Every official operation reaches the registry through Exchange; an unavailable binding is a
      named refusal and never falls back to a local Flux or vendor/plugin execution path.
- [ ] Failing-first tests exercise a synthetic non-HTTP catalogue operation through invoke and prove
      hardcoding `Http` or bypassing either gate cannot satisfy the dispatch API.

## Progress

- (not started)

## Notes

- Depends on X-113 and flux-connectors C-504. Read `docs/designs/invoke.md`, “Where the locks stop”,
  before changing dependency or source fences.
