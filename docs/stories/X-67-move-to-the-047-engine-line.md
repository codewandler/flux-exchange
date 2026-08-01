---
id: X-67
title: "Move to the 0.47 engine line and the 0.10 connector catalogue"
status: ready
priority: 1
areas: [exchange-host, exchange-server, console]
note: "the blocker is gone: connector-pack 0.10.0 requires flux-runtime ^0.47 and flux-connectors v0.10.0 is released. This is not a version bump — the catalogue gained a 54th provider and a new way of saying why an operation names no credential, and several tests are designed to go red on exactly that"
---

# Move to the 0.47 engine line and the 0.10 connector catalogue

## Goal
This repository runs on flux 0.47 and the 0.10 connector catalogue, with every test that a catalogue
change was designed to break either passing or deliberately updated.

## The blocker is gone, and the constraint that replaced it

`Cargo.toml`'s `ENGINE_LINE` comment records why this sat at 0.46: `connector-pack 0.9.0` required
`flux-runtime ^0.46`, and `connector_pack::pack` hands out `Arc<dyn flux_runtime::Tool>` — two
runtime versions in one graph are two unrelated traits.

**`connector-pack 0.10.0` is published and requires `flux-runtime ^0.47`.** flux-connectors `v0.10.0`
is tagged and its whole closure is on the 0.10 line.

⚠ **Both move in one commit.** Raising the `flux-*` pins alone, or `connector-*` alone, puts two
engine lines in one lock and the two `flux_runtime::Tool` traits will not unify. `tests/engine_line.rs`
exists to catch exactly this; flux-connectors' own `flux_engine_line.rs` asserts the property over the
manifest *and* the lock and is worth stealing.

## This is not a version bump — what actually changed

Read `flux-connectors/CHANGELOG.md` `## [0.10.0]` and `## [0.9.1]` before starting.

### 1. A 54th provider, and the counts are asserted

**Algolia ships** — 53 → **54** providers, and 937 → 945 artifacts upstream. Two tests here assert
the shape literally and are *supposed* to fail:

- `crates/exchange-server/src/routes/catalogue/view.rs:303` — *"299 operations across 53 connectors"*
- `crates/exchange-host/src/grant.rs:815` — the same sentence, over the gate's own projection

Two more claims are prose that becomes false: `runtime.rs:106` (*"the honest answer for all 53 shipped
connectors"*) and `grant.rs:116` (*"all 53 are HTTP"*). **Both are load-bearing arguments, not
decoration** — `the_whole_catalogue_declares_http` is what makes X-48's runtime-gate refusal
undrivable and what X-13's `effects` derivation rests on. Re-measure them; do not just edit the number.

### 2. X-47's four refused connectors are catalogue-derived

`no_shipped_connector_lets_a_tenant_supply_its_whole_authority` pins `newrelic`, `docusign`, `okta`
and `freshdesk` with `assert_eq!`. X-47's design says in as many words that a catalogue bump moving a
host template **should** turn this red rather than quietly dispatching. Algolia's template is
`{x}-dsn.algolia.net`, which X-47's review already measured as `None` — it errs closed — but that was
against 0.9. **Re-rehearse the whole catalogue and report the new census** (`WholeAuthority`,
`PinnedTo`, `OutsideTheAuthority`), rather than assuming the four are still four.

### 3. The catalogue can now say *why* an operation names no credential

C-235: it previously emitted `[]` for both a positively-public operation and one whose credential is
**deliberately withheld**, so no host could tell them apart. That distinction is new, and this
repository has a whole branch keyed on the old ambiguity:

- X-46 publishes declared credentials; X-49 pinned the `declares-nothing` render using **`freshdesk`**
  as the one connector reaching it; [[X-50]] is an open story about whether such a connector can be
  connected at all.
- `freshdesk` appears in **seven** files here.

If `freshdesk`'s answer changes shape, X-49's test and X-50's premise both move. **Decide whether this
repository now surfaces the distinction** — an operation that is public and one whose credential is
withheld are different facts and the console currently renders them identically — or record that it
does not, and why.

### 4. `callable` moves for some operations

C-235 also moves operations from `callable: true` to `false`. `console/src/catalog.mts:262` carries
that field and `CoreExplorer.vue` renders it. Check what the console shows now, and whether X-53's
`works` badge needs to account for it — an operation that is not `callable` is a different claim from
one this service cannot run.

### 5. A credential-producing operation returns a handle, never the secret

C-136. Directly adjacent to this repository's north star. Establish whether anything here is on that
path today, and whether the four operations withheld in v0.9.1 (which C-136 explicitly does **not**
restore) change what this host can invoke.

## Acceptance
- [ ] Every `codewandler-flux-*` pin and every `codewandler-connector-*` pin moves **in one commit**,
      with `Cargo.lock` updated and `ENGINE_LINE` and its surrounding argument rewritten to state 0.47
      and why.
- [ ] `tests/engine_line.rs` passes and still asserts the rendered credential address **literally**.
- [ ] The whole catalogue is re-rehearsed and the census reported: provider count, operation count,
      and X-47's `WholeAuthority` / `PinnedTo` / `OutsideTheAuthority` split. **Numbers measured, not
      edited to match.**
- [ ] `the_whole_catalogue_declares_http` still holds, or if it does not, X-48's runtime gate and
      X-13's `effects` derivation are both revisited — the story that breaks it owns both.
- [ ] The `declares-nothing` / withheld-credential distinction is either surfaced or explicitly
      declined in `docs/designs/connections.md`, with X-49's test updated to match whichever.
- [ ] Full gate green, including the console suite and both checker scripts.

## Notes
- **Do not `cargo update` beyond the closure being moved.** A bump that also drags unrelated
  dependencies makes a red gate unattributable, which is the whole reason this is one story.
- flux 0.47's own changes (C-404's credential boundary, L-123's analyzer gate) were measured from
  flux-connectors to live in flux's **binary**, which nothing here links — the vendored sources for
  the six linked crates were byte-identical between 0.46.0 and 0.47.1. Re-verify rather than trusting
  that: it was true of the crates *flux-connectors* links.
