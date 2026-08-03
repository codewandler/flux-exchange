---
id: X-120
title: "Prove rich connectors locally and through Exchange"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "release gate runs the shared connector corpus across HTTP, plugin/process, socket and container placements, including isolation, streams, leases and credential non-disclosure"
---

# Prove rich connectors end to end

## Goal

Demonstrate that the migration is a usable system rather than independent seams by running the same
connector contracts through local Flux, single-tenant Exchange and isolated multi-tenant Exchange.

## Acceptance

- [ ] The flux-connectors C-505 corpus runs representative HTTP, plugin/process, socket and container
      connectors against local and hosted placements with the same operation/event contracts.
- [ ] Docker or Kubernetes proves infrastructure isolation and streaming; SQL proves lease cleanup;
      one observability connector proves a long-running read; collaboration proves ordinary parity.
- [ ] Tests cover Service Account auth, metadata grants, tenant-derived connection/settings,
      cancellation, disconnect, rotation and artifact update without exposing a credential value.
- [ ] Unsupported topology is a tested named refusal and no test silently skips a runtime because a
      backend is absent.
- [ ] Public capability truth and release documentation distinguish what shipped from remaining
      connector migration waves, and Flux plugin retirement consumes this evidence.

## Progress

- (not started)
