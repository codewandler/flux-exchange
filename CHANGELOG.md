# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Hosted and owner-native local management now share one non-resetting decision deadline**
  (X-135). Admission owns the 300-second pre-decision budget, durable decision starts one fixed
  30-second receipt-bearing roll-forward budget, and cancellation can abort only before that
  boundary. Hosted WebSocket, Unix socket and Windows named-pipe transports reserve their mandatory
  close or EOF inside a separate bounded terminal operation, including backpressure and replay.

- **Verified local Exchange release machinery now covers the complete pre-production contract**
  (X-126, implementation slice). CI builds the exact five native targets, runs compatibility and
  named supervisor/ABI process evidence on each applicable runner, packages bounded deterministic
  archives, signs canonical trust/channel/manifest metadata through delegated roles, and verifies
  the immutable public bytes before advancing the stable head. The provider fixture set executes
  161 explicit conformance cases and binds nine native cases to exact tests; immutable version,
  trust-version and channel-generation evidence is append-only. Production publication remains
  intentionally blocked on reviewed external trust/signing inputs, Decision 0007's X-134 schema
  revalidation, and the first authorized public five-target verifier run.

- **Local Exchange state is durable and owner-only on every supported Flux target** (X-127).
  The complete `--dev` composition binds credentials, settings, grants, labelled connections,
  channels, workflows, audit evidence and Service Accounts below one conventional per-user root.
  Unix modes and Windows SID/protected-DACL checks refuse unsafe existing objects without repair;
  the native gate connects, grants and invokes through a labelled connection, restarts the real
  process over the same root, and invokes again with the retained Service Account.

- **A Flux supervisor receives one exact readiness proof from the child it owns** (X-128).
  The machine-only launch ABI uses fixed Unix pipes or an exact two-handle Windows inheritance list,
  binds only an OS-selected `127.0.0.1`/`::1` port, and emits one bounded canonical record containing
  release, executable, protocol and native process-start identity. A strict provider verifier
  refuses malformed or foreign evidence before ownership, while a native liveness thread prevents
  responsive or Tokio-wedged children surviving supervisor death.

- **The four delivered Exchange HTTP v1 identities are bound to production wire types and routes**
  (X-129). Service Account authentication, effective catalogue discovery, raw invocation requests,
  success responses and every reachable bounded refusal now have checked provider fixtures and one
  SHA-256 inventory. Compatibility/readiness use the same typed protocol constants rather than a
  package-version inference.

- **One complete declaration-driven labelled connection plan now serves browser and CLI consumers**
  (X-125). Authenticated humans receive the exact `exchange.connection-plan.v1` contract for name,
  credentials, settings, choices and aliases; composite writes report ordered complete, incomplete,
  refused or partial outcomes, and the console consumes the same committed adversarial fixture.
  Connector 0.19 also activates GitLab's typed custom HTTPS origin through value-free revisioned
  proposal, operator inspection, approval, replacement and revocation. Persisted authority is
  revalidated through the real connector pack, direct setting writes cannot bypass the lifecycle,
  and long-lived channel replacement waits for old projections to terminate.

- **Connection setting reads publish catalogue-declared closed choices** (X-80). A client can build
  Intercom's region picker from one successful `GET` instead of provoking a refused write; fields
  without a closed set omit `choices`, and stored tenant values remain unreadable. The same response
  contract covers sole and labelled connection instances.

- **Tenant-installed Flux Apps now supervise Managed Agents through frozen authority** (X-108).
  Immutable curated App Packages carry exact Program bytes, integrity and provenance without
  tenant values. Atomic installation resolves labelled Connections, metadata-selected operations,
  Datasources and Model Profiles into one reviewed revision; widening upgrades require a new
  fingerprint. Chat and declared Event Types enter a durable inbox before Flux execution, opaque
  runtime tokens can spend only frozen operations through the existing Invoker, unsafe retry is
  marked indeterminate, and Sessions/Runs/value-free Activity are projected from per-tenant/App
  Flux event logs. The console installs and drives the Slack-bot-style template end to end.

- **Service Accounts can now discover their exact remote connector surface** (X-113).
  Authenticated `GET /api/catalogue/effective` intersects the invoker's credential and non-secret
  settings ports with the resolved principal's tenant grants, returns only usable
  operation/connection bindings, and carries a stable content generation for Flux to refresh
  between turns. The existing one-shot
  invoke path remains the execution contract; disconnected connections now have their own bounded
  refusal instead of collapsing into a missing-credential projection failure.

### Fixed

- **Verified vendor helpers now stay inside one absolute setup/result envelope** (X-136). Unix and
  Windows revalidate the complete value-free v2 plan before mutation, preserve old-head replay for
  the server, and share one five-second setup cap plus one non-resetting 335-second result cap across
  private input, terminal framing and capability closure. Exact Linux and native Windows process
  evidence pins the fixed-descriptor/handle-list ABI; MinGW remains compile-only evidence.

### Changed

- **Exchange runtime and binary releases now have one exact Linux product boundary** (X-137).
  Release selection, packaging, download policy, workflows and interim native fixtures close over
  `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`, each as a deterministic `tar.zst`
  containing `flux-exchange`. The server refuses non-Linux targets during its build script, and its
  production owner endpoint, helper capabilities and supervision use only the Linux account,
  `SO_PEERCRED`, fixed-FD, `SCM_RIGHTS` and proc-start contracts. Publication remains fail-closed
  until X-138 recovery evidence and X-139's final native authority are done.

- **The datasource vocabulary now follows cross-repository Decision 0006** (X-130). The concepts
  table's last ambiguous owner cell is resolved: vendor-data Datasource Definitions belong to the
  connector package, Flux keeps the wire vocabulary and the consuming seam, and a tenant Datasource
  is a published connector datasource member bound to a connection label with optional
  entity/filter scoping, frozen at App install. Exchange serves schema/list/get through the
  existing admission gate and owns tenant authorization and connection resolution, never retrieval
  semantics. The released-domain audit's upstream gap now points at chartered connector work, the
  installed-apps design records that `Datasource.kind` becomes a published member reference in
  `oip` form, and X-131–X-133 file the upstream-gated validation, read-seam and
  effective-catalogue work.

- **The rich-runtime program now has one official integration execution placement** (X-124).
  Exchange owns authenticated effective Service Account discovery, invocation, rich runtime
  execution and lifecycle; Flux contributes its guarded substrate and embedded client without a
  local vendor/plugin fallback. Milestone 1 is now the effective catalogue plus existing one-shot
  HTTP invoke, while streams, cancellation and terminal outcomes remain X-117, leases remain X-118,
  and hosted multi-tenant isolation remains X-116. A failing-first repository contract prevents the
  corrected epic, design and child stories from drifting back.

## [0.17.0] - 2026-08-03

### Added

- **Credential acquisition has one fail-closed host seam ahead of its first released declaration**
  (X-75). The reusable host defines acquisition without a transport; the server performs password
  redemption and refresh through the existing HTTP composition, registers supplied secrets before
  fallible work, and atomically stores only access/refresh/expiry records. The deployment posture
  refuses the path before a request unless its declared hazard is opted in. No released connector
  activates this path yet, so X-75 remains open for upstream metadata and live vendor proof.

- **Generated channels now bind to one immutable connection instance** (X-122). Operators choose a
  tenant-local connection label when creating or rebinding a channel; Exchange resolves and stores
  the host-minted UUID. Renaming a connection changes only its management label, deletion refuses
  while a channel still binds the instance, and restored channels select the same credential and
  configuration addresses instead of silently choosing the first account.

- **The public site covers the intended Exchange surface and explains its credential boundary**
  (X-65, X-66). Live and planned capability status remains derived from the agent descriptor, while
  the boundary page now explains principal-derived tenancy, declared execution, grants, runtime
  placement, supplier evidence, and the distinction between Service Accounts and Managed Agents.

### Removed

- **The v0.16 Agent-named Service Account compatibility spellings are gone** (X-121).
  `POST /api/agents`, `FLUX_EXCHANGE_AGENTS`, the serialized `agent` principal kind, and the
  `#/agents` console fragment are no longer accepted. Existing unprefixed bearer tokens continue to
  resolve from the unchanged verifier-keyed store until expiry or revocation.

### Changed

- **The historical Agent-access backlog is reconciled to the canonical Service Account resource**
  (X-35, X-37, X-38). The delivered bearer authentication, listing, revocation, tenant isolation,
  and grant boundary are now recorded as completed work; Agent remains reserved for Flux's model,
  authored loop, and bounded capabilities.

### Operations

- **Production refuses an absent operator policy before it builds an image** (X-123). The workflow
  checks Fly's value-free secret metadata before build and again after rollout, requiring exactly
  one deployed `FLUX_EXCHANGE_OPERATOR_SUBJECTS` entry. Retained evidence records only that the
  policy was deployed, never its digest or the identity-provider subjects it contains.

- **The first protected-main v0.16.1 release and its recovery point are verified** (X-93, X-94).
  The retained 90-day artifact ties one reviewed source commit to the scan-clean static image, SBOM,
  Fly release and live machine verification; the post-release encrypted snapshot remains inside the
  declared 24-hour RPO after the isolated 597-second recovery drill.

## [0.16.2] - 2026-08-03

### Fixed

- **Google production sign-in requests the provider's minimal accepted OIDC scopes** (X-90).
  Live evidence showed Google refusing the bare `openid` request, so authorization now requests
  `openid email`. The email scope is a provider-protocol requirement only: the email claim remains
  unparsed and unused, identity remains keyed by immutable `sub`, and Workspace admission still
  requires exact equality with the signature-verified `hd` claim.

## [0.16.1] - 2026-08-03

Wave #1 closes the first ten security, deployment and self-hosting stories selected from the ready
board: X-58, X-59, X-60, X-68, X-74, X-90, X-91, X-93, X-94 and X-96.

### Fixed

- **Sign-in callback diagnostics now match their documented provider boundary** (X-68). An explicit
  federated-provider refusal remains a credential failure while a development host has no provider
  answer to reject; tests pin that intentional distinction and prove neither path issues a session
  or reflects the provider's error. The anonymous development sign-in page's withholding guard now
  explicitly covers the development identity roster variable.

### Changed

- **Authentication no longer decides either the deployment tenant or operator authority** (X-59,
  X-91). `FLUX_EXCHANGE_TENANT` selects a provider-independent single tenant and refuses a
  principal from any other tenant without rewriting it. Administrative routes independently
  require an immutable id in `FLUX_EXCHANGE_OPERATOR_SUBJECTS`; an absent or malformed policy
  fails closed, while ordinary members retain session, catalogue and grant-gated invocation.

- **Google Workspace admission is checked from the signed token** (X-90).
  `FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN` is an authorization-request hint and, separately, an exact
  requirement on the verified `hd` claim. Email and email suffixes grant nothing, identity remains
  the immutable `sub`, and the requested scope is now only `openid`.

