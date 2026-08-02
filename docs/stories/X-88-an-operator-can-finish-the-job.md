---
id: X-88
title: An operator can finish the job without translating the API
status: done
priority: 0
design: docs/designs/operator-journey.md
areas: [console, exchange-server, catalogue]
note: "owner-directed 2026-08-02: turn the ten highest-impact UX findings into one connect → grant → invoke journey and include it in v0.13.0"
---

# An operator can finish the job without translating the API

## Goal
Make the console an actionable operator workflow rather than a set of accurate but isolated
reference screens. A signed-in person can discover a connector, connect it, grant it, invoke one of
its operations, recover from failures, and understand what changed without translating endpoints or
machine vocabulary by hand.

## Acceptance
- [x] Connections, grants and invocation present one visible three-step setup journey with contextual
      next actions; the current tenant state, never page-local optimism, decides completion.
- [x] Connector selection in Connections and Grants is a searchable combobox showing vendor, id,
      description and connection state rather than a machine-id-only select.
- [x] A stored credential can be rotated atomically from its connection card; the value remains in
      the input and request only and never appears in state, a URL, a response or rendered output.
- [x] Connections are status cards (connected, partial, needs attention) with addresses under
      progressive detail and actions beside the connector.
- [x] Grants offer conservative presets and a custom mode, still producing only metadata selectors
      and never operation-id allow/deny lists.
- [x] Grant preview leads with admitted/declared counts, groups operations by service and risk, and
      marks authority as narrower, unchanged or wider than the held grant before save.
- [x] The catalogue publishes the operation's projected input schema and the console has a signed-in
      Invoke screen with operation choice, JSON validation, result/refusal rendering and elapsed time.
- [x] Every failed or stale read has an in-context Retry action; loading uses stable skeletons rather
      than moving paragraphs, without collapsing failure and empty states.
- [x] The shell has a compact phone navigation which keeps built surfaces primary and places honest
      future surfaces under an explanatory disclosure.
- [x] Catalogue search supports `/` focus, highlighted matches, result-preserving breadcrumbs and
      service grouping without changing relevance as the default ordering.
- [x] Keyboard/focus/reduced-motion behavior, light/dark token use and phone layouts are tested; all
      workspace, console, web and release gates pass.
- [x] X-88 is in the v0.13.0 CHANGELOG, the board is current, versions agree, the release commit is
      tagged `v0.13.0`, and publishing is left to the tag-triggered CI workflow.

## Progress
- 2026-08-02 — owner accepted the ranked ten-item UX audit and asked for the complete package plus a
  release. The untagged tree already names 0.13.0, so this story joins that release.
- 2026-08-02 — implementation and every workspace, console, site and release checker passed. The
  release commit carries this completed story; `v0.13.0` is cut from that commit immediately after.
