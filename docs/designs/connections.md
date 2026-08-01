---
story: X-10
status: accepted
---

# Connections: an address the caller cannot name, and a refusal where the address is incomplete

How a tenant's connection to a connector is created, listed and destroyed, and — the part that took
the most argument — what happens at the one place the address scheme does not yet reach.

Extends [`identity-and-session.md`](identity-and-session.md), which made a request carry a resolved
`Principal`, and builds on X-09's `CredentialStore`. The rule everything below answers to is
`AGENTS.md` § Invariants, first entry:

> **The tenant comes from the resolved principal and from nothing a caller controls.** Not a path
> segment, not a body field, not a header.

and its last:

> **Refuse; never repair.** … each refuses and names the address, never the value.

## The address is derived, and there is no route that could accept one

**Decision: the credential address is a function of `(resolved tenant, connector's declared
authority, connector's declared credential leaf)`, and no route takes an address, a tenant or a
path.**

```text
tenants/<tenant>/<authority>/<credential>
        ^^^^^^^^ ^^^^^^^^^^^ ^^^^^^^^^^^^
        |        |           `Credential::leaf`, from the catalogue
        |        `Provider::authority`, from the catalogue
        `Principal::tenant(), from the guard
```

None of the three comes from the request. The surface is:

| Route | Method | What the caller supplies |
| --- | --- | --- |
| `/api/connections` | `GET` | nothing |
| `/api/connections/{connector}` | `GET`, `POST`, `DELETE` | a **connector id**, and on `POST` a body of credential *values* |

**Who may call which of them is the X-54 addendum below**, and it is not uniform across the verbs of
that second row: `POST` is a `User`'s, `GET` and `DELETE` are every kind's. Nothing in this section
changes — what a caller supplies is still a catalogue key and never an address.

`{connector}` is a catalogue key — `zendesk`, `slack` — looked up in the compiled-in
`connector_catalog` and refused with `404` when nothing declares it. It is not an address and it
cannot become one: it never reaches a path segment of the credential address, only the
`authority`/`leaf` pair the catalogue answers with.

Two structural tests already on the surface do the enforcement, and X-10 adds nothing to either
because it does not need to:
`routes::tests::no_published_route_takes_a_tenant_in_its_path` walks `published()` (X-03 wrote it
saying X-10 would inherit it, and X-10 does), and
`the_anonymous_surface_is_only_what_was_declared_anonymous` needs no new `ANONYMOUS` entry, because
both routes here are `Access::Principal`. A connection is tenant data; there is no version of it
that answers a caller this host has not identified.

### Rendering the address once

`CredentialRef` is the addressing type, and `TenantLayout` is the one thing that renders it into a
path. Both come from `connector-spec` by way of `connector-secrets`, and `exchange_host` now
re-exports `CredentialRef`, `Secret`, `SecretStore` and `StoreError` for the same reason it already
re-exports `async_trait`: a composition should not have to name a second crate, at a version it has
to guess, to spell one address. `FileStore` renders with `TenantLayout` too, so the path a refusal
quotes is the path the store actually looked at — not a second spelling of it.

## The gap this story does not close, and turns into a refusal

**Decision: a second connection to a connector the tenant already has is refused with `409`, naming
X-14 and the instance dimension that would have made it work.**

This is the load-bearing decision of the story, and it is a decision *against* shipping the
Acceptance's address as though it were complete.

`tenants/<tenant>/<authority>/<credential>` has no place to say *which* Zendesk. A tenant with a
sandbox subdomain and a production one renders **one** address for both, because nothing in the
address varies per connection. Left alone, the second `POST` would overwrite the first, return
`201`, and every later call would reach the wrong account while looking entirely healthy. That is
the exact failure mode `AGENTS.md` § Invariants describes — "a store that falls back to memory, or a
mode that is quietly tightened, hides the thing you needed to know" — with the value silently
replaced instead of the mode silently widened.

The obvious local fix is to add an instance segment here. **It is refused**, and X-14 says why: the
address scheme is one scheme shared with `connector-spec`, `connector-secrets` and every other
consumer of `CredentialRef`. Two spellings of an address is how two components stop agreeing where a
credential lives, and this repository would be the one that forked it.

### The dimension has landed upstream, and is not published

flux-connectors **C-406** merged the instance level (`14b5dc7` on its `main`):

```text
tenants/<tenant>/<authority>[/@instances/<uuid>][/<service>]/<credential>
```

`InstanceId` is a validated newtype over a canonical lowercase hyphenated uuid, and the nil uuid is
refused because "no instance" is already spelled by omitting the level. The `@instances` marker
carries the `@` deliberately: it is unspellable in every component grammar, so the level cannot be
forged and no service or credential name is reserved away — a bare uuid segment would have been
ambiguous with a service, a uuid being a well-formed service name. Existing single-instance
addresses are byte-identical, so nothing in this story's rendering changes when the pin moves.

**It is not published.** crates.io serves `codewandler-connector-spec` 0.8.0, this workspace pins
`"0.8"` from the registry, and the pin is fenced. So the level cannot be spelled here yet — but the
refusal can name it, and the derivation can leave it one insertion away.

So the gap becomes a refusal that says what is coming:

```json
{
  "error": "this tenant already has a connection to connector `zendesk`, and the credential address has no instance dimension to tell two of them apart",
  "connector": "zendesk",
  "address": "tenants/acme/com.zendesk.api/api_token",
  "would_have_worked": "an instance level on the address — `tenants/<tenant>/<authority>/@instances/<uuid>/<credential>` — which has landed in flux-connectors (C-406) and is not published yet; this host pins connector-spec 0.8. Wiring it up, including resolving a name you choose to that uuid, is X-14. Until then, delete the existing connection before creating another"
}
```

> **This refusal is the placeholder for the `@instances` level.** Whoever lands X-14 replaces
> `Refusal::AlreadyConnected` in `routes::connections` with an address that carries the instance,
> extends `ConnectorDeclaration::address_of_declared` in `exchange-host` — the single place an
> address is composed, and the seam left for exactly this — and deletes
> `a_second_connection_to_one_connector_is_refused_rather_than_overwriting` along with it. Grep for
> `@instances`.

Upstream's `TenantInstances` states the rule the host then has to satisfy — elide at one, the named
one at several, refuse when several and none is named, refuse a uuid the tenant does not hold — and
records that the label → uuid mapping is the host's job. That is X-14's, and it belongs in
`address_of_declared` rather than beside it, which is why nothing here models a uuid or an instance
today.

The single-instance tenant — the case that works today — works fully: create, list, read and delete
are all unaffected by the refusal, and the tests cover them end to end.

### The refusal has to survive two callers at once

**Decision: one in-process claim per `(tenant, connector)` is held across the whole read-decide-write
of every mutating connection route, and a caller that cannot take it is refused rather than queued.**

The `409` above is a **check-then-write**: probe the address, refuse if occupied, otherwise write.
With nothing between the halves it does not survive concurrency, and not in a subtle way — two
concurrent `POST /api/connections/zendesk` from one tenant both probe an empty address, both write,
and both answer `201`. One value is gone and the caller that lost was told it succeeded. That is the
silent overwrite this entire story exists to prevent, reintroduced by the mechanism meant to prevent
it, and a double-clicked button in the console is enough to trigger it. Left open, the refusal would
have been decorative — and the whole case for landing this address scheme before X-14 rests on the
refusal being real.

The port cannot close it: `SecretStore` has no compare-and-swap and adding one is not this
repository's to do. What is available is that the decision happens in one process, so
[`ConnectionGuard`](../../crates/exchange-server/src/connection_guard.rs) is an in-process claim on
the thing being changed.

- **Per `(tenant, connector)`, not one lock over the surface.** A global lock would make one tenant's
  connection writes wait on another's — shared fate between tenants, in the repository whose entire
  point is that they share nothing.
- **`DELETE` takes the same claim**, so a delete cannot decide against a value a create is in the
  middle of writing and destroy half of it.
- **Refuses rather than waits.** Waiting would need a lock held across an `await`, and it produces a
  worse shape anyway: the queued second `POST` wakes, finds the first caller's value and answers
  `409` regardless. Two racing creates *are* a tenant trying to have two connections to one
  connector, which is exactly what this surface refuses; saying so immediately is more honest than
  making the caller wait to be told the same thing.
- **Released on drop, including while a panic unwinds**, so no failure path leaves a connection
  permanently unchangeable.

**What it does not cover: more than one process.** `FileStore` is a single in-process map written
through to disk on every mutation, so within this process the claim is sufficient — but **two
replicas sharing one store would race again, and nothing here would notice.** That is the same limit
`identity-and-session.md` already records for sessions ("sessions do not survive a restart… a shared
store is a real design question"), and it is the same answer: it should be settled when there is a
deployment asking for it, and the honest thing meanwhile is to write it down. A multi-replica
deployment of this host needs a store with a compare-and-swap, or a lock outside the process, before
this refusal means anything.

Proved by `tests::two_concurrent_creates_cannot_both_succeed`, looped 500 times with the window
between probe and write held open by the test store, plus the same race at 50 attempts against a
real `CredentialStore`, and `a_delete_racing_a_create_leaves_the_connection_whole_or_absent`. Before
the claim, the first of those reproduced on attempt 0.

### Writing a connection is all or nothing

**Decision: a store failure part way through a multi-credential write takes back what it already
wrote, and says so.**

A connector may declare several credentials, so `create` can fail after storing one and before
storing the next. Returning there leaves a connection that is neither present nor absent: the caller
saw a failure, while the `409` above now refuses every retry until somebody works out that a `DELETE`
is needed first. So the values already written are deleted, and the caller is told which of two
things happened — the rollback worked and retrying is safe, or the rollback failed too and some
credentials may remain at the addresses named. Best effort by necessity, since the store has just
failed; what is not optional is admitting which. A refusal claiming nothing was written while
something was is the answer that costs somebody an afternoon.

### What a refusal names

The address, never the value. Every refusal in this module quotes the rendered path and never the
secret, and `no_answer_or_refusal_carries_a_credential_value` drives the whole module's failure paths with a
sentinel value stored and asserts the sentinel appears in no response body. The refusal that matters
most is the cross-tenant one, and it is worth being precise about what it can say: a caller asking
for a connection its tenant does not have is answered with **its own** derived address, because that
is where this host looked. It never names the other tenant, the other tenant's address, or the fact
that somebody else has one.

## A connection is what the credential store says it is

**Decision: there is no second record. A connection exists exactly when the store holds a value at
one of the addresses derived for that tenant and connector.**

The alternative — a `connections` table beside the credential store — is one more thing that can
disagree with the store, and the disagreement is not symmetric: a record with no credential is a
connection that `401`s at the vendor, and a credential with no record is a live credential nothing
lists. `DELETE` destroying the credential is then not a step that could be forgotten; it is the
whole of what deleting means. That is the Acceptance item "deleting a connection destroys its
credential", satisfied by construction rather than by remembering.

The cost is stated rather than hidden, and it is smaller than it looks. `SecretStore` is
`get`/`put`/`delete` with no listing operation — deliberately, since Vault's does not either — so
`GET /api/connections` derives an address for every addressable connector in the compiled-in
catalogue and probes it. Measured by
`tests::a_listing_probes_one_address_per_declared_credential_and_none_collide`: **60 addresses across
52 addressable connectors** (53 in catalogue 0.8.0; `freshdesk` declares no credential). One `get`
per *address*, not per provider, since a connector may declare several.

Against the file store those are **60 lookups in an in-memory `BTreeMap`** — `FileStore` reads the
file once when it opens and writes through on mutation, so a listing does no file IO at all. A probe
reads the value and drops it without exposing it, because `SecretStore` has no `exists`; that is a
copy in memory, not a read from disk. At this size an index would be a second thing to keep in step
with the store in exchange for nothing measurable, so there is not one. The number to watch is the
address count rather than the provider count, and the test fails loudly if it ever reaches 500.

The test's other assertion is the one that is actually load-bearing: **no two connectors render the
same address for one tenant.** Nothing upstream promises that — it falls out of the authority being
per vendor — and if two ever collided, connecting one would read as having connected the other and
deleting one would destroy the other's credential.

### Which credentials a connection carries

A connector declares its credentials at provider level: `zendesk` declares one (`zendesk.api_token`),
`slack` declares two (`slack.bot_token` for outbound calls, `slack.signing_secret` for inbound
webhook verification). A connection carries a value for **any non-empty subset** of them, named by
the flat-namespace name the catalogue publishes:

```json
POST /api/connections/slack
{"credentials": {"slack.bot_token": "…"}}
```

Requiring all of them would force an operator to invent a signing secret they do not use; allowing
none would let an empty `POST` create a connection with nothing behind it. A name the connector does
not declare is refused, listing the ones it does — a typo that stored a value at an address no
operation reads is a credential nobody will find and nobody will rotate.

Two connectors are refused by the catalogue's own data rather than by anything here, and both
refusals name which fact is missing:

- **No declared authority** — `Provider::authority` is `Option`, and `connector_spec` returns
  `Ok(None)` from `credential_ref_for` for exactly this case. Without an authority no address
  renders at all, so there is nowhere to put the value. It is **not** defaulted to anything: a
  guessed authority is a credential written to an address no operation will ever read from, which is
  a silent loss dressed as a success. Every connector in catalogue 0.8.0 does declare one, so the
  test for this drives the derivation directly with a declaration that does not.
- **No declared credential** — `freshdesk` declares none, which flux-connectors' `AGENTS.md` records
  as an intentional gap (C-16: the IR cannot yet say that an API key occupies the *username*
  position). There is nothing to address, so `POST` refuses and names the connector.

## Where the store is bound, and what happens when it is not

**Decision: `AppState` carries `Option<Arc<dyn SecretStore>>` — the port, not the concrete store —
and an unbound store makes every connection route answer `503` naming
`FLUX_EXCHANGE_CREDENTIALS`.**

`AppState`'s own documentation says it carries the *ports* a composition bound, and `CredentialStore`
is a concrete binding of one. Holding `Arc<dyn SecretStore>` keeps that true, keeps the deployment
that wants Vault able to bind it, and — not incidentally — keeps `exchange-server` off the
`#[cfg(unix)]` that `CredentialStore` carries, since only `FileStore` is unix-only.

