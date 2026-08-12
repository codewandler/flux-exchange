---
id: X-152
title: "Settings and verification read the catalog document, not parsed Flux"
status: done
priority: 2
epic: catalog-artifact
areas: [exchange-host]
note: "the four Rehearsal call sites in settings.rs are Exchange's runtime Flux parse; characterize them first, then swap — the upstream promise is same-signature semantics, which is exactly the kind of promise a characterization test should check rather than trust"
---

# Settings and verification read the catalog document, not parsed Flux

## Goal

Retire Exchange's runtime parse of emitted connector Flux. `connector_pack::Rehearsal::of(.., entry.flux)`
becomes the document-backed equivalent, and the configuration surface an operation declares is read
rather than recovered.

## Why now

This is the concrete half of connectors **C-539** — *"Exchange … holds zero runtime Flux parses"* —
and the parse is not incidental. `crates/exchange-host/src/settings.rs` calls `Rehearsal::of` at
**416**, **471**, **1457** and **3449**, the last of which is connection *verification*: the code path
that decides whether a connection an operator just made actually works.

`docs/designs/catalog-artifact.md` promises the replacement is mechanical:

> *`Rehearsal` is replaced by a document-backed equivalent with the same signature semantics so
> Exchange's settings/verify paths migrate mechanically.*

**"Same signature semantics" is a promise about behaviour, and this story's job is to check it rather
than take it.**

Upstream agreed and moved: the differential gate now **extends to the configuration surface** —
endpoint variables, slots and caller path parameters — precisely because Exchange's settings and
connection-verification paths consume it (flux-connectors `a2c0df25`). That closes the gap, and it
does not make this story's characterization redundant. Upstream's gate compares its *own* two
derivations against each other; this one records what Exchange observes, from Exchange, before the
thing it observes is replaced. C-534 names it as **the independent pre-swap check**, and independent
is the word doing the work — two derivations that agree can still both differ from what the consumer
was actually reading.

## Acceptance

- [x] **Characterization tests first, before any migration.** For every catalogued operation, record
      what the current `Rehearsal` derivation reports — declared settings, their kinds, their
      binding targets — as committed expected output. Written against the *current* parse, they must
      pass unchanged after the swap. This is the acceptance criterion that can be satisfied **before
      upstream ships anything**, and it is the reason to start early.
- [x] All four `Rehearsal::of(.., *.flux)` sites read document data. No call site passes a `.flux`
      field.
- [x] Connection verification (`settings.rs:3449`) is covered by a failing-first test proving a
      connection that verified before still verifies, and one that did not still does not. Verification
      is the site where a silent difference would look like an operator's mistake rather than ours.
- [x] A test asserts `crates/exchange-host` reaches no connector `.flux` text on any path from the
      catalogue to a settings or verification answer, so the parse cannot come back.
- [x] The four surfaces the design says reach no artifact today — `roles`, `quirks.pagination`,
      `quirks.rate_limit`, `graphs` — are either consumed here or explicitly recorded as not
      consumed. A document that carries more than the parse did should not leave Exchange silently
      ignoring it.
- [x] Workflow Flux parsing is untouched. `flux_lang` stays a dependency of `exchange-host` for
      X-98's workflows; this story removes the *connector* parse only, and the distinction is stated
      where a future reader will look for it.

## Progress

- 2026-08-12: Filed against Decision 0022 / C-539. Upstream has begun no implementation, so the
  document-backed `Rehearsal` does not exist yet — but the characterization tests do not need it.

- 2026-08-12: Promoted to ready by the cross-repo coordinator: flux-connectors v0.23.0 published
  the complete surface this story consumes (documents, pack, reader, DocumentRehearsal, plan
  API) — the upstream blockers recorded above are closed. X-155 lands the pins first.

- 2026-08-12: Implemented on `impl/X-152` (`4787d0a` characterization-only, `a6007bd` the swap
  — the commit order is the proof), merged `06a92ab`. The story's own Notes had the call-site
  attribution inverted and the implementor corrected it by measurement: settings.rs:1457 is
  PRODUCTION connection verification (`CatalogueCustomOriginPolicy::read`), :3449 is its unit
  test; three production sites, one test site, all four now document-backed. The golden
  (892 lines, 55 providers, 835 operations) passed against the Flux parse and was byte-identical
  after the swap. One deliberate deferral, coordinator-decided: `no_second_request_path`'s lock-2
  does not yet name `DocumentRehearsal` as the pack's fourth entry point — the interim bound is
  `no_connector_flux_parse.rs::the_document_backed_rehearsal_stays_in_the_settings_module`, and
  the lock-2 widening (plus the `docs/designs/invoke.md` sentence its own test demands) rides
  X-156, where the lock-2 surgery happens once for the plan API.

## Notes

- Read [[X-151]] for the epic's scope and for the open question about engine-version independence.
- `settings.rs:1457` sits inside a test module in the current tree; confirm before counting it as a
  production site.
- The upstream design records the measuring commands for its own numbers and asks that they be
  re-measured rather than quoted. Do that before repeating "four call sites" in a commit message.
