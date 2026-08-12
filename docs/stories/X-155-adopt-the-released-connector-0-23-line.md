---
id: X-155
title: "Adopt the released connector 0.23 line, which does not move the engine"
status: ready
priority: 1
epic: catalog-artifact
areas: [exchange-host, exchange-server, build]
note: "connector 0.22/0.23 shipped the whole artifact surface X-152/X-153/X-154 consume — the documents, the pack, the reader, DocumentRehearsal and the engine-free plan API. Index-verified 2026-08-12: connector-pack 0.23.0 requires flux ^0.54, the line already pinned, so this is a connector-only bump exactly like X-146"
---

# Adopt the released connector 0.23 line, which does not move the engine

## Goal

Move the four `codewandler-connector-*` pins from 0.21 to 0.23 without touching a single
`codewandler-flux-*` pin, so every X-151 child has the released surface it consumes: the canonical
documents, the pack and its reader, `DocumentRehearsal`, and `codewandler-connector-resolve`'s plan
API.

## The measured premise (the X-146 rule, applied)

Read from the crates.io sparse index on 2026-08-12, not from a story:
`codewandler-connector-pack` 0.23.0 requires `flux-core ^0.54`, `flux-lang ^0.54`,
`flux-runtime ^0.54`, `flux-spec ^1.3`, `flux-system ^0.54` — the engine line this repository
already pins under the `ENGINE_LINE` marker. A connector-only bump; moving flux "to match" would
*create* the divergence X-11 closed.

## Acceptance

- [ ] The four pins in `[workspace.dependencies]` move 0.21 → 0.23 (`connector-address`,
      `connector-catalog`, `connector-pack`, `connector-secrets`); no `codewandler-flux-*` pin
      moves; `Cargo.lock` resolves exactly one engine line and all three
      `crates/exchange-host/tests/engine_line.rs` tests pass untouched.
- [ ] Any 0.21 → 0.23 API drift in this repository's code is absorbed with behaviour unchanged —
      upstream C-538 kept `connector_pack::resolve`/`project`/`pack` signatures as a wrapper and
      C-537 made `catalog` an additive shim, so the expected drift is zero; a compile error here is
      a finding to report, not to absorb silently.
- [ ] `catalog::Acquisition::OAuth2` data is re-measured against the 0.23 catalogue and the result
      recorded in this story (X-154's premise quotes the 0.21 measurement; its babelforce
      empty-endpoint figure needs the current value before X-154 dispatches).
- [ ] The full repository gate is green: Cargo workspace (build, test, clippy -D warnings, fmt),
      `console/` and `web/` Node gates.
- [ ] CHANGELOG entry; the story records the index-read requirement with its command, per the
      X-146 lesson.

## Progress

- 2026-08-12: Filed by the cross-repo coordinator immediately after flux-connectors v0.23.0
  published all six crates (verified live in the sparse index). This is the enabling story for
  X-152/X-153/X-154/X-156; it runs solo because it moves manifests and the lockfile.

## Notes

- Write set: root `Cargo.toml`, `Cargo.lock`, plus whatever call sites drift (expected none).
  Never shares a wave with anything.
- `connector-resolve` and `connector-catalog-reader` are NOT added here — they enter the
  dependency graph with the stories that consume them (X-156, X-153), each carrying its own
  `no_second_request_path` allow-list sentence.