- **Connection reads carry value-free supplier evidence** (X-60). The latest successful durable
  credential-supply audit event projects its principal and timestamp; missing or aged-out evidence
  says `unknown` without making a held connection unusable or inventing an owner. Instance renames
  no longer masquerade as credential creation.

- **Hazardous credential acquisition is fail-closed before a connector can expose it** (X-74).
  The published host owns a typed `AuthPosture`; the server reads
  `FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS`, rejects unknown values at startup, and otherwise refuses a
  declared shared-resource-owner-secret acquisition unless explicitly opted in.

- **The roadmap now makes Exchange the hosted runtime for every connector kind** (X-111, X-112).
  HTTP invocation and generated socket channels are delivered slices; filed follow-ups cover the
  stable remote protocol, declared runtime-plan dispatch, single-tenant execution, per-tenant
  isolation, streams, leases, attested artifacts, and local/hosted conformance. Vendor-specific
  adapters remain owned by flux-connectors rather than becoming a second Exchange catalogue.

### Added

- **Verifier-backed local users can safely sign in on a reachable self-hosted deployment** (X-58).
  `flux-exchange local-user-secret <user> <tenant>` generates a one-time 256-bit opaque secret and
  verifier-only JSON entry for the owner-only file named by `FLUX_EXCHANGE_LOCAL_USERS`. Wrong and
  unknown credentials are indistinguishable; the same-origin form issues only the ordinary secure
  HttpOnly session cookie, and the console now withholds sign-in when no provider is usable.

- **Traffic admission is fair, observable and still bounded** (X-96). Each resolved
  `(tenant, kind, id)` has its own rolling invocation budget beneath the unchanged process-wide
  rate/concurrency ceilings. Fly Proxy supplies the anonymous occupancy bound; `/metrics` exposes
  fixed-cardinality admission, refusal and active-work series; sustained saturation warns without
  tokens, bodies or identity labels. Forwarding headers select no bucket.

- **Production is an attributable, digest-pinned workflow and persistent state has a tested
  recovery contract** (X-93, X-94). The protected production environment accepts a full commit from
  protected `main`, reruns the gate, builds locked digest-pinned layers, produces an SPDX SBOM,
  scans before push, deploys the immutable image digest and records identifier-safe provenance with
  rollback. Daily snapshot verification enforces encrypted scheduled recovery points, 14-day
  retention and a 24-hour RPO; the runbook defines a timed isolated restore drill and 60-minute RTO.

- **Authority evidence now survives the process in an owner-only SQLite audit journal** (X-95).
  Authentication, authorization, Service Account lifecycle, connection/credential/settings and
  grant changes, and invocation outcomes use a closed JSON vocabulary with request/event ids,
  resolved actor fields and non-secret targets. State-changing actions are recorded before their
  store/runtime is touched and fail closed when evidence cannot be written; a sentinel test proves
  tokens, OIDC material, request bodies and credential/setting values cannot enter any field.

  `FLUX_EXCHANGE_AUDIT` binds the journal; reachable deployments refuse without it. Rows have a
  30-day minimum retention and bounded local `audit-query` queries by event, actor or target.
  Authentication floods, repeated per-actor authorization failures, and credential/grant changes
  append identifier-and-count-only alerts and emit warning notifications. The Fly composition puts
  the journal on its encrypted volume; the runbook names read and early-deletion powers.

- **A tenant can hold several labelled connections to one connector** (X-14). Exchange persists a
  tenant-scoped label-to-host-minted-UUID overlay while deriving connection existence from scoped
  credential addresses. Label-scoped management includes settings and atomic rotation; invocation
  selects with `?connection=<label>` while preserving the operation's raw JSON body, and refuses an
  omitted selector when several connections exist.

  Creating the second connection migrates the first and writes the second in one checked
  `SecretBatch`; deleting one of two returns the survivor to the legacy address atomically. Stores
  without proven inventory and batch support retain sole legacy connections and refuse plural
  management rather than falling back to point writes. `FLUX_EXCHANGE_CONNECTIONS` names the
  durable non-secret label registry. Label-scoped mutations retain label-specific, value-free audit
  targets. Generated channels remain sole-connection-only until X-122 gives their durable records a
  rename-safe instance binding.

- **Service Accounts are now a complete non-human identity resource** (X-107). Signed-in humans can
  create, list and revoke them at `/api/service-accounts`; creation returns a new `fxsa_…` token
  once, the durable store retains only its verifier, and bearer presentation resolves the canonical
  `service_account` principal in its original tenant until expiry or revocation. Authentication
  grants no authority by itself: operation and inbound-channel grants remain metadata selectors,
  and Service Accounts cannot manage credentials, connections, settings, grants or successors.

  Descriptor vocabulary version 2 publishes canonical creation and bearer authentication as live.
  The console uses `#/service-accounts`, replaces the retired fragment, and explains the difference
  between a Service Account and a hosted Flux Agent.

### Deprecated

- `POST /api/agents` and `FLUX_EXCHANGE_AGENTS` are accepted for v0.16 compatibility and are removed
  in v0.17 (X-121). Alias responses carry deprecation, successor and removal headers; two environment
  spellings that name different paths refuse startup. Existing unprefixed tokens continue to resolve
  from the unchanged verifier-keyed file format.

## [0.15.0] - 2026-08-03

### Added

- **Generated connector WebSocket channels are a live, fail-closed Exchange surface** (X-101–X-105).
  Operators persist tenant-derived channel declarations through `/api/channels`; the supervisor
  restores and reconnects vendor sockets independently of subscribers, and connection or credential
  rotation restarts affected channels. The reusable host plans through `connector-pack` without
  constructing a request or opening a transport; the server binds Flux's guarded socket runtime.

  Inbound grants name an explicit connector, binding and closed declared event set. Authenticated
  `/api/subscribe` WebSockets multiplex opaque channel ids, request-correlated acknowledgements and
  live typed events through bounded queues; a slow subscriber is isolated, and no replay or cursor
  is implied. The console now manages Channels and publishes only declaration metadata anonymously,
  never endpoints, auth headers, credentials or retained private payloads. Its Grants screen derives
  inbound binding/event controls from those declarations, previews their consequences and preserves
  held inbound authority when declarations cannot be read (X-110).

### Changed

- **Flux Exchange advances connector-pack and the complete connector set to 0.17 with Flux 0.54 as
  one dependency graph** (X-101). The engine-line seam, manifest and lockfile checks prove one Flux
  runtime line; channel plans cross the same zero-I/O pack boundary as ordinary invocation.

- **The flow editor protects work in progress and explains its state** (X-109). Switching drafts
  with unsaved title, source or graph edits requires explicit discard; saved/modified state and
  publication prerequisites stay visible; malformed node/run parameter objects report inline; and
  an empty palette search is distinct from an unavailable catalogue.

- **The family vocabulary now follows the released domain contracts** (X-106). Flux Programs,
  Journeys, Apps, Managed Agents, Channels and Events are separated from Exchange-owned tenant
  installations, connections, grants, service accounts and delivery policy, with live capability
  claims kept distinct from the target architecture.

### Fixed

- **The documented development command is now an end-to-end release contract** (X-100). CI and the
  tag-triggered publication gate start `cargo run --locked -- --dev` on an ephemeral loopback port,
  verify its one-click browser form, exchange the implied `user:${USER}@dev` identity for an
  HttpOnly cookie and resolve the authenticated session. Explicit-roster instructions also escape
  their `<handle>` placeholder, so browsers no longer display an empty bearer value.

- **Every prose claim about the current Exchange version is checked against the manifest** (X-81).
  The contributor guide, README, roadmap, generated board and published host-crate front page move
  with the release version or fail both ordinary CI and the tag gate. Historical changelog headings
  and milestone citations remain outside that deliberately exact scan.

## [0.14.3] - 2026-08-03

### Fixed

- **The MSRV repair no longer retains a vulnerable PDF parser** (X-99). Exchange now consumes Flux
  0.52.3, whose default web feature treats PDFs as opaque instead of linking the affected parser;
  safe PDF extraction remains an explicit upstream feature. This keeps `cargo run -- --dev` on the
  promised Rust 1.88 line while restoring a green dependency audit.

## [0.14.2] - 2026-08-03

### Fixed

- **`cargo run -- --dev` builds on the declared Rust 1.88 MSRV again** (X-99). Exchange and the
  published Flux 0.52.2 family now resolve one registry-only bundled-SQLite line whose build script
  supports the promised compiler. The browser sign-in action from v0.14.1 is unchanged; this patch
  repairs the dependency regression that kept its ordinary CI job red.

## [0.14.1] - 2026-08-02

### Fixed

- **`--dev` sign-in is now a browser action instead of an instruction page** (X-59). Because the
  shorthand fixes exactly one startup-derived principal, `/api/signin` offers a POST button that
  opens that local session, returns to the console and exposes the token only as an HttpOnly cookie.
  Explicit development rosters keep the bearer-handle exchange, including one-entry rosters; only
  the single-tenant shorthand enables automatic local sign-in.

## [0.14.0] - 2026-08-02

### Added

- **Tenants can author, publish and run versioned Flux workflows** (X-98). Exact Flux source is
  projected into the upstream versioned graph when representable and remains byte-preserved in
  source-only mode otherwise. Drafts use optimistic revisions; publication revalidates against the
  executable catalogue and freezes source, node map, input schema and operation contracts.

  Published workflows enter through `workflow.<id>.run`: the tenant must grant that virtual
  operation, and every nested connector call repeats its own runtime and tenant-grant checks before
  credential resolution. Contract drift refuses before a credential port is constructed. Runs
  target immutable versions, persist in SQLite, can be cancelled, and record only upstream's
  value-free node lifecycle events; a trace write failure fails the run instead of silently losing
  its explanation.

  The console adds full-width Workflows and Activity surfaces with tree, freeform and exact-source
  modes, server validation, searchable connector/pure-cognition palette, publication, JSON run
  parameters, a durable timeline and live node overlays. `FLUX_EXCHANGE_WORKFLOWS` names the durable
  definitions-and-runs directory; definitions and activity are created owner-only and widened
  existing paths refuse at startup. Unset remains a fail-closed `503` rather than a memory fallback.

- **Local development has a one-tenant startup shorthand** (X-59, partial). Running
  `flux-exchange --dev` derives `user:${USER}@dev`, keeps the secretless development identity
  loopback-only, and selects `Deployment::SingleTenant` for runtime admission. From a Cargo checkout
  the command is `cargo run -- --dev`; an explicit `FLUX_EXCHANGE_DEV_IDENTITY` roster remains the
  multi-tenant development path and is never overwritten by the shorthand.

  Credential addresses keep the ordinary `tenants/dev/...` layout, byte-identical to the same
  tenant in a multi-tenant composition, so leaving development mode does not strand stored data.
  X-59 remains in progress for the orthogonal form that selects one tenant independently of OIDC or
  a future verified local-user provider.

