---
id: X-140
title: "Repository agents use the installed Board and Fleet operator workflow"
status: ready
priority: 0
areas: [agents, docs, tests]
note: "Fleet dogfood — replace private Track workflow and pin the bounded handoff/watch commands a plain agent can run"
---

# Repository agents use the installed Board and Fleet operator workflow

## Goal

Make an Exchange story operable from the released Flux Board/Fleet CLI by a plain agent, with no
private Track plugin, tmux control channel or hand-written state-file edits.

## Acceptance

- [ ] Failing first, a focused repository contract test proves the mandatory `AGENTS.md` workflow
      names private Track behavior but does not give a plain agent executable Board/Fleet commands.
- [ ] The Track marker keeps the Exchange story and gate policies but uses copyable
      `flux board --root .` commands for selection, transition, evidence, done, check and sync.
- [ ] A short Fleet handoff section shows the addressed acknowledgement path, bounded worker/result
      inspection and live NDJSON events. It says dashboard is a snapshot and tmux is operator view,
      never IPC.
- [ ] The text distinguishes `accepted`, `delivered` and `completed`; it never claims an accepted
      journal entry means the main agent or worker has answered.
- [ ] A hermetic fixture test executes every documented read-only Board command, verifies the
      mutating Board/Fleet examples exist in the installed schemas, and rejects stale `/track:*`
      commands without modifying this checkout.
- [ ] The public and contributor documentation remains honest about Exchange's Linux-only runtime,
      then the focused test and ordinary integrated repository gate pass.

## Progress

- 2026-08-12: **A partial implementation of this story exists and was nearly lost.** It was recovered
  from `stash@{1}` (`codex-preserve-failed-wave167-exchange-x140-before-cleanup-20260806`) onto the
  branch `rescue/X-140-installed-board-fleet-workflow`, which is pushed. `scripts/check-agent-workflow.sh`
  existed in **no commit anywhere** in the repository; the stash was `apply`-ed rather than popped,
  so it is still in place too.

  **That branch is a preservation point, not a proposal.** Run against this tree the checker reports
  two failures — a stale private `/track` command in `docs/stories/README.md`, and the documented
  Board check command failing against its hermetic fixture — and its `.github/workflows/ci.yml` change
  wires the checker into CI, so merging as-is lands a red gate. It also carries
  `.flux/board/idempotency.json`, which is local run state rather than repository content and must be
  dropped before any merge.

  Owner-stated 2026-08-12: **the `/track` tooling will stop being used within a day or two.** That is
  what makes this story live rather than speculative — the migration it describes is the direction of
  travel, not a hypothetical. Whoever picks it up should treat the rescued branch as a head start on
  `check-agent-workflow.sh` and finish the half that never landed, rather than starting over.

  Note the ordering consequence: `/track:board` is still the tool that regenerates
  `docs/stories/README.md` today, and the rescued checker refuses exactly that. Those two cannot both
  be right, so this story owns the cutover — including what regenerates the board afterwards.
