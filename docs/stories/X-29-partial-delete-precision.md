---
id: X-29
title: "A partial delete does not overstate what survived, or understate why"
status: done
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
- [x] **Failing-first test** — a delete loop that sees both `Unreachable` and `Denied` answers with
      the kind an operator must act on, not the one that happened first. `TestStore` has no
      per-address failure control today; adding it is part of this story.
- [x] `left_behind`'s wording no longer asserts an address is live when this host cannot know that.
      Either narrow the list the way `destroyed` is narrowed, or hedge the sentence the way
      `partly_written` does — decide which, and say why the other is wrong.
- [x] Whichever is chosen, the **safe bias is preserved**: this is a revocation surface, so
      "possibly still live" must never read as "definitely gone".
- [x] The three caller-facing sentences stay byte-identical, still pinned by
      `a_store_failure_says_what_it_has_always_said`.

## Notes
- Read X-18's and X-20's Progress notes first; both argued about exactly this vocabulary and the
  reasoning should not be re-litigated from scratch.
- The review also observed that at X-18's merge base the only guard on those sentences was a
  `contains("Retrying may work")` substring check — the full-sentence pin arrived with X-20,
  *after* the refactor it is now credited with protecting. Nothing wrong shipped, but do not treat
  that pin as having covered X-18.
- **A third finding, recurring:** `no_answer_or_refusal_carries_a_credential_value` claims in its own
  doc to drive *every* answer and refusal this module can produce. It does not drive
  `partly_written`'s two branches (noted by X-20), and now not `allowance_change_in_flight` either
  (noted by X-25). No disclosure has actually been found — each new refusal names only a connector
  id and addresses — but a test whose doc overstates its coverage is worse than one that admits the
  gap, and this is the third story to record the same drift. Fix the claim or fix the coverage.

## Progress
- **Done 2026-08-01.** Gate green: 43 + 182 tests. Genuine merge-base failure — both tests failed at
  the base with all diff hunks inside `mod tests`, and the final form was re-proved after commit.
- **Hedging was chosen over narrowing, and the argument is in the code.** The pre-delete probe is
  stale by the time the loop runs — that staleness is the *stated* reason `remove` deletes the whole
  declared set — so the addresses a narrowing would drop are exactly the ones this host has no
  evidence about. `destroyed` can be narrowed because its failure mode is **over**-reporting a
  revocation; `left_behind`'s is **under**-reporting one. Not the same risk.
- The list is byte-for-byte the same set and the safe instruction is unchanged; only the claim moved,
  from "these are live" to "a credential may remain at any of them".
- **The mixed-kind test drives all four orderings**, so "the worst" cannot be satisfied by an
  implementation that merely keeps the last. `TestStore` gained per-address failure control — which
  is why no earlier story caught this: neither the global flag nor the counter could arm two kinds
  in one `remove`.
- The third finding is closed too: the disclosure test now drives both of `partly_written`'s
  branches and its doc **lists what it reaches** instead of claiming it reaches everything.
  `allowance_change_in_flight` is still undriven and the doc now says so.
- **Carried forward:** a partial `DELETE` now answers `502` rather than `503` whenever any address
  failed `Denied`/`Backend`/`Layout`. Anything retrying on `503` and not `502` changes behaviour.
- **Carried forward:** `TestStore`'s failure controls are now five overlapping knobs whose precedence
  exists only in the order of the `if`s. Due a consolidation; deliberately not done here.
