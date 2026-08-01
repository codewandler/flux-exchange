---
id: X-31
title: "A new exchange error cannot silently inherit a refusal's status"
status: done
epic: serve
areas: [exchange-server]
note: "found by X-26's implementor in the guard it had just written, 2026-08-01: every_refusal_states_the_status_it_answers_with pins the refusal→status edge, and nothing pins the error→refusal edge, so a new ExchangeError folded into an existing refusal would undo X-17's split without touching status()"
---

# A new exchange error cannot silently inherit a refusal's status

## Goal
The mapping from `ExchangeError` to `SignInRefusal` is as hard to get wrong as the mapping from
`SignInRefusal` to a status.

## The gap, found by the story that closed the other half

X-26 moved the status onto `SignInRefusal` and pinned it variant by variant, so a status cannot
change without a test saying so. Its implementor then named what that test does **not** cover:

> `every_refusal_states_the_status_it_answers_with` guards the refusal→status edge, not the
> error→refusal edge. A new `ExchangeError` folded into an existing refusal would silently inherit
> its status and undo X-17's split without touching `status()` at all.

X-17 exists precisely because four causes were collapsed into one refusal and an operator could not
tell their own misconfiguration from a caller's refused credential. The collapse is one careless
`From` arm away from returning, and the test that looks like it would catch that does not.

## Acceptance
- [x] **Failing-first test** — an `ExchangeError` variant mapped to a refusal that does not match its
      kind is caught. Since the point is to make the class hard to reach, a compile-time argument is
      acceptable; say plainly which you are giving.
- [x] Every `ExchangeError` variant is named individually against the refusal it produces, so adding
      a variant forces a decision rather than inheriting one.
- [x] The four back-channel refusals still share **one** caller-facing string and **one** status —
      the split is in the log only, and `a_refusal_tells_the_caller_nothing_about_the_provider`
      stays green.
- [x] X-17's distinctions survive unchanged: `ClientRefused`, `UnpublishedKey`, `NoIdToken` and
      `Rejected` remain distinguishable to an operator reading a log.

## Notes
- `SessionError` -> `SignInRefusal::NoSession` has the same shape and was checked during X-17: all
  four `SessionError` variants render distinctly, so the log already separates them. Confirm that is
  still true rather than assuming, and consider whether it deserves the same pin.
- Do not widen what reaches the caller. Every one of these refusals is deliberately opaque outside
  the log, and several stories have argued that; this story is about the *log* and the *type*.

## Progress
- **Done 2026-08-01.** Gate green: 43 + 180 tests, clippy clean, fmt clean. No production behaviour
  changed; the only non-test edit is a doc comment.
- **The premise was checked mechanically, not argued.** At the merge base, changing one line —
  `NoIdToken => CodeRejected` — leaves build, 43 + 178 tests and clippy `-D warnings` **entirely
  green** while X-17's split is undone. Neither existing guard can see it: one constructs the
  refusal directly, the other asserts upstream of the mapping.
- **Both halves are closed.** Adding a variant does not compile until the decision is written down;
  reusing an existing refusal fails an **injectivity** assertion — the half a pairing test
  structurally cannot catch, and the shape a careless author actually writes.
- **A structural alternative was considered and rejected with reasons:** collapsing the four
  back-channel refusals into `SignInRefusal::BackChannel(ExchangeError)` would make the mapping 1:1
  by construction, but would delete the four variants X-26 pinned one by one — trading this story's
  guard against the last story's.
- **`SessionError` -> `NoSession` was confirmed rather than assumed**, and pinned *differently*: that
  edge cannot fold, because `NoSession` carries its source instead of replacing it. What can rot is
  the delegation that makes one arm acceptable, so that is what the test asserts.
- `SignInRefusal::NoFlow(FlowError)` is the third edge of this shape and is deliberately unpinned —
  `FlowError` has one variant, so a test would assert only that one thing differs from itself. Worth
  a pin the day a second is added; the argument is written down beside the `SessionError` test.