- **The repository has one labelled security posture and a ranked hardening roadmap** (X-89).
  [`docs/security.md`](docs/security.md) maps protected assets, attackers and trust boundaries to
  controls that are enforced in code, deployment-dependent assumptions and known limitations. It
  covers OIDC/session handling, tenant and grant authorization, credential persistence, guarded
  execution, browser policy, resource bounds, audit evidence and supply-chain provenance, and adds
  an incident checklist plus a short operator gate to the Fly runbook.

  A failing-first repository test keeps the posture linked from both indexes and anchored to the
  authoritative identity, invoke, public-hardening and deployment designs. X-90 through X-97 now
  track signed Google organization verification, explicit operators, private reporting and branch
  protection, reviewed deployment provenance, tested recovery, durable audit evidence, fair traffic
  controls and a managed credential backend. Runtime behaviour and the deployed release are
  unchanged by this documentation-only tranche.

## [0.13.0] - 2026-08-02

### Added

- **A signed-in operator can complete Connect → Grant → Invoke in the console** (X-88). Searchable
  connector pickers show vendor context and connection state; connection cards expose status,
  progressive credential addresses and atomic rotation without ever reading a value back. Grants
  add conservative presets, grouped consequence previews and narrower/unchanged/wider comparisons
  while continuing to send metadata selectors only.

  The catalogue now publishes the exact input schema projected by `connector_pack`, and the Invoke
  screen validates a JSON parameter object, renders success and refusal details including
  `sent`/`retryable`, and reports elapsed time. Failed reads have in-context retry actions, loading
  is stable, mobile navigation groups honest future surfaces, and catalogue search gains `/` focus,
  match highlights, service grouping and result-preserving breadcrumbs.

- **The public process has an operational security boundary** (X-87). OIDC logout now invalidates
  the server-side session as well as clearing the browser cookie. Anonymous authorization starts are
  limited to 30 per rolling minute; operation invocation is limited to 120 attempts per rolling
  minute and 16 concurrent executions, refusing with `429` and `Retry-After` without occupying an
  unbounded queue.

  Successful sign-in/out, agent minting, connection credential/settings changes, grant replacement
  and invocation emit structured audit events carrying the resolved actor and a non-secret target,
  never token or credential material. The outer router supplies a same-origin CSP, HSTS, MIME,
  referrer and permissions policy on every response, with `Cache-Control: no-store` on `/api`.

  CI now audits all three dependency trees. RustSec exceptions are narrow and documented: the RSA
  advisory affects key generation while this service verifies with provider public keys, and the
  transitive `ttf-parser` warning is unmaintained-only with no maintained release available. The
  VitePress tree forces the first patched Vite line and both Node audits report zero vulnerabilities.

### Changed

- **One exchange-owned catalogue finder replaces the copied flux-connectors explorer** (X-86).
  The console now has one search field with Connectors, Services and Operations tabs, relevance
  ordering, shareable route-local state, and connector/service drill-down into operations. It
  renders only facts this host serves; Channels stays absent until the catalogue publishes real
  channel metadata. The fifteen copied components and their documentation-shaped adapter contract
  are removed, so catalogue UI changes no longer have to be synchronized between repositories.

  `GET /api/catalogue/connectors` additively publishes each connector's catalogue-declared `vendor`
  and `description`. Those are anonymous vendor facts from the compiled catalogue, never tenant,
  grant, connection or credential state.

## [0.12.0] - 2026-08-02

### Changed

- **The deployed connector seam moves as one graph** (X-85). The four connector crates move from
  0.10 to the newest published line, 0.13, and the Flux engine crates move with `connector-pack` from
  0.47 to its required 0.49 line. The compile-time seam, manifest pin check and resolved-lock check
  all pass, so the host cannot accidentally carry two identically named `flux_runtime::Tool` traits.

### Added

- **A container, a fly.io configuration and a deployment runbook** (X-84). `Dockerfile` (three stages,
  no toolchain in the runtime layer, the console built in and served from `/srv/console`), `fly.toml`,
  `.dockerignore` and [`docs/deploying.md`](docs/deploying.md). **The first deployment any flux-family
  repository has made**, so it is the precedent the siblings copy — `fly deploy` itself is unrun and
  needs an OIDC provider, which is an account action rather than a code change.

  **Two things were measured in the built image rather than reasoned about, and one changed the
  configuration.** A fresh volume mounts its root `0755`, and the credential store refuses a parent
  wider than `0700` rather than tightening it (X-09) — so the obvious
  `FLUX_EXCHANGE_CREDENTIALS=/data/credentials` **does not start**. Pointed one level deeper the store
  creates its own parent `0700` and its file `0600` and boots with no manual `chmod`. All four store
  paths are nested for that reason, with the quoted refusal beside them in `fly.toml`, because
  flattening them looks like tidying up. The same run found that the *agent* store makes that
  complaint as a warning rather than a refusal — it discloses which agents exist and when their tokens
  expire, and no token — so it would have started and quietly disclosed that.

  The bind rule was confirmed inside the container: a reachable bind with no identity exits and quotes
  its refusal, so a misconfigured machine crash-loops with the reason in its log rather than serving
  anonymously. `ca-certificates` is installed deliberately — without it the OIDC token exchange fails
  on the certificate chain and reads as *the provider refused us*, the confusion X-17 exists to split
  apart. The entrypoint is exec form so the binary is pid 1 and `with_graceful_shutdown` sees fly's
  `SIGTERM`, on a store that rewrites its whole file at once.

  **One machine, as a correctness bound rather than a cost decision**, with the reason in the file: the
  store fsyncs the whole file under one mutex (X-22), X-25's allowance race closes only in-process, and
  a fly volume attaches to one machine — two machines is two credential stores diverging silently.

- **The console is served by the host it talks to** (X-83). `crates/exchange-server` served no static
  files, and the console reached the API only through the Vite dev-server proxy — which
  `npm run build` does not emit. So the console was reachable exactly where a developer was running
  two processes, and a remote deployment had nowhere to put it.

  **Hosting it on another origin cannot work, and it is worth knowing why before anybody tries.**
  `console/src/service.mts` addresses every endpoint as a same-origin relative path, and the session
  cookie is `SameSite=Strict` — so a browser never attaches it to a request originating from another
  origin. Not blocked by a missing CORS header: not *sent*. Publishing the console beside the docs site
  would look like it nearly worked. `Strict` was chosen by X-15 and X-40 and is untouched here.

  `FLUX_EXCHANGE_CONSOLE` names a built console directory and `ServeDir` serves it at `/`, with a
  fallback to `index.html` so a deep link survives a refresh. Unset means no static route, which is
  what a checkout already did.

  **The failing-first test earned its keep immediately, and after the fix was already written.** An
  SPA fallback claims every unmatched path, so an unknown `/api/...` would answer `200` with a page of
  HTML — which every client reads as success. A wildcard catch-all handles that, and the test still
  went red: `/api/{*unmatched}` matches one segment *or more*, leaving `/api/` — trailing slash,
  nothing after — falling through to the console. Three routes refuse now.

  The surface's own guards are unaffected because `app()` is now `app_with_console(state, None)` and
  `#[cfg(test)]`: the enumeration walks exactly the router a checkout serves, and a second test proves
  every declared route answers the same status with a console bound as without one.

- **Every capability page's status is derived from the route table, not written by an author** (X-64).
  This repository corrected **five renderings of one false claim** in a single week — that `invoke` was
  not built — each written honestly, each stale within a release, each caught by a review rather than a
  mechanism. A documentation site is a factory for that failure, and X-65 is about to add a page per
  capability. Status badges now read the same descriptor artifact whose `live` flags X-42 and X-52 hold
  to the route table, the build **fails** for a page naming a capability the descriptor does not know,
  and the site build re-derives the artifact rather than assuming the console suite checked it.

  ⚠ **Two review rounds found the same defect in different clothes, and both were invisible to a green
  gate.** The capability pages were the first this site ever published below `dist/` root, and the
  content guards enumerated non-recursively — an IP address and a bearer-token-shaped string reached a
  published page with the suite reporting 23/23. The fix introduced a coverage check that *inherited
  the blind spot of the thing it checked*: one `walk()` closed over a skip set, so the predicted set and
  the scanned set agreed **by both omitting the page**, and `web/test/` and `web/scripts/` — which
  VitePress publishes — leaked the same payload at 25/25 green.

  The rule that came out of it is written in three places: **excluding on the way in is a content
  decision; excluding on the way out is a blind spot.** The walk that reads the built site now excludes
  nothing at all, and machinery directories no longer publish, from one `content.mts` that `srcExclude`
  and the suite both read. The two defences are independent by construction — an independent review
  planted `dist/test/leak.html` directly, simulating `srcExclude` failing entirely, and five guards
  fired.

- **A weakness in how a credential is obtained is a declared kind** (X-73). `AuthHazard`, whose first
  and only value is `ResourceOwnerSecretShared` — the resource owner's own password presented to this
  host rather than to the authorization server. The doc comment carries what makes the name checkable
  rather than a coinage: **RFC 9700 §2.4**, which says the resource owner password credentials grant
  MUST NOT be used and gives the three reasons this vocabulary exists to record (the credentials reach
  the client, they can then leak in more places than the authorization server, and the grant cannot
  carry two-factor); **RFC 6749 §4.3**, which requires the client discard them once a token is
  obtained; and **CWE-522**. A test pins the citations so a tidy-up cannot quietly drop one.

  **It is deliberately not a fifth `Risk`.** `Risk` is an *ordered* ladder that `Selector::at_most`
  compares against, and a password grant that buys a read-only token is `Risk::Low` **and** hazardous —
  so a fifth rung would have silently admitted it to every grant already written. It is not on
  `OperationFacts` either: a hazard is a property of an acquisition, which happens once per connection,
  not of an operation, which happens per call.

  The vocabulary is **closed**, and that is the whole point — a near-miss spelling refuses at
  deserialization rather than reading as *no hazard declared*. An independent review drove 15 inputs
  through four positions (bare, `Option`, `BTreeSet`, and a struct with `#[serde(default)]`); only the
  exact spelling deserializes. It also proved the test discriminates rather than merely requiring the
  type to exist: adding `#[serde(other)]` to the enum keeps everything compiling and turns the test red.

  **Marked `#[non_exhaustive]` from the start**, unlike `HostPinning` and `SettingsRefusal`. That binds
  consumers only — matches inside this crate stay exhaustive with no wildcard arm, per
  `OperationFacts::of`'s rule, because a catch-all answers a value it never heard of with a plausible
  wrong one.

  Nothing consumes it yet; X-74 is the filter. Two limits worth knowing before it is written: the
  citation guard pins `2.4` and `4.3` as bare substrings rather than adjacent to their RFC numbers —
  tight today only because each occurs once — and the derived `Ord` (present because an allow-list
  wants a `BTreeSet`, as `Effect` does) carries *declaration order and no severity*, a claim held by
  prose rather than by a test.

