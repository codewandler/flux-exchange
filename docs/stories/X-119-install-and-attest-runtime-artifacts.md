---
id: X-119
title: "Install and attest connector runtime artifacts"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "operators install digest-pinned connector binaries/images; callers select operations only, and activation/rotation is audited without credential-shaped metadata"
---

# Install and attest connector runtime artifacts

## Goal

Give Exchange an operator-controlled inventory of verified connector runtime artifacts shared with
Flux's local installer, with safe activation and rotation for affected channels, streams and leases.

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

## Progress

- (not started)

## Notes

- Depends on flux-connectors C-498 and Flux C-506.
