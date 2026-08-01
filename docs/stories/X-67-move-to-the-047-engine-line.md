---
id: X-67
title: "Move to the 0.47 engine line and the 0.10 connector catalogue"
status: done
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
- [x] Every `codewandler-flux-*` pin and every `codewandler-connector-*` pin moves **in one commit**,
      with `Cargo.lock` updated and `ENGINE_LINE` and its surrounding argument rewritten to state 0.47
      and why.
- [x] `tests/engine_line.rs` passes and still asserts the rendered credential address **literally**.
- [x] The whole catalogue is re-rehearsed and the census reported: provider count, operation count,
      and X-47's `WholeAuthority` / `PinnedTo` / `OutsideTheAuthority` split. **Numbers measured, not
      edited to match.**
- [x] `the_whole_catalogue_declares_http` still holds, or if it does not, X-48's runtime gate and
      X-13's `effects` derivation are both revisited — the story that breaks it owns both.
- [x] The `declares-nothing` / withheld-credential distinction is either surfaced or explicitly
      declined in `docs/designs/connections.md`, with X-49's test updated to match whichever.
      *Declined here and handed to X-50; see the caveat in Progress about the console half.*
- [x] Full gate green, including the console suite and both checker scripts.

## Progress

**Both pin sets moved in one commit; the gate is green; one test went red on the way and was
re-measured rather than edited.**

### The bump

`flux-*` 0.46 → 0.47, `connector-*` 0.9 → 0.10, together. The lock moved **exactly** the closure
being bumped — 15 packages, every one `codewandler-flux-*` or `codewandler-connector-*`, resolving
to 0.47.1 and 0.10.0. No unrelated dependency moved, so a red gate would have been attributable.

`tests/engine_line.rs` gains a third test, `the_lock_carries_one_engine_line`, ported from
flux-connectors' `crates/connector-cli/tests/flux_engine_line.rs` and rewritten to read lines rather
than parse TOML, so it costs no dependency. It catches what the other two cannot: a divergence that
does not touch the seam. Shown firing — with `flux-system` left at 0.46 it names
`codewandler-flux-core 0.46.0`, which **no manifest pin mentions**, because flux-system dragged it in
transitively.

### The census, measured on catalogue 0.10

| | 0.9 | 0.10 |
|---|---|---|
| Providers | 53 | **54** |
| Operations (`connector_catalog::operations()`) | 681 | **679** |
| `WholeAuthority` (refused) | 4 | **5** |
| `PinnedTo` | 7 | **8** |
| `OutsideTheAuthority` | 13 | 13 |
| Providers declaring `http` | 53/53 | **54/54** |

Operations: −7 (`postmark-server-{get,list}`, `zoom-meeting-{create,get}` withheld by C-430;
`babelforce-{authorize,revoke,get-user-customer}` gone with C-136's refusal of
`produces_credential`), +5 (`algolia-*`). 681 − 7 + 5 = 679.

**The fifth refused connector is `intercom`, not `algolia`.** C-225 made intercom's `base_url`
`https://{host}`; a bare placeholder is the whole authority, so X-47's rule refuses it. Algolia ships
`{app_id}.algolia.net`, which pins two labels and lands in `PinnedTo` — the plan expected
`{x}-dsn.algolia.net` and a sixth refusal, and the measurement is what decided it.

`the_whole_catalogue_declares_http` holds at 54/54, so X-48's runtime-gate refusal stays undrivable
through `invoke` and X-13's `effects` derivation is untouched. `connector_catalog::Operation` still
publishes **no** effects field on 0.10.

### C-235, declined here

`Operation::credential_requirement` is additive (`Operation` is `#[non_exhaustive]`), so nothing
here failed to compile. The measurement is one-sided and it is the finding: **670 `Declared`, 9
`Withheld` (all `freshdesk`), and 0 `NoneRequired`.** There is no positively-public operation in the
shipped catalogue, so `freshdesk` was never the credential-less connector [[X-50]] is written about —
its credential is withheld, which is a third state and the one an operator needs told.

Declined in `docs/designs/connections.md` (new addendum) rather than surfaced, for three reasons the
addendum states: the field is per-operation while this body is per-connector, the console renders the
two identically and **this story may not touch `console/`**, and X-50 is open against exactly this.
`a_connector_that_declares_nothing_is_not_an_unknown_connector` now asserts both halves so the
decline is a decision rather than an oversight.

**Caveat, stated rather than buried:** X-49's *console* test
(`connect.test.mjs::a_connector_that_declares_nothing_says_so_rather_than_rendering_an_empty_form`)
is untouched. It drives a hand-built `freshdesk` fixture and the wire shape did not change, so it is
neither red nor wrong — but the note it pins now says something the catalogue contradicts, and
changing it is X-50's work in a worktree that may edit `console/`.

### C-136 and C-404

Nothing here is on the credential-producing path: no `produces_credential` operation exists in the
catalogue, because a connector declaring one no longer builds upstream. The four operations withheld
in v0.9.1 are simply absent, which narrows what this host can invoke by four and needs no code here.

C-404 was expected to live in flux's own binary. Verified independently with `diff -rq` over the
registry sources rather than carried over: of the eleven `codewandler-flux-*` crates in this
workspace's graph, ten are **byte-identical** between 0.46.0 and 0.47.1 outside `Cargo.toml`,
`Cargo.lock` and `.cargo_vcs_info.json`. The eleventh, `flux-plugin`, differs in exactly two files —
`src/host/credential_boundary.rs`, whose diff is **entirely `//!` documentation** with zero
non-comment lines changed, and `src/bin/platform_plugin.rs`, a fixture binary a library consumer
never compiles. So the claim holds for this repository too, now on this repository's own evidence.

## Notes
- **Do not `cargo update` beyond the closure being moved.** A bump that also drags unrelated
  dependencies makes a red gate unattributable, which is the whole reason this is one story.
- flux 0.47's own changes (C-404's credential boundary, L-123's analyzer gate) were measured from
  flux-connectors to live in flux's **binary**, which nothing here links — the vendored sources for
  the six linked crates were byte-identical between 0.46.0 and 0.47.1. Re-verify rather than trusting
  that: it was true of the crates *flux-connectors* links.