- **A tenant on intercom's or newrelic's non-US region can configure their connection** (X-70).
  Upstream C-225 changed both connectors' `base_url` to a bare `{host}` placeholder, and X-47's rule
  reads the template rather than the value — a bare placeholder **is** the whole destination
  authority, so both were refused. But the same upstream change shipped `config_choices`, and that
  `{host}` is a closed set of hostnames **the vendor published**. Choosing among three regions the
  catalogue declares is not a caller naming a destination.

  **The rule still reads declared data, never the value's shape.** `HostPinning` gains a fourth
  answer, `ChosenFrom`, and admission is **byte equality** against the published strings — nothing
  trimmed, folded, prefixed or parsed. That is deliberate and it is the whole safety argument: there
  is no second parser to disagree with the first, which is [[X-19]]'s defect class. Asked at **both**
  enforcement points, so a value planted directly in the store is refused on the way out as well as
  on the way in. An independent review drove 21 hostile spellings — trailing dot, `:443`,
  `@evil.example`, a Cyrillic homoglyph, a zero-width space, `xn--`, percent-encoded dots — through
  all three paths; all 21 refused.

  **`newrelic` moved with `intercom`, and that was not optional.** A rule derived from the catalogue
  admits it for exactly the reason it admits intercom; refusing it would have meant writing the word
  `"intercom"` into this repository, which is the enumerated list the story exists to avoid. The
  configurable surface is **51 of 54**, up from 49; `docusign`, `freshdesk` and `okta` remain refused.

  ⚠ **This is a breaking change to the published crate.** `HostPinning` and `SettingsRefusal` are
  public re-exports and neither is `#[non_exhaustive]`, so a downstream matching either exhaustively
  will not compile. Deliberate rather than overlooked — marking them `#[non_exhaustive]` now would be
  a second breaking change on top of this one, and the variant is the point.

- **The site shows how to run this and sign in** (X-69). A visitor could read what this service
  refuses to do and could not learn how to start it. Now there is a page, on the nav of every page and
  in the landing hero, and **it was verified by following it** — a clean clone, `cargo run`, the
  roster, the session cookie, and the console leg driven through headless Chrome rather than
  simulated. Doing that changed the page twice, which is the argument for doing it.

  The loopback constraint is **inside the block a reader would copy**, asserted by a test over every
  block containing `cargo run` — a roster handle is a credential with no secret in it, and a page that
  mentions loopback three screens below the command is a page that puts a secret-free roster on a
  public address. It also carries the invoke prerequisite in order (`503` → `403 not_granted` →
  the credential refusal), so nobody follows it to "you are signed in" and then falls off a cliff.

- **A family link lands on the family's documentation, not in a source tree** (X-77). The site called
  itself the platform layer of the flux family and then sent the reader to github.com for both
  siblings. Each publishes a site; neither was linked. `index.md` now points at them and a nav
  dropdown puts both on **every** page — VitePress server-renders a flyout's items, so they are in the
  static HTML rather than only in the client bundle.

  **The rule is about the link's subject, not its hostname**, which is what lets it coexist with the
  github.com links that are correct: a URL addressing anything inside the repository — a path, a
  fragment, the releases page — is a repository link, and a bare repository URL is judged by the
  anchor's own words. The clone command, `surface.md`'s inventory pointer and `index.md`'s
  `#what-exists-today` all stay. It ships with a self-test pinning the discriminator in both
  directions, because a scanner without one is the pattern `console/test/components.test.mjs` already
  rejects.

  `ignoreDeadLinks: false` could never have caught this — it checks internal links, and a link to the
  wrong host is not dead. The guard was seen firing twice: at the merge base against the real links,
  and again with the nav entry pointed back at github.com, where it named a page the overview does not
  cover.

### Fixed

- **The invoke design says what lock 2 now checks, and what it cannot** (X-56). §2 described the locks
  as X-12 shipped them — three rules — while `mod rules` had grown to nine. Two independent reviews
  spent effort rediscovering that **lock 2 checks names, not values**, because that sentence lived only
  in a test's module doc. That cost was being paid per review.

  The design now carries a row per rule with what each catches and what it cannot, and the check that
  holds it there builds its list **out of `mod rules`** rather than restating it — so a tenth rule
  fails the test until the design describes it. The mechanism argument now appears once, in the design,
  with 62 lines of restatement deleted from the test's module doc, which is the two-copies shape that
  caused the drift.

  **The section numbers were wrong in eight places.** The locks are §2; §3 is credential resolution and
  redaction. `lib.rs`, `tests/invoke.rs`, four sites in `no_second_request_path.rs`, X-48's story and
  X-56's own text all said §3. Corrected. `execution.rs` cited the section by *name* and was already
  right, which is an argument for citing by name.

  Note the design says **three** mechanisms, not four: X-55 struck the composition's sandbox posture
  from the count, and this story did not restore it.

  The check is a **presence** check — it asserts the design names every rule, not that what it says
  about one is true. Its doc comment says so.

- **The anonymous-surface guard probes declarations, not paths** (X-61). X-54 introduced a duplicated
  path with two declarations, and the guard that exists to make widening the anonymous surface a
  deliberate act probed every declaration with a `GET`. Both resolved to the same `GET` — served by
  the `Principal` entry — so setting `Access::Anonymous` on the `POST` entry was **invisible to the
  test whose whole job is to notice it**. Demonstrated at the base: the mutation the story names left
  the old guard printing `ok`.

  Discovery now asks each declaration's own **unguarded** method router which verbs it answers, by
  reading `Allow` off a `405`, then drives those verbs through the **assembled** app — so what is
  measured is what the merged router really hands a caller. It must be the unguarded router, because
  `route_layer` wraps the fallback too and a guarded route answers `401` before the fallback could
  name anything; a mistake there fails loudly rather than enumerating nothing. An independent review
  attempted five distinct widenings — an unmerged path, an `options`-only sibling, three declarations
  at one path, a cross-module declaration, a `head`-only sibling — and the guard caught every one.

  **The whole diff is inside `mod tests`.** `ANONYMOUS` is byte-identical: the routes were right and
  the guard was not.

  Two facts captured on the guard's doc comment rather than fixed, both confirmed by mutation: the
  merged router's `405` fallback takes the **second** declaration's guard, so `PATCH`/`OPTIONS`
  behaviour depends on declaration order and nothing pins that order; and `KIND_GATED`'s tuples are
  byte-identical for two declarations at one path, so a failure says the count is wrong without
  saying which declaration caused it.

- **The locks bound the published crate, and no document may say otherwise** (X-55). Lock 2 scans
  `crates/exchange-host/src` only, so `crates/exchange-server/src/execution.rs` — which holds this
  composition's transport and sandbox posture — was unscanned while documents counted it as a fourth
  mechanism. **Widening the scan was considered and rejected**: `exchange-server` legitimately holds a
  transport, so every lock-2 rule would go red on correct code, and the per-file exception list that
  answers would be extended by whoever is adding the thing it should catch. Lock 1's allow-list works
  because a dependency is a rare deliberate addition; that is not the same instrument.

  So the claim narrows instead — three mechanisms, all inside the crate that ships — and **the
  narrowing is enforced rather than promised**. A decision sentence must appear verbatim in
  `AGENTS.md`, `docs/designs/invoke.md` and the test's own module doc, and no paragraph of those three
  may name a control from outside the boundary alongside the vocabulary of leaning on one. The scan
  root is a single constant walked by the real scan, so a future widening fails a test carrying the
  argument rather than passing as an edit to a path string.

  ⚠ **The residual is stated in all three documents rather than left to be found**: a second request
  path added to `exchange-server` is a review matter, not a caught one. That is acceptable only
  because `exchange-server` is `publish = false`; the design says how to close it if that changes.

  Known limit, disclosed: `no_document_claims_more_than_the_locks_reach` carries one assertion that
  cannot fire — it compares a length against the list it was built from. No coverage is lost, since
  an unreadable document panics at the read, but it is worth knowing before trusting it.

- **The console's dev server follows the bind the service was told to use** (X-71). `vite.config.ts`
  hard-coded `http://127.0.0.1:8080` as the `/api` proxy target, so a reader who moved
  `FLUX_EXCHANGE_BIND` — the first thing anyone does when a port is taken — got a console that
  rendered and reached nothing. X-69 hit exactly that while walking its own page and had to move the
  bind back to finish. The target now resolves from the same setting the service reads.

  **A configured value is used as written.** `0.0.0.0` is not turned into loopback and a malformed
  address is not corrected: repairing one here would dial a service the operator did not ask for, or
  quietly agree with a bind the service itself refused to start on. A blank value reads as unset,
  because `FLUX_EXCHANGE_BIND=` is how a shell clears a variable rather than how it names a host.

  The resolution lives in `console/vite.proxy.mts` so it can be asserted without standing up a dev
  server, and it reads the environment off `globalThis` rather than through `@types/node` — the
  dependency whose cost the old comment cited as the reason not to do this at all.

  ⚠ **The default address is now spelled in two trees** — `DEFAULT_BIND` in `console/vite.proxy.mts`
  and in `crates/exchange-server/src/bind.rs` — and no test ties them. That is deliberate rather than
  overlooked: `127.0.0.1:8080` appears in several unrelated assertions in `bind.rs`, so every
  mechanical check tried would have passed while the two disagreed, and a guard that cannot fail is
  worse than the doc comment that now names the other file.

- ⚠ **The site's credential-shape scanner could not see inside a code block** (X-69). `textOf`
  replaced each tag with a space and the syntax highlighter puts every token in its own element, so
  `export FOO=bar` reached the rule as `export FOO = bar` and the check against a value on the
  right-hand side of an `=` never fired. Demonstrated with a throwaway page before anything was
  written: `export OTHER_PROBE=realvalue` passed the suite.

  It had never been asked, either — **this site carried no fenced code block on any page until now**,
  so a rule about what an example may contain had never met an example. The scanner now reconstructs
  each block verbatim and both scanning tests read prose *and* blocks.

## [0.11.0] - 2026-08-01

### Changed

- **Moved to the flux 0.47 engine line and the 0.10 connector catalogue** (X-67). Both pin sets in one
  commit — raising either alone puts two engine lines in one lock, and `connector_pack::pack` hands
  out `Arc<dyn flux_runtime::Tool>`, so two runtime versions are two unrelated traits. A third test
  now reads `Cargo.lock` itself, because the manifest-reading one cannot see a crate dragged in
  transitively.

  ⚠ **`intercom` is now refused for configuration, and an EU or AU tenant will read that as an
  outage.** Upstream changed its `base_url` to `https://{host}` — a bare placeholder is the whole
  destination authority — so X-47's guard refused it, exactly as designed: *a catalogue bump that
  moves a host template turns a test red rather than quietly dispatching.* A value stored before the
  bump is refused on the way **out** of the store as well.

  The refusal is right under the rule and wrong about intercom, because that `{host}` is a **closed
  set of three vendor hostnames the catalogue publishes**. X-70 is the story that admits a declared
  choice without reopening the door to a free value.

  **Census, measured rather than assumed:** 54 providers (algolia is new), 679 operations, 5
  `WholeAuthority`, 8 `PinnedTo`, 13 `OutsideTheAuthority`, and all 54 still declare HTTP — so X-48's
  runtime gate and X-13's `effects` derivation are unmoved. Four operations left the invocable surface
  (postmark and zoom operations that returned a credential) plus three from babelforce.

  Two claims in this repository turned out to be prose nothing asserted, and both were **already
  wrong** before this change: *"299 operations across 53 connectors"* appeared in two doc comments
  while the real count on the previous catalogue was 681. Corrected, and each now says where the
  number came from and that nothing checks it.

