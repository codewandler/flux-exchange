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

X-12 made this host execute, and the first thing that fell out was a count. **Sixteen of the
fifty-three shipped connectors cannot be invoked at all** — zendesk, shopify, jira, confluence,
freshdesk, okta, salesforce, supabase, mailchimp, newrelic, docusign, statuspage, bitbucket,
cloudflare, contentful, vercel — because each declares a configuration variable its operations
substitute into a request, and there was nowhere for a tenant to put one. `execution::invoker` bound
`MemoryConfig::new()`, so every one of them refused by name.

The refusal was correct. It failed closed, and it named the field, the service and the tenant. But a
correct refusal is still a connector that does not work, and the shipped surface ran forty of
fifty-three.

The story's own note says thirteen. The measured number is sixteen, and the difference is the whole
of § *The surface is read off the connector, not off its base URL* below.

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

A base-URL scan finds twelve connectors and misses four entirely, plus docusign's second variable. A
host enumerating the surface that way would tell an operator they had supplied everything and then
refuse the call.

The second kind is the non-secret user half of a `basic` credential — `Field::Username`. Four
connectors need one (zendesk, jira, confluence, twilio), and **zendesk needs both kinds**: without
the user half it refuses before it ever reaches the subdomain. A story that shipped only
`endpoint.*` would have left its own headline connector uninvocable.

## §4 Supplying configuration does not become a way to name a host

The invariant this story is most able to break, and the shape of the attack is specific:

```text
subdomain = "acme.zendesk.com@evil.example"
  -> https://acme.zendesk.com@evil.example.zendesk.com/api/v2/tickets/1.json
     authority: evil.example.zendesk.com
```

where the `@` turns everything before it into userinfo and the request reaches a host the operator
never named, carrying that operator's own token.

**This host does not defend against that, and must not.** `connector-pack` validates the *composed
authority* at the one substitution point it makes, against an allow-list of host characters — so no
permitted character can delimit, and the string a transport resolves is exactly the string the
template composed. It does that knowing which position of the URL the value lands in, which this
crate does not know and would have to guess.

So the decision is: **store what you are given, and let the pack refuse what may not be
substituted.** A second opinion here would be a second spelling of one rule, and the one that
disagreed would be the one deciding whether a tenant's value can move an origin.
`connection_settings.rs::no_setting_can_move_the_destination_host` holds the refusal to arriving and
to dispatching nothing, on six hostile spellings, and
`invoke.rs::no_parameter_can_move_the_destination_host` is **unmodified**.

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
- **No validation of what a value means.** This host refuses a value at an address the connector
  never declared, and one past a bound. Whether `acme` is a real Zendesk subdomain is a question only
  Zendesk can answer, and it answers it with a `404` that reaches the caller whole.
- **The store is single-process**, like `ConnectionGuard`: the read-decide-write around the tenant
  allowance is claimed within this process and not across a cluster. The same limit `connections.md`
  already records, in the same words.
