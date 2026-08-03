# Design: the flux-exchange flow editor

## Product shape

One workflow draft edits one bare Flux `flow`. The console offers a top-down graph, an optional local
freeform layout and exact source mode. The graphical subset is call/bind, condition, bounded loops,
parallel and return; any other valid Flux stays byte-preserved and source-only. Drafts never execute.
Publishing freezes source, graph-node map, derived metadata and referenced-operation fingerprints;
runs address that immutable version.

The interaction borrows the useful parts of babelforce's map view: insert controls between nodes,
search-and-jump, neighbor focus, direct node actions and tree/freeform modes. The runtime remains
Flux, not a canvas interpreter.

## Delivery order

The compiler/projection contract lands and publishes in Flux first. Connector-pack then publishes on
that same Flux engine line. Only then may this repository move both pin sets together and implement
storage, routes, execution and console. The exchange deliberately carries no local copy of Flux's
projection logic and no sibling `path`/`git` override: either would make the shipped service depend
on code outside its reviewed dependency graph.

That sequence first completed on Flux 0.52: L-126 shipped in the engine line and flux-connectors
0.16.0 published the compatible pack. Exchange now moves forward atomically on connector-pack 0.17
and Flux 0.54, retaining the registry-only dependency graph; no sibling checkout is part of the
shipped build.

## Catalogue and authority

The palette contains the compiled connector catalogue grouped by connector/service and the upstream
`cognition` group only. A test requires every included builtin to be low-risk, idempotent and free of
effects/host IO.

A publication projects as virtual connector `workflow.<id>` with operation
`workflow.<id>.run`. Invocation requires a grant for that virtual connector. Every nested connector
call independently repeats the real connector's runtime and tenant-grant gates before credential
resolution. Pure cognition calls carry no credential. All calls execute through Flux's dispatcher;
the exchange constructs no request of its own.

## Storage and API

`WorkflowStore` owns tenant-scoped drafts and immutable versions; `WorkflowRunStore` owns status and
redacted structural events. Writes use revision preconditions. Definitions use atomic owner-only
files and runs use SQLite under one configured workflow directory.

The authenticated surface is `/api/workflows` (collection, draft, validation, versions, publish and
runs), `/api/workflow-runs/{id}` (read/cancel), and `/api/workflows/editor-catalog`. Users author and
inspect; any resolved principal may invoke a published operation through the existing invocation
route once grants admit it. Tenant is always read from the resolved principal.

## Console

`Workflows` and `Activity` are operator surfaces. The canvas uses Vue Flow for pan/zoom/selection and
a deterministic local tree layout. Graph projections and source edits go to the server for lowering
and validation; TypeScript never implements Flux semantics. Published runs are polled, cancellable
and painted onto nodes by the upstream node map. Freeform dragging changes local arrangement only;
persisted execution order remains the Flux source, which the console states rather than implying a
Switching drafts while title, source or graph edits are unsaved requires an explicit discard;
saved/modified state and publication prerequisites stay visible, and malformed node or run
parameter objects are reported inline at the field that needs correction.

## Authoring and publication sequence

1. Resolve the authenticated principal and derive the tenant; no workflow route accepts a tenant
   field.
2. Load the tenant's draft with its revision and the latest upstream editor-schema version.
3. In source mode, project source server-side into graph nodes, source-only regions and diagnostics.
   In graph mode, lower the submitted upstream graph server-side and parse the resulting Flux again.
4. Return validation diagnostics and the reconciled node map without writing when either direction
   fails.
5. Save a valid draft only when the caller's revision matches; otherwise return a conflict carrying
   the current revision.
6. On publish, resolve every referenced operation from the current catalogue and freeze its contract
   fingerprint beside canonical source, graph/node map and derived effects.
7. Write a new immutable version atomically, then update the workflow's published-version pointer.
   Existing versions and runs never move.

## Run execution sequence

1. The existing invocation route resolves `workflow.<id>.run` under the authenticated tenant and
   checks the grant for that virtual workflow operation.
2. Load the selected immutable version. A run never reads the mutable draft.
3. Re-resolve every frozen operation fingerprint. Refuse before credential access when a connector
   contract changed since publication.
4. Create the run record, cancellation token and value-free upstream trace observer, then mark the
   immutable version as the run's target.
5. Execute the published Flux source in the ordinary Flux engine. The canvas and stored graph never
   interpret control flow.
6. For each connector call, resolve the connector under the same principal, repeat its tenant grant
   and runtime checks, and only then resolve credentials. The workflow entry grant does not replace
   this nested grant.
7. Pure cognition calls still traverse Flux's dispatcher but resolve no connector credential and
   perform no host IO.
8. Append entered, branch-selected, succeeded and failed node events using the published node map
   and occurrence counters. Never persist arguments, return values or credentials in these events.
9. A cancellation request trips the run token; the engine completes its normal cancellation path and
   the store records the terminal state rather than inventing a second execution path.
10. Redact the ordinary result through the existing response boundary, atomically finalize the run,
    and let Workflows/Activity poll the same durable record shown by the API.
