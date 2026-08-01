---
id: X-35
title: "Agent access (epic)"
status: ready
priority: 1
epic: agent-access
design: docs/designs/agent-access.md
areas: [exchange-host, exchange-server]
note: "EPIC — the vision's primary caller cannot authenticate. PrincipalKind::Agent exists as a type and appears in the loopback dev roster; nothing mints or verifies an agent's token. Not blocked by X-11, unlike everything else downstream"
---

# Agent access (epic)

## Goal
The caller `docs/vision.md` calls primary can authenticate against a reachable deployment.

## Why this epic exists

The charter's second sentence is a claim the service does not honour: *its primary caller is an
agent, not a human.* Today an agent can become a principal only through the **development identity**
— a roster of handles with no secret, which forces a loopback bind precisely because it is unsafe to
expose. `oidc/mod.rs` even says so: *"Agents carry their own tokens and do not sign in here."* Nothing
mints such a token.

**This is the largest unblocked gap in the platform.** `invoke`, grants and execution records all
wait on X-11's upstream version conflict; agent access needs none of it, because it is principals,
tokens and tenancy — this repository's own domain row.

See [`docs/designs/agent-access.md`](../designs/agent-access.md) for the argument, the three
lifetimes it must not be confused with, and what it deliberately does not do.

## Children
- **X-36** — mint an agent token, shown once, verifier stored.
- **X-37** — an agent token authenticates a request through the existing `Identity` port.
- **X-38** — revoke and list, so minting is not a one-way door.

## Acceptance
- [ ] All three children are `done`.
- [ ] A reachable (non-loopback) deployment can serve an authenticated agent — i.e. agent access does
      **not** depend on the development identity, and the bind rule is satisfied without it.
- [ ] The tenant-derivation vectors in `routes::identity` cover the agent path: no path segment, body
      field or header can influence the tenant an agent's token resolves to.