## [0.10.0] - 2026-08-01

### Added

- **You can sign in without an identity provider** (X-57). Everything this service does was reachable
  only by a signed-in principal, and the only thing that made one was an authorization-code flow
  against a configured OIDC provider. That was a lot of setup to look at a page.

  `SignIn::available()` meant *is OIDC configured*, not *can somebody sign in here* — so a deployment
  with the development identity armed, which **can** turn a caller into a principal, reported that it
  could sign nobody in. It now answers the question it is named for, and `GET /api/signin` on such a
  host serves a page explaining the mechanism instead of a refusal.

  ⚠ **Loopback only, and that is structural.** A roster handle is a credential with no secret in it,
  so `admit_bind` refuses every non-loopback address while the development identity is armed — driven
  under review with a roster and a complete OIDC environment set *simultaneously*, refused on
  `0.0.0.0`, `[::]` and a LAN address. A reachable deployment still needs a real provider. Local users
  with an actual verifier are X-58.

  Two things this corrected on the way. The premise that *the console hides its sign-in affordance*
  was **false** — it renders the anchor unconditionally and nothing reads `sign_in_available`, which
  X-43 published for exactly that purpose. And one of X-43's own assertions encoded the same
  conflation a layer up, asserting that an *available* composition answers `/api/signin` with a
  redirect.

- **An operator can see and change what their tenant may run** (X-62). v0.9.0 gated invocation by
  grant and shipped no way to write one, so a deployment ran nothing until somebody hand-wrote a
  file. `GET/PUT /api/grants` and `POST /api/grants/preview` (signed-in humans only), plus a console
  screen.

  **A grant is a selector, never a list of operation ids** — this connector, at most this risk, these
  effects — and an id is refused by two independent mechanisms: a recursive key scan that runs
  *before* serde, and `deny_unknown_fields` on the selector. That is the property the gate was built
  around: X-13 decides from what an operation declares, and a surface writing ids back would undo it.

  **The preview is the point.** The screen will not offer to save until it has been told what the
  draft would admit, and changing a bound asks again — a grant nobody can evaluate before saving is a
  grant somebody sets too wide.

  Two refusals beyond what was asked for, both deliberate: a `PUT` is refused when the tenant's
  existing grant carries id exceptions this surface cannot express, rather than dropping them
  silently; and two grants for one connector are refused rather than resolved by an unstated
  precedence. ⚠ The first blocks exactly today's population — anyone who already hand-wrote a grants
  file with a deny.

- **A public documentation site** (X-63). VitePress in `web/`, published to GitHub Pages by a
  workflow that builds on every pull request and deploys only from `main` — so a broken site cannot
  reach the URL. A dead internal link fails the build.

  Three pages, deliberately. The epic is ordered around the mechanism rather than the volume: this
  repository corrected **five renderings of one false claim** in a single week, and a documentation
  site is a factory for that failure. So these pages claim nothing about what is or is not live —
  they route that question to `GET /api/onboarding`, and per-capability status arrives with X-64,
  derived from the same descriptor whose `live` flags are held to the route table.

  Eight guards run over the **built** site — base-prefix drift, an IP address on a page, a
  token-shaped string, the contributor readme rendering — each verified against a real violation.

  ⚠ One repository setting is still required and no workflow can perform it: **Settings → Pages →
  Source = GitHub Actions.**

## [0.9.0] - 2026-08-01

### Added

- **An operation runs only if a grant admits it** (X-13). v0.8.0 gated invocation by identity alone
  and published that fact anonymously: *any principal this host resolves may run any operation in the
  catalogue against its own tenant's connections.* That sentence is now false, and the descriptor says
  what replaced it.

  **The decision is derived, never listed.** A grant is a selector over what an operation *declares* —
  its risk, its effects, its idempotency — not a set of operation ids. The route's own three
  projections of those facts were deleted in favour of the one the gate uses, with a test pinning the
  **served bytes** against it, so the catalogue cannot describe an operation differently from the
  thing that decides on it.

  **Both gates are a compile error to skip.** `admit_grant` consumes the runtime gate's `Admitted` and
  returns `Granted`; `Granted::resolve` is the only route to the resolver and `Admitted::resolve` is
  gone.

  ⚠ **Fail-closed, and it will look like an outage.** A deployment that upgrades runs nothing until
  `FLUX_EXCHANGE_GRANTS` names a file and grants are written into it, and **no surface writes one
  yet** — expect `503` if no store is bound, `403 not_granted` if one is bound and the tenant holds
  nothing. That is the intended posture; the surface that fixes the ergonomics is X-62.

- ⚠ **Breaking, `codewandler-flux-exchange-host`:** `Invoker::new` takes a sixth argument
  (`Arc<dyn Grants>`) and `Admitted::resolve` is replaced by `Granted::resolve`. An
  `Option<Arc<dyn Grants>>` was rejected deliberately: its only plausible `None` behaviour is *admit
  everything*, which is the exposure this closed.

- **Only a signed-in human may supply or rotate a credential** (X-54). `POST /api/connections/{connector}`
  and `PUT /api/connections/{connector}/credentials/{credential}` are gated by principal *kind*. At
  v0.8.0 an agent could do both — measured, not inferred: an agent `POST` answered `201` and left its
  own value at the tenant's credential address.

  Neither route hands a value out, so the literal reading of *"an agent's token grants access to an
  operation, never to a credential"* is not what they break. What they grant is the **credential
  position** — a caller deciding which vendor account every operation the tenant runs will reach has
  been granted it, value or no value. Two properties settle it: nothing records who supplied a
  credential, so a listing reads identically for a planted value; and revoking the token does not take
  the value back out.

  `DELETE` stays open to every kind, deliberately. It destroys tenant data inside the tenant the
  caller already belongs to, an operator can see and undo it, and no authority survives it. Whether an
  agent should reach a destructive route is the *grant*-shaped question, not the kind-shaped one.

### Fixed

- **The catalogue explorer stops badging operations this service runs as "not live yet"** (X-53).
  The **fifth** rendering of one falsehood — after the onboarding page, the mint screen, the shell
  inventory and the descriptor — this one in `console/README.md` as well as in the explorer itself.

  `works` now means *this service runs this operation*: a build fact, identical for every caller, and
  derived from the same `served` flag the server holds to its route table. The two tenant-specific
  readings were rejected because this explorer is **anonymous** — a badge from either turns a public
  page into a report on somebody's connections. The "could this principal invoke it" reading was
  rejected because `admitted` is three-valued and its `null` is not `false`; folding it into a
  boolean would make a public badge move with who is looking.

- **The agent descriptor's guard checks what its name claims** (X-52). X-42's liveness test compared
  a capability's `live` flag against *does the mapped route exist*, and never against the endpoint the
  capability itself publishes — so republishing `be-minted` at `/api/session` passed 253 tests. And
  `call.method` was pinned by nothing: changing the catalogue's `GET` to `DELETE` left the whole gate
  green. Both are held now, and both were demonstrated by mutation before being fixed.

  ⚠ **The obvious test for the method does not work, which is worth knowing.** Driving each endpoint
  anonymously and asserting the answer is not `405` fails to distinguish anything: on a guarded route
  axum's `route_layer` runs *before* the method router, so an anonymous request answers `401` first.
  That test would have passed for `DELETE /api/agents` — the exact defect. It was caught by the
  test's own control rather than by review.

## [0.8.0] - 2026-08-01

### Changed

- ⚠ **Breaking, `codewandler-flux-exchange-host`:** `admit_runtime` returns
  `Result<Admitted, RuntimeRefusal>` rather than `Result<(), RuntimeRefusal>` (X-48). `?;`,
  `.is_ok()` and `.expect_err()` all still compile; a caller binding the unit — `let () =
  admit_runtime(…)?` — does not. `Deployment::admits` is unchanged.

  **The type is the point.** The deployment gate is an invariant with deliberately no override, and
  it was held by a test that read `Invoker::invoke`'s source for the substring `admit_runtime(`.
  Three mutations defeated it with every test green: a discarded result, an `if false` branch, and a
  **string literal that merely mentioned the gate**. `Admitted` has a private field, no public
  constructor, no `Default` and no `Clone`, and `Admitted::resolve` is the only route from `invoke`
  to the resolver — so all three are now compile errors. It is a method on the witness rather than an
  ignored parameter because an ignored parameter is what the next person deletes as dead weight.

### Fixed

- **The invoke path's safety claims are as strong as its code** (X-48). Four findings from an
  independent review of X-12, all one shape: *the code said something stronger than it did.* The
  sandbox posture is written out field by field (`SandboxMode::Require`) instead of inheriting
  `System::new`'s disabled default — in the same function that already wrote two other settings
  longhand to avoid exactly that. A comment claiming no process could be spawned is replaced by what
  is true.

  **Lock 2 stopped chasing accessor spellings.** A first attempt refused `.system(`; the review then
  demonstrated `ctx.workspace_context().active()` reaching process spawn while naming nothing
  forbidden, and pointed out that the accessor the comment cited does not exist. The rule now bounds
  where the capability *handle* may live: only the two files that may name `Egress` may name
  `ToolContext`. A file that cannot name the handle has nothing to call an accessor on, whatever
  upstream renames next.

  **Lock 1's allow-list was overstated for a reason worth repeating:** it had no self-test, while
  lock 2's rules have had one since X-12. Its parser matched the literal line `[dependencies]`, so a
  `[dependencies.reqwest]` table escaped it entirely. Both directions are now self-tested.

### Added

- **An agent can fetch what this service is instead of reading a page** (X-42). `GET /api/onboarding`
  answers anonymously with what the platform is, the auth scheme, and which capabilities are live —
  the same facts the console's onboarding page renders, from the same source.

  **The first attempt published a falsehood, and how it happened is the useful part.** It told the
  caller the vision calls primary that `invoke` was not built, while
  `POST /api/operations/{operation}/invoke` had been in the published surface since v0.7.0. The
  page-and-descriptor agreement test was green the whole time, because the two renderings agreed
  **with each other** while both were wrong. Deriving from one source protects against drift, not
  against the source being false.

  The cause was one flag answering two questions: `built` means *has this console a screen*, and the
  document asks *does this service do this*. Those had the same answer for every surface until
  `invoke` shipped a route with no screen. They are now two fields, and liveness is held to
  `routes::MODULES` by a test that runs in **both** directions — a capability cannot be published as
  not-live while a route serving it is in the surface, and a route cannot be published without either
  being a capability or carrying a written argument for why not.

  The falsehood was in three renderings. All three are corrected.

  **It publishes one real exposure deliberately**: this build gates invocation by identity alone, so
  any principal it resolves may run any catalogue operation against its own tenant's connections.
  That is a fact about the software rather than a deployment, and publishing the endpoint while
  withholding it would be the dishonest half of a disclosure.

