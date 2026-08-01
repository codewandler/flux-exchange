---
id: X-20
title: "A create refused because the store denied us does not say 'retrying may work'"
status: ready
priority: 3
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
- [ ] **Failing-first test** — a create whose write fails with a `Denied` answers with that kind's
      status and advice rather than `503` "retrying may work".
- [ ] The three existing caller-facing sentences are **unchanged** for the kinds that already
      produced them, asserted so the refactor cannot quietly reword a refusal.
- [ ] `a_write_that_fails_half_way_leaves_nothing_behind` and the other existing create tests stay
      green, unmodified.
- [ ] Nothing in the refusal carries a credential value or another tenant's address.

## Notes
- Follow `remove`'s shape from X-18 rather than inventing a third one.
- The rollback report (`left_behind`) is orthogonal and already correct on this path; this story is
  only about the status and the advice.
