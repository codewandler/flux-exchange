---
id: X-138
title: "Bind provider recovery, replay and native C-515 evidence"
status: blocked
epic: connections
areas: [exchange-host, exchange-server, connections, protocol, tests, windows]
depends_on: [X-135, X-137]
design: docs/designs/local-release-v1.md
note: "X-134 child — crash/query/same-proposal recovery and exact five-target connector-secrets 0.20 release bindings"
---

# Bind provider recovery, replay and native C-515 evidence

## Goal

Prove the production server's one retained connector-secrets 0.20 `FileStore` and five-method
prepared port survive every decision boundary, restart and replay on all five native targets,
without a second store, point-write emulation or secret-shaped Exchange state.

## Acceptance

- [ ] Failing first, exact native process tests inject a crash before provider prepare/decision and
      after the durable decision, then restart the real server. Pre-decision recovery aborts with no
      visible label; post-decision recovery queries state, repeats commit, completes publication
      exactly once and returns one durable receipt without re-prepare, edit or abort.
- [ ] Exact QUERY and byte-identical same-proposal replay tests restart between every durable
      provider/publication boundary, return the original receipt and never allocate another
      transaction or semantic revision. A changed proposal conflicts value-free.
- [ ] The Exchange-owned exact test
      `real_server_retains_the_c515_lease_through_recovery_and_readiness` runs on all five native
      targets and proves a second process is excluded through recovery/readiness, abrupt server exit
      releases the lease, and reopen preserves committed state.
- [ ] The native authority binds upstream connector-secrets 0.20.0 checksum
      `edf98bece86f6364aba3e7dd48c3b7e161146942e9e8450d5dc286143b627717` and released tag commit
      `c764f5c3b8e745cc65e90a298b04851647b76778`, plus exact inherited tests
      `every_durable_transaction_boundary_recovers_one_complete_state`,
      `two_children_prove_lease_refusal_and_abrupt_release`,
      `file_store_holds_a_lifetime_non_blocking_lease`,
      `unix_lease_metadata_is_owner_only_one_link_and_never_repaired` on Unix, and
      `native_upgrade_fixture_proves_legacy_quiescence_and_v2_refusal`.
- [ ] Every applicable server-process binding joins the retained 0.20 lease assertion. Parent tests
      are inherited release evidence, not locally faked dependency tests; every Exchange binding is
      listed once, executes `--exact`, and reports one passed, zero ignored and zero filtered on its
      declared native runner.
- [ ] The production boundary remains exactly `prepare`, `state`, `commit`, `abort`, `reclaim` over
      one shared concrete `Arc<FileStore>` exposed as ordinary and prepared ports. Exchange durable,
      receipt, audit and log state contains no credential bytes, ordinals, presence facts or extra
      provider API.

## Progress

- Provider composition at accepted checkpoint `bd9ad187c89eccbfb1f9b70c2bfc20b010284011`
  and later coordinator/recovery tests are useful implementation evidence. Exact five-target
  process selection and complete crash/query/replay closure remain required here.

## Notes

- Child of X-134. Sequenced after X-135 so cancellation cannot invalidate crash semantics and after
  X-137 so the Windows native row names the final production endpoint/helper path.
