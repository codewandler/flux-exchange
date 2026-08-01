---
id: X-47
title: "A connector with a templated host can actually be invoked"
status: done
epic: connections
design: docs/designs/connection-settings.md
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
- [x] **Failing-first test** — a connector with a templated `base_url` is invoked successfully after
      its per-connection value is supplied, and fails before the value can be supplied.
- [x] The value is **per connection and per tenant**, derived from the resolved principal like every
      other address on this surface. No route accepts a tenant.
- [x] A connection missing a value its connector declares is still **refused by name** — this story
      adds a way to supply the value, it does not weaken the refusal.
- [x] **Configuration is not a credential and must not be stored as one.** A subdomain is not a
      secret; putting it in the credential store would make `held` and the occupancy bound mean two
      different things. Decide where it lives and argue it.
- [x] The existing invoke tests stay green unmodified, including
      `no_parameter_can_move_the_destination_host` — **supplying configuration must not become a way
      for a caller to name a host.** That is the invariant this story is most able to break.

## Notes
- Read `docs/designs/connections.md` on why this was deferred, and X-14, which is the neighbouring
  story on the same upstream dimension. Decide explicitly whether this lands before, after, or with
  X-14 — they are close enough that doing them blind of each other will cost a rewrite.
- The refusal today names the field and the service. Whatever is built should make that refusal
  actionable — an operator who reads it should know what to supply and where.

## Progress
- **Done 2026-08-01.** Gate green: 321 tests (52 + 13 + 3 + 10 + 5 host, 238 server), clippy clean,
  fmt clean. **Independent review dispatched** — new tenant-scoped store, two new routes, code moved
  inside `credentials.rs`, and a widened address guard.
- **The count in this story was wrong: it is sixteen, not thirteen** — and *how* it was measured is
  the point. Derived from `connector_pack::Rehearsal` against the shipped catalogue rather than by
  scanning `base_url`, because four connectors (bitbucket, cloudflare, contentful, vercel) carry
  configuration variables **elsewhere in the operation's Flux**. A scan would have shipped those four
  still broken while reporting them configured.
- **Zendesk — this story's own headline connector — needs two *kinds* of value**, not one:
  `endpoint.subdomain` *and* the non-secret user half of its Basic credential, and it refuses on the
  second first. Supporting only the endpoint would have left the example uninvocable.
- **Configuration is not a credential and is not stored as one:** its own file, its own port, and two
  bounds **never summed** with the credential ones. A test asserts the credential store stays empty
  and its allowance unspent when a setting is written.
- **`{service}` is a required path segment**, not defaulted, following upstream's own argument:
  contentful declares `endpoint.space_id` under two services, and a silent default once sent a
  management write into the delivery space.
- **Values are not read back out** — `GET` answers targets and a `set` boolean. Stricter than "not a
  secret" requires, because a `username` field is an account name or an email address, and adding a
  read later is additive.
- **`no_parameter_can_move_the_destination_host` is untouched**, verified byte-identical, with a
  sibling added for settings. `no_route_here_accepts_an_address` widened by two names and paid for
  behaviourally, exactly as X-39 did for `{credential}`.
- **X-14 ordering decided and argued: this lands first.** `ConfigStore::get` is upstream's signature
  and has no instance parameter, so an instance-aware key could not be designed against it. X-14 now
  inserts `@instances/<uuid>` at `SettingsStore::at` exactly as it does at `address_of_declared` —
  one component, one seam each.
- **Carried forward — the least-exercised path:** `SettingsStore::set` rolls the in-memory change
  back when the file write refuses, and no test drives a persist failure. If a tenant's value
  vanishes after a restart, the map and the file disagreed.
- **Carried forward:** `declared_settings` rehearses every operation of a connector on every settings
  request, uncached — and the console will call it per page load.
- **Filed as adjacent:** `console/` does not know these routes exist, which is what turns this from an
  API into a feature; and `GET /api/connections/{connector}` still reports `held: true` for a
  connection that holds a token but no subdomain and will refuse every invocation.
