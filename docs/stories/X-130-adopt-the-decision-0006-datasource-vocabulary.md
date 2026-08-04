---
id: X-130
title: "Adopt the Decision 0006 datasource vocabulary"
status: done
priority: 0
epic: apps
areas: [docs]
design: docs/designs/installed-apps.md
note: "Milestone 0 — resolve the concepts table's last ambiguous owner cell and point the audit's upstream gap at chartered connector work"
---

# Adopt the Decision 0006 datasource vocabulary

## Goal

Make Exchange's datasource vocabulary agree with cross-repository Decision 0006: a datasource is a
named, declared, read-only record surface; vendor-data Datasource Definitions belong to the
connector package; Exchange owns the tenant binding and the read seam, never retrieval semantics.
This is the Milestone 0 adoption story — one atomic documentation reconciliation, following the
X-124 pattern, before any implementation work builds on the old ambiguity.

## Acceptance

- [x] `docs/concepts.md` resolves the "Datasource Definition" owner cell from "Flux or connector
      package" to the connector package, with Flux keeping only the wire vocabulary and the
      consuming seam, and its status note points at flux-roadmap Decision 0006 and the chartered
      connectors vendor-datasource design.
- [x] `docs/concepts.md` states the tenant Datasource binding triple: a published connector
      datasource member × a connection label × optional entity/filter scoping, frozen at App
      install. Exchange serves schema/list/get through the existing admission gate and owns tenant
      authorization and connection resolution, never retrieval semantics.
- [x] `docs/designs/released-domain-audit.md` records the "Connector datasource" gap as chartered
      upstream (Decision 0006 rule 6, the connectors vendor-datasource-declarations design) while
      keeping the standing instruction: do not invent a connector datasource declaration in
      Exchange; a tenant Datasource binds the published member when it exists.
- [x] `docs/designs/installed-apps.md` notes that `Datasource.kind` — the free-form String X-108
      shipped — will validate against published connector datasource member references in `oip`
      form once the upstream surface ships, and that a binding names a member reference, never
      vendor fields.
- [x] The upstream-gated implementation work is filed as X-131 (kind validation at
      `put_datasource`), X-132 (the tenant Datasource read seam) and X-133 (the effective-catalogue
      datasources section), and the board is regenerated.

## Progress

- 2026-08-04: Filed and executed from flux-roadmap Decision 0006 on its acceptance day, in an
  isolated worktree off canonical `origin/main`. Documentation-only: no route, store or contract
  code changes, and the Milestone 1 first-run path is untouched.
- 2026-08-04: Done. The concepts table has no ambiguous owner cell, the audit's upstream-gap row
  points at chartered connector work, the installed-apps design carries the `Datasource.kind`
  hardening note, and X-131–X-133 hold the gated implementation.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0006-datasources-are-declared-read-surfaces.md`.
- Sibling repositories file their own atomic adoption stories in the same tranche; the connectors
  adoption also amends `connectors/C-137`…`connectors/C-140` (indexed mode) and charters the
  vendor-datasource declaration surface (rule 6).
- Implementation ordering is unchanged: the connector datasource surface lands with the Milestone 2
  runtime-declaration work, the Exchange read seam with Milestones 2–3, streaming reads with
  Milestone 3 (X-117/X-118 territory).
