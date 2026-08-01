---
id: X-55
title: "Lock 2 sees the crate that composes, or says why it does not"
status: ready
priority: 2
epic: invoke
areas: [exchange-host, exchange-server]
note: "found by X-48 round 2: lock 2 scans crates/exchange-host/src only. exchange-server gets one rule and is otherwise unscanned — including execution.rs, which holds this composition's transport and sandbox posture"
---

# Lock 2 sees the crate that composes, or says why it does not

## Goal
The boundary of what the locks cover is a decision, not the shape of one `sources_under` call.

## What was found

`no_second_request_path.rs` scans `crates/exchange-host/src`. `exchange-server` gets exactly one rule
— it may not name `connector_pack` — and is otherwise unscanned.

That is arguably right: **`exchange-host` is the published crate, and locks 1–2 are the whole of what
ships.** A consumer binds its own `Contexts` and may well pass `System::new` with the sandbox
disabled, so the posture in `execution.rs` is a property of *this repository's composition* and not of
the package. X-48 round 2 wrote that down.

But it leaves a gap the same round created an argument around. `execution.rs` holds this
composition's transport **and** its sandbox posture, and the four-mechanism section presents that
posture as the backstop for lock 2's admitted blind spot — a backstop sitting outside what lock 2 can
see. Meanwhile a second request path added to `exchange-server` is caught by nothing structural.

## The question to settle

Are the locks about **the published crate** or about **the deployed binary**? They are different
claims and the repository currently makes the first while several documents read as the second.

If the answer is "the crate", then the honest fix is a sentence, not a wider scan — and the
four-mechanism section must stop leaning on a composition-level backstop to cover a crate-level
blind spot. If the answer is "the binary", the scan widens and `execution.rs` needs an exception list
with an argument per entry, the way lock 1's allow-list works.

## Acceptance
- [ ] The question above is answered in `docs/designs/invoke.md`, with the argument, not in a comment.
- [ ] **Failing-first test** — whichever boundary is chosen, a violation on the wrong side of it is
      caught. If the boundary stays where it is, the test is that the *claim* narrows: no document may
      present a composition-level control as covering a crate-level gap.
- [ ] `AGENTS.md`'s statement of the invariant agrees with whichever answer is chosen.

## Notes
- Do not widen the scan reflexively. `exchange-server` legitimately holds a transport — that is what
  a composition is for — so a naive widening turns lock 2 red on correct code, and the exception list
  it would need is exactly the "one more file on the list" drift the locks exist to avoid.