The binding is additive: `AppState::with_credentials` is a builder method rather than a fourth
constructor or a widened signature on the existing three, so a composition that binds no store is
still spelled the way it was and X-04's identity work does not collide with this.

`main` binds a store when `FLUX_EXCHANGE_CREDENTIALS` names one, and refuses to start when it names
one that cannot be opened. **Unset binds nothing**, which is the same shape as
`FLUX_EXCHANGE_DEV_IDENTITY`: the default is inert, nothing has to be turned off to be safe, and a
value that is set and wrong stops the process rather than being worked around. This is not the
in-memory fallback X-09 refuses — there is no fallback, there is a `503` that names the setting and
the example path. A host with no store bound cannot hold a credential and says so, every time it is
asked.

## What this deliberately does not do

- **No configuration on a connection.** The Goal names "a connector plus its credential and
  configuration", and configuration is exactly where the per-instance facts live — the Zendesk
  subdomain that `base_url` templates. It has no home until the address can tell two instances
  apart, so it lands with X-14 rather than being stored now against an address that would collide.
  The Acceptance does not ask for it.
- **No update or rotation.** `POST` on an existing connection is the X-14 refusal, not an upsert,
  because an upsert is precisely the silent overwrite this story exists to prevent. Rotation needs a
  shape that can say "replace *this* instance's value", which is X-14's.
