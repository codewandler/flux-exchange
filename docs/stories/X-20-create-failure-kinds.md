---
id: X-20
title: "A create refused because the store denied us does not say 'retrying may work'"
status: done
epic: connections
areas: [exchange-server]
note: "found by X-18's implementor, 2026-08-01: partly_written flattens every store-failure kind to 503, so a create refused because the store Denied this host's access tells the caller to retry — the same defect class X-18 fixed on the delete side"
---

# A create refused because the store denied us does not say "retrying may work"

## Goal
A partially-written create reports the store failure's **kind**, the way a partially-failed delete
now does.

## What is wrong

X-18 fixed this on the delete side and deliberately left the create side alone, because changing it
was outside that story's Acceptance and would alter tested behaviour. The defect is the same:
`partly_written` flattens every `SecretStoreError` kind to `503` and the generic "Retrying may
work". A create refused because the store **denied this host access** is not a condition retrying
resolves, and an operator told to retry goes and does that instead of fixing the permission.

`AGENTS.md` requires failures an operator responds to differently to stay distinguishable, and
`store_failed`'s own doc argues the point at length — it is `partly_written` that does not follow it.

## Why this is cheap now

X-18 factored `store_failed`'s match into `store_failure`, returning `(status, what-happened,
what-to-do)`, precisely so this could be done without two copies of the mapping. The machinery is
already there and already tested on the delete path.

## Acceptance
- [x] **Failing-first test** — a create whose write fails with a `Denied` answers with that kind's
      status and advice rather than `503` "retrying may work".
- [x] The three existing caller-facing sentences are **unchanged** for the kinds that already
      produced them, asserted so the refactor cannot quietly reword a refusal.
- [x] `a_write_that_fails_half_way_leaves_nothing_behind` and the other existing create tests stay
      green, unmodified.
- [x] Nothing in the refusal carries a credential value or another tenant's address.

## Notes
- Follow `remove`'s shape from X-18 rather than inventing a third one.
- The rollback report (`left_behind`) is orthogonal and already correct on this path; this story is
  only about the status and the advice.

## Progress
- **Done 2026-08-01.** Gate green: 39 + 161 tests, clippy clean, fmt clean. Genuine merge-base
  failure — a `Denied` create answered `503` at the base where the test asserts `502`.
- The three caller-facing sentences are pinned **byte for byte** by
  `a_store_failure_says_what_it_has_always_said`, so the shared mapping cannot be reworded by
  accident.
- **A wording trade the implementor made and flagged rather than hid:** each existing sentence is
  kept whole and the advice appended, so `Unreachable` now reads "…so retrying is safe. Retrying may
  work" — mildly redundant. It was chosen over rewording because the two clauses answer different
  questions (the rollback says whether a retry is *safe*, the kind says whether it is *worth
  anything*), and rewording is the quiet restatement the Acceptance forbade. Accepted; tidy it only
  with a test pinning the new wording.
- **Status change worth knowing:** a partly-written create is now `502` for `Denied`/`Backend`/
  `Layout`. Anything downstream that retries on `503` and not on `502` changes behaviour here.
- **Recorded in the design doc rather than left in rustdoc only** — `docs/designs/connections.md`
  gained an addendum covering both X-18 and X-20, since neither had updated it.
- **Known coverage gap, now written down:** `no_answer_or_refusal_carries_a_credential_value` claims
  to drive every refusal this module produces and does not drive `partly_written`'s two branches.
  Predates this story. The disclosure properties are asserted directly by X-18's and X-20's own
  tests; closing the gap means rearranging that test's arming order, which is more churn than either
  story warranted.
