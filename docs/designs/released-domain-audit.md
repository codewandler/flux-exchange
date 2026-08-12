# Released domain audit: Flux 0.54 and connectors 0.21

**Status:** implementation boundary for X-106, updated by X-101 and X-146
**Observed:** 2026-08-03 from crates.io package metadata and the tagged Flux 0.54 / connector 0.17
sources, re-observed 2026-08-12 for the connector 0.21.0 line; local sibling worktree changes are
not treated as released APIs.

## Decision

Flux Exchange adopts the connector line and the Flux 0.54 engine line as one graph. It uses published
contracts directly, writes only tenant/credential/installation bindings locally, and does not copy
an upstream runtime. The connector line has since moved to **0.21** (X-146) while the engine line
stayed at 0.54, which is the ordinary case rather than an exception: the two are one compatibility
unit in that they must not diverge, not in that they move together.

The intended top-level Exchange resources remain Connections, Datasources, Apps, Managed Agents,
Service Accounts, Triggers, Event Deliveries, Model Profiles, Sessions, Runs, Grants and Activity.
The release does not make all of them implementable: declarations, storage primitives and the
guarded connector-channel host and installed App assembly are shipped.

## What actually shipped

| Planned concept | Published contract | Classification | Exchange consequence |
|---|---|---|---|
| Connector, operation, credential addressing | `codewandler-connector-{catalog,pack,address,secrets}` 0.21.0 (0.17.0 when first audited) | **Direct** | Adopted. The Rust catalogue compatibility type remains `Provider`; Exchange prose says Connector. |
| Services | `connector_catalog::Operation::service` | **Direct metadata** | Service-aware connection settings remain valid. There is no published standalone Service resource to mirror. |
| Event types and channel bindings | `connector_catalog::Event` and `Channel`; `connector_pack::channel_plan` | **Direct declaration and plan** | X-101–X-105 host generated socket bindings without giving the pack a transport. |
| Connector datasource | Chartered upstream: flux-roadmap Decision 0006 rule 6 and the connectors vendor-datasource-declarations design; nothing published yet | **Upstream gap, chartered** | Do not invent a connector datasource declaration/backend in Exchange. A tenant Datasource binds the published connector datasource member when it exists (X-131–X-133). |
| Program and declaration members | `codewandler-flux-lang` 0.54: `Program`, `AgentDecl`, `ChannelDecl`, `DatasourceDecl`, `TriggerDecl`, `JourneyDecl` | **Direct declaration** | Exchange can parse/review package intent while keeping tenant installation local. |
| App execution | `codewandler-flux-app` 0.54 | **Direct primitive** | X-108 composes immutable installed Programs, frozen tools and tenant/App-isolated durable event stores through this runtime. |
| Channel execution | `codewandler-flux-channels` and `codewandler-flux-system` 0.54 | **Direct guarded runtime** | The server maps a redacted connector plan into `ConnectorChannel`; the reusable host still opens no socket. |
| Managed agent definition | `codewandler-flux-agent` 0.54 `AgentSpec` plus Flux-Lang `AgentDecl` | **Direct definition** | Agent is the hosted runtime; Exchange token principals remain Service Accounts. X-108 supervises declared Agents inside installed Apps. |
| Datasource vocabulary/live seam | `codewandler-flux-datasource` and `flux_capabilities::LiveDatasource` | **Direct interface** | Use for future live bindings; Exchange owns tenant authorization and connection resolution, not retrieval semantics. |
| Durable sessions/runs/activity | `codewandler-flux-events` 0.54 `EventStore`, including `EventContext` and account-scoped reads | **Direct storage primitive** | Project Activity from the event log; keep delivery payload retention in a separate encrypted inbox. |
| Agent-to-agent messaging | `codewandler-flux-a2a` 0.54 | **Direct protocol** | Use for managed-agent conversation only after the runtime can be hosted. |
| Model profile | model provider/credential contracts are published, but no Exchange binding exists | **Exchange-owned binding** | Persist provider/model/config references per tenant; never expose model credential material. |
| Trigger installation and event delivery | Trigger declarations are pure Flux-Lang data; X-108 supplies the tenant binding and durable inbox | **Exchange-owned binding** | One installed Event Type resolves to its reviewed target; unsafe retry becomes indeterminate rather than guessing. |
| App package registry | No upstream package/index contract | **Exchange-owned packaging** | X-108 verifies immutable revisions and curated provenance without putting registry HTTP in the reusable host crate. |

