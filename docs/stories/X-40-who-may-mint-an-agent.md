---
id: X-40
title: "A leaked agent token cannot mint its own successors"
status: ready
priority: 1
epic: agent-access
design: docs/designs/agent-access.md
areas: [exchange-server]
note: "found by X-36's implementor in the surface it had just built, 2026-08-01: nothing gates minting by principal kind, so once X-37 binds the Identity port a leaked agent token mints successor agents — and revoking the first will not kill the descendants"
---

# A leaked agent token cannot mint its own successors

## Goal
Revoking a leaked agent token actually ends the access it gave.

## The finding, and why it is priority 1

X-36 built minting and then reported the hole in it:

> **An agent can mint another agent.** Nothing gates minting by `PrincipalKind`, because the design
> doc explicitly defers authorization to X-13. But once X-37 lands, that means a leaked agent token
> mints successor agents, and revoking the first (X-38) will not kill the descendants. **This is
> worth a decision before X-37 merges, not after.**

That is right, and it is why this story sits **ahead of X-37 in the epic** rather than after it.

Today the hole is unreachable: nothing resolves an agent token, so no agent can call anything.
**X-37 is what opens it.** And the damage is not "one extra agent" — it is that **revocation stops
being a remedy.** X-38 exists so a leaked token has an answer; a token that mints successors makes
that answer incomplete in a way an operator cannot see, because the descendants are ordinary agents
with no recorded relationship to the one that was revoked.

## Why "wait for X-13" is the wrong answer

The design doc defers *authorization* to X-13 (grants), and X-13 is blocked upstream on X-11. That
deferral is sound for **what an agent may call** — an agent authorising nothing beyond any principal
is a stated gap, and `invoke` inherits it.

It is **not** sound for *who may mint*. Minting is not an operation against a connection; it is the
creation of a new principal in this tenant. Leaving it ungated until an upstream version conflict
resolves would ship a revocation mechanism that does not revoke.

## Acceptance
- [ ] **Failing-first test** — a caller whose principal is `PrincipalKind::Agent` is refused at the
      minting route, and no agent is created. Assert against the store, not only the status.
- [ ] A `User` principal still mints, asserted in the same run, so the refusal cannot pass by
      breaking minting for everyone.
- [ ] `Service` is decided explicitly rather than by omission — a `Service` principal acts on behalf
      of an account and actor, so whether it may mint is a real question. Decide, implement, and say
      why the other answer is wrong.
- [ ] The refusal names nothing about what exists — no agent id, no tenant, no count. Follow
      `an_anonymous_caller_is_refused_and_told_nothing`.
- [ ] The rule is stated where a reader meets it: on the route, and in
      `docs/designs/agent-access.md`, which currently says an agent "authenticates and authorises
      nothing beyond what any principal may do" — a sentence this story makes false.

## Notes
- **This is authentication-shaped, not grant-shaped**, which is why it does not wait for X-13: it
  asks *what kind of principal is calling*, which this host knows today from the token it issued,
  rather than *what has this principal been granted*, which needs the grant model.
- Consider whether the same argument reaches other routes. Minting is the sharp case because it
  creates a principal, but a `Service` or `Agent` calling `DELETE /api/connections/{connector}` is
  the same class of question and this story should at least record an answer.
- X-38 (revoke) should be read alongside this: together they are what make a leaked token
  recoverable. Neither is sufficient alone.