- **The two branches X-46 opened are pinned** (X-49). Publishing declarations changed how a connector
  that declares nothing renders — it used to arrive as `refused` and now arrives as `ready` with an
  empty list — and nothing exercised the branch it took. Both are held by tests now, and each was
  proved by removing the thing it pins.

  The catalogue guard is the one worth reading: `the_existing_catalogue_answers_gained_no_field`
  asserted key sets inside a loop over the catalogue with **no non-vacuity check**, so an empty
  catalogue would have passed it without comparing a single set. It was non-vacuous in fact, which is
  exactly why it could stop being so silently. The proof runs the counterfactual — the same emptied
  walk with the counter removed reports `ok`.

- **A connector with a templated host can be invoked** (X-47). Invoke landed and immediately showed
  that a large minority of connectors could not run at all: their `base_url` is templated on a
  per-connection value and there was nowhere for a tenant to supply it. **Seventeen connectors** —
  the count is derived by rehearsing the shipped catalogue rather than scanning `base_url`, because
  five carry their configuration variables elsewhere in the operation's compiled Flux.

  **Configuration is not a credential and is not stored as one**: its own file, its own port, and
  bounds never summed with the credential allowance. Values are not read back out.

  **Only a signed-in human may supply a setting.** An independent re-review measured a tenant's
  credential on the wire at an origin the *caller* chose — because a suffix pin constrains which
  **vendor** a request reaches, not **whose account** at that vendor, and `*.zendesk.com`,
  `*.atlassian.net`, `*.myshopify.com`, `*.supabase.co` and `*.my.salesforce.com` are all
  self-service registrable. An earlier draft of this entry claimed the four refusals below were the
  security property; they were half of it. The write is now gated by principal *kind*, so an agent's
  token cannot become delivery of its tenant's credential to an origin it named, and the value is
  refused on the way **out** of the store as well as on the way in.

  What is *not* closed is stated in the design rather than left to be found: a human of the tenant
  who did not supply the credential can still read it out this way, because values are write-only
  here. That needs an operator-scoped surface, which does not exist yet.

  **Four connectors are deliberately refused**: for
  `newrelic`, `docusign`, `okta` and `freshdesk` the templated value *is the entire destination
  authority*, so supplying it would have been a way for a caller to name a host — and the tenant's
  credential would have travelled there. The rule is about the **template**, not the value, and it is
  enforced on read as well as on write, so a value that reached the file some other way is still
  refused. The listing says which connectors are unconfigurable and why, rather than letting them
  read as broken.

- **An operator can mint an agent from the console** (X-45), and the token is shown **once**. The
  store keeps a verifier, so this host genuinely cannot show it again — the screen says so, and
  offers no affordance implying otherwise. The token is held in the view's own scope rather than the
  application root, so navigating away is the state ceasing to exist rather than something
  remembering to clear it, and that is asserted through a real component lifecycle.

- **A connector's declared credentials are published** (X-46). `GET
  /api/catalogue/connectors/{id}/credentials` names what a connector requires. Before this, nothing
  published the fact, so the console discovered it by issuing a create it knew would be refused and
  reading the refusal — which coupled it to an error body. The declaration only: names, authority
  and leaf, **never whether anyone holds them**.

## [0.7.0] - 2026-08-01

### Added

- **This host executes an operation** (X-12). A caller names an operation id and **nothing else about
  the request is theirs** — not the host (the URL comes from the operation's own compiled Flux), not
  the credential (the address is derived from the resolved principal's tenant and the connector's
  declared authority), not the tenant. That is the whole confused-deputy answer, and it is what makes
  this an execution platform rather than a credential store with a catalogue.

  **The "this host builds no request of its own" rule is now enforced structurally rather than
  promised** — three locks covering different ground: the manifest's dependency list as an allow-list
  with a reason per entry, a single dispatch seam with no reachable socket (guarded by a scanner that
  self-tests against sources it must reject *and* accept), and a transport counter so a test cannot
  pass by never dispatching.

  A missing credential refuses **by address, never by value**, and is terminal — the request was
  never sent. A runtime this deployment does not admit is refused before the credential store is
  touched.

### Changed

- **`codewandler-flux-exchange-host` now carries the flux engine.** `connector-pack` and
  `flux-runtime` moved from dev-dependencies to dependencies, because the published crate executes
  now. `flux-web` did **not** — it holds the transport, and the crate that dispatches holds none.

### Known

- **Thirteen of fifty-three connectors cannot yet be invoked.** Their `base_url` is templated on a
  per-connection value and there is nowhere to supply it, so they refuse by name. It fails closed and
  says which field is missing. Tracked as X-47.

### Changed

- **The flux engine line is aligned, and `connector-pack` links** (X-11). Upstream published 0.9.0:
  `connector-pack` now requires `flux-runtime ^0.46` where it required `^0.41` against a flux line at
  0.45 — the conflict that made execution impossible from this repository. `connector-spec` (the
  compiler) is gone; its vocabulary now comes from `connector-address` 0.9.

  `connector-pack` is a **dev-dependency**, deliberately: nothing published here executes an
  operation yet, and a normal dependency would put the whole flux engine into every consumer's graph
  to satisfy a proof rather than to run code. The engine line is pinned at `0.46` in one place and a
  test refuses a second value — `flux-runtime` 0.47 exists and taking it would recreate the failure
  this removes.

  **This unblocks `invoke`, grants-gate-invoke, and per-instance connections.** Addresses are
  unchanged: `connector-address` carries an optional instance level and `CredentialRef::new` still
  elides it, asserted rather than assumed.

## [0.6.0] - 2026-08-01

### Added

- **A connector can be connected from the console** (X-44). The console could show what was wired and
  offered no way to wire anything, so an operator read their connections in a browser and created
  them with `curl` — it could do neither of the two jobs the charter gives it.

  The inputs come from **the connector's own declaration**, not a list the console keeps, so a
  connector that gains a credential gains an input with nobody editing the console. No value is ever
  rendered back: after a write the page shows addresses and whether each credential is held, through
  the same renderer the read-only listing uses. An already-connected connector points at **rotation**,
  never at delete.

- **Only a human mints an agent** (X-40). Nothing gated minting by principal kind, so once agent
  tokens authenticate, a leaked one could mint successors — and revoking the first would not kill the
  descendants. Revocation would have stopped being a remedy **invisibly**, because those descendants
  are ordinary agents with no recorded relationship to the token that was revoked.

  `Agent` and `Service` are both refused. `Service` is the interesting one: the property this
  defends holds only if every minter is itself revocable by this host's operator, and a `User` is —
  sign-in is federated — while nothing here mints, verifies, lists or revokes a *service* credential.
  Admitting it would reproduce the same defect one level further out of sight.

- **Whether this deployment can sign anyone in is a field** (X-43). The console linked to
  `/api/signin` unconditionally, so on a host with no identity provider the **Sign in** button led to
  a `503` — the operator learned the platform could not sign them in by being refused. The
  distinction existed only in a human-readable sentence, and a client branching on the wording of a
  refusal breaks when someone improves the wording.

  `GET /api/signin/availability` answers `{"sign_in_available": …}` — one key, anonymously. It is a
  **boolean and not the three internal states**, because a three-valued answer would tell a stranger
  whether this host's OIDC variables are set; the two unavailable compositions answer byte for byte
  identically, status included.

## [0.5.0] - 2026-08-01

### Added

- **An arriving agent is told what this is** (X-41). The charter calls the agent the primary caller,
  and nothing anywhere told one how to reach this service. A public page — no account needed, linked
  from the console's footer — says what the platform is, how to get an identity for an agent, and
  what that identity can and cannot do **today**.

  It is **honest by construction**: what it claims derives from the same surface declaration the
  navigation reads, so it cannot advertise a capability the console marks unbuilt. The rule is
  one-directional by design — the derivation can take a claim *off* the page, never put one *on*.
  Flipping a surface to built turns four tests red, so the wiring is checked rather than trusted.

### Added

- **An agent principal can be minted, and this host keeps only a verifier** (X-36). `docs/vision.md`
  says the primary caller is an agent, not a human — and until now `PrincipalKind::Agent` appeared
  only in its own definition, a loopback development roster, and a comment saying agents carry their
  own tokens. Nothing minted one, so the stated primary caller could authenticate only on loopback,
  in the mode that exists because it must not be exposed.

  `POST /api/agents` mints an agent for the caller's tenant and shows the token **once**. The store
  keeps a digest: a test presents every value in the file — and the whole file — back to the
  resolver, and none of them authenticates. **Reading that store is a roster disclosure; writing it
  is a full authentication bypass**, so a group- or world-writable store is refused at startup while
  a merely readable one warns.

  It **authenticates nothing yet** — binding it to the identity port is a following story, and the
  question of *who may mint* is settled before that lands.

- **A credential can be rotated in place** (X-39). The surface could create, read and delete but not
  *replace*, so rotating a credential — the remedy for a leak — meant `DELETE` then `POST`, with a
  window where the tenant had no connection at all and anything relying on it failed.

  Rotation replaces **one** credential rather than the declared set, and the reason is the north star:
  this host never hands a credential value back out, so a wholesale replace would make a caller
  re-send every value it wanted to *keep* — and an operator rotating one of two credentials has no way
  to obtain the other. It is separated from create by path, method **and** body type, so `POST`'s
  `409` on an existing connection is untouched: an upsert is still the silent overwrite the
  connections story exists to prevent.

  A refused rotation leaves the old value in place, including when it would exceed the tenant's
  allowance.

- **The console presents an execution platform** (X-34). It rendered the connector catalogue and
  nothing else, with no header and no navigation, while the service behind it grew sign-in, expiring
  sessions and a per-tenant connection surface. `docs/vision.md` gives the console two jobs — *wire
  things up* and *see what happened* — and the catalogue is neither, so it has stopped being the
  front door.

  There is now a shell: the service's name, an identity affordance (sign in, or who you are and your
  tenant), and a rail covering **every** surface with its true state. **Connections** is a read-only
  view showing addresses and whether each credential is held — never a value. **Activity**, **Invoke**
  and **Subscribe** are named, struck through and tagged `NOT BUILT`, and a test asserts they have no
  path, no route and no screen — negative-controlled, so each prong is known to fire on its own.

- **CI proves the MSRV the crate promises** (X-33), reading `rust-version` out of `Cargo.toml` rather
  than repeating it. The first real run confirmed the value reaches the toolchain (`1.88`) rather
  than silently defaulting — which would have made the job green while proving nothing.

## [0.4.0] - 2026-08-01

### Fixed

- **A partial delete reports the worst failure, and claims only what it knows** (X-29). The loop kept
  the *first* failure kind, so an unreachable store followed by a denied one answered "retrying may
  work" while a denied address sat in that same response. And `left_behind` told an operator to treat
  addresses as still usable when a connector may legitimately hold a subset of what it declares — so
  some had never held anything. The claim now hedges; the list and the safe instruction are
  unchanged. A partial `DELETE` answers `502` rather than `503` when any address failed in a way
  retrying will not fix.

- **Console tests are found at every depth** (X-32). The test script globbed one directory level, so
  a test in a subfolder never ran and the suite reported green — which became a silently-green
  pipeline once CI started running it.

- **`rust-version` was wrong, and shipped wrong in three releases** (X-30). The manifest declared
  `1.87`. It has never been true: `jsonwebtoken`, `time`, `time-core` and `time-macros` each require
  `1.88.0`, and cargo refuses before compiling anything — so `cargo +1.87 build` has failed since
  X-04 introduced `jsonwebtoken`, on the day 0.1.0 was cut. `v0.1.0`, `v0.2.0` and `v0.3.0` all
  carry the false floor.

  **`rust-version` is now `1.88`.** This is a *correction*, not a raise: no consumer can have been
  building on 1.87, because it never worked. The alternative — pinning `jsonwebtoken` and `time`
  backwards — would downgrade the library doing id-token signature verification in order to preserve
  a number nobody had verified. [X-33](docs/stories/X-33-msrv-job.md) adds the CI job that keeps it
  honest, reading the number from the manifest rather than repeating it.

### Added

- **Every `ExchangeError` is pinned against the refusal it becomes** (X-31). The status mapping was
  guarded variant by variant, but nothing guarded the edge *before* it — a new exchange error folded
  into an existing refusal would have inherited its status and silently undone the operator-vs-caller
  split, without touching the mapping any test was watching.

- **CI checks the action pins and the version pairing** (X-30). Both checkers **self-test before
  they scan**, so one that has stopped catching violations cannot report there are none. The pin
  scanner classifies YAML rather than grepping, because a comment or a `run:` block containing an
  example pin will fool a line-wise grep — and the sibling repository's own error hint is such a
  line.

## [0.3.0] - 2026-08-01

### Added

- **A tenant's allowance holds against its own concurrent creates** (X-25). X-22's occupancy bound
  was read and written under a claim keyed per `(tenant, connector)`, so one tenant's concurrent
  creates to *different* connectors each read an occupancy the others had not written yet. A second
  claim keyed on the tenant closes it. `DELETE` deliberately stays outside that claim — it only frees
  allowance, and the case a delete exists for is revoking a leaked secret, which must not wait.

  A client firing several creates for one tenant in parallel now sees a retryable `409` where it
  previously got a `201` and an allowance that did not hold. Different tenants still do not contend.

### Changed

- **OIDC configuration is read by name, not by position** (X-27). The read pulled values out of a
  vector positionally, and three lists described one set of variables — so adding a variable to one
  and not another silently shifted every value after it. That drift had already shipped once. The
  parallel lists are gone: both are now derived from the read itself, so the same mistake is a
  compile error rather than a host that starts up with a blank client secret. No refusal, order or
  message changed.

- **A sign-in refusal carries its own status** (X-26). The refusal-to-status mapping moved from
  inline in the callback route onto `SignInRefusal`, beside `caller_facing()` — where the argument
  for it already lived. Every status on the wire is unchanged and now pinned variant by variant.

- **CI gates every push and pull request** (X-28). This repository had one workflow and it fired on
  a version tag, so a red `main` was invisible until someone tried to release, and the console had
  never been built by CI at all. `ci.yml` now runs the whole Rust gate and builds and tests the
  console in its own job. The release workflow **keeps** its own inline gate: a tag can be pushed at
  a commit no CI run ever covered, and publishing is the irreversible path.

## [0.2.0] - 2026-08-01

## [0.1.0] - 2026-08-01

### Added

- **One tenant cannot make every other tenant's writes slow** (X-22). Nothing bounded a credential's
  size or how much of the store one tenant could occupy, and the file store rewrites and `fsync`s a
  single file under one mutex on every write — so one tenant's data set the latency of every other
  tenant's writes. That is shared fate between tenants in the service whose central claim is that
  tenants share nothing.

  Two bounds, because they answer different questions. **8 KiB per credential** is about *kind*: a
  credential is a token or a signing secret, and at the largest an RSA-4096 PEM is ~3.2 KiB, so a
  value that does not fit is not a credential that grew. **64 KiB per tenant** is the one that
  protects the neighbours — a per-value bound alone leaves a ceiling that grows every time upstream
  publishes another connector. An oversized value is `413`; an exhausted allowance is `409`, because
  the remedy is to disconnect something rather than to send less.

- **A browser-facing OIDC endpoint is refused in cleartext too** (X-23). X-17's refusal covered only
  the token endpoint and the key set, on the argument that a browser enforces the transport of the
  addresses it navigates. That does not cover the authorization URL carrying `state`, `nonce` and the
  PKCE challenge readable and modifiable in flight, nor an operator who typed `http` and was told
  nothing. All four `FLUX_EXCHANGE_OIDC_*` endpoints are now checked; loopback stays exempt, private
  ranges do not.

  **Upgrading:** this is a refusal, so a deployment with an `http` authorization endpoint or redirect
  URI on a non-loopback address will stop offering `/api/signin` at startup, naming the variable.
  `/health` and the catalogue keep serving. Look for `InsecureEndpoint` in the startup log.

- **An operator can tell their own misconfiguration from a refused credential** (X-17).
  `ExchangeError::Rejected` collapsed four causes, one of which was *this host's own client secret
  being wrong* — logged as "the provider refused the authorization code", which sends an operator to
  check a caller's credential instead of their own configuration. Four variants now, and **one**
  caller-facing answer: the split is in the log only, and the guard that the caller learns nothing
  about the provider stays green. Same shape X-15 established on the front channel.

- **A cleartext back channel is refused at startup, naming the variable** (X-17). An
  `http://` token endpoint sent this host's client secret as HTTP Basic credentials in the clear,
  with no refusal at all. **Loopback is exempt** — a local test IdP is a real workflow, and
  forbidding it pushes operators toward disabling verification or testing against production, while
  loopback packets never reach an interface. **Private ranges are not exempt**: "it's only the
  internal network" is exactly the assumption that makes a cleartext secret worth taking. An absent
  or unrecognised scheme is refused rather than guessed.

