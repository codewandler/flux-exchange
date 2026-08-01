---
id: X-11
title: "Align the flux engine line so connector-pack can link"
status: blocked
epic: invoke
note: "BLOCKER — connector-pack 0.8.0 requires flux-runtime ^0.41 (i.e. <0.42); flux is at 0.45.0. Two flux-runtime versions are two incompatible types. Not fixable from this repo"
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
- (blocked — the work is upstream, in flux-connectors)

## Notes
- Measured 2026-08-01 from crates.io: `codewandler-connector-pack` 0.8.0 requires
  `codewandler-flux-core`, `-flux-lang`, `-flux-runtime` at `^0.41` and `-flux-spec` at `^1.2`. For a
  `0.x` crate Cargo reads `^0.41` as `>=0.41.0, <0.42.0`. The flux family is at **0.45.0**.
- Why it is fatal rather than untidy: `connector_pack::pack` hands out `Arc<dyn flux_runtime::Tool>`.
  Two versions of `flux-runtime` are two distinct traits, so the registry cannot accept it. This is
  the same "one engine version" constraint every consumer of the pack hits.
- **Only `invoke` is affected.** `connector-catalog` has no dependencies; `connector-spec` and
  `connector-secrets` have no flux dependency. Do not let this block X-01…X-10.