- **No use of a connection.** Invoking an operation is X-12 and is blocked on the engine line
  (X-11). Nothing here reads a credential value back out to a caller — `GET` answers with addresses
  and never with values, and there is no route that returns one.
- **No listing straight from the store.** See the cost note above; `FileStore::paths()` exists but
  is not on the `SecretStore` port, and reaching past the port for it would tie this surface to one
  store implementation.

## Addendum, 2026-08-01 — a store failure keeps its kind (X-18, X-20)

The rollback decision above is unchanged. What it did not say, and what two stories then had to
establish separately, is that **the kind of store failure survives into the refusal** rather than
being flattened.

Both `partly_written` and `partly_destroyed` originally answered `503` "retrying may work" for every
kind. A create or delete refused because the store *denied this host access* is not a condition
retrying resolves, and an operator told to retry does that instead of fixing the permission —
which is the misinformation `store_failed`'s own doc argues against at length, reappearing on the
partial paths.

Both now read one shared mapping, `store_failure`, returning `(status, what-happened, what-to-do)`:

- **The rollback clause and the kind's advice are separate sentences**, because they answer different
  questions. The rollback says whether a retry is *safe*; the kind says whether it is *worth
  anything*. For `Unreachable` that reads slightly redundantly — "so retrying is safe. Retrying may
  work" — and it was left that way deliberately, because rewording would have quietly restated a
  sentence operators may already have seen.
