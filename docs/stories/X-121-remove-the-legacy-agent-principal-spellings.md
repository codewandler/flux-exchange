---
id: X-121
title: "Remove the legacy Agent principal spellings"
status: ready
priority: 3
epic: apps
areas: [exchange-host, exchange-server, console, docs]
design: docs/designs/service-accounts.md
note: "v0.17 compatibility checkpoint; do not invalidate existing bearer tokens"
---

# Remove the legacy Agent principal spellings

## Goal

End the one-minor compatibility window for the former Agent-named Service Account surface. Agent
then names only a hosted Flux Agent, while existing verifier-keyed bearer tokens remain valid until
expiry or revocation.

## Acceptance

- [ ] The workspace version is at least v0.17 and `POST /api/agents` no longer exists.
- [ ] `FLUX_EXCHANGE_AGENTS` is no longer accepted; startup diagnostics name only
      `FLUX_EXCHANGE_SERVICE_ACCOUNTS`.
- [ ] The serialized principal kind accepts only `service_account` after checking that no committed
      durable data still requires the `agent` alias.
- [ ] The console redirect, compatibility documentation and deprecation response tests are removed.
- [ ] Removing the spelling does not invalidate an existing unprefixed token in the unchanged
      verifier-keyed store.

## Notes

- This story removes names, not authority or credentials. It must not rewrite stored tokens.
