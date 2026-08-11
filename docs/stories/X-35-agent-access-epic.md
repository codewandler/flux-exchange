---
id: X-35
title: "Service Account access (epic)"
status: done
priority: 1
epic: agent-access
design: docs/designs/agent-access.md
areas: [exchange-host, exchange-server]
note: "The durable non-human principal now has complete verifier-only mint, bearer authentication, list and revoke lifecycle under the canonical Service Account vocabulary."
---

# Service Account access (epic)

## Goal
A non-human caller can authenticate as a Service Account against a reachable deployment.

## Why this epic exists

When this epic was filed, the charter's primary non-human caller could become a principal only
through the **development identity** — a roster of handles with no secret, which forces a loopback
bind precisely because it is unsafe to expose. Nothing minted or verified a durable bearer token.

X-36 through X-38 closed that gap. X-107 subsequently gave the resource its canonical name:
**Service Account**. An Agent is the model, authored loop and bounded capabilities hosted by an
installed App; a Service Account is the durable bearer principal that authenticates the caller.
The rename changed no tenant and invalidated no token.

See [`docs/designs/agent-access.md`](../designs/agent-access.md) for the argument, the three
lifetimes it must not be confused with, and the delivery note that maps its original vocabulary to
[`docs/designs/service-accounts.md`](../designs/service-accounts.md).

## Children
- **X-36** — mint a token once and retain only its verifier. **Done.**
- **X-37** — authenticate its bearer at the same guarded identity boundary as every other
  principal. **Done.**
- **X-38** — list and revoke, so minting is not a one-way door. **Done.**
- **X-40** — refuse every non-human successor mint. **Done before X-37 made the bearer path
  reachable**, because revocation is not a remedy if a revoked token's descendants survive it.

## Acceptance
- [x] All three children are `done`.
- [x] A reachable (non-loopback) deployment can serve an authenticated Service Account. Bearer
      authentication does **not** depend on the development identity, and a bound verifier store
      satisfies the bind rule without making the secretless development roster reachable.
- [x] The tenant-derivation vectors cover the Service Account path: no path segment, body field or
      header can influence the tenant its token resolves to.

## Progress

- **Done 2026-08-03.** X-107 delivered the authentication, list and revoke slices while migrating
  the resource to its canonical Service Account name. X-37 and X-38 are retrospective records of
  those already-tested units; they repair the missing story files rather than claiming new runtime
  work.
- `canonical_create_list_authenticate_and_revoke_form_one_resource` proves the complete lifecycle;
  `a_bearer_resolved_by_two_identity_ports_is_refused_as_ambiguous` pins fail-closed composition;
  and `binding_a_service_account_store_admits_a_reachable_bind` proves the non-loopback path without
  weakening the development-identity refusal.
- `a_tenant_in_a_path_segment_does_not_influence_the_tenant_minted_for`,
  `a_tenant_in_a_body_field_does_not_influence_the_tenant_minted_for` and
  `a_tenant_in_a_header_does_not_influence_the_tenant_minted_for` pin every caller-controlled
  tenant vector named by this epic.
