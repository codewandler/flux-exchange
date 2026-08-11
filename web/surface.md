# The surface

An index of the things flux-exchange deals in, and what each word means here. One word per thing,
because most of the confusion in this problem space is two names for one concept or one name for two.

> [!IMPORTANT]
> **Every entry below is a definition, and none of them is a claim that the capability is available.**
> This page says what a channel *is*; it does not say whether the build you are talking to serves
> one.
>
> Where a capability has a page of its own, that page carries the answer as a badge in its chrome,
> derived from this build's `GET /api/onboarding` descriptor — anonymous, machine-readable, and held
> to the service's route table by a test in both directions. Nobody writes those badges by hand and
> nobody can forget to update one.

## The three lifetimes

Routinely conflated, and conflating them produces real bugs — a webhook endpoint that dies when an
agent's conversation ends, or a lease that outlives the grant that opened it. So they are three
words, and each has one owner:

| | Scoped to | Direction | Ends when |
|---|---|---|---|
| **Session** | a caller's conversation | — | it is closed or expires; resumable |
| **Channel** | a deployment | pushes | the operator removes it |
| **Lease** | a caller's grant | pulls | the holder releases it, or its TTL passes |

`lease` is deliberately not called "session": in flux, `session` already means an event-sourced
conversation, and the two have opposite lifetimes and opposite owners. One name each, so a sentence
about one can never be misread as a sentence about the other.

## The index

### Principals and tenants

Who is asking, and on whose behalf. A **principal** is resolved by the host's identity port — a
signed-in human or a **Service Account** presenting its own bearer token. A Service Account has no
model or loop; it is the API identity an App, Agent or other automation may use. A **tenant** is derived from that
principal and from nothing a caller controls, which is the invariant the whole
[credential boundary](/boundary) rests on.

### Connectors, connections and credentials

[The standalone page](/capabilities/connections) carries the derived status for connection and
credential management.

A **Connector** is the compiled declaration of what one vendor can do: operations, services,
credential requirements, settings, event types and channel bindings. It is not a running
integration. The historical Rust catalogue type is still named `Provider`; in product vocabulary,
“provider” is qualified as **Model Provider** or **Identity Provider** and vendor integrations are
Connectors.

A **connection** is one tenant's configured relationship with one connector: a host-minted stable
instance identity, an operator-chosen mutable label, the credential it needs, plus any non-secret
per-connection values a templated connector requires. One tenant may hold several connections to
the same Connector. The label selects one inside that tenant; it is never the credential-address
component, and changing it moves no credential.

Credentials and non-secret settings are stored separately on purpose — a subdomain is not a
credential, and one store holding both would make "held" mean two different things. When a tenant
has several connections, an operation invocation names the label in `?connection=` while preserving
the operation's declared JSON body. Omitting it is valid only when the Connector has a sole
connection; ambiguity is refused rather than defaulted.

A credential is addressed, never handed out. Nothing that reads a connection returns a secret value.

### Apps and Agents

[The Agents page](/capabilities/agents) carries the derived status and names the story that owns the
hosted runtime.

An **App Package** is an immutable versioned Flux Program plus provenance and installation
requirements. An **App** is one tenant's installed package revision with reviewed, frozen bindings:
connections, operations, datasources, triggers, model profile, scopes, risk ceiling and quotas.

An **Agent** is a model plus an authored loop plus bounded operation and datasource capabilities. A
**Managed Agent** is such an Agent declaration hosted inside an installed App. Agent is never a
synonym for Service Account: the first is an execution runtime; the second is an API principal.

### Datasources, events and triggers

A **Datasource Definition** declares a readable record/retrieval surface without tenant values or
credentials. A **Datasource** is the tenant-bound readable surface an installed App or Managed Agent
can use. Operations *do*; datasources *know*.

An **Event Type** is a declared schema/name. An **Event Delivery** is one occurrence with source,
identity and delivery outcome. A **Trigger Declaration** names an event label and an Agent or Journey
target; a **Trigger** is the tenant-installed binding of that declaration to an installed source and
target. A webhook is only one Channel transport that can admit deliveries—it is not an Event Type or
a Trigger.

### `invoke`

Run one catalogue operation for the caller's tenant. The caller names the operation; the request is
built from that operation's own compiled Flux, and the credential is resolved by address from the
caller's tenant. It is the outbound verb of a connector binding.
[Its own page](/capabilities/invoke) carries the derived status.

### `subscribe` and channels

The inbound verb of the same binding. The host authenticates a generated vendor WebSocket from
tenant-held configuration and the subscriber receives a typed, declared event, scoped to the closed
binding/event set that tenant's grant admits.
[Its own page](/capabilities/subscribe) carries the derived status.

**A webhook is a Channel** — not a trigger, and not a session artifact. It belongs to the deployment
and it outlives every conversation that reads from it, which is exactly the distinction the three
lifetimes above exist to keep.

### Grants

The authority a tenant hands an agent, expressed as a selection over declared operation metadata
rather than a list of names, with an explicit deny beating an explicit allow. See
[authority is granted by property, not by name](/boundary#authority-is-granted-by-property-not-by-name).
[Its own page](/capabilities/grants) carries the derived status.

### Workflows

[The workflow page](/capabilities/workflows) carries the derived status and explains which semantics
belong to the stored Flux Program rather than to Exchange.

What an operator calls a workflow — triggers, conditions, schedules, a flow of operations — is **a
stored, versioned, per-tenant flux-app Program**. It is not a second execution model living here, and
describing triggers as a flux-exchange feature would be the largest untruth this site could tell.

The reason is not purity, it is what falls out for free: determinism, replay, fork and diff, approval
gates, typing and risk derivation. And it makes a composed operation **indistinguishable from a
vendor one** — same catalogue entry, same gating, same address — so an agent cannot tell whether an
operation came from an OpenAPI document or from somebody dragging boxes on a canvas. A visual editor
emits IR; the IR lowers to Flux. The simplified schema an editor wants is a *projection*, never a
second model.

### Leases

A **Lease** is the pull-oriented, caller-grant-scoped lifetime in the table above. [Its own
page](/capabilities/leases) carries the derived status and names the story that owns rich runtime
resources.

### Execution records

Who asked, which grant admitted it, what was called, and what came back. The measure is that every
execution is explainable after the fact without reconstructing it from logs.

## Reading this site safely

The [repository](https://github.com/codewandler/flux-exchange) carries the itemized inventory of what
is not built, and it is expected to stay accurate — a page or a type that implies a working service
costs more than an honest gap. This site is a *third* rendering of overlapping facts, so treat the
descriptor as the source and this page as the vocabulary.