- **The three caller-facing sentences are pinned byte for byte** by
  `a_store_failure_says_what_it_has_always_said`, so the shared mapping cannot be reworded by
  accident. That tripwire drives them through `store_failed` only; a wording change made *inside*
  `partly_written`'s own format strings is still uncovered.
- Rollback is not available on the delete path at all — a destroyed credential cannot be put back,
  because this host never held the plaintext — so there the answer is honest reporting rather than
  restoration. See [X-18](../stories/X-18-delete-partial-failure.md).

**Known coverage gap:** `no_answer_or_refusal_carries_a_credential_value` claims to drive every
answer and refusal this module can produce, and does not drive `partly_written`'s two branches. The
disclosure properties are asserted directly by X-18's and X-20's own tests instead. Closing the gap
means rearranging that test's arming order around an already-half-destroyed connection; it is
recorded here rather than left as a false claim in the test's doc.
*Closed by X-29 — see the addendum below.*

## Addendum, 2026-08-01 — the partial delete says only what it knows (X-29)

X-18's review, which ran late, found two things the addendum above did not settle. Both are about a
`DELETE` that fails part way, and neither is a regression — they are cases the earlier stories did
not reach.

**`left_behind` asserted more than this host can know.** The refusal said the addresses in
`left_behind` were to be treated "as still usable by anyone holding them", flatly. But a connector
may legitimately hold a *subset* of what it declares (`a_connection_may_carry_a_subset_of_what_is_declared`),
and `remove` deletes the whole declared set — so an address in that list may never have held
anything. Reproduced with `slack` connected by `bot_token` alone: `signing_secret` was named as
still-usable at an address where the store held nothing in the same run.

**Decision: hedge the sentence, do not narrow the list.** The sibling `partly_written` already says
"Some credentials *may* remain", and this now reads the same way — "a credential may remain at any of
them, so treat every one as still usable by anyone holding it". Narrowing `left_behind` to what the
pre-delete probe saw, the way `destroyed` is narrowed, was rejected and the asymmetry is deliberate:

- The two halves are not symmetric in what a mistake costs. Calling an empty address `destroyed`
  over-reports a revocation, so that list is narrowed. Dropping an address from `left_behind`
  *under*-reports one — and on a revocation surface an address nobody mentions reads as gone.
- The probe is stale by the time the loop runs, which is the stated reason the whole declared set is
  deleted rather than only what the probe saw. A value may have appeared since. So the addresses a
  narrowing would drop are exactly the ones this host knows least about.
- The safe bias therefore survives intact: the list is unchanged and the *instruction* is unchanged.
  Only the claim behind it moved, from something this host cannot know to something it can.

**The first failure kind won, not the worst.** `failure.get_or_insert(error)` kept the first error
the loop saw, so an `Unreachable` at one address followed by a `Denied` at the next answered `503`
"retrying may work" — with the denied address named in that same response's `left_behind`. That is
the misinformation X-18 and X-20 exist to end, in the one case where the loop sees more than one
kind, and after X-18 made the loop best-effort it was the only place on this surface a `Denied` could
still be answered "retrying may work".

**Decision: keep the worst, ordered by `Escalation`** — transient, then restore-this-host's-access,
then repair-the-store. The boundary that matters is the first, between a failure that may resolve
itself and one that will not; the second separates two kinds that already share `502` and "retrying
will not help", and is settled by which refusal admits less, since a store this host could not
interpret should not be summarised as one that gave a clear answer. It is a second match on the same
variants rather than a fourth element of `store_failure`'s tuple, because what a caller is *told* and
how two failures *compare* are different questions.

