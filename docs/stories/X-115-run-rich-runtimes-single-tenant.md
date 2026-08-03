---
id: X-115
title: "Run every connector runtime in a single-tenant Exchange"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "bind Flux's guarded http/socket/process/container/plugin mechanisms in --dev and one-team deployments; no ambient path, host or credential fallback"
---

# Run rich runtimes single-tenant

## Goal

Make `Deployment::SingleTenant`'s existing admission promise real by binding each connector runtime to
Flux's guarded system in the Exchange server composition.

## Acceptance

- [ ] HTTP, guarded socket, argv-only process, container backend and stdio plugin plans execute through
      their shared Flux mechanisms; `remote` delegates through the same port contract.
- [ ] Runtime artifacts are operator-installed and digest-addressed; endpoints and credentials resolve
      from the tenant connection, never ambient environment or caller parameters.
- [ ] Process environment, filesystem, output, network and cancellation limits match Flux's substrate
      contract and are recorded as Exchange-observed evidence.
- [ ] Failing-first journeys exercise representative plugin/process, socket and container connectors
      under `--dev`, including refusal when a required backend or artifact is absent.
- [ ] Enabling single-tenant runtime bindings changes no multi-tenant admission result.

## Progress

- (not started)

## Notes

- Depends on Flux C-397/C-435/C-502 and flux-connectors C-498/C-504.
