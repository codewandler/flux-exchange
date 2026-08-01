---
id: X-29
title: "A partial delete does not overstate what survived, or understate why"
status: ready
priority: 2
epic: connections
areas: [exchange-server]
note: "found by X-18's belated review, 2026-08-01: `left_behind` names addresses that never held a value and calls them still usable, and a mixed-kind delete loop reports the FIRST failure kind — so a Denied address can be reported with 'retrying may work'"
---

# A partial delete does not overstate what survived, or understate why

## Goal
A partially-failed delete describes what actually happened: no address is called "still usable"
without evidence, and the refusal's kind reflects the worst failure, not the first.

## Two findings, from the review that finally ran on X-18

### 1. `left_behind` asserts more than it knows

A connector may legitimately hold a **subset** of what it declares — `a_connection_may_carry_a_subset_of_what_is_declared`
pins exactly that. So an address in `left_behind` may never have held anything at all. The refusal
nonetheless says:

> the ones in `left_behind` this host could not destroy — **treat those as still usable by anyone
> holding them**

Reproduced: `slack` connected with `bot_token` only, store armed to fail the second delete, and the
answer named `signing_secret` as still-usable at an address where `store.at(...) == None` in the same
run.

The bias is the **safe** one for a revocation surface, and it is documented — but the sentence states
flatly what the sibling hedges: `partly_written` says "Some credentials **may** remain",
`partly_destroyed` does not.

### 2. The first failure kind wins, not the worst

`failure.get_or_insert(error)` keeps the first error the loop sees. If address 1 fails `Unreachable`
and address 2 fails `Denied`, the refusal is `503` "retrying may work" **while a `Denied` address
sits in that same response's `left_behind`** — the exact misinformation X-18 and X-20 exist to end,
reappearing in the case where the loop sees more than one kind.

Not a regression: before X-18, `remove` returned on the first error and reported the same kind. It
self-corrects on retry, and both halves are named. But it is now the only place on this surface where
a `Denied` can be answered "retrying may work".

## Acceptance
- [ ] **Failing-first test** — a delete loop that sees both `Unreachable` and `Denied` answers with
      the kind an operator must act on, not the one that happened first. `TestStore` has no
      per-address failure control today; adding it is part of this story.
- [ ] `left_behind`'s wording no longer asserts an address is live when this host cannot know that.
      Either narrow the list the way `destroyed` is narrowed, or hedge the sentence the way
      `partly_written` does — decide which, and say why the other is wrong.
- [ ] Whichever is chosen, the **safe bias is preserved**: this is a revocation surface, so
      "possibly still live" must never read as "definitely gone".
- [ ] The three caller-facing sentences stay byte-identical, still pinned by
      `a_store_failure_says_what_it_has_always_said`.

## Notes
- Read X-18's and X-20's Progress notes first; both argued about exactly this vocabulary and the
  reasoning should not be re-litigated from scratch.
- The review also observed that at X-18's merge base the only guard on those sentences was a
  `contains("Retrying may work")` substring check — the full-sentence pin arrived with X-20,
  *after* the refactor it is now credited with protecting. Nothing wrong shipped, but do not treat
  that pin as having covered X-18.
