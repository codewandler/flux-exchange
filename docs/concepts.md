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
| **Agent** | A model plus an authored loop plus a bounded operation and datasource surface. It is not a bearer-token principal. | Flux | Target architecture as a **Managed Agent** |
| **Session** | Event-sourced conversational continuity for an agent. | Flux | Target architecture |
| **Run** | One execution of an operation, journey or agent turn, correlated to its durable evidence. | Flux + Exchange binding | Target architecture |

## Vendor and connection vocabulary

| Term | Definition | Owner | Exchange status |
|---|---|---|---|
| **Connector** | A compiled declaration of what one vendor can do in both directions and what an operator must supply. It is not a running connection. | flux-connectors | Live; the published Rust catalogue type is still named `Provider` |
| **Service** | One connector API surface with its own endpoint/version and a partition of that connector's operations. | flux-connectors | Live as operation metadata; there is no standalone published `Service` value |
| **Connection** | A tenant-owned installation of a connector, including a stable instance identity, settings and credential addresses. | Exchange | One connection per connector is live; multiple instances are X-14 |
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
| **Datasource Definition** | A declaration of a readable record/retrieval surface. It contains no tenant value or credential. | Flux or connector package | Flux contracts are published; connectors do not yet publish vendor-data definitions |
| **Datasource** | A tenant-bound readable surface available to an app or managed agent. V1 reads live systems through governed operations; indexing is a separate later binding. | Exchange | Target architecture |
| **Event Type** | A connector- or program-declared event schema/name. It is a type, not an occurrence. | flux-connectors / Flux-Lang | Connector declarations are published |
| **Event Delivery** | One occurrence of an event, with identity, source, payload lifecycle and delivery outcome. | Exchange | Live at-most-once delivery exists for generated channels; durable inbox is target architecture |
| **Webhook Endpoint** | An HTTP transport endpoint that verifies and admits vendor callbacks into a Channel. It is neither an Event Type nor a Trigger, and its URL is deployment configuration rather than app authority. | Exchange channel host | Target architecture; generated WebSocket channels are the current inbound slice |
| **Trigger Declaration** | A Program member naming an event label and a journey or agent target. | Flux-Lang | Published in `codewandler-flux-lang` |
| **Trigger** | A tenant-installed binding of exactly one trigger declaration to an installed event source and target. | Exchange | Target architecture |
| **Activity** | A safe projection of retained evidence: actor, resource, outcome and correlation—not secret payload storage. | Exchange over Flux evidence/events | Target architecture |

External live subscriptions may be at-most-once. Once Exchange accepts an event for an installed
trigger, its durable delivery contract is at-least-once only where retry is safe; an unsafe or
ambiguous side effect becomes dead-lettered or indeterminate rather than silently repeated.

## Installed application and authority vocabulary

| Term | Definition | Owner | Exchange status |
|---|---|---|---|
| **App Package** | An immutable, versioned Program plus metadata and integrity/provenance from a curated signed registry. | Exchange packaging over Flux Program | Target architecture |
| **App** | A tenant-installed App Package revision with frozen reviewed bindings: connections, operations, datasources, triggers, model profile, scopes, risk ceiling and quotas. | Exchange | Target architecture; the Flux host crate is not yet published |
| **Managed Agent** | An Agent declaration inside an installed App, hosted and supervised by Exchange. | Exchange binding over Flux Agent | Target architecture |
| **Service Account** | A non-human Exchange principal holding a one-time bearer token and receiving grants. It calls APIs but is not a Flux Agent. | Exchange | Currently exposed under the legacy “agent” name; migration is required before managed agents ship |
| **Model Profile** | A tenant-owned selection of Model Provider, model and credential/configuration used by managed agents. | Exchange | Target architecture |
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
