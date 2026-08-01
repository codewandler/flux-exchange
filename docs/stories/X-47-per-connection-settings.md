---
id: X-47
title: "A connector with a templated host can actually be invoked"
status: done
epic: connections
design: docs/designs/connection-settings.md
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

## REVERTED 2026-08-01 — the review found a credential-exfiltration path

`impl/X-47` was merged as `90ee254` and **reverted as `dcae5a1`**. The branch is untouched and keeps
its history; rework is in flight.

**Measured end to end by the reviewer, not argued:**

```
newrelic endpoint.host="evil.example"  stored_ok=true outcome=OK
  urls=["https://evil.example/v2/applications.json"]  SECRET_ON_WIRE=true
```

For **`newrelic` and `docusign` the tenant-supplied setting is the entire destination authority** —
`connector-catalog` declares `base_url: "https://{host}/v2"` with `hosts: ["{host}"]`, and
`"https://{account_host}/restapi/..."`. The design's defence — that `connector-pack` validates the
composed authority against an allow-list of host *characters* — constrains the **characters** of the
value, not the **identity** of the host. Sound where the template pins a suffix
(`{subdomain}.zendesk.com`); **vacuous where the variable is the whole authority**.

**The writer needs no special standing.** The settings route is `Access::Principal`, which
`require_principal` admits for *any* kind, and agent tokens resolve to `PrincipalKind::Agent`. So an
agent holding only an operation grant converts it into delivery of the raw credential to an endpoint
it chose.

It breaks two things **by name**:

- This story's own Acceptance item 5, verbatim: *"supplying configuration must not become a way for
  a caller to name a host. That is the invariant this story is most able to break."*
- `AGENTS.md` § Invariants: *"An agent's token grants access to an operation, never to a credential."*

**New reachability:** before the diff, `execution::invoker` bound `MemoryConfig::new()`, so both
connectors refused before dispatch. The evidence test drove **zendesk only** — the one shape where
the property is structurally free.

**The distinction needed to fix it is already published, with no new dependency:**
`connector_catalog::Operation` carries `hosts`, so `"{host}"` and `"{account_host}"` are
distinguishable from `"{subdomain}.zendesk.com"` and `"api.bitbucket.org"` — a template that pins no
suffix is a template whose variable *is* the authority.

### Also found, to fold into the rework
- **The route's allowance check contradicts its own comment** — it calls `admit_tenant_settings`
  without subtracting what the write replaces, directly under a comment saying it decides "against
  what the write **replaces**". `SettingsStore::set` *does* subtract. A tenant at the bound is
  refused a same-size rotation the store would have accepted.
- **The count is 17, not 16** — off by one in the same direction the original thirteen was. `twilio`
  is equally uninvocable and is named in the design's own section 3 as needing a username, but
  excluded from its headline.
- `SettingsStore::bind` does not refuse a pre-existing widened mode where `CredentialStore` does.
  Argued in the code as deliberate; noted, not blocking.
- **`connector_pack::Rehearsal` is a third pack entry point lock 2 does not count.** It cannot
  dispatch today — no `Egress`, no `execute` — but the scanner would not notice if it became able to.

### What the review cleared
The widened address guard (26 hostile `{service}`/`{field}` spellings, all refused, store file never
created), the verbatim `credentials.rs` move (one doc word changed; all five escape tests survive),
configuration-is-not-a-credential, the rollback path, and the `paths` module being **private** rather
than new public surface as I had assumed.

## Rework round 1 — the path is closed, and the measurement found more than the review did

**Done 2026-08-01.** Gate green: 326 tests (52 + 16 + 3 + 10 + 5 host, 240 server), clippy, fmt.
Merged as the rework of the reverted `90ee254`.

- **The review named two connectors. The measurement found four.** `newrelic` and `docusign` as
  reported, plus **`okta`** (`{domain}`, would have carried `okta.api_token`) and **`freshdesk`**
  (`{domain}`, declares no credential but this host would still have been an open proxy). *That is
  the argument for deciding it from the catalogue rather than from a list* — a hand-written list
  would have shipped two more holes.
- **The rule is about the *template*, never the value.** `acme.newrelic.com` is refused exactly as
  `evil.example` is, because a value rule would be a blocklist and a blocklist catches only what
  somebody enumerated.
- **Enforced twice, and the second is the one that matters.** `ConnectionSettings::set` refuses, and
  `ConfigStore::get` refuses **again** — so the property belongs to the **port** rather than to one
  write path. An edited file, a restored backup, or a value written by an older build all bypass
  `set`. The value is not deleted: *refuse, never repair*, and on this path somebody has to be able
  to find out how it got there.
- **The suffix rule needs two further labels, not one**, because `.com` pins nothing anybody cannot
  register under. The honest answer is a public-suffix list, which is a dependency this crate may not
  take — the approximation is stated in the code and **errs closed**.
- **Verified at integration by falsification**, not on report: neutering `host_pinning` fails three
  tests including the end-to-end one, which reproduces the original — `newrelic` dispatching to a
  tenant-supplied host with the credential on the wire.
- **Four connectors are refused rather than made to work**, taking the dispatch's explicit sanction
  that a smaller working surface beats a larger one that leaks. Closing that gap needs somewhere an
  **operator** can pin an allowed host per tenant — a new surface with its own authorization
  question, and its own story.
- **The route's allowance check was deleted rather than corrected**: `SettingsStore::set` decides
  under the same lock it reads and inserts under — a *tighter* read-decide-write than a route-level
  claim — so a guard on top would guard nothing and be a second place to drift.
- **The count is 17, and a base-URL scan misses five.** Shipped surface: **49 of 53**.
- The design doc keeps the **flawed §4 argument on the page** as an explicit correction. The flaw —
  *a character allow-list constrains what a value looks like, not where the request goes* — is the
  part worth not losing.
- **Carried forward:** `suffix_of` is the whole rule in twelve lines, and its two-label threshold
  approximates a public-suffix list. A vendor template shaped `{x}.co.uk` would pass it while pinning
  a public suffix. Nothing shipped is in that shape, and the catalogue-wide test asserts the property
  of every accepted suffix — but it is the assumption to re-examine first.
