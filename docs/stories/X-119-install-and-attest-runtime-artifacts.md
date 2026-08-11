---
id: X-119
title: "Install and attest connector runtime artifacts"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "Exchange installs digest-pinned connector artifacts from the connector/Exchange pipeline; Flux supplies substrate but distributes no official plugin artifact"
---

# Install and attest connector runtime artifacts

## Goal

Exchange installs and executes only attested connector runtime artifacts from an operator-controlled
inventory, with safe activation and rotation for affected channels, streams and leases. Flux
contributes guarded runtime substrate, not a second official-integration execution placement.

## Acceptance

- [ ] Installation verifies flux-connectors' signature/provenance, immutable digest, platform and
      runtime-protocol compatibility; mutable tags and ambient filesystem paths are refused.
- [ ] Only a signed-in operator may install, activate, roll back or remove an artifact; a Service
      Account and operation caller cannot name a version or path.
- [ ] Activation previews affected connections/channels/leases, drains or cancels them safely and
      records artifact identity without environment or credential values.
- [ ] Startup re-verifies installed artifacts and refuses drift rather than repairing or downloading
      silently.
- [ ] Tamper, downgrade, incompatible protocol and partial activation tests fail closed and retain the
      previous usable version where rollback is declared safe.
- [ ] Connector/Exchange release machinery owns executable artifacts; no artifact becomes a Flux
      release output, helper executable, installed pack or fallback path.

## Progress

- (not started)

## Notes

- Depends on flux-connectors C-498. Flux C-506 consumes the migration evidence to remove Flux's
  remaining official plugin distribution infrastructure; it does not install these artifacts.
