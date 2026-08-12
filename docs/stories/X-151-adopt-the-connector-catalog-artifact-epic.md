---
id: X-151
title: "Adopt the connector catalog artifact (epic)"
status: in-progress
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
- [ ] Exchange consumes the resolver's **engine-free plan API** rather than its `Tool`-returning
      wrapper, and owns the `Tool`/`ToolSpec` projection itself — the amended Decision 0022 point 3.
      When that holds, the `ENGINE_LINE` marker and `engine_line.rs`'s three tests describe a
      constraint that no longer exists and are retired in the same change that removes it, not left
      standing as folklore.

## Progress

- 2026-08-12: Filed. Upstream C-534 is `in-progress` with no implementation begun, so every story
  here is `backlog` until a schema release exists. The work that does **not** wait is X-152's
  characterization tests — writing down what the current parse produces is worth doing before the
  thing that produces it is replaced.

## Notes — the engine line is in scope, and that lands work here

Decision 0022 was amended on 2026-08-12 (flux-roadmap `02a2ccf`) to cover the engine line, not only
the release train. Point 3 now reads:

> **The resolver's published surface is engine-free.** It returns the request plan as data — the same
> unit the migration gate compares — and dispatch plus the flux `Tool`/`ToolSpec` projection belong
> to the consumer… after migration the connector publish closure carries no `codewandler-flux-*`
> dependency, and the decoupling covers the engine line, not only the release train.

**That consumer is us.** Upstream keeps `connector-pack`'s `Tool`-returning surface as a *wrapper*
until Exchange adopts the plan API, so nothing breaks on the day it ships — but the wrapper is also
what keeps the coupling alive. Exchange gets engine-version independence only by moving to the plan
API and owning the `Tool`/`ToolSpec` projection itself.

Two consequences worth stating before anyone starts:

- **This is the end of the [[X-146]] situation.** Once Exchange consumes the plan, adopting a newer
  flux stops requiring a `connector-pack` release compiled against it. The `ENGINE_LINE` marker, the
  three `engine_line.rs` tests and the "both pin sets move together" rule exist *because* of the
  `Arc<dyn flux_runtime::Tool>` return type; when it goes, they are describing a constraint that no
  longer exists and should be retired deliberately rather than left as folklore.
- **It is not licence to build a second request path.** Exchange projects a plan into a `Tool`; it
  does not compose a request. Decision 0022 keeps resolution upstream with *"its enforcement topology
  unchanged"* — credential resolution ordering, checked redactor registration, scheme placement,
  endpoint substitution with declared-authority validation. `no_second_request_path.rs` is the guard,
  and adopting the plan API is exactly the change that should be reviewed against it hardest.

Whether that projection is a fifth child of this epic or a criterion inside [[X-152]] is open, and
should be decided when the plan API's shape is published rather than guessed now.
