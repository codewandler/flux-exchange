---
id: X-112
title: "Align the Exchange roadmap with rich connector runtimes"
status: done
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "state explicitly that Docker/Kubernetes/SQL/observability remain connectors and are hosted through declared runtimes; replace stale 'not yet filed' text with the complete program"
---

# Align the Exchange roadmap with rich connector runtimes

## Goal

Make Exchange's vision, README, roadmap and backlog describe one accepted hosted connector model and
its honest implementation gaps.

## Acceptance

- [x] The vision and README explicitly include rich protocols and the connector/runtime ownership
      split without claiming non-HTTP operation dispatch already ships.
- [x] The roadmap replaces stale unfiled runtime/lease work with X-111 and X-113…X-120, linking the
      delivered channel/workflow foundations.
- [x] The docs reference the delivered Service Account and sibling migration programs rather than
      duplicating them.
- [x] The engineering changelog records the roadmap alignment and generated story board remains
      consistent with frontmatter.
- [x] Relevant docs/site tests pass; no capability page claims a future executor is live.

## Progress

- 2026-08-03: Done after cross-repository and linked-worktree audit; the delivered generated-channel
  slice and future general rich-runtime dispatcher are now distinguished explicitly.

## Notes

- Documentation-only; no failing-first behavioral test applies.
- X-124 supersedes this story's earlier optional-hosted-placement assumption. The reconciled
  roadmap now makes Exchange the sole executor for official integrations and splits the first HTTP
  milestone from rich-runtime lifecycle work.
