---
id: X-85
title: "Run the latest published connector line"
status: in-progress
epic: invoke
areas: [build, exchange-host, deployment]
note: "connector-pack 0.13.0 sets the compatible engine line to Flux 0.49; the exchange moves both pin sets together before its Fly machine is replaced"
---

# Run the latest published connector line

## Goal
The deployed exchange runs the newest published connector library without splitting the Flux engine
graph: connector crates 0.13 and Flux 0.49 move together, then the tested build replaces the Fly
machine.

## Acceptance
- [x] Failing first, `the_engine_line_is_recorded_in_exactly_one_place` names an old Flux pin when
      only `ENGINE_LINE` is changed to the line required by `connector-pack` 0.13.0.
- [x] All four connector dependencies resolve on 0.13 and every Flux engine dependency resolves on
      0.49, with the manifest, compile-time seam and lockfile tests proving there is no second line.
- [ ] The Rust workspace, console, public site, crate package dry-run and container build gates pass.
- [ ] The workspace version and published `exchange-host` dependency move together, the changelog
      states the engine/connector upgrade, and a matching tag publishes the tested crate through CI.
- [ ] `fly deploy` replaces the machine with that tested source; `/health` and the same-origin console
      answer successfully after deployment, and Fly reports the new release healthy.

## Progress
- 2026-08-02: crates.io reports non-yanked 0.13.0 releases for connector-address, catalog, secrets and
  pack. `cargo info codewandler-connector-pack@0.13.0 --verbose` reports Flux core/lang/runtime 0.49.
- 2026-08-02: with only `ENGINE_LINE` changed, the manifest test failed on the first 0.47 dependency,
  proving the guard sees the stale pin before the dependency update.
- 2026-08-02: all four engine-line tests pass; the full workspace test suite (283 route/library tests
  plus integration tests), clippy, formatting, console (97 tests/build), and site (28 tests/build)
  are green on connector 0.13.0 and Flux 0.49.0.

## Notes
- The connector repository is read-only for this story. Its release is an upstream input, not work
  owned by the exchange agent.
- The engine line is set by `connector-pack`, never independently selected here; see X-11 and X-67.
