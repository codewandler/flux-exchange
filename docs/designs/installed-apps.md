# Installed Apps and Managed Agents

**Status:** accepted for X-108 implementation
**Builds on:** [`released-domain-audit.md`](released-domain-audit.md),
[`connection-instances.md`](connection-instances.md), and [`invoke.md`](invoke.md)

## Decision

An App Package is immutable publisher material. An App is a tenant-owned installation record whose
authority was resolved once and frozen. `flux-app` runs that frozen Program; Exchange supplies only
tools representing the installation's frozen operations and datasources. The resulting Managed
Agent can exercise those tools, but neither its model input nor its runtime authority contains a
credential address or value.

The reusable host owns package validation, tenant resources, installation review, frozen authority,
durable event-delivery state and tenant projections. The server owns composition: it maps an opaque
installation authority to the existing `Invoker`, binds the selected model profile, builds a
`flux_app::App` over a durable `flux_events::EventStore`, and exposes guarded HTTP routes. This keeps
the published host free of a provider transport and preserves the existing connector-pack/egress
split.

## Immutable package boundary

`AppPackage` is keyed by `(id, version)` and contains:

- exact Flux Program source;
- a SHA-256 integrity digest over the complete canonical package document;
- publisher, source repository and immutable source revision provenance;
- required connection capabilities, operation and datasource selectors, model requirement, and
  trigger declarations; and
- no tenant identifier, connection label, setting, credential address, credential value or model
  credential.

The curated registry verifies that digest before admitting a revision and refuses a different
document at an occupied `(id, version)`. Installation parses the source with Flux-Lang and checks
that package trigger declarations agree with the Program. A package is therefore data to review,
not authority merely because it appeared at a familiar name.

## Atomic installation and frozen review

The operator submits labels and resource ids, never credential addresses. Under one store write
claim, installation resolves all requirements against the principal's tenant:

1. each connection label resolves to one host-minted immutable instance id and the connector must
   match the package requirement;
2. the selected Model Profile exists;
3. operation metadata selectors are evaluated over the executable catalogue and intersected with
   the operator's requested access/risk ceiling;
4. datasource selectors resolve only tenant-owned Datasources; and
5. every declared event/target pair resolves inside the installed Program.

Only after every check succeeds is one complete installation record persisted. Any refusal leaves
the prior file bytes and in-memory map unchanged. The record stores exact operation contracts,
datasource revisions, connection instance ids, scopes, risk ceiling, model-profile revision and
trigger bindings. Later catalogue, label, profile or package changes do not widen it.

An upgrade is another installation review. If its authority fingerprint is not a subset of the
currently reviewed fingerprint, the request must explicitly carry the new review fingerprint. This
makes widening a visible optimistic-concurrency decision rather than a side effect of changing a
package version.

## Runtime authority and supervision

`AppRuntimeToken` is an opaque, process-local capability minted from one installation revision. Its
fields are private and it serializes nowhere. The only useful operation on it asks the App store to
authorize an operation already frozen on that installation. The answer identifies the
frozen operation and selected connection instance; it never returns a `CredentialRef` or secret.
The existing `Invoker` derives the credential address from the stored tenant, connector declaration
and connection instance at dispatch.

The server builds one supervised `flux_app::App` per active installation revision. Its tool registry
contains exactly the frozen operation adapters. Datasource revisions are frozen into the same
installation boundary; a package requiring one refuses until its binding exists, and a future live
retrieval adapter can expose only that frozen record. The Program and each Agent declaration
apply an additional upstream capability ceiling. The Flux execution environment carries the
installed App identity, not the human who happened to activate it. Chat and event delivery enter
through `App::deliver`; no second interpreter or request builder exists.

## Triggers and durable deliveries

A Trigger is an installed binding of one declared Event Type to one declared Journey or Managed
Agent target. Admission first writes an Event Delivery with a host-minted id and the payload in the
private durable inbox, then the supervisor attempts it. Completion or refusal is appended to the
delivery history.

Retry is derived from the frozen target's effects. Only an idempotent, non-destructive target may
return to `pending`; unsafe or ambiguous effects end as `indeterminate`. Transport
retry policy never guesses from the event source or HTTP status.

## Sessions, Runs and Activity

Flux's `EventStore` is the execution record. Exchange gives every tenant/App pair an isolated
durable event database and projects Sessions, Runs and run Activity from its streams. Installation
and delivery lifecycle facts remain in the atomic App binding/inbox store. The public run Activity
projection contains ids, session, delivery, timestamps and outcome;
it never includes delivery payloads, prompts, results, credentials or provider configuration.
Every read begins with the resolved principal's tenant and can address only installations already
found beneath it.

## HTTP and console

Operator routes list curated packages and tenant resources, install/upgrade Apps, inspect activation
and list safe activity. Any authenticated tenant principal may talk to a Managed Agent in an App it
can address; tenant derivation still comes solely from the resolved principal. The console's Apps
surface receives all data from `App.vue`, lets an operator select the Slack connection, optional
operation/datasource layers, Model Profile, risk and scopes, and renders activation/chat/activity.
It never receives the runtime token, a credential address, a delivery payload or raw Flux events.

The first curated `exchange-apps/slack-bot` revision is deliberately key-free at the model seam: it
uses a deterministic provider that follows Flux's intent-routing protocol for the checkout demo. A
production composition may
bind real model providers behind the same Model Profile port without changing package or App
authority.
