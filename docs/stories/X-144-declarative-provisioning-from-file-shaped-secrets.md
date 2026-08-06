---
id: X-144
title: "Declarative provisioning from file-shaped secrets"
pillar: "Core"
status: backlog
epic: hosted-single-org
design: hosted-single-org
note: "Decision 0019 rule 4: connections, settings, grants and tenant Datasource bindings declared in config; secrets only as mounted file references; idempotent startup reconciliation"
---

# Declarative provisioning from file-shaped secrets

## Goal

A deployment declares its connections the way it declares everything else. Today every connection,
setting, instance label and grant arrives through an authenticated operator HTTP call, and the
Decision 0007 boundary deliberately closes every secret-bearing JSON, argv and environment channel
— which also closes the door on a cluster deployment that wants a statically defined connection
and datasource. Decision 0019 rule 4 opens the declarative path without reopening the boundary:
non-secret declarations live in deployment configuration; secret material arrives only as
file-shaped references to deployment-mounted files (the Decision 0008 secret kind), read by
Exchange-owned input during startup reconciliation. The deployment operator owns the pod spec the
way the OS owner owns the local socket.

## Acceptance

- [ ] Declared connections, settings, instance labels, grants and tenant Datasource bindings
      reconcile idempotently at startup against the connector-declared forms; an invalid
      declaration is a startup refusal naming the field, never a partial apply.
- [ ] Secrets are accepted only as file references resolved by Exchange-owned input; an
      environment value, inline JSON value or argv value is refused, proven by all three refusal
      faces.
- [ ] Re-running reconciliation after a mounted-file rotation re-mints the stored credential and
      advances the credential-head revision; unchanged declarations are no-ops.
- [ ] The structural no-second-request-path lock still passes: reconciliation reuses the delivered
      ports, and no new route or request path appears.
