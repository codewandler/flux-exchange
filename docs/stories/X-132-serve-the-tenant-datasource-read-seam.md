---
id: X-132
title: "Serve the tenant Datasource read seam"
status: backlog
epic: apps
areas: [exchange-host, exchange-server]
design: docs/designs/installed-apps.md
note: "Decision 0006, Milestones 2–3 — schema/list/get for a bound Datasource execute as admitted operations; Exchange-minted opaque cursors, grants stay metadata selectors"
---

# Serve the tenant Datasource read seam

## Goal

Serve schema, list and get for a bound tenant Datasource through the machinery Exchange already
trusts: every datasource read executes as an admitted operation through the existing
Invoker/admission gate, because a connector datasource member is a projection over that connector's
declared operations. Exchange owns tenant authorization and connection resolution, never retrieval
semantics — it constructs no vendor request of its own.

## Acceptance

- [ ] Failing first, schema/list/get for a bound Datasource dispatch only through the existing
      Invoker and admission gate as the member's bound operations; a second request-building or
      retrieval path is refused by the same structural guards that protect invoke.
- [ ] Cursors are Exchange-minted opaque continuation tokens. A failing-first test proves a cursor
      carries no credential material, no vendor endpoint and no tenant-crossing state, and that a
      cursor from one tenant or Datasource is refused on another.
- [ ] Grants remain metadata selectors over the member's backing operations; an operation-id list
      is still refused, and no new grant machinery is introduced for reads.
- [ ] A read against a Datasource whose connection, grant or frozen App binding is missing refuses
      with a named cause before any request leaves the process; tenant derivation still comes
      solely from the resolved principal.
- [ ] v1 is one-shot list/get with opaque cursors. Tail and incremental-stream reads are explicitly
      out: they are a declared datasource-member capability carried over the per-agent socket with
      lease-owned lifetimes (X-117/X-118, Milestone 3).

## Progress

- (not started)

## Notes

- Filed by X-130 from flux-roadmap Decision 0006 (rules 6, 7 and 10); Milestones 2–3.
- **Gated on upstream:** requires published connector datasource members (the connectors
  vendor-datasource-declarations design) and X-131's validated bindings.
- Paged reads share transport territory with X-117's request-correlated streams, but v1 is
  deliberately deliverable before X-117 and X-118 and must not grow a dependency on them.
- `docs/designs/installed-apps.md` already states the shape: datasource revisions are frozen into
  the installation boundary, and a live retrieval adapter can expose only that frozen record.
