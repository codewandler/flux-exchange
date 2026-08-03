---
id: X-111
title: "Host every connector runtime through Exchange (epic)"
status: in-progress
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "EPIC — Exchange is the sole official-integration executor: ship the effective catalogue and existing HTTP invoke first, then rich runtimes and lifecycle"
---

# Host every connector runtime through Exchange

## Goal

Every official external integration executes through Exchange. Make that one placement support every
connector runtime—including Docker, Kubernetes, SQL and other rich protocols—while preserving
tenant-derived authority and the rule that a shared process never executes one tenant's local
runtime with another's host identity. There is no local Flux execution placement or local
vendor/plugin fallback.

## Acceptance

- [x] Canonical docs and roadmap say every official integration is a connector executed through
      Exchange and distinguish that destination from today's HTTP-only dispatcher (X-112, X-124).
- [ ] An authenticated effective Service Account catalogue exposes connected and granted operations
      with stable generation identity, beside the existing one-shot HTTP invoke contract (X-113).
- [ ] Exchange consumes one zero-IO connector runtime plan and dispatches every runtime without
      constructing a second vendor request/command/handshake path (X-114).
- [ ] Local single-tenant Exchange executes rich runtimes; hosted multi-tenant placement remains a
      separately gated isolation milestone and both fail closed where absent (X-115, X-116).
- [ ] Streams, cancellation and terminal outcomes survive disconnect and failure in X-117; lease
      ownership and expiry remain X-118. Neither blocks the one-shot HTTP milestone.
- [ ] Exchange installs only connector-owned, attested runtime artifacts and the accumulated
      migration corpus passes through local single-tenant Exchange (X-119, X-120).

## Progress

- 2026-08-03: Filed from the accepted ecosystem runtime axis after auditing current and linked
  worktrees. X-98, X-101…X-105 and delivered X-107 are reused foundations.
- 2026-08-03: X-124 adopted the cross-repository execution decision: Exchange is the sole executor
  for official integrations, the effective catalogue plus existing HTTP invoke is Milestone 1, and
  rich-runtime lifecycle work no longer blocks that path.

## Notes

- Flux counterpart: C-500. flux-connectors counterpart: C-495.
- Flux contributes guarded substrate and an embedded Exchange client, but no second official
  integration placement or fallback.
