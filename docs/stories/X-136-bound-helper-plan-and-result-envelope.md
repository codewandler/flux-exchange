---
id: X-136
title: "Bound helper plan validation and the absolute result envelope"
status: done
epic: connections
areas: [exchange-server, protocol, tests, windows]
depends_on: [X-135]
design: docs/designs/local-release-v1.md
note: "X-134 child — revalidate the complete v2 plan and keep every helper operation inside one 5s setup / 335s result envelope"
---

# Bound helper plan validation and the absolute result envelope

## Goal

Make the released Unix and Windows helpers revalidate the complete value-free v2 plan immediately
before mutation and obey exactly one 5-second request/setup cap plus one non-resetting 335-second
result cap, including private input, terminal writes and closure.

## Acceptance

- [x] Failing-first tests `every_plan_projection_fact_is_revalidated_before_connection_two` and
      `old_credential_head_replay_reaches_server_before_current_head_validation` cover canonical
      target order/revisions, settings, authorities, selection, state and durable head grammar. A
      nonzero 64-lowerhex old head reaches the server replay lookup even when it differs from the
      current selected head; the helper never substitutes current-head equality.
- [x] Failing-first `absolute_helper_deadlines_close_the_4_5_and_334_335_boundaries` proves request
      completion is five seconds from entry, endpoint plus both connections and PLAN validation
      share one five-second cap from request EOF, and all later reads, prompts, COMMIT handling,
      response write and EOF share one absolute 335-second cap from that EOF. NEED_SECRETS and
      COMMIT reset nothing, and helpers manufacture no server-owned 300/30 phase clock.
- [x] Unix `terminal_response_write_and_eof_share_the_absolute_result_deadline` and Windows
      `blocked_terminal_write_and_close_share_the_absolute_result_deadline` hold a partial/blocking
      response capability across 335 seconds and prove neither success nor error framing can cross
      it. Failure remains value-free and stdout/stderr stay empty.
- [x] Windows `blocked_console_read_is_cancelled_at_the_unchanged_outer_deadline` and the Unix real
      `/dev/tty` process counterpart hold private input beyond the same `result_by`. Console/TTY
      mode, echo and handles/descriptors are restored or closed on every exit without using stdio.
- [x] The production helper grammar remains exactly the X-134 Unix fixed-descriptor and Windows
      handle-list ABI. Linux process tests and native `windows-2025` MSVC tests execute by exact
      name with one passed, zero ignored and zero filtered; MinGW is compile-only evidence.

## Progress

- X-134 checkpoints `f863de6` and `23baba8` contain passing plan-validation and outer-envelope
  implementation slices. They remain incomplete until the exact process tests and native Windows
  evidence above are durably selected.
- The X-136 failing-first process run reached a canonical refusal before `/dev/tty` because the old
  harness encoded BEGIN targets as `{id, revision}` instead of the released `{target, revision}`
  grammar; after that correction it exposed an early whole-millisecond `poll` wake that could leave
  time to emit a terminal error after private-input expiry. The production reader now waits to the
  unchanged absolute instant, restores echo and closes value-free.
- `supervised_unix_helper_private_input_and_outer_deadline_are_exact` lists exactly once and passes
  with one passed, zero ignored and zero filtered. It launches the production binary twice: once
  for successful null-stdio `/dev/tty` input and an ordinary provider read after server exit, and
  once holding the real TTY beyond the shortened projection of the same immutable outer cap.
- MinGW compiles the complete binary plus `local_helper_windows_envelope`; the dedicated native
  test lists only `supervised_windows_helper_outer_deadline_is_exact`, and `windows-2025` is wired
  to require its one-passed/zero-ignored/zero-filtered MSVC report before the story can become done.
- `windows-2025` diagnostic run `30993306744` compiled the complete MSVC composition from dispatched
  source `cb1e99315b14febfc50c95b851b60422d5353ff4`, then selected
  `x134-windows-helper-envelope` and passed
  `supervised_windows_helper_outer_deadline_is_exact` with one passed, zero ignored and zero
  filtered. The matching Linux authority selection and MinGW compile-only path are green locally.

## Notes

- Child of X-134 and sequenced after X-135 so helper expectations cannot outrun the server timing
  contract. It preserves X-134 Acceptance lines 284–324 without deferring any parent blocker.
