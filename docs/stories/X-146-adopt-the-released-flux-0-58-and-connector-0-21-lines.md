---
id: X-146
title: "Adopt the released flux 0.58 and connector 0.21 lines"
pillar: "Core"
status: backlog
areas: [build, exchange-host, exchange-server, docs]
design: docs/designs/released-domain-audit.md
note: "post-v0.18.0: both pin sets move as one compatibility unit once connector-pack 0.21 requires flux-runtime ^0.58; the one known compile break in range is the C-531 DispatchId parameter on the flux-lang FlowSink impl"
---

# Adopt the released flux 0.58 and connector 0.21 lines

## Goal

Move Exchange onto the published connector 0.21 / flux 0.58 graph as one compatibility unit and
re-issue the released-domain audit for that line. This is scheduled after the v0.18.0 release
train: X-134, X-139 and X-126 build and freeze evidence on the 0.54 line, and nothing they consume
exists only in 0.55–0.58. Going first is impossible anyway — connector-pack 0.20 requires
`flux-runtime ^0.54`, so a lone flux bump would put two `Tool` traits in one lock, the exact
failure X-11 removed.

## Acceptance

- [ ] The registry preflight verifies connector-pack 0.21 requires `flux-runtime ^0.58` and every
      `codewandler-flux-*` engine crate this workspace pins resolves at 0.58.0 before the first
      manifest edit.
- [ ] Failing first: changing only `ENGINE_LINE` to 0.58 makes
      `the_engine_line_is_recorded_in_exactly_one_place` reject every stale pin by name.
- [ ] Both pin sets — all `codewandler-flux-*` engine pins and all four `codewandler-connector-*`
      pins — move together in one commit; the manifest, compile-time seam and lockfile engine-line
      tests prove there is no second line, and the independent `flux-spec` floor is raised to the
      published 1.4 line only if the resolved graph requires it.
- [ ] The `flux_lang::sink::FlowSink` implementation adopts the C-531 `DispatchId` parameters and
      pairs tool call/timing/result events by dispatch id rather than by op name.
- [ ] `EDITOR_SCHEMA_VERSION` and the console-served workflow schema are verified unchanged, or
      the schema bump is treated as a console contract change with its own changelog entry.
- [ ] The catalogue-derived safety censuses are re-audited against the 0.21 connector artifacts,
      and `docs/designs/released-domain-audit.md` is re-issued for the 0.58/0.21 line per its own
      update rule.
- [ ] The `no_second_request_path` dependency allow-list is unchanged, or every added name carries
      its written reason; `"flux_system"` remains a banned host source string.
- [ ] The Rust workspace gate, console tests/build and public-site build/tests pass; the changelog
      records the dependency move as a Changed entry for exchange-host consumers.