`TestStore` gained per-address delete control (`delete_fails_at`) to drive it. Neither a global flag
nor a counter can arm two kinds in one `remove`, which is why no earlier story caught this.

**And the coverage gap above is closed on the side that was cheap.**
`no_answer_or_refusal_carries_a_credential_value` now drives both of `partly_written`'s branches. It
still does not drive `allowance_change_in_flight`, which needs a tenant-wide claim held across a
request from another task; that one names no address at all, only a connector id. The test's doc now
carries the list of what it reaches instead of the claim that it reaches everything — three stories
in a row had to re-discover that the claim was false.

## Addendum, 2026-08-01 — the allowance is decided at a wider claim (X-25)

The claim decision above is unchanged in its own terms, and its test
(`claims_do_not_reach_across_tenants_or_connectors`) is still green. What X-22 then added underneath
it was a *second* read-decide-write of a different width, and the claim above is too narrow for it.

`MAX_TENANT_STORE_BYTES` is read as a sum over **every** connector a tenant could hold. A claim on
`(tenant, connector)` therefore leaves that read true only until another of the same tenant's
connectors is written — so one tenant issuing concurrent `POST`s to *different* connectors had each
of them read an occupancy the others had not written yet, all were admitted, and the tenant ended up
past the allowance. The overshoot was bounded rather than unbounded, because every value still
passed the per-value bound, but "bounded by the thing the per-tenant bound exists to improve on" is
not the property that was claimed.

**Decision: `create` additionally holds a claim on the tenant, `ConnectionGuard::claim_tenant`,
across the allowance decision and the writes that make it stale.**

- **Two claims of two widths, not one wider claim.** The per-connection claim keeps its meaning —
  it is what makes a `POST` and a `DELETE` to one connection exclude each other — and the wider one
  is taken only where the allowance is actually decided. Widening the existing key instead would
  have made every connection change tenant-wide, including `DELETE`.
- **`DELETE` stays outside it.** Destroying credentials only *frees* allowance, so it cannot cause
  an overshoot, and the case a `DELETE` exists for is revoking a leaked secret — an operator doing
  that must never be made to wait on an unrelated create.
- **It stops at the tenant.** Two tenants still never contend; that was the property the original
  granularity existed to protect, and a global lock would have been shared fate between tenants in
  the repository whose entire point is that they share nothing. The second half of
  `one_tenants_concurrent_creates_cannot_overshoot_its_allowance` asserts it in the same run as the
  bound.
- **Neither claim ever waits**, so a request holding both cannot deadlock against one holding them
  in the other order — there is no order to get wrong.
- **A refusal of its own, `allowance_change_in_flight`.** `change_in_flight` names the connection as
  the thing in flight, which would be a false statement here: nothing is wrong with the connector
  the caller asked for, and another of their own connections is being changed.

**What it costs.** Only concurrent creates by one tenant contend; sequential ones never do, because
the claim is released before the response is written. What a sequential create pays is one hash-set
insert and one removal — measured at ~80 ns for a take-and-release, against a create that costs
~300 µs, dominated by the per-address walk `occupied` already made. End to end, eight sequential
creates measured 2.33 ms before and 2.39 ms after, which is inside the run-to-run spread on the
machine that measured it. The trade is therefore paid entirely by a client that fires several
creates for one tenant in parallel: it now gets a `409` it can retry, where before it got a `201`
and an allowance that did not hold.

**Still single-process, and no more than before.** Both claims are in-process, and two replicas over
one store race exactly as the section above already records. Nothing here narrows that.

## Addendum, 2026-08-01 — a credential is replaced in place, and that is not an upsert (X-39)

"**No update or rotation**" above is no longer true, and the sentence it sat next to still is: `POST`
on an existing connection is the X-14 refusal and **nothing about that changed**. What was missing is
the other operation — an operator saying *replace this, I know it is there* — and its absence meant
that rotating a leaked secret was `DELETE` then `POST`, with a window in between where the tenant has
no connection and everything relying on it fails. On a revocation path that also hands the operator
X-18's partial delete, which is the one place a live vendor credential can survive a `DELETE`.

```text
PUT /api/connections/{connector}/credentials/{credential}   {"value": "…"}
```

**Decision: rotation replaces one credential, named in the path, and is refused when that credential
is not already there.**

### Why it is not an upsert, and cannot be reached from one

An upsert is a create that does not know whether it is replacing something. A rotation knows: it
names the credential it expects to find, and where a create would write into an empty address this
**refuses** — `not_connected` when the tenant has no connection to the connector, and a new
`nothing_to_rotate` when the connection is there and does not hold that credential. The second is an
ordinary case rather than damage, since a connection may legally carry a subset of what the connector
declares, and answering it by writing would be the `409` undone through the other door.

The separation is structural rather than a flag: a different **path**, a different **method**, and an
**incompatible body**. `{"credentials": {…}}` does not deserialise as a rotation and `{"value": "…"}`
does not deserialise as a create, so reaching a replacement takes all three being deliberate.
`a_create_cannot_slip_into_a_rotation` drives every crossing and asserts the stored value is
unchanged after each.

### One credential, not the declared set

