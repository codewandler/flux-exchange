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
