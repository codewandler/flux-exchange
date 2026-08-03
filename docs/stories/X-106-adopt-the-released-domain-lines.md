---
id: X-106
title: "Adopt the released Flux and connector domain lines"
status: done
epic: apps
areas: [build, exchange-host, exchange-server, docs]
design: docs/designs/released-domain-audit.md
note: "Release gate opened 2026-08-03: connector-pack 0.16 requires Flux 0.52; audit the shipped contracts before Exchange builds on them"
---

# Adopt the released Flux and connector domain lines

## Goal
Move Flux Exchange onto the published connector 0.16 / Flux 0.52 graph as one compatibility unit,
then replace the speculative cross-repository concept map with an audit of the contracts that
actually shipped. That evidence is the implementation boundary for Exchange-hosted apps.

## Acceptance
- [x] Failing first, `the_engine_line_is_recorded_in_exactly_one_place` rejects the old Flux pins
      when only `ENGINE_LINE` is changed to `0.52`.
- [x] All four connector dependencies resolve on 0.16 and every Flux engine dependency resolves on
      0.52, with the manifest, compile-time seam and lockfile tests proving there is no second line.
- [x] `docs/designs/released-domain-audit.md` classifies every planned top-level concept as shipped,
      an Exchange-owned binding, or an upstream gap, citing the published API that supports it.
- [x] The canonical glossary is aligned across the three family repositories and clearly separates
      live capabilities from target architecture.
- [x] The Rust workspace gate, console tests/build, and public-site build/tests pass without
      overwriting the in-progress X-101 through X-105 channel work.
- [x] The changelog records the dependency move and the clarified domain vocabulary; this story and
      the generated board are current.

## Progress
- 2026-08-03: crates.io reports connector-address, connector-catalog, connector-pack and
  connector-secrets 0.16.0. Published connector-pack metadata requires Flux core/lang/runtime 0.52,
  and Flux 0.52.1 is published.
- 2026-08-03: the existing worktree contains user-owned X-101 through X-105 channel implementation;
  dependency adoption will preserve and compile that work rather than reset it.
- 2026-08-03: the engine-line test failed first against the old 0.49 pins, then the complete graph
  resolved to connector 0.16.0 and Flux 0.52.1. The released Zendesk and Asterisk catalogue changes
  were reviewed and pinned in the host's safety censuses.
- 2026-08-03: `cargo test --workspace`, clippy with warnings denied, formatting, all 113 console
  tests and its production build, and the public-site build and 28 tests pass. Flux's generated
  website mirrors and 32-test website contract pass; all three repository diffs are clean.

## Notes
- Published crates are the source of truth. Local sibling worktrees may be used to understand design
  history, but Exchange must not gain a sibling `path` or `git` dependency.
- Feature implementation begins from the completed release audit, not from guessed adapter types.
