# Design: rich connector runtimes through Exchange

**Status:** accepted direction, Decision 0001 adopted 2026-08-03 · **Epic:** X-111

Every official external integration executes through Exchange. Docker, Kubernetes, SQL,
observability, secret stores, telephony and future protocol-rich integrations remain connectors;
Exchange executes their declared HTTP, socket, process, container, plugin or remote runtime under
tenant-derived authority. There is no local Flux execution placement or local vendor/plugin fallback.

Flux still owns the language, agent loop, tool projection, authorization and interactive approval.
It also contributes the guarded runtime substrate Exchange binds for rich execution. Its native
Exchange client is compiled into the Flux binary and holds only an Exchange Service Account token;
it neither receives vendor credentials nor selects a second placement. When Exchange is unavailable,
official external tools are unavailable while the language, agent loop and core tools remain useful.

## Existing foundation

The host already reads the closed runtime vocabulary and refuses locally executing runtimes in a
multi-tenant process. Ordinary `invoke` is grant- and tenant-gated. X-101…X-105 persist and supervise
generated connector WebSocket channels and expose authenticated, bounded `/api/subscribe`. X-98 runs
stored Flux programs. X-107 delivers canonical Service Account lifecycle and bearer authentication at
those same boundaries; X-121 removed the bounded legacy spelling.

Those are real prerequisites, but operation dispatch is still HTTP-shaped: `Invoker::invoke`
resolves a catalogue operation through `connector_pack`, and the server supplies
`flux_web::HttpRequestTool`. The other runtime variants currently gate placement; they do not execute
rich outbound operations.

## Milestone 1: effective catalogue and one-shot HTTP

The first useful Flux-to-Exchange path does not wait for rich runtimes or long-lived lifecycle. X-113
adds an authenticated effective Service Account catalogue beside the existing one-shot HTTP invoke.
The projection contains exactly the connected and granted operations available to the resolved
Service Account. It carries a stable generation identity so Flux can refresh tools between turns:
the identity stays stable while the effective projection is unchanged and changes when relevant
catalogue declarations, connections or grants change.

The catalogue and invocation derive tenant and grants from the authenticated principal. Invocation
continues to accept only the operation id, its arguments and the existing tenant-local connection
label. Neither surface accepts or returns a credential, tenant selector, endpoint authority, runtime
or artifact choice. The caller learns usable authority, never the credential behind it.

This milestone is request/response only. Streams, cancellation and terminal outcomes remain X-117;
leases remain X-118. The already delivered subscription socket is a foundation for that later
lifecycle work, not a prerequisite for effective discovery or one-shot invocation.

## One rich-runtime dispatch seam

`connector-pack` projects the connector's compiled declaration into a zero-IO runtime plan. The
reusable host admits runtime and grant, derives tenant-bound configuration and credential ports, and
hands that plan to a closed runtime registry. Exchange dispatches the connector-declared runtime plan;
the server composition binds implementations. Flux contributes guarded runtime substrate, not a
second official-integration execution placement.

Neither the host crate nor server constructs a vendor request, command or handshake independently.
The registry is closed over `http`, `socket`, `process`, `container`, `plugin` and `remote`. A newly
added upstream runtime fails compilation until Exchange decides its placement and grant semantics.
Caller input contains operation arguments only—not runtime, artifact, endpoint authority, tenant or
credential reference. A missing binding is a named refusal, never a fallback.

## Placement

- **Local single-tenant Exchange:** may execute every admitted runtime by binding Flux's guarded
  substrate. This is the `--dev` topology and may also serve one team in production.
- **Hosted multi-tenant Exchange:** the control-plane process executes only shareable HTTP or
  delegates a `remote` plan to an operator-selected per-tenant worker. A socket, process, container
  or plugin plan is refused before credential access unless that isolated placement exists.
- **Worker trust:** an isolated worker is inside Exchange's credential boundary for that tenant. It
  may receive an operation-bound secret in memory when the protocol requires it, but never persists,
  logs or returns it. The external caller still receives authority, never the credential.

X-115 delivers local single-tenant rich execution. X-116 separately owns hosted worker isolation;
that later deployment guarantee does not block the Milestone 1 HTTP path or X-120's local migration
proof. The worker protocol reports what it executed, and Exchange run records distinguish locally
observed from worker-reported evidence.

## Long-lived operations

After one-shot HTTP ships, X-117 and X-118 extend the authenticated connector WebSocket to carry:

1. inbound declared events (`subscribe`, already delivered for generated socket channels),
2. streamed operation output such as logs, process stdout or a socket read loop,
3. cancellation and terminal outcomes,
4. lease acquire, renew, release and disconnect cleanup.

Frames are request-correlated, bounded and grant-scoped. A reconnect never implies replay unless a
connector declares a cursor/replay contract. Refused, unreachable, interrupted and expired remain
distinct outcomes. These are Milestone 3 lifecycle guarantees, not hidden completion criteria on
X-113.

## Runtime artifacts

Exchange installs and executes only attested connector runtime artifacts described by
flux-connectors. The operator chooses which bundles and versions are installed; a caller cannot name
a path, image or tag. Digests, platform/runtime compatibility and provenance are checked before
activation. Updating an artifact is an operator-visible authority change and restarts affected
leases or channels safely.

Executable artifacts are built and distributed by the connector/Exchange pipeline. They are never a
Flux release artifact, helper executable, installed pack or fallback. During migration an artifact
may still speak the framed stdio protocol behind Exchange; that implementation detail does not move
execution into Flux.

## Migration proof

X-120 runs the accumulated flux-connectors C-505 migration corpus through local single-tenant
Exchange. HTTP, plugin/process, socket and container fixtures retain their declared operation and
event contracts while proving Service Account authentication, grants, tenant-derived connection
settings, lifecycle behavior and credential non-disclosure. Each migrated adapter adds evidence to
the corpus before its Flux artifact is removed in the same release train.

Hosted multi-tenant isolation remains X-116. X-120 neither waits for that proof nor substitutes a
local Flux execution path; an unsupported topology is a named refusal.

## Story map

X-111 tracks the program. X-112 recorded the earlier rich-runtime alignment and X-124 corrected its
execution placement. X-113 publishes the effective catalogue and one-shot HTTP contract. X-114
consumes connector runtime plans; X-115 binds all runtimes in local single-tenant Exchange; X-116
supplies hosted multi-tenant isolation; X-117 streams, cancels and records terminal outcomes; X-118
makes leases own resources; X-119 installs attested artifacts; and X-120 proves the accumulated
migration corpus through local single-tenant Exchange. Flux C-500 and flux-connectors C-495 are the
sibling epics.
