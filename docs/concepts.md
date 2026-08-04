# Flux family concepts

This is Flux Exchange's canonical vocabulary. It uses the same execution terms as Flux and the
same vendor terms as flux-connectors, then defines the tenant-bound resources Exchange adds. A term
may exist in the domain model before its Exchange API exists; the final column makes that explicit.

## Shared execution vocabulary

| Term | Definition | Owner | Exchange status |
|---|---|---|---|
| **Operation** | The universal callable unit, carrying input/output schemas, effects, risk and idempotency. | Flux; connectors declare vendor operations | Live |
| **Tool** | The model-visible projection of an operation. Every tool is an operation; an operation with `expose false` is not a tool. | Flux | Live through the connector pack |
| **Program** | A Flux-Lang module declaring agents, channels, datasources, triggers, journeys and composed operations. | Flux | The declaration is published; storage/hosting is target architecture |
| **Journey** | A named Flux flow that may suspend and resume. It is the durable-flow noun; “workflow” is descriptive prose, not another runtime type. | Flux | Target architecture |
| **Agent** | A model plus an authored loop plus a bounded operation and datasource surface. It is not a bearer-token principal. | Flux | Live as a **Managed Agent** inside an installed App |
| **Session** | Event-sourced conversational continuity for an agent. | Flux | Live for installed Managed Agents through tenant/App-isolated Flux event logs |
| **Run** | One execution of an operation, journey or agent turn, correlated to its durable evidence. | Flux + Exchange binding | Live for installed Managed Agent turns |

## Vendor and connection vocabulary

| Term | Definition | Owner | Exchange status |
|---|---|---|---|
| **Connector** | A compiled declaration of what one vendor can do in both directions and what an operator must supply. It is not a running connection. | flux-connectors | Live; the published Rust catalogue type is still named `Provider` |
| **Service** | One connector API surface with its own endpoint/version and a partition of that connector's operations. | flux-connectors | Live as operation metadata; there is no standalone published `Service` value |
| **Connection** | A tenant-owned installation of a connector, including a stable instance identity, mutable operator label, settings and credential addresses. The label selects; the host-minted UUID addresses. | Exchange | Multiple instances per tenant and connector are live; omission is allowed only for a sole connection |
| **Channel Binding** | A connector declaration composing inbound event types with an optional outbound reply operation and transport requirements. It declares; it does not install. | flux-connectors | Generated socket declarations are published and hosted; webhook bindings remain target architecture |
| **Channel** | A deployment-scoped, long-running installed input/output surface. It outlives callers and pushes events. | Flux runtime + Exchange binding | Generated WebSocket channels are live; durable delivery is target architecture |

“Provider” is always qualified in prose:

- **Model Provider** supplies model inference to a managed agent.
- **Identity Provider** authenticates a human or service to Exchange.
- Vendor integration declarations are **Connectors**, even while the connector catalogue retains
  its compatibility type name `Provider`.

## Data, event and activation vocabulary

| Term | Definition | Owner | Exchange status |
|---|---|---|---|
| **Datasource Definition** | A declaration of a readable record/retrieval surface. It contains no tenant value or credential. | flux-connectors for vendor data; Flux owns the wire vocabulary and the consuming seam | Flux contracts are published; the connector datasource surface is chartered by flux-roadmap Decision 0006 and the connectors vendor-datasource design, not yet published |
| **Datasource** | A published connector datasource member bound to a connection label with optional entity/filter scoping, frozen at App install. Exchange serves schema/list/get through the existing admission gate and owns tenant authorization and connection resolution, never retrieval semantics. | Exchange | Live as a frozen installation resource; the read seam and member-reference validation wait on the chartered upstream surface (X-131–X-133) |
| **Event Type** | A connector- or program-declared event schema/name. It is a type, not an occurrence. | flux-connectors / Flux-Lang | Connector declarations are published |
| **Event Delivery** | One occurrence of an event, with identity, source, payload lifecycle and delivery outcome. | Exchange | Live as a durable installed-App inbox; generated channel fan-out remains at-most-once |
| **Webhook Endpoint** | An HTTP transport endpoint that verifies and admits vendor callbacks into a Channel. It is neither an Event Type nor a Trigger, and its URL is deployment configuration rather than app authority. | Exchange channel host | Target architecture; generated WebSocket channels are the current inbound slice |
| **Trigger Declaration** | A Program member naming an event label and a journey or agent target. | Flux-Lang | Published in `codewandler-flux-lang` |
| **Trigger** | A tenant-installed binding of exactly one trigger declaration to an installed event source and target. | Exchange | Live for installed App Event Types |
| **Activity** | A safe projection of retained evidence: actor, resource, outcome and correlation—not secret payload storage. | Exchange over Flux evidence/events | Live for installed Managed Agents and workflows |

External live subscriptions may be at-most-once. Once Exchange accepts an event for an installed
trigger, its durable delivery contract is at-least-once only where retry is safe; an unsafe or
ambiguous side effect becomes dead-lettered or indeterminate rather than silently repeated.

## Installed application and authority vocabulary

| Term | Definition | Owner | Exchange status |
|---|---|---|---|
| **App Package** | An immutable, versioned Program plus metadata and integrity/provenance from a curated signed registry. | Exchange packaging over Flux Program | Live with a built-in curated Slack-bot-style package |
| **App** | A tenant-installed App Package revision with frozen reviewed bindings: connections, operations, datasources, triggers, model profile, scopes, risk ceiling and quotas. | Exchange | Live; authority-widening upgrades require a new review fingerprint |
| **Managed Agent** | An Agent declaration inside an installed App, hosted and supervised by Exchange. | Exchange binding over Flux Agent | Live through chat and declared Event Deliveries |
| **Service Account** | A non-human Exchange principal holding a one-time bearer token and receiving grants. It calls APIs but is not a Flux Agent. | Exchange | Live: canonical create/list/revoke and bearer authentication; the legacy create spelling is removed in v0.17 |
| **Model Profile** | A tenant-owned selection of Model Provider, model and credential/configuration used by managed agents. | Exchange | Live with the key-free static profile binding; production provider bindings remain composition work |
| **Grant** | Tenant authority over metadata selectors and bound resources. Deny wins; a grant conveys operation/datasource access, never a credential value. | Exchange | Tenant-wide operation grants are live; connection/app scopes are target architecture |

An App revision never gains authority because its package changed. Installing or upgrading resolves
the requested operations and datasources, freezes that set, and requires review whenever the set,
scope, risk or model/connection requirements change.

## Ownership test

- Does it change what happens when an effect executes? **Flux.**
- Is it true of a vendor regardless of who runs it? **flux-connectors.**
- Does it require a tenant, held credential, installed binding or retained delivery? **Flux Exchange.**

The credential boundary remains the deciding invariant: **the credential never crosses the
boundary; the authority does.**
