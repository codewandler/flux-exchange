---
id: X-99
title: The development build keeps the declared MSRV
status: in-progress
priority: 0
epic: local-identity
areas: [exchange-server, supply-chain]
note: "cargo run -- --dev regressed when bundled SQLite moved beyond Rust 1.88; align Exchange and Flux on the compatible registry line and prove the real browser round trip"
---

# The development build keeps the declared MSRV

## Goal
`cargo run -- --dev` builds on the Rust 1.88 version the manifests promise and gives a browser a
working one-click session for `user:${USER}@dev`.

## Acceptance
- [x] A failing-first Rust 1.88 build reproduces the bundled `libsqlite3-sys` compiler failure.
- [x] Exchange and its published Flux 0.52 dependencies resolve one registry-only SQLite line that
      builds on Rust 1.88, without raising `rust-version` or adding a path/git override.
- [x] A route test exercises the development sign-in form, HttpOnly cookie exchange, redirect, and
      resolved `dev` tenant without exposing the roster handle in the response body.
- [x] A real `cargo run --locked -- --dev` process has completed the same browser-shaped cookie
      round trip for a temporary startup user.
- [ ] The complete gate and crates.io release workflow pass, and the corrective version is visible
      in the registry.

## Progress
- 2026-08-03: v0.14.0 and v0.14.1 published successfully on stable but their ordinary CI MSRV jobs
  failed in `libsqlite3-sys 0.38.1`, masking the otherwise-working browser sign-in repair.
- 2026-08-03: Flux v0.52.2 restored its Rust 1.87 graph through CI-only publication. Exchange's
  direct rusqlite 0.40 pin was then the final conflicting native SQLite line.
- 2026-08-03: The lock now contains Flux 0.52.2 throughout and one rusqlite 0.39/libsqlite3-sys
  0.37 line. `cargo +1.88 build --workspace --locked`, workspace tests and clippy, console tests and
  build, and public-site build and tests pass.

## Notes
- This repairs an existing compatibility promise. Raising Exchange's MSRV would turn a transitive
  patch regression into a consumer-facing compatibility break.
