---
id: X-11
title: "Align the flux engine line so connector-pack can link"
status: ready
epic: invoke
priority: 1
areas: [exchange-host, exchange-server]
note: "UNBLOCKED 2026-08-01: upstream published 0.9.0. connector-pack now requires flux-runtime ^0.46 and 0.46.0 exists, so the ^0.41-vs-0.45 conflict is gone. connector-spec is replaced by connector-address and stopped at 0.8"
---

# Align the flux engine line so connector-pack can link

## Goal
Make it possible for this repository to depend on `connector-pack` and on the current flux engine at
the same time. Until that holds, nothing can execute an operation.

## Acceptance
- [ ] `codewandler-connector-pack` is published against the flux line this repository uses, and a
      trivial binary here links `connector_pack::pack` and `flux_web::http::HttpRequestTool`
      together and compiles.
- [ ] The engine line this repository targets is recorded in one place, so the next bump is a value
      change rather than an archaeology exercise.

## Progress
- **The upstream half is DONE, 2026-08-01.** flux-connectors' C-403 moved all seven pins to the 0.45
  line (`flux-spec` to `1.3` on its own line) and is merged to its `main`. The emitted Flux is
  unchanged — a full build rewrote 2 of 557 artifacts, both README snippet SVGs, and the delta is
  four `fill=` attributes from a highlighter reclassification.
- **Still blocked, on publication only.** Published `codewandler-connector-pack` 0.8.0 still requires
  `flux-runtime ^0.41`; a tag and a CI publish are what close this. Nothing further can be done from
  this repository.
- Worth knowing while you wait: the same release also carries C-406's instance dimension, so it turns
  **X-11, X-12, X-13 and X-14** from blocked into ready in one move.
- Found during that work and worth filing against flux: `flux-runtime` 0.45.0 declares
  `flux-secret = "1"` but calls `Redactor::try_add_secret`, which first exists in 1.1.0 — so a legal
  resolve of 1.0.x fails to compile. The requirement should be `"1.1"`.

## Notes
- Measured 2026-08-01 from crates.io: `codewandler-connector-pack` 0.8.0 requires
  `codewandler-flux-core`, `-flux-lang`, `-flux-runtime` at `^0.41` and `-flux-spec` at `^1.2`. For a
  `0.x` crate Cargo reads `^0.41` as `>=0.41.0, <0.42.0`. The flux family is at **0.45.0**.
- Why it is fatal rather than untidy: `connector_pack::pack` hands out `Arc<dyn flux_runtime::Tool>`.
  Two versions of `flux-runtime` are two distinct traits, so the registry cannot accept it. This is
  the same "one engine version" constraint every consumer of the pack hits.
- **Only `invoke` is affected.** `connector-catalog` has no dependencies; `connector-spec` and
  `connector-secrets` have no flux dependency. Do not let this block X-01…X-10.

## Unblocked, 2026-08-01 — what actually changed upstream

flux-connectors published **0.9.0** today, and the conflict this story was waiting on is gone:

| | was | now |
|---|---|---|
| `connector-pack` requires | `flux-runtime ^0.41` | **`flux-runtime ^0.46`** |
| latest `flux-runtime` | 0.45.0 | 0.47.0, and **0.46.0 exists** |

So `connector-pack` 0.9.0 resolves against a published engine line, and this repository can depend on
it. **Verify that rather than assuming it** — a resolvable manifest is not a linking binary.

**The upgrade is not just a version bump.** `connector-spec` — the *compiler* — has been split, and
the vocabulary extracted to a new crate:

- `codewandler-connector-spec` stopped at **0.8.0**. There is no 0.9.
- `codewandler-connector-address` **0.9.0** is the replacement for the vocabulary, and depends on
  nothing but `thiserror`.
- `connector-catalog` and `connector-secrets` are both at **0.9.0**.

This repository's only use of the old crate is `connector_spec::DEFAULT_SERVICE` in
`exchange-host/src/connections.rs`, plus doc references to
`connector_spec::Connector::credential_ref_for`. Small, but it is on the **address derivation path**,
which is the repository's central invariant — so the migration must not be done by search and replace
without reading what moved.

## Acceptance
- [x] *(superseded — recorded above)* `connector-pack` is published against a usable flux line.
- [ ] The workspace builds against `connector-catalog`, `connector-secrets` and
      `connector-address` at **0.9**, with `connector-spec` gone from the manifests entirely.
- [ ] **A test proves `connector-pack` can actually link here**, not merely resolve — the Acceptance's
      original wording asked for a trivial binary that compiles against it. Adapt it to whatever the
      0.9 surface is, and say what you did.
- [ ] **The address derivation is unchanged**, asserted by the existing tests staying green
      *unmodified* — the 18-hostile-name sweep, `no_route_here_accepts_an_address`, and the
      tenant-derivation vectors are what guard the central invariant and they must not be edited to
      accommodate an upgrade.
- [ ] The engine line this repository targets is recorded **in one place**, so the next bump is a
      value change rather than archaeology.
- [ ] Whether `connector-pack` is *added as a dependency now* is a decision to make and argue: X-12
      needs it, and an unused dependency carried early is weight without benefit.

## Notes
- **Read the upstream changelog before migrating.** `connector-spec` was the compiler and
  `connector-address` is the vocabulary — that is a real reorganisation (upstream's own note calls it
  "C-407 extracted the vocabulary"), not a rename, and assuming the latter is how an address
  derivation quietly changes shape.
- `connector-secrets` re-exports `CredentialRef` from `connector-address` now, where it used to come
  from `connector-spec`. Check which path this repository is actually using.
- This runs **solo**: it changes manifests and the lockfile, so nothing else may be in flight.
