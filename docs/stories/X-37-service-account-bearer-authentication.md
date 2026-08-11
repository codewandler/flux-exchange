---
id: X-37
title: "A Service Account bearer authenticates at the guarded identity boundary"
status: done
epic: agent-access
design: docs/designs/agent-access.md
areas: [exchange-server]
note: "Retrospective tracking repair: the bearer-authentication slice landed with X-107 under the canonical Service Account vocabulary."
---

# A Service Account bearer authenticates at the guarded identity boundary

## Goal

A durable non-human bearer resolves to its Service Account principal through the same central route
guard every authenticated surface uses, with its tenant fixed by the stored verifier record.

## Acceptance

- [x] The failing-first lifecycle test mints a bearer, presents it to `/api/session`, and receives
      the stored `service_account` principal before revocation makes the same bearer return `401`.
- [x] Service Account resolution is part of the central `require_principal` boundary rather than a
      parallel endpoint mechanism, so every guarded route receives the same `Principal` extension.
- [x] A bearer that resolves through both configured verifier sources is refused as ambiguous rather
      than admitted according to provider order, and the refusal reveals no token.
- [x] Binding the Service Account verifier store admits a reachable listener without OIDC or the
      development roster; adding that store never makes the secretless development roster safe for
      a reachable bind.
- [x] The resolved tenant and expiry come from the durable verifier record. No path, body or header
      field can rename the tenant, and an expired bearer resolves to no principal.

## Progress

- **Done 2026-08-03 as part of X-107.** The missing X-37 file was discovered when closing the parent
  epic; this record names work already delivered and tested rather than introducing a second
  implementation.
- Evidence: `canonical_create_list_authenticate_and_revoke_form_one_resource`,
  `a_bearer_resolved_by_two_identity_ports_is_refused_as_ambiguous`,
  `binding_a_service_account_store_admits_a_reachable_bind`,
  `a_token_stops_resolving_when_its_stated_expiry_passes`, and the three tenant-vector tests in
  `routes::service_accounts`.

## Notes

- X-107 reserved Agent for model + loop + bounded capabilities. The bearer principal this story
  originally called an Agent is canonically a Service Account; see
  [`docs/designs/service-accounts.md`](../designs/service-accounts.md).
