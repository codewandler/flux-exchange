---
id: X-133
title: "Publish the effective-catalogue datasources section"
status: backlog
epic: rich-connector-runtimes
areas: [exchange-host, exchange-server]
design: docs/designs/rich-connector-runtimes.md
note: "Decision 0006 — the X-113 effective catalogue grows a datasources section under the same forbidden-authority-field refusal rules as operations"
---

# Publish the effective-catalogue datasources section

## Goal

Extend the X-113 effective Service Account catalogue with a `datasources` section, so Flux can
discover the granted tenant Datasources a resolved principal can actually read and bind each one
through its existing live registration seam. Flux reads vendor datasources only through the
embedded Exchange client: Exchange unavailable means vendor datasources unavailable, with no local
vendor adapter and no local index fallback.

## Acceptance

- [ ] Failing first, the authenticated effective catalogue response carries a `datasources` section
      listing only the resolved principal's bound-and-granted tenant Datasources — member
      reference, entity schema surface and scoping metadata — intersected the same way the
      operations section already is.
- [ ] The section obeys the same forbidden authority-field refusal rules as operations: no
      credential or credential address, no tenant identifier a caller could select, no vendor
      endpoint, no runtime placement and no caller-selected authority; the existing refusal tests
      extend to the new section.
- [ ] The X-113 stable content generation covers the datasources section, so a binding, grant or
      scoping change is visible to Flux as a generation change between turns.
- [ ] An ungranted, unbound or upstream-unpublished Datasource is absent or refused by the
      established contract rules — never guessed at, and never widened by the projection.

## Progress

- (not started)

## Notes

- Filed by X-130 from flux-roadmap Decision 0006 (rule 8). Extends the X-113 contract; it does not
  reopen it.
- **Gated on upstream** (published connector datasource members) and on X-131/X-132 giving the
  section something true to publish.
- Flux-side consumption — binding each granted tenant Datasource through the live registration
  seam — is the sibling repository's story; this story owns only the Exchange contract surface.
