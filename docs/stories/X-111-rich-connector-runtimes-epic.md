---
id: X-111
title: "Host every connector runtime through Exchange (epic)"
status: in-progress
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "EPIC — HTTP is the first runtime, not the boundary: host socket/process/container/plugin connectors through single-tenant or isolated remote placement, with streams and leases"
---

# Host every connector runtime through Exchange

## Goal

Make Exchange the secure hosted placement for every connector runtime—including Docker, Kubernetes,
SQL and other rich protocols—while preserving tenant-derived authority, local-first Flux and the rule
that a shared process never executes one tenant's local runtime with another's host identity.

## Acceptance

- [ ] Canonical docs and roadmap say all integrations become connectors and distinguish the accepted
      destination from today's HTTP operation dispatcher (X-112).
- [ ] A stable remote connector protocol covers one-shot invoke, subscribe, streamed results,
      cancellation and lease lifecycle under Service Account authentication (X-113).
- [ ] The host consumes one zero-IO connector runtime plan and dispatches every runtime without
      constructing a second vendor request/command/handshake path (X-114).
- [ ] Single-tenant execution and per-tenant isolated multi-tenant placement are implemented and
      fail closed where absent (X-115, X-116).
- [ ] Stream and lease behavior survive disconnect, cancellation, expiry and runtime failure without
      leaking credentials or unbounded payloads (X-117, X-118).
- [ ] Only operator-installed, attested runtime artifacts execute, and local/hosted conformance proves
      representative HTTP, plugin/process, socket and container connectors (X-119, X-120).

## Progress

- 2026-08-03: Filed from the accepted ecosystem runtime axis after auditing current and linked
  worktrees. X-98, X-101…X-105 and delivered X-107 are reused foundations.

## Notes

- Flux counterpart: C-500. flux-connectors counterpart: C-495.
