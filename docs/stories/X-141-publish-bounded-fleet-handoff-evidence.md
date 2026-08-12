---
id: X-141
title: "Exchange story workers publish bounded Fleet handoff evidence"
status: done
priority: 1
areas: [agents, scripts, tests]
note: "Fleet dogfood — one deterministic targeted-check receipt without terminal scraping or duplicate full gates"
---

# Exchange story workers publish bounded Fleet handoff evidence

## Goal

Give an isolated native Fleet story worker one Exchange-owned way to run a closed targeted check and
return a small deterministic receipt before the repository's final integrated gate.

## Acceptance

- [x] Failing first, a hermetic self-test proves a free-form test transcript can grow without bound
      and does not bind the exact story, commit, check profile or terminal outcome.
- [x] A repository script accepts an explicit story id and closed targeted-check profile, refuses a
      dirty or mismatched checkout before execution, and emits one versioned JSON receipt.
- [x] The receipt carries commit, profile, exact commands, outcomes and durations under a hard byte
      ceiling. Oversized stdout/stderr becomes atomic byte-count plus digest metadata, never sliced
      JSON or retained native-evidence, console-build or compiler output.
- [x] Unknown profiles, timeout, signal termination and command failure remain typed terminal
      outcomes. The script never changes Board/Fleet state, creates branches/worktrees, runs the full
      repository gate, pushes, publishes, releases or uses tmux as IPC.
- [x] Hermetic self-tests cover pass, fail, timeout, oversized output, unknown profile and dirty
      checkout behavior without network, release keys or provider credentials.
- [x] Contributor guidance describes this as targeted story evidence only; the complete Cargo,
      console, web, security and release-policy gate still runs once on the integrated wave.
