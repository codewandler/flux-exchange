---
id: X-131
title: "Validate Datasource.kind against published connector datasource members"
status: backlog
epic: apps
areas: [exchange-host, exchange-server]
design: docs/designs/installed-apps.md
note: "Decision 0006 — put_datasource refuses a kind that is not a published connector datasource member reference; gated on the upstream connector surface"
---

# Validate Datasource.kind against published connector datasource members

## Goal

Harden the X-108 placeholder: a tenant Datasource's `kind` stops being a free-form String and
becomes a reference to a published connector datasource member (`oip` form), validated at
`put_datasource`. A binding names a member reference, never vendor fields — which entities a vendor
exposes and how to read them is the connector package's declaration, not something a tenant write
can restate.

## Acceptance

- [ ] Failing first, `put_datasource` refuses a `kind` that is not a member reference to a
      published connector datasource member, naming the reference and the refusal cause; today's
      free-form String is accepted nowhere on the write path once the validation lands.
- [ ] The accepted reference form is the published `oip` member-reference vocabulary; Exchange
      parses it through the published contract and never defines a second reference grammar.
- [ ] A Datasource write carries only the member reference, the connection label and optional
      entity/filter scoping; vendor field names, parameter mappings and cursor vocabulary remain
      the connector member's declaration and are refused as tenant input.
- [ ] App installation's datasource requirement matching and the frozen installation record keep
      their existing semantics: validation happens at the tenant Datasource write, and an already
      frozen App revision is not widened or invalidated by later catalogue changes.
- [ ] Existing stored free-form Datasources have an explicit, tested posture (refused at read with
      a named cause, or migrated) — never silently reinterpreted as a member reference.

## Progress

- (not started)

## Notes

- Filed by X-130 from flux-roadmap Decision 0006 (rule 7: the `exchange/X-108` placeholder
  hardens).
- **Gated on upstream:** the connector IR datasources surface (Decision 0006 rule 6, the connectors
  vendor-datasource-declarations design) must publish member declarations through the manifest and
  catalogues before this validation has anything real to check against. Do not invent a local
  member registry to unblock it.
- See `docs/designs/installed-apps.md` § Runtime authority for the `Datasource.kind` note, and
  `docs/designs/released-domain-audit.md` for the standing upstream-gap instruction.
