---
id: X-137
title: "Prove Windows private input and FXHA in production"
status: in-progress
epic: connections
areas: [exchange-server, protocol, tests, windows]
depends_on: [X-136]
design: docs/designs/local-release-v1.md
note: "X-134 child — native MSVC process evidence for CONIN$, owner pipes and the Decision 0007 FXHA writer attachment"
---

# Prove Windows private input and FXHA in production

## Goal

Close the Windows-only released process boundary: secrets enter through the real console, ordinary
FXLM remains multi-frame, and the exact same authenticated named-pipe client attaches one validated
writer HANDLE to the immediately following MINT without a new protocol or secret-bearing state.

## Acceptance

- [ ] Failing first, native MSVC test
      `supervised_windows_service_account_helper_delivers_exact_fxsa_and_closes_fxha_adversaries`
      launches the production supervised server and production `local service-account-mint` helper,
      reads one exact FXSA frame plus EOF concurrently, requires helper exit 0 and queries the
      durable receipt. The helper handle list contains only the writer and an unrelated inheritable
      canary is absent from the child.
- [ ] The same production-process test proves ordinary PLAN and one multi-frame CONNECT or GRANT
      ceremony still use `ActiveSession`; FXHA is exactly the committed 16-byte prelude followed by
      the immediate MINT on that one authenticated connection. No one-shot parser replaces the
      ordinary FXLM state machine.
- [ ] Table-driven native cases cover truncated/mutated fields, alias, wrong object/direction,
      inheritable or unusable handle, wrong PID/creation/SID/session, wrong process, extra byte,
      second FXHA and non-MINT opcode. Every source/duplicate is closed on all exits and endpoint
      rearm occurs only after the authenticated pipe and pinned client are dropped.
- [ ] Failing first, `windows_private_console_input_survives_null_stdio_and_restores_mode` drives
      production `CONIN$` input with null standard streams and covers cancellation at X-136's
      unchanged outer deadline. No token, numeric HANDLE or canary appears raw, JSON-escaped,
      percent-encoded or base64 in logs, receipts, persistence or diagnostics.
- [ ] Implementation uses the security-sensitive Windows APIs named by Decision 0007: same-pipe
      `GetNamedPipeClientProcessId`, pinned process creation identity and token/session validation,
      server-side `DuplicateHandle`, noninheritability/distinct writable `FILE_TYPE_PIPE`
      validation. Documented `PeekNamedPipe` may inspect only queued byte availability and its
      prefix, without consuming data, on that same authenticated pipe solely to classify immediate
      surplus before MINT. Remote argv/PEB, undocumented process-information and handle-table
      enumeration remain structurally absent.
- [ ] MinGW proves the production path is referenced; the exact test above executes only on
      `windows-2025` MSVC with one passed, zero ignored and zero filtered.

## Progress

- X-134 already contains typed helper/endpoint/duplication primitives and passing MinGW checks.
  Those are supplemental and do not satisfy the native process rows in this story.
- The production process test now contains exactly one test named
  `supervised_windows_service_account_helper_delivers_exact_fxsa_and_closes_fxha_adversaries`.
  It launches the real supervised server and suspended released helper, proves the helper handle
  list excludes a live inheritable canary before resume, reads exact FXSA plus EOF, requires helper
  exit zero, queries the durable receipt, and keeps ordinary PLAN plus multi-frame CONNECT on the
  shared endpoint state machine.
- Its table mutates every fixed FXHA field and covers zero/planted, aliased, wrong-object and
  wrong-direction handles, extra bytes, second FXHA, non-MINT, truncated-client rearm and durable
  state stability. Production now revalidates client PID, creation identity, SID, session and
  liveness immediately before `DuplicateHandle`; the pure production predicate has mutation tests
  for every component.
- MinGW links the complete process test. Native `windows-2025` execution and the real `CONIN$`
  execution remains required before any Acceptance row is checked.
- The same single production-process test now hosts the released vendor helper in a supported
  pseudoconsole, passes only request-read/response-write in its explicit handle list, and proves an
  unrelated inheritable canary is absent. Production nulls all three standard handles, opens the
  real `CONIN$`, and a feature-only non-secret probe on that console observes echo disabled before
  input and the exact original mode restored after success and deadline cancellation. The test
  reads the committed secret back through the label's durable instance after server exit, while
  four-form scans exclude it from the helper response, console transcript, server diagnostics and
  every non-provider state file.
- MinGW clippy with warnings denied and a full test-executable link are green with the console,
  helper-deadline and owner-root features. CI selects the one exact test with a one-pass,
  zero-ignored, zero-filtered assertion; only its native `windows-2025` MSVC execution can close the
  remaining evidence boundary.

## Notes

- Child of X-134, sequenced after X-136's helper clocks and grammar. Roadmap Decision 0007 items 10
  and 17 at authority commit `904e382` define the FXHA boundary; this story may not invent a ninth
  protocol.
