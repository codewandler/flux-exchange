---
id: X-54
title: "Who may create a connection and rotate a credential is decided, not inherited"
status: done
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
- [x] Every route under `/api/connections` declares a kind, or its `Access::Principal` carries a
      written argument for why every kind is right. **No route is left at the default by omission.**
- [x] **Failing-first test** — whichever kinds are refused, an attempt is refused and logged, the way
      `an_agent_may_not_write_a_connection_setting_and_the_refusal_is_logged` does.
- [x] `the_kind_gated_surface_is_only_what_was_declared` covers the new gates, so widening stays a
      deliberate, tested act.
- [x] The answer is written in `docs/designs/connections.md`, not only in the route table.

## Progress

**Landed.** `POST /api/connections/{connector}` and
`PUT /api/connections/{connector}/credentials/{credential}` are
`Access::PrincipalOfKind(MAY_SUPPLY_A_CREDENTIAL)` — `PrincipalKind::User` only. The other four
verbs on this surface stay open to every kind, each with the argument written where the route is
declared (`routes/connections.rs`, `MODULE`).

- **The invariant reading.** Neither route hands a value out, so *"never to a credential"* on its
  literal reading is not what they break. The argument taken is that a caller deciding **which**
  credential the tenant's operations run under has been granted the credential position — the
  substitution in the other direction. Written up in `docs/designs/connections.md` § X-54.
- **`DELETE` and the two reads stay open**, which is X-40's decision left standing rather than swept
  up. `an_agent_may_still_read_a_connection_and_disconnect_one` is what holds that, and it is a real
  risk rather than a formality: `POST` shares its path with them.
- **The gate is per method**, so `/api/connections/{connector}` is declared twice in `MODULE` — once
  for `get(show).delete(remove)` at `Access::Principal`, once for `post(create)` at the kind gate.
  axum merges the two method routers and each verb carries its own guard. The alternative was a
  check inside `create`, which `Access` exists to refuse.
- **`Service` is refused now, not deferred**, on `agents::MAY_MINT`'s argument. The cost is named:
  credential rotation is exactly what a service integration would want, and the story that wants it
  is the story that gives `Service` a revocation path.

### What a resuming agent should know is still open

- **There is no operator kind, and inventing one was rejected.** `User` is every signed-in human of
  the tenant, so a `User` who did not supply a credential can still replace it. Same gap X-47
  recorded, and it is X-13's grant model rather than a wider kind list — a kind gate cannot express
  *which humans of a tenant manage it*.
- **The declared access is the only enforcement point**, X-47's residual carried forward. `create`
  and `rotate` cannot mirror `AgentStore::mint`'s enforce-twice, because both write through
  `SecretStore::put`, whose port takes a `CredentialRef` and a `Secret` and has no principal —
  widening it is a change to a published crate this repository does not own.
- **Nothing records who supplied a credential.** That absence is half of why this gate is needed;
  it is also why an operator still cannot audit a connection after the fact. A record beside the
  store is deliberately not kept (`docs/designs/connections.md`), so this wants its own story.

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

## Closed 2026-08-01 — reviewed PASS, with the mechanism driven rather than reasoned about

Gate green: 349 Rust (54 + 17 + 3 + 10 + 6 + 4 + 255), 75 console.

**The review verified the vulnerability at the base directly** rather than accepting the revert
experiment: at `1225dd2`, an agent `POST /api/connections/zendesk` answered **201** with
`tenants/acme/com.zendesk.api/api_token == "AGENT-PLANTED"`, and an agent rotation answered **200**
with `"AGENT-ROTATED"`. That is stronger than a failing test, and it is the measurement this story
exists for.

**The per-verb binding is real, not accidental.** Full matrix on `/api/connections/zendesk`:

| caller | GET | POST | PUT | PATCH | DELETE | HEAD | OPTIONS |
|---|---|---|---|---|---|---|---|
| `alice` (User) | 404 | **201** | 405 | 405 | 404 | 404 | 405 |
| `triage-bot` (Agent) | 404 | **403** | 403 | 403 | 404 | 404 | 403 |
| unresolvable | 401 | 401 | 401 | 401 | 401 | 401 | 401 |

`HEAD` follows **GET's** guard, never POST's. No undeclared verb reaches a handler, and the store is
empty after every one.

**Only two paths reach `SecretStore::put`** in this binary and both are now gated (`connections.rs:706`
`create`, `:859` `rotate`). The invoke path cannot write: `Credentials`' only store call is `get`.

**Delete-then-anything gives an agent nothing** — driven: human creates → agent `DELETE` 204 → agent
`PUT` on the now-empty connector **403**, store empty. There is no two-step substitution.

**`Service` was genuinely refused and had never been driven before.** No test in the tree used a
`service:` roster entry; the reviewer added one and confirmed 403 on create and rotate, 200 on read,
204 on delete — matching the documented decision exactly.

### Carried out as [[X-61]] (priority 1)

The surface-wide anonymous guard is **blind to the second declaration** at a duplicated path: setting
this story's POST entry to `Access::Anonymous` leaves
`the_anonymous_surface_is_only_what_was_declared_anonymous` green, because it probes by path with a
GET and both entries resolve to the `Principal` one. Caught today only by a module-local test the next
module to copy this pattern will not have.

### Carried, smaller

- **The merged router's 405 fallback takes the second declaration's guard.** Swapping the two entries
  changes an agent's `PATCH`/`OPTIONS` from 403 to 405. Not a hole in either order; undocumented in
  both.
- `MAY_SUPPLY_A_CREDENTIAL` and `MAY_CONFIGURE` are interchangeable while equal — swapping one for the
  other leaves the suite green. Divergence is safe (both tests reference the constants), but the
  pairing is not enforced.
- Two sentences elsewhere claimed the kind gate reached only the minting route. **X-54 widened that
  falsehood from one route to three**, and this story's own test doc cited one of them as authority.
  Corrected at integration in `routes/agents.rs` and `docs/designs/agent-access.md`.
