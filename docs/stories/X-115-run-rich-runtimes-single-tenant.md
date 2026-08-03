---
id: X-115
title: "Run every connector runtime in a single-tenant Exchange"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "Exchange binds Flux's guarded substrate in --dev and one-team deployments; it remains the sole official-integration execution placement"
---

# Run rich runtimes single-tenant

## Goal

Exchange binds Flux's guarded substrate to make `Deployment::SingleTenant`'s existing admission
promise real for each connector runtime. Flux contributes guarded runtime substrate, not a second official-integration execution placement.

## Acceptance

- [ ] HTTP, guarded socket, argv-only process, container backend and stdio plugin plans execute through
      Exchange using their shared Flux mechanisms; `remote` delegates through the same Exchange-owned
      port contract.
- [ ] Runtime artifacts are operator-installed and digest-addressed; endpoints and credentials resolve
      from the tenant connection, never ambient environment or caller parameters.
- [ ] Process environment, filesystem, output, network and cancellation limits match Flux's substrate
      contract and are recorded as Exchange-observed evidence.
- [ ] Failing-first journeys exercise representative plugin/process, socket and container connectors
      under `--dev`, including refusal when a required backend or artifact is absent.
- [ ] Enabling single-tenant runtime bindings changes no multi-tenant admission result.
- [ ] Missing substrate, backend or artifact refuses at Exchange without any local integration
      fallback in Flux.

## Progress

- (not started)

## Notes

- Depends on X-114, X-119, Flux C-397/C-435 and flux-connectors C-498/C-504.