The alternative — `PUT` the whole connection, replacing every declared credential at once — is
refused, and the argument is this repository's north star rather than taste. **This host never hands
a credential value back out**: `GET` answers with addresses, and there is no route that returns a
value. So a wholesale replace would require a caller to re-send every value it wants to *keep*, which
an operator rotating one of `slack`'s two credentials has no way to obtain. A body carrying only what
they hold would destroy the rest. A surface whose safe use depends on reading values back out cannot
exist on the host whose whole claim is that the credential never crosses the boundary.

Per credential also matches the failure it exists for: it is one secret that leaks. Rotating several
is several requests, each atomic on its own, and a credential nobody named is untouched.

### The window, closed by using the store rather than working around it

`SecretStore::put` is an atomic whole-file replace, so a rotation is **one `put`** and the address
holds the old value until it holds the new one. "No observable state in which the tenant has no
connection" is therefore a property of the operation, not a promise about it, and it is asserted two
ways in `a_credential_is_rotated_in_place_and_the_connection_is_never_gone`: a reader hammering
`GET /api/connections/{connector}` throughout, with the test store's window widened, never sees the
connection incomplete; and the store served **zero deletes**, which is the structural half, since a
`delete` is the only operation that could empty the address.

That is also why there is no third `partly_*` refusal. `partly_written` and `partly_destroyed` exist
because their operations loop over several addresses and can stop in the middle. A rotation cannot:
`rotation_failed` reads the same `store_failure` mapping, so the kind survives exactly as X-18 and
X-20 established, and says the thing that is actually true instead of naming a half — **the value at
that address is the one that was there before the request**, `"replaced": false`.

### A refused rotation destroys nothing, including at the allowance

Every refusal is ordered before the only write there is, so the guarantee is structural rather than
maintained. The case that matters is X-22's: a rotation to a *larger* value that would take the
tenant past `MAX_TENANT_STORE_BYTES` is refused and the old value survives —
`a_rotation_past_the_tenant_allowance_is_refused_and_the_old_value_survives`. The naive shape here is
delete-then-write with the bound checked between the two, which leaves the tenant holding neither.

The allowance is decided on the **difference**, since a rotation is a replacement: the value being
replaced is taken out of the tenant's occupancy before the new one is added in. Counting the whole
new value against an occupancy that still includes the old one would refuse rotations that fit, and
telling somebody with a leaked secret to go and disconnect something is the wrong instruction at the
worst moment. Both claims are taken exactly as `create` takes them — `(tenant, connector)` across the
probe-decide-write, and the tenant across the allowance decision.

### What it does not do

- **It does not add a credential to an existing connection.** `POST` refuses with `already_connected`
  and there is no other route, so an operator wanting `slack.signing_secret` on a connection that has
  only `bot_token` still has to `DELETE` and re-`POST` the set. `nothing_to_rotate` says so rather
  than naming a remedy that would answer `409`. That is a gap this story deliberately did not widen,
  because "add" is a create and creates are what the `409` governs.
- **It does not touch the instance dimension.** A rotation addresses the same single address X-14
  will make plural, through the same `address_of_declared` seam.

### A path parameter that is a catalogue key, not an address

**See also the X-54 addendum below**: `PUT .../credentials/{credential}` is no longer
`Access::Principal`. Everything in this section about `{credential}` being a catalogue key is
unchanged; what changed is which kinds of principal reach it at all.

`{credential}` is the flat-namespace name the catalogue publishes, and it is admitted on exactly
`{connector}`'s argument: it is a key into a declaration compiled into this host, refused when the
connector declares no such name, and it never reaches the address — the address carries the declared
`leaf`, which the catalogue supplies. `no_route_here_accepts_an_address` widened its allowed set by
that one name and paid for it with `a_hostile_credential_name_cannot_reach_the_address`, which drives
rendered addresses and traversals at the route and asserts the store is untouched — and, because the
`UndeclaredCredential` refusal echoes the caller's own name back, asserts the refusals are **byte
identical** whether or not another tenant holds a connection. A mirror is not an oracle.

## Addendum, 2026-08-01 — who may touch a connection is decided, not inherited (X-54)

X-47 gated the connection **settings** write to `PrincipalOfKind(MAY_CONFIGURE)` — humans only —
and stopped there, which was correct: that was the narrowest change closing a *stated* invariant
against a *measured* exfiltration. It left the two routes that write the credential itself at
`Access::Principal`, which admits every kind, and that answer was never decided. It is what
`Access::Principal` happened to mean before agents existed.

**Decision: `POST /api/connections/{connector}` and
`PUT /api/connections/{connector}/credentials/{credential}` are
`Access::PrincipalOfKind(MAY_SUPPLY_A_CREDENTIAL)` — `PrincipalKind::User` and nothing else. Every
other route under `/api/connections` stays open to every kind, and each carries the argument for
that beside its declaration.**

| Route | Who | Why, in one line |
| --- | --- | --- |
| `GET /api/connections` | any kind | addresses and a `held` boolean, never a value |
| `GET /api/connections/{connector}` | any kind | the same, for one connector |
| `POST /api/connections/{connector}` | a `User` | it decides which credential this tenant's operations run under |
| `DELETE /api/connections/{connector}` | any kind | visible, undoable, and nothing survives revoking the token that did it |
| `PUT .../credentials/{credential}` | a `User` | the same substitution, and invisible — it replaces in place |
| `GET .../settings` | any kind | `binds` targets and a `set` boolean, never a value |
| `PUT`/`DELETE .../settings/{service}/{field}` | a `User` | X-47; the value steers where the credential is sent |

