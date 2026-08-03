# Design: rich connector runtimes through Exchange

**Status:** accepted direction, owner-confirmed 2026-08-03 · **Epic:** X-111

Exchange is the hosted placement for the same connector bundle Flux can execute locally. HTTP is the
first delivered runtime, not the boundary of the product. Docker, Kubernetes, SQL, observability,
secret stores, telephony and future protocol-rich integrations remain connectors; Exchange hosts
their declared socket/process/container/plugin runtime inside the required tenant isolation boundary.

## Existing foundation

The host already reads the closed runtime vocabulary and refuses local execution in a multi-tenant
process. Ordinary `invoke` is grant- and tenant-gated. X-101…X-105 persist and supervise generated
connector WebSocket channels and expose authenticated, bounded `/api/subscribe`. X-98 runs stored
Flux programs. X-107 delivers canonical Service Account lifecycle and bearer authentication at those
same boundaries; X-121 owns removal of the bounded legacy spelling.

Those are real prerequisites, but operation dispatch is still HTTP-shaped: `Invoker::invoke`
resolves a catalogue operation through `connector_pack`, and the server supplies
`flux_web::HttpRequestTool`. The other runtime variants currently gate placement; they do not execute
rich outbound operations.

## One dispatch seam

`connector-pack` must project the connector's compiled declaration into a zero-IO runtime plan. The
reusable host admits runtime and grant, derives tenant-bound configuration/credential ports and hands
that plan to a runtime registry. The server composition supplies implementations; neither host crate
nor server constructs a vendor request, command or handshake independently.

The runtime registry is closed over `http`, `socket`, `process`, `container`, `plugin` and `remote`.
A newly added upstream runtime fails compilation until Exchange decides its placement and grant
semantics. Caller input contains operation arguments only—not runtime, artifact, endpoint authority,
tenant or credential reference.

## Placement

- **Single tenant:** may execute every runtime through Flux's guarded substrate. This is local `--dev`
  and a valid one-team production topology.
- **Multi tenant:** the control-plane process executes only shareable HTTP or delegates a `remote`
  plan to an operator-selected per-tenant worker. A socket/process/container/plugin plan is refused
  before credential access unless that isolated placement exists.
- **Worker trust:** an isolated worker is inside Exchange's credential boundary for that tenant. It
  may receive an operation-bound secret in memory when the protocol requires it, but never persists,
  logs or returns it. The external caller still receives authority, never the credential.

The worker protocol reports what it executed; the Exchange run record must distinguish locally
observed from worker-reported evidence.

## Long-lived operations

One authenticated connector WebSocket carries:

1. inbound declared events (`subscribe`, already delivered for generated socket channels),
2. streamed operation output such as logs, process stdout or a socket read loop,
3. cancellation and terminal status,
4. lease acquire/renew/release and disconnect cleanup.

Frames are request-correlated, bounded and grant-scoped. A reconnect never implies replay unless a
connector declares a cursor/replay contract. Refused, unreachable, interrupted and expired remain
distinct outcomes.

## Runtime artifacts

Exchange installs only connector runtime artifacts described and attested by flux-connectors. The
operator chooses which bundles and versions are installed; a caller cannot name a path, image or
tag. Digests, platform/runtime compatibility and provenance are checked before activation. Updating
an artifact is an operator-visible authority change and restarts affected leases/channels safely.

## Worktree audit

The 2026-08-03 audit used `git worktree list --porcelain`, `git status --short --branch` in every
linked worktree and branch diffs against each repository's current main line. Relevant in-flight work
is reused:

- Flux `generated-connector-websocket-channels` carries the generic channel/runtime program that
  Exchange X-101…X-105 already consumed.
- Flux `impl/C-453` implements remote approval, a prerequisite for a hosted Flux runtime rather than
  a second approval story here.
- flux-connectors `impl/C-494` is actively implementing instance-aware host ports for Exchange X-14.
- this repository's delivered X-107 Service Account authentication supplies the non-human caller;
  X-121 removes only the compatibility spelling.

The remaining linked worktrees are release/maintenance, clean branches already represented on the
boards, or unrelated subsystem work; none supplies a rich-operation dispatcher, isolated connector
worker, streamed invoke or lease implementation.

## Story map

X-111 tracks the program. X-112 aligns the charter and roadmap. X-113 fixes the remote wire contract;
X-114 consumes connector runtime plans; X-115 binds all runtimes single-tenant; X-116 supplies
multi-tenant isolation; X-117 streams and cancels; X-118 makes leases own resources; X-119 installs
attested artifacts; X-120 proves local/hosted conformance. Flux C-500 and flux-connectors C-495 are
the sibling epics.
