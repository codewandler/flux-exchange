---
id: X-47
title: "A connector with a templated host can actually be invoked"
status: ready
priority: 1
epic: connections
areas: [exchange-server, exchange-host]
note: "found by X-12's implementor once invoke worked, 2026-08-01: thirteen of fifty-three connectors declare a templated base_url and there is nowhere to put the value, so the invoker binds an empty config and they refuse by name"
---

# A connector with a templated host can actually be invoked

## Goal
A tenant can supply the per-connection value a connector's own manifest asks for.

## What X-12 exposed

Invoke works. It also revealed that **thirteen of the fifty-three shipped connectors cannot be
invoked at all** — Zendesk among them — because their `base_url` is templated on a per-instance
value (a vendor subdomain) and **there is nowhere for a tenant to put it.**

The invoker binds an empty `MemoryConfig`, so those connectors refuse by name. It **fails closed and
says which field and service are missing**, which is the right failure — but the shipped surface runs
40 of 53 connectors, and no amount of correct refusing changes that.

This was deferred once already, deliberately: `docs/designs/connections.md` records that
per-connection configuration was left out of X-10 because *"a vendor subdomain is exactly the
per-instance fact with no home until two instances can be told apart"*. **That blocker is gone** —
X-11 brought in `connector-address` 0.9 with C-406's instance dimension, and X-14 is the story that
uses it.

## Acceptance
- [ ] **Failing-first test** — a connector with a templated `base_url` is invoked successfully after
      its per-connection value is supplied, and fails before the value can be supplied.
- [ ] The value is **per connection and per tenant**, derived from the resolved principal like every
      other address on this surface. No route accepts a tenant.
- [ ] A connection missing a value its connector declares is still **refused by name** — this story
      adds a way to supply the value, it does not weaken the refusal.
- [ ] **Configuration is not a credential and must not be stored as one.** A subdomain is not a
      secret; putting it in the credential store would make `held` and the occupancy bound mean two
      different things. Decide where it lives and argue it.
- [ ] The existing invoke tests stay green unmodified, including
      `no_parameter_can_move_the_destination_host` — **supplying configuration must not become a way
      for a caller to name a host.** That is the invariant this story is most able to break.

## Notes
- Read `docs/designs/connections.md` on why this was deferred, and X-14, which is the neighbouring
  story on the same upstream dimension. Decide explicitly whether this lands before, after, or with
  X-14 — they are close enough that doing them blind of each other will cost a rewrite.
- The refusal today names the field and the service. Whatever is built should make that refusal
  actionable — an operator who reads it should know what to supply and where.
