---
id: X-40
title: "A leaked agent token cannot mint its own successors"
status: done
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
- [x] **Failing-first test** — a caller whose principal is `PrincipalKind::Agent` is refused at the
      minting route, and no agent is created. Assert against the store, not only the status.
- [x] A `User` principal still mints, asserted in the same run, so the refusal cannot pass by
      breaking minting for everyone.
- [x] `Service` is decided explicitly rather than by omission — a `Service` principal acts on behalf
      of an account and actor, so whether it may mint is a real question. Decide, implement, and say
      why the other answer is wrong.
- [x] The refusal names nothing about what exists — no agent id, no tenant, no count. Follow
      `an_anonymous_caller_is_refused_and_told_nothing`.
- [x] The rule is stated where a reader meets it: on the route, and in
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

## Progress
- **Done 2026-08-01.** Gate green: 45 + 220, clippy clean, fmt clean. Genuine merge-base failure —
  at the base an agent minted a successor and the test quoted the successor's own token back.
  **Independent review dispatched**: this is authorization on a principal-creating route and it adds
  to the published crate's API.
- **`Service` is refused, and the argument is the substance.** The property defended is that
  *revoking a token ends the access it gave*, and that holds only if **every minter is itself
  revocable by this host's operator**. A `User` is — sign-in is federated, the account is disabled at
  the provider, and X-16 makes this host notice. A `Service` is **not**: nothing in this repository
  mints, verifies, lists or revokes a service credential; `PrincipalKind::Service` is a kind the
  identity port may return and nothing more. Admitting it would reproduce the exact defect one level
  up and further out of sight, where there is not even a revoke route to be incomplete.
- **The two errors are not symmetric**, which is what makes refusing the safe direction: refusing a
  `Service` that should mint is a `403` an operator meets on their first attempt; admitting one that
  should not is invisible until a credential leaks.
- **Enforced twice, deliberately.** Declaratively on the route, where `published()` can see it, and
  again inside `AgentStore::mint` — the store creates the principal, so the store must refuse. The
  route gate alone would be bypassed by any later handler reaching `mint` without declaring an
  access, and the module's own spy test already calls `mint` directly, so that path is not
  hypothetical. Each is pinned by its own test.
- **`Access` gained a variant rather than the handler gaining a check**, because `routes/mod.rs`
  states a route is not guarded by its handler remembering to ask — a handler-level check would have
  been invisible to the surface enumeration.
- **Carried forward — the lockout constraint, and it binds X-37.** The gate reads `Principal::kind()`
  from whatever the identity port returned. `oidc/mod.rs` constructs `PrincipalKind::User`
  unconditionally today, so this is safe now — but **any third `Identity` binding that resolves a
  human as something other than `User` locks that operator out of minting.**
- **Filed as adjacent, worth a story:** nothing records **which principal minted a given agent**. The
  gate makes descendants impossible so revocation is complete again, but if X-38 or a later story
  wants an audit trail of minting, `Agent` has no field for it and this was the cheapest moment to
  add one.
- **Reviewed PASS**, verified with four mutations rather than by re-running the report: removing
  **both** gates reproduces the defect verbatim (a `201` carrying a real minted token for
  `successor-of-an-agent`); removing **either one** leaves the other carrying it, each with its own
  failing pin. So neither layer is dead code masking the other's absence.
- The reviewer also checked the filter change I would have missed: `the_surface_publishes_a_route_that_requires_a_principal`
  moving from `== Access::Principal` to `!= Access::Anonymous` **did not hollow it out** — its only
  assertion is an emptiness check, and the old filter would still pass today regardless.
- **`Service` locks nobody out**: `PrincipalKind::Service` is constructed in exactly one production
  place (the development roster), `Oidc` constructs `User` unconditionally, and `AppState` binds one
  identity port — so there is no composition where a `Service` is the only principal an operator can
  present.
- **Two claims corrected at integration, both stronger than the code:** `AgentError::MayNotMint.kind`
  was documented "for the log line" and nothing emits it (it is write-only, and the doc now says so
  and why it is kept); and `Display`'s doc named `a_kind_renders_as_it_serialises` as what keeps the
  two spellings from drifting, when that test **hand-enumerates** three variants — a fourth would
  force an arm without forcing a correct one.
