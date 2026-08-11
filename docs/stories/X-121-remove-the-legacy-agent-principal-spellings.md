---
id: X-121
title: "Remove the legacy Agent principal spellings"
status: done
priority: 3
epic: apps
areas: [exchange-host, exchange-server, console, docs]
design: docs/designs/service-accounts.md
note: "v0.17 compatibility checkpoint; existing verifier-keyed bearer tokens remain valid"
---

# Remove the legacy Agent principal spellings

## Goal

End the one-minor compatibility window for the former Agent-named Service Account surface. Agent
then names only a hosted Flux Agent, while existing verifier-keyed bearer tokens remain valid until
expiry or revocation.

## Acceptance

- [x] The workspace version is at least v0.17 and `POST /api/agents` no longer exists.
- [x] `FLUX_EXCHANGE_AGENTS` is no longer accepted; startup diagnostics name only
      `FLUX_EXCHANGE_SERVICE_ACCOUNTS`.
- [x] The serialized principal kind accepts only `service_account` after checking that no committed
      durable data still requires the `agent` alias.
- [x] The console redirect, compatibility documentation and deprecation response tests are removed.
- [x] Removing the spelling does not invalidate an existing unprefixed token in the unchanged
      verifier-keyed store.

## Notes

- This story removes names, not authority or credentials. It must not rewrite stored tokens.

## Progress

- 2026-08-03 — failing-first tests proved all four v0.16 aliases were still live. The API route,
  environment setting, serialized principal alias and console redirect are now removed; the
  canonical route/store spelling remains, and the existing reopen fixture proves an unprefixed
  verifier-keyed token still resolves as a Service Account. The v0.17 workspace version, complete
  Rust gate, 130-test console suite and production console build now pass.
