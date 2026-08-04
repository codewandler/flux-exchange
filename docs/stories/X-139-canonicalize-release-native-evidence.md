---
id: X-139
title: "Canonicalize the release-native fixture inventory"
status: blocked
epic: connections
areas: [exchange-release, tests, workflows, docs, windows]
depends_on: [X-135, X-136, X-137, X-138]
design: docs/designs/local-release-v1.md
note: "X-134 child — replace the historical 13-case projection with one canonical JSON authority and final five-target reports"
---

# Canonicalize the release-native fixture inventory

## Goal

Turn the historical 13-case release-native inventory into one canonical, derived and
publication-blocking authority for the final X-134 tree. The authority names every inherited and
X-134 obligation, exact test and native runner once; no copied list, literal count or stale fixture
may certify publication.

## Acceptance

- [ ] Failing first, `native_evidence_authority_rejects_each_missing_family_target_and_test`
      removes or substitutes each authority class, family, target, runner, feature and exact Cargo
      binding independently and makes publication fail. The sole source is
      `crates/exchange-release/native-evidence-v1.json`; no TSV, Python/digest oracle,
      `native_fixture_cases()`, Rust expected list, copied YAML target matrix, numeric 13/19 ratchet
      or `EXPECTED_MATRIX_SHA256` remains.
- [ ] The JSON models literal X-134, inherited X-128 and inherited C-515 authority classes; native
      target/runner sets; obligations; exact Cargo tests/features; named adversarial cases;
      inherited release evidence; and the cross-cutting connector-secrets 0.20 lease assertion. The
      historical 13-case assessment is input only—the final family/binding totals are derived from
      the complete contract and are never hard-coded as acceptance.
- [ ] Derived projections alone drive the Rust generator, frozen fixture tree, publication checker,
      GitHub runner/target matrix and exact native runner. Every selected test is listed exactly
      once, executed with `--exact`, proves one passed/zero ignored/zero filtered, and emits a
      target/runner/inventory-identity report. Publication requires terminal reports from all five
      native targets and permits no `gap` row.
- [ ] Exact families cover unsafe production-root ancestry; Unix/Windows owner PLAN and peer/TCP
      adversaries; private input; SCM_RIGHTS and FXHA positive/refusal/canary/surface exclusion;
      pre/post-decision crash; restart QUERY/replay; concurrent grant CAS; four-form sentinel scans;
      X-134 Acceptance 507–513 hosted-origin, clock-boundary, native-stream and hosted-message-state
      obligations; all nine inherited X-128 cases; and all inherited C-515 tests from X-138.
- [ ] Failing first, `fixture_and_release_guards_are_derived_from_the_candidate_commit` proves the
      fixture inventory, hashes and selection are regenerated from the final candidate-bearing
      commit, followed by the required non-self-referential signed fixture commit. Both readiness
      and native-fixture checkers pass self-test and real mode without stale v1 consumers.
- [ ] X-126 remains in progress: its nine X-128/fourteen-binding claim is kept distinct from X-134
      additions, and public signing/tag/manual-resume verification remains an explicit X-126
      blocker. This story may not claim a release, publish a crate or weaken X-126's public verifier.

## Progress

- Immutable audit baseline `3dc7b28` contained a committed fixture selecting 12 cases while its
  release self-test required 13. That exact contradiction is failing-first provenance for replacing
  copied inventories; 13 is not the final authority or a numeric acceptance ratchet.
- The committed fixture predating this story is incomplete, and rejected commits `7c1b238` and
  `5e2b3f4` must never be integrated. Useful list/`--exact` mechanics may be reimplemented only from
  the final product-test names delivered by X-135 through X-138.

## Notes

- Child of X-134 and the final serialized integration boundary. It consumes, rather than predicts,
  the exact production tests from X-135–X-138.
