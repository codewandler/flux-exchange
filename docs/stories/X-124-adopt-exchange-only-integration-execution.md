---
id: X-124
title: "Adopt Exchange-only official integration execution"
status: done
priority: 0
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "Milestone 0 — reconcile X-111 through X-120 with the cross-repository execution decision before runtime implementation"
---

# Adopt Exchange-only official integration execution

## Goal

Make Exchange's rich-runtime contracts agree with the cross-repository decision that every official
external integration executes through Exchange. This correction must leave an independently
shippable one-shot HTTP milestone and move streams, cancellation and leases back to lifecycle work.

## Acceptance

- [x] `AGENTS.md` identifies `../flux-roadmap` as the scheduling authority for work named by its
      program while preserving repo-local Goal and Acceptance as the implementation contract.
- [x] X-111 and `docs/designs/rich-connector-runtimes.md` require Exchange-only execution for official
      integrations; they contain no local-first Flux placement or local vendor/plugin fallback.
- [x] X-113 covers the authenticated effective Service Account catalogue and existing one-shot HTTP
      invocation contract only, including stable generation identity, tenant/grant derivation and no
      credential or caller-selected authority.
- [x] Streams, cancellation, terminal outcomes and leases remain owned by X-117 and X-118, so they do
      not block the Milestone 1 HTTP path.
- [x] X-114, X-115 and X-119 state that connector-declared plans and attested artifacts are installed
      and executed by Exchange; Flux contributes guarded runtime substrate without becoming a second
      official-integration execution placement.
- [x] X-120 proves the accumulated migration corpus through local single-tenant Exchange and does not
      require hosted multi-tenant isolation, which remains X-116.
- [x] A repository contract check fails first when the corrected epic/design/story vocabulary drifts
      back to a second official execution placement, then passes with the reconciled documents.
- [x] The generated story board and roadmap-facing notes describe the corrected dependency split.

## Progress

- 2026-08-03: Filed from flux-roadmap's preservation/adoption schedule after the worktree audit found
  the primary Exchange checkout dirty on an obsolete wave branch. Implementation must use a fresh
  canonical worktree and must not modify or clean that checkout.
- 2026-08-03: Started from canonical `origin/main` in an isolated story worktree. The repository
  contract was added before the reconciliation and failed against the former local-placement and
  combined-protocol wording.
- 2026-08-03: Done. X-111 through X-120, the accepted design, vision, README and roadmap now carry
  one official execution placement. X-113 is the ready Milestone 1 HTTP contract; X-117/X-118 own
  lifecycle; X-116 alone owns hosted isolation; and the ordinary Rust gate enforces the split.

## Notes

- Cross-repository authority: `../flux-roadmap/decisions/0001-exchange-executes-official-integrations.md`.
- Flux and connector repositories receive their own atomic adoption stories in the same tranche.
- This story changes contracts and their drift guard; it does not implement rich runtime execution.
