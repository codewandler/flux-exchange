---
id: X-116
title: "Isolate rich connector runtimes per tenant"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "a shared Exchange delegates local runtimes to operator-selected OS/container/pod workers; absent isolation refuses before credential access"
---

# Isolate rich connector runtimes per tenant

## Goal

Allow a multi-tenant Exchange to host socket/process/container/plugin connectors only by delegating
them to an authenticated, operator-controlled per-tenant isolation boundary.

## Acceptance

- [ ] A placement resolver derives the worker from deployment configuration and tenant identity; no
      request can name or override it.
- [ ] Worker authentication, tenant binding, runtime/artifact allowlist and encrypted transport are
      checked before an operation-bound credential may enter worker memory.
- [ ] The worker never persists/logs credentials or returns them; disconnect and crash revoke the
      operation/lease and leave bounded diagnostics.
- [ ] Missing, unhealthy or mismatched isolation is a named refusal before credential lookup; there
      is no fallback to executing in the control-plane process.
- [ ] Adversarial tests prove cross-tenant worker reuse, artifact substitution and caller-selected
      placement fail closed, and distinguish worker-reported from locally observed evidence.

## Progress

- (not started)

## Notes

- Flux C-399 supplies the generic remote guarded-IO seam; C-397 supplies container placement.