## Compatibility findings from the dependency move

`connector-pack` 0.17.0 requires Flux core/lang/runtime `^0.54`, so the workspace's direct Flux
pins moved to `0.54`. The engine seam, manifest pin and lockfile tests prove
one line.

The catalogue expansion changed two Exchange safety censuses:

- Asterisk declares `endpoint.host` as the whole `{host}:8089` authority. Exchange refuses tenant
  configuration of it because a tenant-supplied value would choose the credential destination.
- Zendesk now publishes settings across `default`, `help-center` and `messaging`; the settings census
  includes all service-scoped endpoint and Basic-username bindings.

Neither change weakens the credential boundary. They are pinned so the next catalogue expansion is
again a reviewed diff.

### Connector 0.21, at engine 0.54 (X-146, observed 2026-08-12)

`connector-pack` 0.21.0 requires `codewandler-flux-{core,lang,runtime} ^0.54` and
`codewandler-flux-spec ^1.3` — identical to 0.20.0. **The engine line did not move**, and neither did
any `codewandler-flux-*` pin here, notwithstanding that 0.59.3 and `flux-spec` 1.4.0 are published.
The one structural change in the graph is that `connector-pack` now names `connector-address`
directly instead of reaching it through `connector-secrets`; `connector-catalog` still has zero
dependencies.

Same 55 providers, 870 → 878 operations (`github` +4, `gitlab` +2, plus GitLab's new
`gitlab.oauth_token` credential). **No new service appears**, which is the fact the settings census
turns on. Four declaration axes are new, and each is either read-only here or fail-closed:

| New in 0.21 | Exchange consequence |
|---|---|
| `Operation::direction` (`OperationDirection`) | Read-only metadata; grants still select on risk, effects and idempotency. Not used to decide anything yet. |
| `Credential::subject` (`Subject`) | Not read. `Unstated` on 58 of 63 credentials means *unreviewed*, not *app* — a consumer that needs the distinction must refuse on it rather than assume, so nothing here may quietly start branching on it. |
| `Credential::hazard` (`AuthHazard`) | Not read. babelforce is the first released connector to declare one (`ResourceOwnerSecretShared`, upstream C-440). Exchange still decides hazard admission from the injected `AcquisitionBinding`, and production composes an empty `AcquisitionBindings`, so the fail-closed path stays fixture-tested rather than vendor-live. |
| `Acquisition::OAuth2`, `ConfigField::also_services` | Not acted on. GitLab's three `oauth.*` config bindings are an unrecognised namespace: `DeclaredSetting::parse` returns `None`, the non-secret two render visible-but-unroutable, and the client secret takes the existing refusal that keeps a deployment-owned secret out of the tenant settings store. |

The two censuses are unchanged in what they *produce*: the set of `Acquisition::BasicJoin`
credentials driving the Basic-username census is identical across 0.20 and 0.21, and every declared
credential still projects a target. What changed is the surface a future story must build — X-147
owns the OAuth client-registration surface, and `also_services` becomes load-bearing the moment this
host composes a URL for a service other than an operation's own.

## Implementation boundary and order

1. X-101–X-105 host generated connector WebSocket channels against the released guarded runtime.
2. Complete X-14 so every app binding can name a stable connection instance.
3. Complete X-107: migrate legacy agent-token principals and `/api/agents` to Service Accounts with
   a compatibility read path; reserve Agent for hosted Flux agents.
4. X-108 adds Model Profiles and immutable App Package metadata/integrity verification, then
   freezes every tenant binding before activation.
5. X-108 adds tenant Datasource and Trigger resources only against published upstream interfaces.
6. X-108 composes the published App host into durable delivery, App supervision, Managed Agents,
   Flux-event Session/Run/Activity projections and console chat. A2A remains a later protocol edge.

If a future upstream release changes these seams, update this audit and the story sequence before
feature code. A missing upstream contract is a blocker to record, not a license to recreate it.
