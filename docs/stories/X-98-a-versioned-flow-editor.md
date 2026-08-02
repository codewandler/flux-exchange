---
id: X-98
title: A versioned visual and source flow editor
status: done
epic: flow-editor
design: docs/designs/flow-editor.md
note: "connector operations plus the pure cognition group; immutable publication, dual grants and node-addressed run records"
---

# A versioned visual and source flow editor

## Goal
Let a tenant author one invocable Flux flow visually or as source, publish immutable versions, and
run the published operation without a credential or tenant ever becoming caller-controlled.

## Acceptance
- [x] Failing-first API tests prove tenant-derived CRUD, optimistic draft conflicts and immutable
      publication.
- [x] The editor catalogue contains connector operations and only the pure cognition built-ins.
- [x] Source and graph edits use Flux's upstream projection; unsupported valid source is preserved
      and remains source-only.
- [x] A published workflow is a tenant-local operation whose entry grant and every nested connector
      grant are both required before credentials are read.
- [x] Runs target an immutable version, are cancellable, persist redacted node status and refuse a
      changed operation contract until republished.
- [x] The Vue console provides Workflows and Activity surfaces with tree/freeform/source modes,
      validation, publication and run inspection.
- [x] Rust, console and public-site gates are green; README, onboarding, CHANGELOG and board agree.

## Progress
- 2026-08-02: story and design opened from the accepted implementation plan.
- 2026-08-02: upstream L-126 implemented and fully gated in the sibling Flux checkout: versioned
  projection/lowering, source-only diagnostics, stable node ids and value-free execution tracing.
- 2026-08-02: exchange integration is release-blocked. The newest published connector pack checked
  (`0.15.0`) still requires Flux `0.49`. A registry and remote check then showed Flux `0.52.0` is
  already published and current Flux main is ahead of the dirty local 0.51.1 tree carrying L-126, so
  targeting 0.51 would knowingly create another stale trait seam.
- 2026-08-02: flux-connectors C-488 now prepares the complete registry-only 0.49 → 0.52 bridge. Its
  engine-line tests, generated-artifact fixed point, workspace build/test/clippy/fmt gate and
  four-crate dependency closure are green; it is intentionally uncommitted and unpublished. The
  remaining order is: integrate/gate L-126 on current Flux main, publish Flux on the 0.52 line,
  commit and publish the compatible connector pack, then move both exchange pin sets atomically.
- 2026-08-02: the dependency sequence completed: L-126 is published on Flux 0.52 and
  flux-connectors 0.16.0 is published against that line. Exchange moved the pins together with no
  local override.
- 2026-08-02: tenant-derived draft/version APIs, dual-gated execution, contract drift refusal,
  cancellation, durable value-free run tracing and the Workflows/Activity console landed. The
  console visually adds/reorders/removes calls through the upstream graph contract and preserves
  exact source when source mode is authoritative.
- 2026-08-02: workspace build/test/clippy/fmt, console test/build and public-site build/test are
  green for the v0.14.0 release candidate. A final failing-first storage check also made the
  definitions directory/file and run database explicitly refuse group/other access.

## Notes
- Delivered from published Flux 0.52 and connector-pack 0.16.0. The registry-only rule remains: do
  not replace either with a sibling `path` or `git` dependency.