### The invariant does not say this in as many words, and it covers it

`AGENTS.md` reads *"an agent's token grants access to an operation, never to a credential"*, and the
obvious reading is about a credential travelling **out** to the agent. Neither route does that —
this host hands no value back on any route, which is the north star. What they do is the
substitution in the other direction, and the sentence covers it on any reading that is about
*authority* rather than about *bytes*: a caller that can decide **which** credential the tenant's
operations run under has been granted the credential position, whether or not it ever sees a value.
The account those operations then reach is the writer's, and every ticket, message and record the
tenant's agents create afterwards is created there.

### Why these two and not the `DELETE` beside them

X-40 left `DELETE` open on a stated test — *what does this outlive?* — and the same test is what puts
these two on the other side of it.

- **Nothing records who supplied a credential.** *"A connection is what the credential store says it
  is"* above is the reason: there is no record beside the store, deliberately. So
  `GET /api/connections` answers `held: true` for a value an agent planted exactly as it does for the
  one a human did. A rotation is more invisible still — X-39 made it a single atomic `put` with no
  observable state in which anything is missing, which is a virtue for an operator and a gift to an
  attacker.
- **Revocation is not a remedy.** Revoking the agent's token stops the agent; it does not take the
  value back out of the store, and nothing points an operator at the address to look at. That is
  `agents::MAY_MINT`'s argument — an incomplete remedy an operator cannot see — reached by a
  different route.
- **`DELETE` has neither property.** It is visible, the operator holds the plaintext this host never
  did and can reconnect, and no authority survives it. Whether an agent should reach a destructive
  route at all is the **grant-shaped** question — *what may this principal do* — which is X-13.
  `an_agent_may_still_read_a_connection_and_disconnect_one` is what keeps this story from having
  quietly taken it, and it is a real risk: `POST` shares a path with `GET` and `DELETE`.

### The gate is declared per method, so the path is declared twice

`Access` is a field on a `Route`, and `/api/connections/{connector}` has three verbs that do not
agree about it. So `routes::connections::MODULE` declares that path **twice** — once for
`get(show).delete(remove)` at `Access::Principal`, once for `post(create)` at
`PrincipalOfKind(MAY_SUPPLY_A_CREDENTIAL)` — and axum merges the two method routers, each verb
carrying the guard its own declaration asked for.

The alternative was a check inside `create`. It is rejected for the reason `Access` exists at all:
*a route is not guarded by its handler remembering to ask*, and a rule applied inside a handler is
invisible to `the_kind_gated_surface_is_only_what_was_declared`, which walks `published()`. A
duplicated path costs one extra line in that enumeration. An invisible gate costs the enumeration.

### `Service` is refused, and that is a decision made now rather than deferred

Refused on `agents::MAY_MINT`'s argument rather than a new one: nothing in this repository mints a
service credential, verifies one, lists one or revokes one — `PrincipalKind::Service` is a kind the
identity port may return and nothing else. A credential this host cannot attribute to a revocable
caller, written at an address nothing records the author of, is the same incomplete remedy one level
further out of sight.

What that costs is real and should be named rather than discovered: **credential rotation is exactly
what a service integration would want.** A provisioning backend that wires a tenant's connectors up,
or rotates them on a schedule, is a legitimate caller and it is refused here. The decision is still
to refuse today, on the direction the two mistakes point — admitting a kind with no revocation path
is a hole nobody meets until a credential leaks, while refusing one is a `403` met on the first
attempt and filed as a story. Widening this is one kind added to a list with an argument beside it
and one line changed in `KIND_GATED`; narrowing it after something depends on it is not. **The story
that wants `Service` here is the story that gives it a revocation path.**

### What this does not close: there is no operator kind

`User` is **every** signed-in human of the tenant. So this gate says *a human, not a bot*; it does
not and cannot say *the human who set this tenant up*. A `User` who did not supply a credential can
still replace it with one they control, and every operation the tenant runs afterwards reaches their
account.

That is the same within-tenant gap [`connection-settings.md`](connection-settings.md) § *What this
does not close* records — and it is the same gap, not a second one. That section wants a surface
where an **operator** pins what a tenant may configure; this wants one where an operator says which
humans manage connections. Both are the authorization question, which is X-13's grant model, and
**no kind gate can answer it.** Inventing an `Operator` variant on `PrincipalKind` was considered
and rejected: it would put a policy model in the identity vocabulary, where nothing mints it,
nothing revokes it, and no identity port knows how to return it. `PrincipalKind` answers *what kind
of caller is this*, which this host reads off the credential it issued; *which humans of a tenant
may manage it* is a fact about a tenant's own organisation that this host is told, not one it can
derive.

### What the gate rests on, unchanged from X-47

A kind gate is only as good as what the identity port reports. `dev_identity.rs` mints
`PrincipalKind::Agent` from a roster string, and a composition whose identity port mislabels an
agent as a user bypasses every gate here. That is the port's contract rather than this surface's,
and it is why the roster is fixed at startup and forces a loopback bind.

