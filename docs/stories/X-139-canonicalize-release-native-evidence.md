---
id: X-139
title: "Canonicalize the two-target Linux native-evidence inventory"
status: ready
epic: connections
areas: [exchange-release, tests, workflows, docs]
depends_on: [X-135, X-136, X-137, X-138]
design: docs/designs/local-release-v1.md
note: "X-134 child — one canonical JSON authority and terminal reports for exactly the two Linux release targets"
---

# Canonicalize the two-target Linux native-evidence inventory

## Goal

Replace every historical copied or five-target release-native inventory with one canonical, derived
and publication-blocking authority for the final Linux-only X-134 tree. It names every retained
inherited obligation, X-134 obligation, exact test and supported Linux runner once; no copied list,
literal count, stale fixture or non-Linux report may certify publication.

## Acceptance

- [ ] Failing first, `native_evidence_authority_rejects_each_missing_family_target_and_test`
      removes or substitutes each authority class, family, target, runner, feature and exact Cargo
      binding independently and makes publication fail. The sole source is
      `crates/exchange-release/native-evidence-v1.json`; no TSV, Python/digest oracle,
      `native_fixture_cases()`, copied YAML target matrix, frozen family/binding count or
      `EXPECTED_MATRIX_SHA256` remains.
- [ ] The JSON models literal X-134, inherited X-128 and inherited C-515 authority classes;
      obligations; exact Cargo tests/features; named adversarial cases; inherited release evidence;
      and the cross-cutting connector-secrets 0.20 lease assertion. Its supported target/runner set
      is exactly `aarch64-unknown-linux-gnu`/`ubuntu-24.04-arm` and
      `x86_64-unknown-linux-gnu`/`ubuntu-24.04`. It contains no Darwin/MSVC target, Windows family or
      skipped/gap row. `all-native` and `linux-native` both mean exactly those two targets;
      `unix-native` and `windows-native` are absent.
- [ ] Derived projections alone drive the Rust generator, complete frozen fixture tree and digest,
      publication/readiness checkers, GitHub target matrix and exact native runner. Every selected
      test is listed exactly once, executes with `--exact`, proves one passed/zero ignored/zero
      filtered and emits a target/runner/inventory-identity report. Publication requires terminal
      reports from both targets and permits no `gap` row.
- [ ] The retained X-128 obligations are exactly `x128-expiry-live`,
      `x128-supervisor-sigkill-responsive`, `x128-supervisor-sigkill-wedged`,
      `x128-supervisor-unix-normal`, `x128-supervisor-unix-wedged` and
      `x128-unix-inherited-abi`, projected onto Linux only. The retained X-134 families are exactly
      `x134-c515-retained-lease`, `x134-connect-crash-replay`, `x134-four-form-sentinel`,
      `x134-grant-cas`, `x134-helper-deadlines`, `x134-hosted-origin-and-message-state`,
      `x134-local-management-deadlines`, `x134-native-owner-endpoint`,
      `x134-native-private-input`, `x134-native-service-account-handoff`,
      `x134-native-stream-framing` and `x134-production-root-safety`. X-138's
      exact portable C-515 release identity and Linux Exchange bindings are included. Counts remain
      derived; non-Linux families are absent, not copied into a negative or optional inventory.
- [ ] Failing first, `fixture_and_release_guards_are_derived_from_the_candidate_commit` proves the
      fixture inventory, hashes and selection are regenerated from the final candidate-bearing
      commit, followed by the required non-self-referential signed fixture commit. Both readiness
      and native-fixture checkers pass self-test and real mode without stale v1/five-target
      consumers. The release workflow runs publication readiness as the first post-checkout action;
      deriving a matrix or taking another candidate action before readiness is rejected.
- [ ] X-126 remains in progress: trust, signing, tag/manual-resume and public verification remain
      its blockers. This story may not claim a release, publish a crate or weaken the public
      verifier.

## Progress

- Immutable audit baseline `3dc7b28` contained a committed fixture selecting 12 cases while its
  release self-test required 13. That contradiction remains failing-first provenance for derived
  inventories; neither number is the new acceptance ratchet.
- 2026-08-07: unblocked. This story was filed `blocked` as the serialized tail of the X-134
  sequence; X-135, X-136, X-137 and X-138 are all `done` on canonical `origin/main`, so the exact
  production tests this story consumes now exist and it is dispatchable.

## Notes

- Child of X-134 and the final serialized integration boundary. It consumes, rather than predicts,
  the exact production tests from X-135–X-138.
