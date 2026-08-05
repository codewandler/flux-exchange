---
id: X-135
title: "Close hosted and native local-management deadlines"
status: in-progress
priority: 0
epic: connections
areas: [exchange-server, protocol, tests, windows]
depends_on: [X-125, X-127, X-128, X-129]
design: docs/designs/local-release-v1.md
note: "X-134 child — one admission clock, one durable-decision transition and bounded terminal behavior on every transport"
---

# Close hosted and native local-management deadlines

## Goal

Make the X-134 300-second pre-decision and 30-second post-decision budgets one non-resetting
production controller across hosted WebSocket, Unix and Windows transports, including cancellation,
blocked stores and terminal framing. No timeout may turn an uncertain durable write into an abort.

## Acceptance

- [x] Failing first, `real_store_decisions_at_299_and_300_select_the_only_safe_phase` proves that a
      durable write not started by 300 seconds is refused pre-decision, while a write already in
      flight at the boundary is retained as receipt-bearing post-decision roll-forward. Repeated
      observation of one receipt cannot reset 30 seconds and another receipt is an invariant
      refusal.
- [x] Failing first, `allocated_ceremony_drop_tombstones_only_until_the_decision_guard_disarms`
      drops begin, prepare, secret and commit futures through the real coordinator/provider. One
      armed cancellation guard aborts or tombstones every allocated pre-decision row and disarms
      atomically at durable decision; disconnect, protocol failure and future cancellation cannot
      abort afterward.
- [x] Failing first, `blocked_grant_and_mint_ports_do_not_block_the_deadline_runtime` holds the real
      grant store, Service Account store/audit and one-shot writer before and after decision. Owned
      worker tasks are raced by the controller; pre-decision cancellation is closed, post-decision
      work detaches and becomes query/replay-visible.
- [ ] The exact hosted test
      `hosted_slot_idle_and_ping_traffic_expire_on_the_admission_clock`, Unix test
      `authenticated_native_idle_and_partial_traffic_expire_on_one_absolute_clock`, and Windows
      test `supervised_windows_local_management_deadlines_are_phase_exact` each cover 299/300,
      29/30, traffic without reset, idle between frames, disconnect and recovery. WebSocket closes
      are exactly 1008 before decision or 1000 after it, with empty reasons; native streams end in
      clean EOF.
- [ ] Failing first, `backpressured_terminal_frame_reserves_the_required_close_or_eof` proves one
      separately bounded finalization operation on all transports. If the canonical FXLM error
      cannot be written safely, the mandatory close/EOF remains prioritized; no branch reuses an
      already-expired operation deadline or waits unbounded after terminal selection.
- [ ] Linux targeted tests, MinGW compilation and native `windows-2025` MSVC execution are selected
      by exact test name with one passed, zero ignored and zero filtered. This story narrows no
      opcode, refusal, receipt or no-secret invariant in X-134.

## Progress

- `fc22341a0ca73aa20d21d2bc02292f329ee745fe` is an intermediate X-134 checkpoint: admission anchors,
  same-receipt non-reset, phase-aware session abort and hosted/Unix idle expiry pass. The immutable
  deadline/cancellation audit still blocks the decision-at-boundary, cancellation-guard,
  blocking-port and complete terminal-finalization rows above.
- `46a331e` and `e272a16` close the real cancellation/store/audit/writer rows and hosted durable
  replay. The current X-134 integration adds one bounded terminal finalizer per transport: hosted
  WebSocket reserves an admitted FXLM frame and empty-reason close atomically before flushing; Unix
  retains and drains the read half only through the same one-second close budget; Windows uses the
  production authenticated named-pipe loop and explicit disconnect.
- Linux exact runs pass for every named controller, cancellation, hosted and Unix row. The Unix
  transport passed five consecutive 299/300 + 29/30 + replay + flood/backpressure executions after
  the retained-handle correction. MinGW compiles the complete Windows binary and dedicated
  `local_management_windows_deadline` integration target. `ci.yml` lists its sole exact test once
  and rejects any native MSVC report other than one passed, zero ignored and zero filtered.
- The descendant X-135 selector checkpoint gives hosted and Unix their own one-test integration
  targets over feature-gated production binary fixtures. Both list the contract name exactly once
  and report one passed, zero ignored and zero filtered; the hosted fixture checks sink readiness
  before both atomic reservations and the Unix fixture retains the half-closed handle only through
  the fixed terminal budget even while an authenticated peer floods unread bytes.
- Status remains `in-progress` until `windows-2025` executes that dedicated production named-pipe
  target and returns the required exact native report; cross-compilation is not recorded as runtime
  evidence.

## Notes

- Child of X-134. X-134 cannot complete while this story is unfinished or without explicit
  retirement evidence that satisfies the same parent Acceptance rows.
- The normative timing and close tables are in `docs/designs/local-release-v1.md`, “Hosted
  local-management transport” and “Deadline and terminal behavior”.
