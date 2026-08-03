---
id: X-107
title: "Service Accounts are not Flux Agents"
status: done
priority: 3
epic: apps
areas: [exchange-host, exchange-server, console, docs]
design: docs/designs/service-accounts.md
note: "migrate the legacy agent-token API without invalidating existing callers; reserve Agent for the managed Flux runtime"
---

# Service Accounts are not Flux Agents

## Goal

Expose Exchange's non-human bearer principal as a **Service Account**, while preserving a bounded
compatibility path for tokens and clients created through the legacy `/api/agents` surface. The
**Agent** name then means only a Flux Agent hosted inside an installed App.

## Acceptance

- [x] A design settles route/version compatibility, stored-kind migration, descriptor/console
      rollout and the date or condition that removes the legacy spelling.
- [x] Failing first, a newly minted non-human principal reports `service_account`, never `agent`,
      while an existing stored legacy principal continues to authenticate within its original
      tenant and expiry.
- [x] The primary API and console say Service Account; any legacy route is visibly compatibility
      surface and cannot mint a different kind of authority.
- [x] Grants remain metadata selectors over operations and bound resources. Neither spelling grants
      access to a credential, and only a signed-in human can mint one.
- [x] The anonymous descriptor, public capability page, changelog and migration documentation agree
      with the live routes.

## Progress

- 2026-08-03: Raised by X-106 after the family glossary reserved Agent for model + loop + bounded
  capabilities. The current `/api/agents` principal is explicitly recorded as migration debt.
- 2026-08-03: Accepted the complete resource design: canonical create/list/revoke API, bearer
  authentication, one-minor route and environment compatibility, and v0.17 removal checkpoint.
- 2026-08-03: Delivered the canonical `service_account` kind, human-only create/list/revoke API,
  bearer resolution, console lifecycle management, descriptor v2 and public capability page. X-121
  owns removal of the bounded v0.16 compatibility spellings.

## Notes

- This is a compatibility migration, not a search-and-replace. Existing tokens must not silently
  widen, move tenant or become Managed Agents.