### Fixed

- **A sign-in reads the clock once** (X-24). X-16 consolidated the wall clock to one function, but
  one function is not one reading: `complete` read it for `admit` and the session store read it
  again, so a token expiring between the two was admitted and then refused — the caller seeing a
  `503` "cannot open a session" for what was really an expired credential, and the log saying the
  same. The reading is now taken once and spent on both decisions.

  It is still taken **after** the token exchange rather than at the top of the call. Moving it
  earlier reads plainer and fails open: the deadline is measured from it, so a reading taken before
  a slow token endpoint would let the session outlive the token by the round-trip.

- **A create the store refuses keeps its kind** (X-20). `partly_written` flattened every
  store-failure kind to `503` "retrying may work", so a create refused because the store *denied this
  host access* sent the operator to retry instead of to fix the permission — the same defect X-18
  fixed on the delete side. A partly-written create is now `502` for denied, backend and layout
  failures. The three existing caller-facing sentences are pinned byte for byte, so the shared
  mapping cannot be reworded by accident.

- **The cleartext check now parses an authority the way the client that dials it does** (X-19).
  X-17's refusal read `http://evil.example\@127.0.0.1/token` as loopback and admitted it, while the
  `url` crate reqwest actually dials with ends the authority at the backslash and resolves the host
  to `evil.example` — so the client secret would have gone out as Basic credentials, in cleartext,
  to a remote host, past the check built to stop precisely that. Operator-supplied configuration
  only, never caller-reachable.

  The agreement is now **measured**: 475,270 generated spellings through the old parser, the new one
  and real `url` 2.5.8. The old parser admitted 15 endpoints `url` dials remotely over `http`; the
  new one admits none. The doc no longer claims it cannot admit a cleartext endpoint — it promises
  one direction, names the working configurations it refuses, and says the agreement is measured
  rather than proved.

- **A delete that fails half way says what it destroyed** (X-18). `DELETE` looped over a
  connection's credentials and returned a generic `503` on the first error, leaving some destroyed
  and some live while telling the operator only "retrying may work" — so a *live* vendor credential
  could survive a delete, which is the worst possible outcome for the case a delete exists for:
  revoking a leaked secret. The refusal now names what was destroyed and what is still held.

  **Rollback is not available in this direction** — a destroyed credential cannot be put back,
  because this host never held the plaintext — so the answer is honest reporting rather than a copy
  of create's rollback. The loop is best-effort rather than stopping at the first failure, since a
  delete is a revocation and destroying two of three beats destroying one. The store failure's kind
  survives into the refusal instead of being flattened, because answering a "denied" with "retrying
  may work" would be a fresh instance of the same misinformation.

- **A failing key set can no longer be hammered once per sign-in** (X-17). The refetch floor gated
  only unknown-`kid` refetches and was written after a *successful* parse, so while the JWKS endpoint
  was down every callback provoked a fresh outbound fetch. The floor now gates going out at all, and
  the rate-limited branch answers "provider unreachable" rather than "unpublished key" when no
  current key set is held — without which the fix would have made an outage read as a refused
  credential.

- **A session ends when the identity behind it does** (X-16). Deferred twice — X-03 left it to X-04
  on the grounds that an id token carries an `exp` worth binding to, and X-04 deferred it again
  because no composition could produce an id token. X-04 removed that reason, and the position was
  then worse than before: the host knew when an identity expired and discarded it.

  `Oidc::complete` passes the id token's `exp` to the session store **verbatim**. A five-minute token
  yields a five-minute session; this host invents no lifetime, because one it invented would outlive
  the credential it was shown.

  **An `exp` already past, or further out than thirty days, refuses the sign-in rather than being
  clamped.** Clamping would issue a session neither the provider nor this host described, and would
  leave the misconfigured provider in place for nobody to find. An expired session is **removed**
  rather than left unresolvable, so expiry cannot become a back door through the store's bound, and
  it answers exactly as a session that never existed.

  One wall clock, not two: `now()` moved to `session.rs`, because `admit` decides whether a token has
  expired and the store decides how long a session may live, and two clocks could admit a token and
  then refuse it a session. The development identity keeps its process-lifetime session — a roster
  handle carries no secret and no expiry, so any lifetime there would be invented, and that port
  already forces a loopback bind.

