# Released domain audit: Flux 0.52.1 and connectors 0.16.0

**Status:** implementation boundary for X-104  
**Observed:** 2026-08-03 from crates.io package metadata and the tagged `v0.52.1` / `v0.16.0`
sources; local sibling worktree changes are not treated as released APIs.

## Decision

Flux Exchange adopts connector 0.16 and the Flux 0.52 engine line as one graph. It uses published
contracts directly, writes only tenant/credential/installation bindings locally, and does not copy
an upstream runtime that was omitted from the release.

The intended top-level Exchange resources remain Connections, Datasources, Apps, Managed Agents,
Service Accounts, Triggers, Event Deliveries, Model Profiles, Sessions, Runs, Grants and Activity.
The release does not make all of them implementable: declarations and storage primitives shipped,
but the Program host and channel host did not ship as consumable crates.

## What actually shipped

| Planned concept | Published contract | Classification | Exchange consequence |
|---|---|---|---|
| Connector, operation, credential addressing | `codewandler-connector-{catalog,pack,address,secrets}` 0.16.0 | **Direct** | Adopted. The Rust catalogue compatibility type remains `Provider`; Exchange prose says Connector. |
| Services | `connector_catalog::Operation::service` | **Direct metadata** | Service-aware connection settings remain valid. There is no published standalone Service resource to mirror. |
| Event types and channel bindings | `connector_catalog::Event` and `Channel` | **Direct declaration** | X-99–X-103 may host them; connector-pack itself has no channel runner. |
| Connector datasource | Design/backlog C-137–C-140 only | **Upstream gap** | Do not invent a connector datasource declaration/backend in Exchange. A tenant Datasource can bind a future published backend. |
| Program and declaration members | `codewandler-flux-lang` 0.52.1: `Program`, `AgentDecl`, `ChannelDecl`, `DatasourceDecl`, `TriggerDecl`, `JourneyDecl` | **Direct declaration** | Exchange can parse/review package intent once it directly depends on flux-lang. |
| App execution | `flux-app::App` exists in the tagged workspace, but `flux-app` is absent from the release script and crates.io | **Distribution gap** | No hosted App runtime or speculative adapter. Wait for a published host crate. |
| Channel execution | `flux-channels` exists in the tagged workspace, but is absent from the release script and crates.io | **Distribution gap** | Keep Exchange's connector-channel work behind its host port; do not depend on a sibling checkout. |
| Managed agent definition | `codewandler-flux-agent` 0.52.1 `AgentSpec` plus Flux-Lang `AgentDecl` | **Direct definition** | Reserve Agent for the managed runtime; rename Exchange token principals to Service Accounts. Hosting still waits on App assembly. |
| Datasource vocabulary/live seam | `codewandler-flux-datasource` 1.3.0 and `flux_capabilities::LiveDatasource` in 0.52.1 | **Direct interface** | Use for future live bindings; Exchange owns tenant authorization and connection resolution, not retrieval semantics. |
| Durable sessions/runs/activity | `codewandler-flux-events` 0.52.1 `EventStore`, including `EventContext` and account-scoped reads | **Direct storage primitive** | Project Activity from the event log; keep delivery payload retention in a separate encrypted inbox. |
| Agent-to-agent messaging | `codewandler-flux-a2a` 0.52.1 | **Direct protocol** | Use for managed-agent conversation only after the runtime can be hosted. |
| Model profile | model provider/credential contracts are published, but no Exchange binding exists | **Exchange-owned binding** | Persist provider/model/config references per tenant; never expose model credential material. |
| Trigger installation and event delivery | Trigger declarations are pure Flux-Lang data; no Exchange installation/delivery store exists | **Exchange-owned binding** | Bind one declaration to one tenant source/target, with durable delivery identity and fail-closed retry. |
| App package registry | No upstream package/index contract | **Exchange-owned packaging** | Define an immutable package revision and signed curated index without putting registry HTTP in the reusable host crate. |

## Compatibility findings from the dependency move

`connector-pack` 0.16.0 requires Flux core/lang/runtime `^0.52`, so the workspace's direct Flux
pins moved to `0.52` and resolved to `0.52.1`. The engine seam, manifest pin and lockfile tests prove
one line.

The catalogue expansion changed two Exchange safety censuses:

- Asterisk declares `endpoint.host` as the whole `{host}:8089` authority. Exchange refuses tenant
  configuration of it because a tenant-supplied value would choose the credential destination.
- Zendesk now publishes settings across `default`, `help-center` and `messaging`; the settings census
  includes all service-scoped endpoint and Basic-username bindings.

Neither change weakens the credential boundary. They are pinned so the next catalogue expansion is
again a reviewed diff.

## Implementation boundary and order

1. Finish X-99–X-103 against the released connector declarations.
2. Complete X-14 so every app binding can name a stable connection instance.
3. Complete X-105: migrate legacy agent-token principals and `/api/agents` to Service Accounts with
   a compatibility read path; reserve Agent for hosted Flux agents.
4. Add Model Profiles and immutable App Package metadata/registry verification. Package review may
   parse `Program`, but execution stays unavailable while the App host is unpublished.
5. Add tenant Datasource and Trigger resources only against published upstream interfaces.
6. When an App host crate is published on the connector-compatible engine line, unblock X-106 and
   add durable event delivery, App supervision, Managed Agents, sessions/runs, A2A and console chat.

If a future upstream release changes these seams, update this audit and the story sequence before
feature code. A missing upstream contract is a blocker to record, not a license to recreate it.
