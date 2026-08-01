---
id: X-31
title: "A new exchange error cannot silently inherit a refusal's status"
status: ready
priority: 3
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
- [ ] **Failing-first test** — an `ExchangeError` variant mapped to a refusal that does not match its
      kind is caught. Since the point is to make the class hard to reach, a compile-time argument is
      acceptable; say plainly which you are giving.
- [ ] Every `ExchangeError` variant is named individually against the refusal it produces, so adding
      a variant forces a decision rather than inheriting one.
- [ ] The four back-channel refusals still share **one** caller-facing string and **one** status —
      the split is in the log only, and `a_refusal_tells_the_caller_nothing_about_the_provider`
      stays green.
- [ ] X-17's distinctions survive unchanged: `ClientRefused`, `UnpublishedKey`, `NoIdToken` and
      `Rejected` remain distinguishable to an operator reading a log.

## Notes
- `SessionError` -> `SignInRefusal::NoSession` has the same shape and was checked during X-17: all
  four `SessionError` variants render distinctly, so the log already separates them. Confirm that is
  still true rather than assuming, and consider whether it deserves the same pin.
- Do not widen what reaches the caller. Every one of these refusals is deliberately opaque outside
  the log, and several stories have argued that; this story is about the *log* and the *type*.