The second residual is X-47's, and this story does not clear it either: **the declared access is the
only enforcement point.** The credential surface's own pattern is enforce-twice —
`agents::MAY_MINT` is re-checked inside `AgentStore::mint` — and neither `create` nor `rotate` can
mirror it, because both write through `SecretStore::put` on a port that takes a `CredentialRef` and
a `Secret` and has no principal to check. Widening that port is a change to a published crate this
repository does not own. If a second handler ever reaches `SecretStore::put` without declaring an
access, nothing stops it.

## Addendum, 2026-08-01 — an empty credential list now says *why*, and this host declines to (X-67)

**Decision: the catalogue's new `Declared` / `Withheld` / `NoneRequired` distinction is read and
asserted, and it is deliberately *not* published on
`GET /api/catalogue/connectors/{id}/credentials` in this story. X-50 owns that, and this addendum
is the measurement it was waiting for.**

Catalogue 0.10 (upstream C-235) added `connector_catalog::Operation::credential_requirement`. Until
it, an empty `Operation::credentials` meant one of two opposite things and nothing could tell them
apart: the vendor genuinely wants no credential, or a credential is needed and the connector cannot
declare it. This repository has a whole branch keyed on that ambiguity — X-46 publishes declarations,
X-49 pinned the `declares-nothing` render with `freshdesk` as the one connector reaching it, and
X-50 asks whether such a connector can be connected at all.

### The measurement, which is one-sided

Over catalogue 0.10, walking `connector_catalog::operations()`:

| Requirement | Operations | Providers |
|---|---|---|
| `Declared` | 670 | 53 |
| `Withheld` (`no-credential`) | 9 | `freshdesk`, all of it |
| `NoneRequired` (`no-credential-required`) | **0** | — |

**There is no positively-public operation in the shipped catalogue.** The one connector reaching the
`declares-nothing` branch is not a connector that needs nothing; it is a connector whose API key
occupies the Basic *username* position, which upstream's model treats as non-secret config, so
declaring it would route a live key outside the secret gate. Nothing is missing and nothing is
supplyable, *and the operation does not work* — upstream's own words for it are that this is
"neither of the other two states".

### Why that answers X-50's question rather than this one

X-50 asks: *is a credential-less connection a thing this platform has?* Its argument is that a
connection exists exactly when the store holds a value at a derived address, so a connector with
nothing to store has no address to occupy and the disabled Connect button is correct. That argument
survives — but its *example* does not. `freshdesk` was never the credential-less connector the
question is about, and the catalogue now says so in a field rather than by an absence.

So the note X-50 has to write is a sentence this measurement changes: not "this connector needs no
credential, and a connection is a stored value, so there is nothing to connect", but "this
connector's credential cannot be supplied through this platform, so it cannot be connected and it
will not work". The second is the one an operator can act on, and it agrees with X-47's refusal of
the same connector for a different reason — which is X-50's fourth Acceptance item, and now
reachable, because both refusals are about `freshdesk` being unserviceable rather than about two
unrelated gaps.

### Why the distinction is not published here

Three reasons, in the order they bind:

1. **It is per-operation and this body is per-connector.** `credential_requirement` sits on
   `Operation`; `ConnectorCredentials` publishes `Provider::auth`. Folding nine operation-level
   facts into one connector-level field is a modelling decision with a wrong answer available
   (a connector whose operations disagree), and X-67 is a bump.
2. **The console renders the two identically and this story may not touch it.** A field nothing
   reads is a wire change that buys nothing; the value arrives when the note changes, and the note
   is `Connect.mts`'s.
3. **X-46 pinned these answers byte-identical, and X-50 is open against them.** Adding a field here
   ahead of X-50 would decide X-50's question in a commit that is not about it.

What *is* done here is that the fact stops being invisible:
`a_connector_that_declares_nothing_is_not_an_unknown_connector` now asserts both halves — every
operation reaching the branch is `Withheld`, and no operation anywhere is `NoneRequired`. The day
upstream ships a genuinely public operation, the empty list has two meanings on one wire field and
that test is what says so.

### Two catalogue facts this bump also measured, recorded where they will be looked for

- **`freshdesk` is still the only connector with no provider-level `auth`**, and the listing still
  probes **60 addresses across 53 addressable connectors** — 54 in catalogue 0.10. The address count
  did not move even though `algolia` arrived declaring one, because upstream C-430 withdrew
  Postmark's whole `account` service and the credential it needed went with it. The numbers in
  *A connection is what the credential store says it is* above were written against catalogue 0.8.0
  and are re-measured here rather than left to decay.
- **`intercom`'s refusal is now over-broad, and deliberately left that way.** C-225 gave it
  `https://{host}` with a *closed set* of three vendor hostnames (`config_choices` on `Provider`,
  new in 0.10), so the value a tenant supplies cannot in fact be anything they choose. `host_pinning`
  reads the template and not the choice set, so it returns `WholeAuthority` and X-47's rule refuses
  the connector. That is fail-closed and correct under the rule as written; teaching `host_pinning`
  to admit a closed set is a safety-surface change that belongs in a story of its own, not in a bump.
