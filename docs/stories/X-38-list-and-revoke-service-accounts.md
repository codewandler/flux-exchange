---
id: X-38
title: "An operator can list and revoke Service Accounts"
status: done
epic: agent-access
design: docs/designs/agent-access.md
areas: [exchange-server]
note: "Retrospective tracking repair: list and revoke landed with X-107 and close the one-way door opened by minting."
---

# An operator can list and revoke Service Accounts

## Goal

An operator can see the live Service Accounts in their tenant and revoke one, so a leaked bearer has
a complete remedy and minting is not a one-way door.

## Acceptance

- [x] The failing-first lifecycle test creates, lists and revokes one Service Account, then proves
      its formerly valid bearer no longer authenticates.
- [x] Listing returns stable id and expiry only. Neither the bearer nor its stored verifier has a
      serializable route representation.
- [x] The published list/create and revoke routes require operator authority, and an anonymous
      caller is refused before learning whether any Service Account exists.
- [x] Revocation makes the formerly valid bearer fail authentication; a second revoke reports the
      same not-found state as an id the tenant never held.
- [x] X-40's store and route tests prove a Service Account cannot mint a successor before its own
      bearer is revoked.

## Progress

- **Done 2026-08-03 as part of X-107.** The missing X-38 file was discovered when closing the parent
  epic; this record restores the intended mint → authenticate → list/revoke story chain.
- Evidence: `canonical_create_list_authenticate_and_revoke_form_one_resource`,
  `a_user_lists_and_revokes_only_its_own_service_accounts`,
  `the_surface_manages_service_accounts_and_refuses_an_anonymous_caller`, and
  `only_a_user_mints_and_no_other_kind_creates_a_successor`.

## Notes

- X-40 is the prerequisite that makes this remedy complete: a Service Account cannot mint a
  successor that survives revocation.
- The current resource contract is [`docs/designs/service-accounts.md`](../designs/service-accounts.md).
