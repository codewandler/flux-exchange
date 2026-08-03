---
id: X-120
title: "Prove migrated rich connectors through local Exchange"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "release gate accumulates HTTP, plugin/process, socket and container migration fixtures through local single-tenant Exchange; hosted isolation remains X-116"
---

# Prove rich connectors end to end

## Goal

Demonstrate that the migration is a usable system rather than independent seams by running the
accumulated migration corpus through local single-tenant Exchange. Hosted multi-tenant isolation
remains X-116 and does not block this proof.

## Acceptance

- [ ] The flux-connectors C-505 corpus accumulates each migrated HTTP, plugin/process, socket and
      container connector and runs it through local single-tenant Exchange with the same declared
      operation/event contracts.
- [ ] Docker or Kubernetes proves guarded infrastructure execution and streaming; SQL proves lease
      cleanup; one observability connector proves a long-running read; collaboration proves ordinary
      one-shot parity.
- [ ] Tests cover Service Account auth, metadata grants, tenant-derived connection/settings,
      cancellation, disconnect, rotation and artifact update without exposing a credential value.
- [ ] Unsupported topology is a tested named refusal and no test silently skips a runtime because a
      backend is absent.
- [ ] Public capability truth and release documentation distinguish what shipped from remaining
      connector migration waves, and Flux plugin retirement consumes this evidence.
- [ ] The corpus neither requires X-116's hosted worker-isolation proof nor offers a second execution
      placement when Exchange is unavailable.

## Progress

- (not started)

## Notes

- Depends on X-115, X-117, X-118, X-119 and flux-connectors C-505. X-116 remains the separate
  hosted multi-tenant isolation milestone.
