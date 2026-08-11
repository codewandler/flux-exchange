---
id: X-108
title: "Host installed Flux Apps and Managed Agents"
status: done
priority: 4
epic: apps
areas: [exchange-host, exchange-server, console, docs]
design: docs/designs/installed-apps.md
note: "Immutable packages, atomic frozen authority, durable deliveries, Flux-event projections and the Slack-bot-style console path are live"
---

# Host installed Flux Apps and Managed Agents

## Goal

Make an App a first-class tenant installation and execution boundary. A curated package such as an
`exchange-apps/slack-bot` template declares required connections and optional operation/datasource
requirements; installation freezes the selected connections, grants, risk ceiling, scopes, model
profile and triggers. Exchange then supervises its Managed Agents for chat and event-driven turns.

## Acceptance

- [x] An immutable, versioned App Package carries a Flux Program plus integrity/provenance and
      declares required connector capabilities without carrying tenant values or credentials.
- [x] Failing first, installation refuses a missing required Connection, Model Profile, Operation
      or Datasource and writes no partial binding.
- [x] Installation resolves metadata selectors to a frozen reviewed operation/datasource set;
      package upgrades that widen authority require a new review.
- [x] A Managed Agent receives only its installed App's frozen capabilities. Its runtime token can
      invoke authority but cannot read or address a credential.
- [x] Triggers bind declared Event Types to an installed Journey or Managed Agent through durable
      Event Deliveries, with retry only where effects make retry safe.
- [x] Sessions, Runs and Activity are projected from durable Flux events and remain tenant-scoped.
- [x] The console can install a Slack-bot-style template, choose its Slack Connection and optional
      access layers, configure its model/risk/scope, talk to it and inspect its activation state.

## Progress

- 2026-08-03: Delivered immutable curated packages, atomic tenant installation with frozen reviewed
  authority, opaque runtime capabilities, durable safe-retry deliveries, real Flux App supervision
  and event-derived projections, plus the operator Apps surface and tenant-isolation route proof.
- 2026-08-03: Flux 0.54 publishes `flux-app` and `flux-channels`, connector-pack 0.17 publishes the
  channel planner on that engine line, and X-101 binds the runnable channel seam. The upstream
  publication blocker is gone; the next work is an Exchange design for immutable packages,
  installation authority and tenant-owned durable bindings.
- 2026-08-03: X-106 found the declarations and supporting interfaces in published Flux 0.52.1,
  but `flux-app` and `flux-channels` were omitted from both crates.io and the tagged release script.
  `connector-pack` 0.16 also publishes no channel runner.

## Notes

- `docs/concepts.md` is the vocabulary contract; `docs/designs/released-domain-audit.md` records the
  ownership split and implementation order.
- Webhooks and WebSockets are Channel transports, not Trigger or Event synonyms.