- **OIDC sign-in completes** (X-04, closing the `PARTIAL` this story shipped as). The owner took the
  dependency decision on 2026-08-01 — `reqwest`, `jsonwebtoken`, `sha2` — and `TokenExchange` is no
  longer an unbound port. The authorization code is redeemed back-channel with `client_secret_basic`
  and the id token's signature is verified against the provider's published keys, so `/api/signin`
  redirects to a real provider instead of serving an explanation, and the composition reports
  `Bound`. Configure the eight `FLUX_EXCHANGE_OIDC_*` variables and sign-in works end to end.

  **The permitted algorithms are derived from the JWK's key type and never from the token header**,
  which is what closes `alg: none` and RSA/HMAC algorithm confusion — the two attacks a *caller* of
  a JOSE library can still get wrong. Both are tested, the confusion case forged with both the
  published PEM and the JWK modulus spelling, because a vulnerable verifier passes whichever bytes
  it happens to hold. An unpublished `kid` is refused rather than falling back to trying keys until
  one verifies; a token with no `kid` resolves only when the provider publishes exactly one key.

  **Signature verification only.** Every claim check — `iss`, `aud`, `exp`, `nonce`, `sub` — stays in
  `Oidc::admit`, where it was already tested, so an expired token is refused as `Expired` rather than
  collapsing into a generic rejection. Two independent reviews verified that split claim by claim
  against `admit` rather than taking the comment on trust.

  `sha2` retires the hand-written `oidc/sha256.rs`, which existed only because no digest crate was
  allowed in. RFC 7636 Appendix A's vector is unchanged and still passes, which is what makes the
  swap checkable rather than merely plausible.

  Two endpoints are configured rather than discovered — `FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT` and
  `_JWKS_URI`. Discovery stays rejected, now on a different argument: with an HTTP client available
  it is a choice, and it keeps which keys can mint a session here legible from the environment
  rather than from a document re-fetched at runtime.

  **Note for deployers:** reqwest's `rustls` feature resolves to `aws-lc-rs`, so this build now
  compiles C and assembly. A container with no C toolchain that built this repository before will
  fail. OpenSSL and a second TLS stack are genuinely absent.

- **A sign-in a victim did not start cannot become a session in their browser** (X-15). Server-side
  `state` closes a *forged* callback, not login-CSRF: an attacker who starts a sign-in here honestly,
  authenticates at the provider as themselves and stops at the redirect holds a genuine `code` and a
  still-unspent `state`, and walking a victim into that callback passed every check X-04 had. The
  victim came away holding the attacker's session, inside the attacker's tenant — the north star
  inverted from the other end: the credential does not cross the boundary, the *human* does.

  A **`__Host-` binder cookie** planted at `/api/signin` is the missing tie — 256 bits from the one
  entropy path, `Secure` + `HttpOnly`, redacted in `Debug` like a session token, and never a URL
  parameter. `PendingAuthorizations::claim(state, binder)` **replaces** `take(state)` rather than
  sitting beside it: a method that spends an authorization on `state` alone *is* the hole, so the
  reliable way to stop a later story reaching for it is for it not to exist.

  A binder mismatch leaves the authorization **unspent** — the browser with the wrong binder is more
  likely the victim than the perpetrator, and a hostile callback must not cancel someone else's
  sign-in. A missing binder is refused *before* the pending store is consulted, so omitting the
  cookie neither falls through to the state-only path nor probes whether a `state` is live.
  `UnknownState`, `NoBinder` and `AnotherBrowser` are three log lines and not one, and deliberately
  indistinguishable to the caller.

  The binder is `SameSite=Lax` where the session cookie is `Strict` — deliberate, and documented at
  the definition: its whole job is to survive exactly one cross-site-initiated navigation, the
  provider's redirect back, which a `Strict` cookie would never arrive for.
- **Connections, addressed by a tenant the caller cannot name** (X-08, X-10). Create, list and
  delete a connection, scoped to the caller's tenant. The credential address is **derived** —
  `tenants/<tenant>/<authority>/<credential>`, with the tenant from the resolved principal and the
  authority from the connector's declaration — and **no route accepts an address**. A connector that
  declares no authority is refused rather than stored at a guessed one. Deleting a connection
  destroys its credentials. Tenant A cannot read, use or delete tenant B's connection, and the
  refusal names A's *own* address, never B's and never a value; 18 hostile connector ids across three
  methods were all refused.

  **A second connection to the same connector is refused (409), not silently overwritten.** The
  address has no instance dimension yet — upstream flux-connectors C-406 adds one and this repository
  cannot use it until it is published — so the refusal quotes the shape that will replace it and
  names X-14. Per-connection configuration is deferred for the same reason: a vendor subdomain is
  exactly the per-instance fact with no home until two instances can be told apart.

  The refusal is guarded across the whole probe-decide-write, because a check-then-write lost to two
  concurrent requests and produced precisely the silent overwrite it exists to prevent. **Single
  process only**, stated in the guard, the routes and the design: two replicas over one store would
  race again, and `SecretStore` has no compare-and-swap to close it properly.
- **OIDC sign-in, up to the token exchange** (X-04, partial). The authorization request is real:
  authorization-code flow with PKCE `S256`, and `state` and `nonce` bound at `/api/signin`,
  single-use and TTL-bounded. A callback carrying a `state` this host did not open is refused with
  **no session issued** — proven by committing the whole flow *without* the binding first, where the
  forged callback cheerfully answered "Signed in", i.e. a victim signed in as the attacker.

  **It cannot complete, deliberately.** Redeeming the code needs an HTTP client and verifying the id
  token needs a JOSE library; this workspace has neither, so `TokenExchange` is an unbound port and
  `/api/signin` serves an explanation rather than a redirect it could never return from. Nothing
  hand-rolls signature verification. Following X-03's precedent, a configured-but-unbound OIDC
  composition reports **`Unbound`**, so "OIDC is configured" cannot make a reachable bind legal while
  nothing can actually resolve a caller.

  The one crypto exception is a hand-written SHA-256 for the PKCE challenge, verified against
  `hashlib` over every message length 0..=600 and at the 2^32-bit boundary; it goes when `sha2` can
  be depended on. The tenant is fixed at startup rather than mapped from a claim, because some
  providers let users edit their own profile claims.
- **Identity, bound — with a dev principal that cannot open the door** (X-03). The `Identity` port is
  wired: a request carries a session, the host resolves it to a `Principal`, and every tenant is read
  from *that* and from nothing a caller controls — asserted three times, once each for a path
  segment, a body field and a header, against a route that genuinely declares `/{tenant}` so the
  claim is delivered and then ignored rather than never parsed.

  The load-bearing decision is that a development identity is a **third** bind state, not "bound". It
  resolves principals, so counting it as bound would have made `0.0.0.0` legal — but a roster handle
  is a credential with no secret in it, which is worse than an unauthenticated port, because
  everything downstream believes the principal. Arming it therefore confines the process to loopback,
  and the refusal names the opposite remedy from the unbound one.

  Sessions are a `__Host-` cookie with `Secure`/`HttpOnly`/`SameSite=Strict` and 32 bytes from
  `/dev/urandom`, refusing rather than falling back if the CSPRNG is unavailable. **A session token is
  returned in the body only to a caller that presented a readable credential**, so the route cannot
  turn an unreadable credential into a readable one — without that rule `HttpOnly` was a control that
  only appeared to exist, since script could POST with the ambient cookie and read an equally
  powerful token out of the response. The store is bounded and **refuses at the bound rather than
  evicting**, because evicting signs out a caller who did nothing wrong. No expiry yet, stated rather
  than implied.
- **The connector catalogue, served and read** (X-05, X-06, X-07). `GET /api/catalogue/connectors`
  and `/api/catalogue/connectors/{id}/operations` publish 53 connectors and 299 operations with the
  metadata a `Selector` is written over — `risk`, `effects`, `idempotency` — so the grant model stops
  being server-only folklore. The response distinguishes **what exists** from **what a principal may
  call**: nothing is filtered by grant, and `admitted: null` says so on the wire rather than omitting
  an operation a caller lacks, because an agent that cannot see an operation cannot report being
  refused. `effects` is *derived* (`network` iff the operation declares hosts, since the catalogue
  declares no effects) and carries `effects_derived: true` so an inference is never read as a
  declaration. Adding a connector needs no change to the route.

  The console now reads that catalogue live; `console/src/fixtures/catalog.ts` and its banner are
  deleted in the same change. An unreachable service renders an error **naming the endpoint** — "zero
  connectors" and "cannot reach the server" must not look alike. The 15 explorer components carried
  from flux-connectors are untouched; four findings against them were reported upstream
  (flux-connectors C-408) rather than patched locally.
- **A credential store, honest about what protects it** (X-09). `exchange_host::CredentialStore`
  binds `connector-secrets`' file-backed store — `0600` in a `0700` directory, modes set in the
  create call and re-checked at open, a widened mode **refused rather than tightened**, and atomic
  writes through temp + `fsync` + `rename(2)`. What this host adds is startup honesty: a path inside
  a working tree is refused (one `git add -A` from a committed credential), a configuration naming
  no path is a startup error naming what would have worked with **no fallback to memory**, and the
  banner reads its path back off the store that was actually bound. The README states what does
  *not* protect a value there: the file mode and nothing else.
- **An HTTP surface that refuses an open bind** (X-02). `cargo run` binds `127.0.0.1:8080` and
  answers `GET /health`. Startup on a reachable address with no identity provider configured is
  **refused before the socket opens**, and the refusal names what would have worked — a daemon
  holding credentials behind an open listener is the failure this exists to prevent, so it does not
  start-and-warn. Routes are declared as data per feature module and the `Router` is derived from
  them, so `routes::published()` is the whole surface by construction and a test can enumerate it;
  an opaque per-module `Router` would have let a module publish an unauthenticated route no test
  could see. Framework choice and its reasons: `docs/designs/http-surface.md`.

- **The backlog** — vision, roadmap, and thirteen stories across four epics (X-01…X-13), plus the
  operating contract in `AGENTS.md`. The first wave is eight ready stories: the HTTP surface,
  sign-in, the catalogue and the credential store.

## [0.0.1] - 2026-08-01

### Added

- **The charter, and the rules as tested types.** `crates/exchange-host` carries `Principal`/`Tenant`,
  `Grant`/`Selector`, `Runtime`/`Deployment`, `Lease` and the `Identity` port, with 19 tests. Four
  rules are executed rather than described: a tenant id that would traverse its credential-address
  prefix is refused at construction; a multi-tenant deployment refuses every locally-executing
  runtime, naming what would have worked; a grant selects by declared metadata with deny beating
  allow; and a lease requires the same principal, not merely the same tenant.
- **A binary that reports and exits**, deliberately not a service.
- **A console** over the 15 framework-free explorer components carried from flux-connectors,
  rendering fixture data behind a banner that says so, with the components' no-framework-import
  invariant ported and strengthened.
