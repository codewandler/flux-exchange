# The credential boundary

> **The credential never crosses the boundary; the authority does.**

That is the product, not a security slogan. flux-exchange exists for the part of integration work
that requires holding a credential or knowing a tenant. Flux executes programs, and the Connector
catalogue declares remote capabilities; Exchange resolves tenant-owned authority at the moment an
admitted operation or event needs it.

This page argues about the software, not any deployment. When it mentions a capability, its linked
page carries the build status derived from `GET /api/onboarding`; the prose here does not guess.

## The domain test

Ask one question: **does this responsibility require holding a credential or knowing a tenant?**

- If no, it belongs in Flux or in a Connector's public declaration.
- If yes, it belongs behind Exchange's principal, tenant, connection and grant boundary.

This keeps policy and secret-bearing state out of the engine without creating a second execution
engine in the credential host. Exchange chooses no vendor protocol of its own: execution ends in the
Connector's compiled Flux.

The dependency and source locks that prevent the reusable host crate from acquiring a second
request path have separate [manifest](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/no_second_request_path.rs#L343)
and [dispatch-seam](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/no_second_request_path.rs#L521)
tests.
Those locks deliberately cover the published host crate; transport additions in the composing
server remain a review responsibility rather than something this page claims the test closes.

## Outbound: one operation, no destination fields

A caller of [`invoke`](/capabilities/invoke) supplies an operation id and that operation's declared
parameters. It cannot supply these three pieces of authority:

| It cannot name | Why that would be dangerous | Where the value comes from |
|---|---|---|
| the **host** | a caller could point a credential at an origin it controls | the operation's compiled Flux and declared connection settings |
| the **credential** | selecting a secret is already possessing secret authority | the selected tenant connection and Connector declaration |
| the **tenant** | one caller could act against another tenant's state | the principal resolved by the host |

The tenant rule is exercised from all three common injection points: a
[path segment](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/identity.rs#L242),
[body field](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/identity.rs#L294)
and [header](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/identity.rs#L322).
The destination rule is driven at dispatch for both
[ordinary parameters](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/invoke.rs#L461)
and [templated hosts](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/invoke.rs#L516).

The caller cannot choose a runtime either. The Connector declares its runtime plan, and a
multi-tenant deployment mechanically refuses locally executing runtimes. The control is
[the deployment-admission test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/invoke.rs#L959).

## Inbound: the same binding, reversed

[`subscribe`](/capabilities/subscribe) is not a separate integration product. It is the inbound verb
of the same remote Connector binding: the host authenticates the remote Channel, accepts only the
closed event set the Connector declares, and projects events to an authenticated subscriber.

A subscriber cannot name an arbitrary source. The channel belongs to the principal's tenant, and
the connector, binding and complete selected event set must be admitted by an inbound grant. The
grant-side refusal is exercised by
[the declared-event-subset test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/src/grant.rs#L848).

That is the inbound confused-deputy problem: handing one tenant events from a binding it merely
named would leak remote authority just as surely as sending that tenant another tenant's credential.

## Grants describe properties, not spelling

[Grants](/capabilities/grants) select facts the Connector declares—connector, risk, effects and
idempotency—rather than enumerating operation ids. An explicit deny wins over an explicit allow.
The precedence rule is checked by
[the deny-precedence test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/src/grant.rs#L900),
and the catalogue-to-policy projection by
[the selector-facts census](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/src/grant.rs#L947).

Property selection matters when a Connector gains an operation. The new operation immediately
meets the existing risk-and-effect policy; it does not inherit authority because somebody forgot to
update a list, and it does not become unusable merely because its name was new.

## Three corrections that define the boundary

### A character allow-list is not a destination policy

Restricting a setting to hostname-safe characters says what a value looks like. It says nothing
about who controls the resulting host. When a template makes one setting the whole authority,
Exchange accepts only an exact choice from a closed set published by the Connector catalogue; when
there is no such set, the value is refused rather than stored or dispatched.

The full shipped catalogue is censused by
[the whole-authority catalogue test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/connection_settings.rs#L888),
and the behavioral path is driven by
[the destination-authority test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/connection_settings.rs#L638).
The latter checks both that nothing reaches the offered origin and that no credential reaches the
wire; it is not satisfied by a string validator that happens to return an error.

### A suffix pins a vendor, not an account

A template that ends in a literal vendor suffix keeps traffic inside that vendor's namespace. It
does not prove that the credential's supplier controls the selected account in that namespace.
This is why “the hostname looks vendor-owned” is useful routing structure but not a complete
authorization argument.

The whole-authority refusal above remains necessary, but it cannot close this smaller within-tenant
gap. That residual is stated below rather than hidden behind the stronger cross-tenant and
agent-versus-operator controls.

### A name check checks spelling, not capability

A block-list of dangerous field names catches only vocabulary somebody predicted. The enforced
rules instead ask structural questions: can this declared field become the whole authority; does
the catalogue publish a closed choice; which principal kind and operator policy guard every write;
and did anything actually dispatch?

The [connection route census](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/connections.rs#L7794)
holds the administrative surface to operator policy. The end-to-end control
[that drives an Agent's offered origin](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/connections.rs#L7511)
then proves that an Agent cannot turn a setting write plus invocation into delivery to its chosen
origin.

## Refuse; never repair

The boundaries above refuse rather than guessing missing authority into existence. The
[planted-store test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-host/tests/connection_settings.rs#L774)
demonstrates the rule in both directions: a whole-authority value that bypassed the write guard still
cannot reach dispatch, and the store remains byte-for-byte unchanged so the evidence is not
“repaired” away.

## What remains open

The operator boundary is real: connection management requires an explicitly configured operator
subject. The [inventory and deletion
test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/connections.rs#L7448)
holds that an Agent cannot read or delete the state, while the setting-write test below covers
rewriting it. Supplier evidence is also real: the connection view can identify which principal last
supplied a held credential and when, without serializing the credential.

Neither is supplier-based authorization. An authorized operator in the tenant who did not supply a
credential may still choose a suffix-pinned account inside the vendor namespace and invoke against
it. If that operator controls the selected remote account, the request can expose the authority of
the tenant's write-only credential there. Operator policy limits *who may act* and supplier evidence
records *who last wrote*; neither proves those are the same person.

The scope control is exercised by [the Agent-setting refusal and logging
test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/connections.rs#L7182),
and the provenance semantics by [the supplier-evidence loss
test](https://github.com/codewandler/flux-exchange/blob/main/crates/exchange-server/src/routes/connections.rs#L4169).
Those tests are evidence for the controls they name, not evidence that the residual gap is closed.

That limit is narrower than allowing an Agent or another tenant to choose the destination, but it is
still a limit. A credible boundary says so in the same voice it uses for the guarantees.
