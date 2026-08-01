---
id: X-54
title: "Who may create a connection and rotate a credential is decided, not inherited"
status: in-progress
priority: 1
epic: connections
areas: [exchange-server]
note: "the ring-fenced half of X-47: the settings write is now gated to humans, but POST /api/connections and PUT .../credentials/{credential} are still Access::Principal, so an agent can create a connection and replace a credential"
---

# Who may create a connection and rotate a credential is decided, not inherited

## Goal
Every route that touches a credential states which kind of principal may call it, and the answer is
one somebody chose.

## Why now

X-47 closed a measured exfiltration by gating `PUT /api/connections/{connector}/settings/{service}/{field}`
to `PrincipalOfKind(MAY_CONFIGURE)` — humans only. That was the narrowest change that closed a
**stated** invariant, and it was correct to stop there.

It leaves two neighbours untouched, both still `Access::Principal`, which admits every kind:

- `POST /api/connections/{connector}` — an agent can **create a connection**, supplying credential
  values.
- `PUT /api/connections/{connector}/credentials/{credential}` — an agent can **replace** a tenant's
  credential.

Neither reads a credential out, so neither breaks `AGENTS.md`'s invariant the way the settings write
did — *"an agent's token grants access to an operation, never to a credential"* is about the
credential reaching the agent. **But an agent that can overwrite the credential its tenant's
operations run under is not obviously inside that sentence either**, and the current answer is not a
decision. It is what `Access::Principal` happened to mean before agents existed.

## The question, which is a product question and not a code one

`PrincipalKind`'s own published doc already divides the labour — `User` *"manages connections"*, and
for `Agent`, *"humans sign in to wire things up"*. Taken literally that settles it: connection
management is a human act, and all three routes are `MAY_CONFIGURE`.

Two things argue against just doing that:

- **There is no operator kind.** `User` is every signed-in human, so "humans only" does not
  distinguish the person who set the tenant up from anyone else who can sign into it. The within-tenant
  gap X-47 left open (`docs/designs/connection-settings.md` § *What this does not close*) is the same
  gap, and it wants a surface that does not exist.
- **A service integration might legitimately rotate.** `PrincipalKind::Service` is refused alongside
  `Agent` today on `MAY_MINT`'s reasoning — no service integration exists to be broken. When one
  does, credential rotation is exactly what it would want.

## Acceptance
- [ ] Every route under `/api/connections` declares a kind, or its `Access::Principal` carries a
      written argument for why every kind is right. **No route is left at the default by omission.**
- [ ] **Failing-first test** — whichever kinds are refused, an attempt is refused and logged, the way
      `an_agent_may_not_write_a_connection_setting_and_the_refusal_is_logged` does.
- [ ] `the_kind_gated_surface_is_only_what_was_declared` covers the new gates, so widening stays a
      deliberate, tested act.
- [ ] The answer is written in `docs/designs/connections.md`, not only in the route table.

## Notes
- **X-47's residual risk is worth carrying here**: `MAY_CONFIGURE` is the *only* enforcement point
  for the settings write. The credential surface's pattern is enforce-twice — `agents::MAY_MINT` is
  re-checked inside `AgentStore::mint` — and X-47 could not mirror it, because
  `ConnectionSettings::set` takes a `&Tenant` rather than a `Principal` and widening it is a breaking
  change to a published crate. If a second handler ever reaches `SettingsStore::set` without
  declaring an access, nothing stops it.
- A kind gate is only as good as what the identity port reports. `dev_identity.rs:47` mints
  `PrincipalKind::Agent` from a roster string; a composition whose identity port mislabels an agent
  as a user bypasses every gate in this story.
