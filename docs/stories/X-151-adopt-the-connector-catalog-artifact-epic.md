---
id: X-151
title: "Adopt the connector catalog artifact (epic)"
status: backlog
priority: 2
epic: catalog-artifact
areas: [exchange-host, exchange-server, build, docs]
note: "EPIC — the Exchange half of Decision 0022 / connectors C-534: settings, verification and invoke read a catalog document instead of re-parsing emitted Flux, and the catalogue arrives as a pack rather than a crate release"
---

# Adopt the connector catalog artifact (epic)

## Goal

Stop re-parsing emitted Flux at runtime, and start reading the connector's published surface as data
— per [Decision 0022](../../../flux-roadmap/decisions/0022-connectors-compile-to-a-catalog-artifact.md)
and the upstream epic **C-534**, whose child **C-539** is the Exchange-facing obligation:

> *Exchange consumes the reader and resolver from a schema release and holds zero runtime Flux
> parses; `.connector.toml` remains emitted as a projection until flux/D-214 repoints.*

## Why this exists as an Exchange epic

C-534's Notes state *"Exchange adoption stories are filed in `../flux-exchange`."* They were not.
Filed 2026-08-12 to make that true, so the upstream epic has something real to point at and the
sequencing is visible from this side.

Exchange is not a passive consumer here. It performs the same parse upstream does — the design
measures it: `connector_pack::Rehearsal::of(.., entry.flux)` has **four** call sites in
`crates/exchange-host/src/settings.rs` (416, 471, 1457, 3449), including connection verification.
Those are the parses C-539 says must reach zero.

## Children

- **[[X-152]]** — settings and verification read the document. The four `Rehearsal` sites migrate to
  the document-backed equivalent, behind characterization tests written *first*, so the swap is
  provably behaviour-preserving rather than hopefully so.
- **[[X-153]]** — the catalogue arrives as a pack through the reader, including the `load`
  constructor that lets a deployment serve a catalogue newer than the binary it was built with.
  That is new capability, not only a migration.
- **[[X-154]]** — the complete OAuth2 declaration is read from the artifact, which is what
  [[X-147]]'s three remaining criteria actually need.

## Acceptance

- [ ] The union of X-152, X-153 and X-154's acceptance.
- [ ] `crates/exchange-host` holds **zero** runtime Flux parses of connector artifacts. Flux parsing
      for *workflows* (X-98) is unaffected and stays — this epic is about connector data, not about
      the language.
- [ ] A test pins that: no path from the catalogue to a request plan reads `entry.flux`.
- [ ] Nothing in this epic weakens `no_second_request_path`. Decision 0022 keeps resolution upstream
      with *"its enforcement topology unchanged"*, so Exchange reads document **fields** and never
      composes a request from them.

## Progress

- 2026-08-12: Filed. Upstream C-534 is `in-progress` with no implementation begun, so every story
  here is `backlog` until a schema release exists. The work that does **not** wait is X-152's
  characterization tests — writing down what the current parse produces is worth doing before the
  thing that produces it is replaced.

## Notes — one open question that is not ours to answer

**Decision 0022 does not decouple Exchange from the flux engine line, and it is worth being explicit
that it was never meant to.** `docs/designs/catalog-artifact.md` says *"`resolve(entry, egress,
credentials, configuration)` and `project(entry)` keep their signatures"*, and that signature returns
`flux_core::Result<Arc<dyn flux_runtime::Tool>>`. C-534's own Goal scopes the decoupling to
*"catalogue **data** … from the crates.io engine-line release train"*.

So after this epic lands, adopting a newer flux in Exchange **still** requires a `connector-pack`
release that asks for it — the [[X-146]] situation, unchanged. That is a legitimate scope choice, not
an oversight. But it is the one thing a reader of Decision 0022 is most likely to assume it fixed.

If engine-version independence is wanted, the step is small and the migration is its natural moment:
0022 already defines a **request plan** as the unit the differential gate compares (method, URL,
headers, query, body, `permission_subjects`, redaction set). Returning that plan instead of a
`Tool` would move the wrapper — and the engine choice — to Exchange, which depends on flux directly
anyway for workflows. **That is an upstream decision and belongs in C-534's scope or a successor,
not here.** Recorded so the question is asked deliberately rather than discovered later.
