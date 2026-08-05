---
id: X-138
title: "Bind provider recovery, replay and Linux-native C-515 evidence"
status: done
priority: 0
epic: connections
areas: [exchange-host, exchange-server, connections, protocol, tests]
depends_on: [X-135, X-137]
design: docs/designs/local-release-v1.md
note: "X-134 child — retain the exact portable connector-secrets 0.20 identity and prove Exchange-owned recovery on both Linux release targets"
---

# Bind provider recovery, replay and Linux-native C-515 evidence

## Goal

Prove the production server's one retained connector-secrets 0.20 `FileStore` and five-method
prepared port survive every decision boundary, restart and replay on both supported Linux targets,
without narrowing the provider library's published portable five-target evidence or creating a
second store, point-write emulation or secret-shaped Exchange state.

## Acceptance

- [x] Failing first, exact native process tests inject a crash before provider prepare/decision and
      after the durable decision, then restart the real server. Pre-decision recovery aborts with no
      visible label; post-decision recovery queries state, repeats commit, completes publication
      exactly once and returns one durable receipt without re-prepare, edit or abort.
- [x] Exact QUERY and byte-identical same-proposal replay tests restart between every durable
      provider/publication boundary, return the original receipt and never allocate another
      transaction or semantic revision. A changed proposal conflicts value-free.
- [x] Exchange-owned tests
      `unix_connect_crashes_recover_before_readiness_and_replay_one_receipt` and
      `real_server_retains_the_c515_lease_through_recovery_and_readiness` execute with `--exact` on
      `aarch64-unknown-linux-gnu`/`ubuntu-24.04-arm` and
      `x86_64-unknown-linux-gnu`/`ubuntu-24.04`, each with one passed, zero ignored and zero
      filtered. The lease test proves a second process is excluded through recovery/readiness,
      abrupt exit releases the lease and reopen preserves committed state.
- [x] The authority retains upstream connector-secrets 0.20.0 checksum
      `edf98bece86f6364aba3e7dd48c3b7e161146942e9e8450d5dc286143b627717`, released tag commit
      `c764f5c3b8e745cc65e90a298b04851647b76778` and its published portable five-target evidence. It inherits the
      exact applicable tests `every_durable_transaction_boundary_recovers_one_complete_state`,
      `two_children_prove_lease_refusal_and_abrupt_release`,
      `file_store_holds_a_lifetime_non_blocking_lease`,
      `unix_lease_metadata_is_owner_only_one_link_and_never_repaired` and
      `native_upgrade_fixture_proves_legacy_quiescence_and_v2_refusal`. No new Connectors release or
      Linux-only reinterpretation of that library is permitted.
- [x] Every Exchange-owned process binding joins the retained 0.20 lease assertion, is listed once
      in X-139's authority and emits one terminal report on each supported Linux runner. Windows and
      macOS Exchange process rows are absent rather than skipped or reported as gaps.
- [x] The production boundary remains exactly `prepare`, `state`, `commit`, `abort`, `reclaim` over
      one shared concrete `Arc<FileStore>` exposed as ordinary and prepared ports. Exchange durable,
      receipt, audit and log state contains no credential bytes, ordinals, presence facts or extra
      provider API; four-form sentinels prove that boundary after every crash/replay case.

## Progress

- Linux exact execution already proves the two named Exchange tests locally. This story remains
  blocked until X-137 lands the final two-target production boundary, after which both native Linux
  runners must emit the canonical reports consumed by X-139.
- 2026-08-05: X-137 landed the two-target production boundary, so the remaining X-138 obligation was
  binding — not re-deriving — the recovery, replay and lease evidence the authority publishes.
  `crates/exchange-release/tests/retained_provider_evidence.rs` is failing-first: before this change
  `NativeEvidenceAuthority::validate` accepted a substituted
  `real_server_retains_the_c515_lease_through_recovery_and_readiness` or
  `unix_connect_crashes_recover_before_readiness_and_replay_one_receipt`, a substituted inherited
  C-515 test, a removed `x134-c515-retained-lease` obligation and a process binding whose class no
  assertion join selects. Each of those five mutations now refuses, and `native-evidence-v1.json`
  is unchanged, so X-139 keeps sole ownership of the final document, digest and fixture projection.
- 2026-08-05: The Exchange-owned recovery/replay/lease proofs and the five inherited portable C-515
  tests are now named in code (`RETAINED_PROVIDER_TESTS`, `RETAINED_PROVIDER_OBLIGATIONS`,
  `INHERITED_PROVIDER_TESTS`), both bound to a target set that must be exactly the two supported
  Linux targets. The upstream 0.20.0 checksum and released commit remain pinned by
  `validate_upstream_package`; no provider source, portable claim or Connectors release moved.
- 2026-08-05: The isolated story worktree had no reachable Cargo registry, so no typed Rust check
  ran here: `cargo check -p flux-exchange-release --offline` refused with `no matching package named
  codewandler-connector-address`, and the networked attempt failed DNS resolution for
  `index.crates.io`. The exact `--exact` executions on `ubuntu-24.04` and `ubuntu-24.04-arm`, the
  release contract suite and the repository gate therefore remain the integrator's Linux runs.
- 2026-08-05: Failing-first process evidence showed the server returned a receipt instead of
  stopping at provider prepare, and reached readiness without exposing a recovery-active lease
  boundary. The exact tests now crash after provider prepare, after the durable decision and after
  provider commit, then observe only `prepare,abort`, `prepare,decision,state,commit` and
  `prepare,decision,commit,state` respectively.
- 2026-08-05: Every provider and publication crash restarts the real supervised server. QUERY and
  byte-identical replay retain one receipt and one transaction row; changed proposals conflict;
  raw, JSON-escaped, percent-encoded and base64 sentinels remain absent after every crash, query and
  replay outside the provider-owned credential store.
- 2026-08-05: The retained-store test pauses inside coordinator recovery after the one concrete
  `FileStore` is open, proves a second process receives the provider lease conflict, resumes through
  readiness, proves contention again, then proves abrupt release and committed-state reopen.
- 2026-08-05: Both exact parent tests pass locally with one passed, zero ignored and zero filtered.
  The unchanged authority that X-139 exclusively owns selects each test once on
  `ubuntu-24.04-arm` and `ubuntu-24.04`; it retains the released C-515 five-target evidence, while
  X-139 remains the sole owner of terminal reports and the final derived authority.
- 2026-08-05: The complete Rust, console and public-site repository gate passed. The manifest and
  lock remain on registry `codewandler-connector-secrets` 0.20.0 checksum
  `edf98bece86f6364aba3e7dd48c3b7e161146942e9e8450d5dc286143b627717`; no dependency, authority,
  fixture, digest, release script or CI workflow identity changed.

## Notes

- Child of X-134. C-515 remains a portable completed provider release; Decision 0012 narrows only
  Exchange-owned runtime/publication evidence.
