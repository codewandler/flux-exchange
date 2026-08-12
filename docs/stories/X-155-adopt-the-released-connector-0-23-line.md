---
id: X-155
title: "Adopt the released connector 0.23 line, which does not move the engine"
status: done
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

- [x] The four pins in `[workspace.dependencies]` move 0.21 → 0.23 (`connector-address`,
      `connector-catalog`, `connector-pack`, `connector-secrets`); no `codewandler-flux-*` pin
      moves; `Cargo.lock` resolves exactly one engine line and all three
      `crates/exchange-host/tests/engine_line.rs` engine-line tests pass untouched. *(The file's
      fourth pinning test — the connector-line version + archive checksums — necessarily moved
      with the line, checksums re-taken from the sparse index per its own doc-comment rule; that
      move was the story's failing-first test.)*
- [x] Any 0.21 → 0.23 API drift in this repository's code is absorbed with behaviour unchanged —
      **zero drift, verified by full compile**, confirming upstream C-538 kept the
      `connector_pack` signatures and C-537's catalog shim is additive.
- [x] `catalog::Acquisition::OAuth2` re-measured against the 0.23 catalogue: **byte-identical to
      the 0.21 declarations for both gitlab and babelforce** (gitlab: endpoint `login`,
      authorize_path `/oauth/authorize`, grants AuthorizationCode+RefreshToken; babelforce:
      endpoint and authorize_path EMPTY, grants Password+RefreshToken, hazard
      ResourceOwnerSecretShared). X-154's premise stands unrevised.
- [x] The full repository gate is green: Cargo workspace (build, test, clippy -D warnings, fmt),
      `console/` 125/125 and `web/` 34/34 Node gates, plus `check-dev-signin.sh` and a `--locked`
      build proving the lock is not stale.
- [x] CHANGELOG entry (written at integration); the index-read requirement and its command are
      recorded in the manifest comment beside the pins they justify, and quoted in this story's
      Progress.

## Progress

- 2026-08-12: Filed by the cross-repo coordinator immediately after flux-connectors v0.23.0
  published all six crates (verified live in the sparse index). This is the enabling story for
  X-152/X-153/X-154/X-156; it runs solo because it moves manifests and the lockfile.
- 2026-08-12: Implemented on `impl/X-155` (`c35f7a2`), merged `7fa6cc3`. The index read, re-run
  in-session (`curl -sS https://index.crates.io/co/de/codewandler-connector-pack | jq -r
  'select(.vers=="0.23.0") | .deps[] | "\(.package // .name) \(.req)"'`): flux-core/-lang/-runtime
  `^0.54`, flux-spec `^1.3` — byte-identical to 0.21.0's requirements, confirming the
  connector-only premise independently. The lock gains `connector-resolve` 0.23.0 and
  `catalog-reader` 0.23.0 transitively (connector-pack 0.23.0 requires the former); neither is a
  direct dependency until X-156/X-153. AGENTS.md's dependency section updated at integration per
  the X-146 precedent.

## Notes

- Write set: root `Cargo.toml`, `Cargo.lock`, plus whatever call sites drift (expected none).
  Never shares a wave with anything.
- `connector-resolve` and `connector-catalog-reader` are NOT added here — they enter the
  dependency graph with the stories that consume them (X-156, X-153), each carrying its own
  `no_second_request_path` allow-list sentence.
