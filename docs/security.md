# Security posture

This is the contributor and operator map of flux-exchange's security boundary. It describes the
software and the observed public deployment as of 2026-08-02. It is not a vulnerability-reporting
channel; suspected vulnerabilities belong in the monitored private route named by
[`SECURITY.md`](../SECURITY.md).

The north star is the [vision](vision.md): **the credential never crosses the boundary; the
authority does.** The detailed decisions remain in their designs and source. This document links to
those authorities instead of making a second implementation specification.

## How to read a claim

Every posture claim below carries one of these labels:

- **Enforced in code.** The repository contains a type, guard or test that makes the claim true for
  the named component. A link points to the owner.
- **Deployment-dependent.** The application cannot enforce the claim alone. The operator, identity
  provider, Fly configuration or GitHub repository setting must keep it true.
- **Known limitation.** The control is missing or deliberately narrower than a reader might assume.
  The linked story is the work, where one exists.

The label's scope matters. In particular, a control in `exchange-server` describes this binary,
not every downstream composition of the published `exchange-host` crate. The exact edge is recorded
under [“Where the locks stop”](designs/invoke.md#where-the-locks-stop).

## Threat model

### Protected assets

- **Enforced in code.** Vendor credentials are write-only through the HTTP surface: addresses are
  derived from connector declarations and a resolved principal's tenant, while no route reads a
  stored value back. The store and address rules are owned by
  [`credentials.rs`](../crates/exchange-host/src/credentials.rs) and
  [`connections.rs`](../crates/exchange-host/src/connections.rs).
- **Enforced in code.** OIDC authorization codes, the client secret, ID tokens, session tokens,
  sign-in binders, PKCE verifiers and Service Account tokens use redacting types or fixed refusal variants at
  the seams where they are handled. The authentication argument is in
  [`oidc-signin.md`](designs/oidc-signin.md) and the local session rules are in
  [`identity-and-session.md`](designs/identity-and-session.md).
- **Enforced in code.** Tenant assignments, connection labels, grants, settings and Service Account
  verifier records are kept in stores separate from credential material. Invocation reads a tenant once from the resolved
  principal and binds both credential and setting ports from that value in one expression; see
  [`invoke.rs`](../crates/exchange-host/src/invoke.rs).
- **Enforced in code.** Operational audit records are a separate, durable, typed journal and are
  not execution records. Process loss and provider-log expiry do not remove rows inside their
  30-day minimum retention; invocation inputs, outputs and trace values remain outside this model.

### Trust boundaries

- **Enforced in code.** The browser crosses one same-origin HTTP boundary. The server owns both the
  console and `/api`, and the session cookie is never designed for a second origin; see
  [`remote-deployment.md`](designs/remote-deployment.md) and
  [`routes/mod.rs`](../crates/exchange-server/src/routes/mod.rs).
- **Deployment-dependent.** Fly terminates public TLS and forwards to the one machine. The committed
  [`fly.toml`](../fly.toml) forces HTTP to HTTPS, but certificate management, edge isolation and
  availability remain Fly controls.
- **Enforced in code.** The OIDC provider is trusted only through explicitly configured issuer,
  endpoints, audience and key-set URI. This host does not discover a new provider document at
  runtime; [`config.rs`](../crates/exchange-server/src/oidc/config.rs) owns that choice.
- **Enforced in code.** Vendor egress crosses the `connector_pack::Egress` port. The reusable host
  builds no request of its own, and the server composition that owns an HTTP client may not name
  `connector_pack`; the three enforcement mechanisms and their blind spots are in
  [`invoke.md`](designs/invoke.md).
- **Deployment-dependent.** The filesystem, Fly volume, snapshots, logs, GitHub repository and CI
  runners are trusted operator/platform boundaries. A process running as `root`, a platform
  administrator, a readable backup or a compromised workflow can bypass application-level
  controls.

### Assumed attackers

- **Enforced in code.** An anonymous internet caller is assumed able to enumerate public routes,
  start sign-ins, replay arbitrary callback input and send requests at the process continuously.
  Anonymous-route enumeration, sign-in binding and process-local traffic limits are code and tests,
  not assumptions; see [`routes/mod.rs`](../crates/exchange-server/src/routes/mod.rs),
  [`flow.rs`](../crates/exchange-server/src/oidc/flow.rs) and
  [`traffic.rs`](../crates/exchange-server/src/traffic.rs).
- **Enforced in code.** An authenticated caller is assumed hostile to tenant isolation: request
  paths, bodies and headers are never accepted as a tenant, credential address, runtime or request
  origin. The caller may choose an operation and its declared parameters only.
- **Enforced in code.** A malicious connector parameter or public DNS result is assumed capable of
  targeting loopback, private, link-local or cloud-metadata addresses. This server composition sets
  the Flux private-network policy to deny all of them in
  [`execution.rs`](../crates/exchange-server/src/execution.rs).
- **Enforced in code.** Authentication and operator authority are separate axes. A signed-in member
  may use the catalogue, session and grant-gated invocation surfaces, but management routes require
  an exact immutable user subject in deployment-owned `FLUX_EXCHANGE_OPERATOR_SUBJECTS`. An absent
  or malformed policy admits nobody; Service Accounts do not become operators even if an id happens
  to match. Operator refusals and successful administrative actions are audited without recording
  the policy contents or session.
- **Known limitation.** A host or platform administrator is outside the application threat model.
  File modes, non-root execution and volume encryption reduce accidental exposure; none protects a
  credential from a party controlling the running machine or deployment account.

## Identity and sessions

- **Enforced in code.** Federated sign-in uses the authorization-code flow with PKCE. The token
  signature is verified against an explicitly configured JWKS, algorithms are constrained by the
  published asymmetric key kind, and symmetric keys are refused. Issuer, audience, expiry and nonce
  are then checked before a principal is created; see
  [`http_exchange.rs`](../crates/exchange-server/src/oidc/http_exchange.rs) and
  [`oidc/mod.rs`](../crates/exchange-server/src/oidc/mod.rs).
- **Enforced in code.** `state` is random, server-held, expiring and single-use. A separate random
  `__Host-` binder ties that state to the browser that began the sign-in, which closes login CSRF
  that a server-side state check alone does not. The binder is `Secure`, `HttpOnly`, `SameSite=Lax`
  because it must survive the provider's top-level redirect; the full argument is in
  [`flow.rs`](../crates/exchange-server/src/oidc/flow.rs).
- **Enforced in code.** Session tokens are drawn from OS randomness, stored only in process memory,
  and carried in `Secure; HttpOnly; SameSite=Strict` host cookies. An OIDC session cannot outlive the
  ID token that established it, and logout removes the presented session server-side as well as
  clearing the browser cookie; see [`session.rs`](../crates/exchange-server/src/session.rs) and
  [`public-service-hardening.md`](designs/public-service-hardening.md).
- **Enforced in code.** Reachable binds require federation or the distinct verifier-backed local
  users binding. Its owner-only file stores generated-secret verifiers, never plaintext; malformed
  entries and widened modes refuse startup. No identity and the
  secretless development roster both refuse every non-loopback bind in
  [`bind.rs`](../crates/exchange-server/src/bind.rs).
- **Enforced when configured.** `FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN` requires byte-for-byte equality
  with Google's signature-verified `hd` claim. The authorization request carries the same value
  only as an account-selection hint; a missing or mismatched signed claim refuses before a session
  is opened. Membership is never inferred from email, and sign-in requests only `openid` because
  identity is the immutable `sub`.
- **Known limitation.** Sessions have no durable inventory, per-principal revocation or global
  revocation endpoint. A process restart invalidates all sessions; normal logout invalidates only
  the session presented to it.

## Tenant and authorization boundary

- **Enforced in code.** A tenant is a validated part of a resolved `Principal`; a route cannot take
  it from a path, body or header. OIDC uses the startup-configured tenant, and credential addresses
  are always prefixed from that value. The type-level rule lives in
  [`principal.rs`](../crates/exchange-host/src/principal.rs).
- **Enforced in code.** Route access is declared as data and the router derives its guard from that
  declaration. Tests enumerate the anonymous and principal-kind-gated surfaces so a handler cannot
  become public merely by forgetting a check; see
  [`routes/mod.rs`](../crates/exchange-server/src/routes/mod.rs).
- **Enforced in code.** Connection and credential management, settings mutation, grant editing,
  Service Account lifecycle, workflows and channels require operator authority. A Service Account
  cannot use a stolen token to create a successor principal or replace the credential behind its
  authority, and an ordinary authenticated member cannot administer the tenant.
- **Enforced in code.** Invocation is fail-closed. Without a grant store the invoker is not built;
  without an admitting tenant grant the operation is refused before a credential is read. Selectors
  decide from catalogue-declared risk, effects and idempotency, with explicit operation exceptions;
  an explicit deny wins over an explicit allow. See
  [`grant.rs`](../crates/exchange-host/src/grant.rs) and [`invoke.md`](designs/invoke.md).
- **Known limitation.** Grants belong to a tenant, not to one principal. Every resolved principal
  in that tenant is evaluated against the same grant set. Operator authorization narrows
  administrative routes; it does not silently turn tenant invocation grants into per-user grants.
- **Enforced in code.** Service Account tokens resolve through the same principal boundary as human
  identity, remain tenant-bound, and can be listed and revoked only by a signed-in human. A token
  authenticates; grants independently decide what that tenant may invoke or subscribe to. The
  former Agent spelling is a v0.16 compatibility alias, not a second principal kind.

## Credentials and persistent state

- **Enforced in code.** The file credential store creates a `0700` parent and `0600` file, re-checks
  both modes on open, refuses a widened mode rather than repairing it, resolves symlinks and `..`,
  and refuses a path inside a Git working tree. It never falls back to an in-memory store when a
  configured path is missing or unusable; see
  [`credentials.rs`](../crates/exchange-host/src/credentials.rs).
- **Enforced in code.** Writes use a sibling temporary, `fsync` and rename so a crash does not leave
  a truncated store. Delete rewrites immediately, and connection rotation replaces a credential
  without a deliberate absent window. Partial multi-value deletion reports what was removed and
  what may remain.
- **Enforced in code.** Multiple connections use a host-minted UUID in the credential address and a
  separate tenant-scoped operator label. Existence comes from scoped credential inventory, not the
  naming overlay. First-to-second and two-to-one transitions are checked atomic batches; a backend
  that cannot prove inventory and atomic mutation retains the sole legacy surface and refuses the
  plural operation.
- **Enforced in code.** One credential value is limited to 8 KiB and one tenant's credentials to
  64 KiB. Non-secret settings have separate 1 KiB-per-value and 16 KiB-per-tenant bounds. These
  limits bound both storage and the cost of whole-file rewrites; their reasons live beside the
  constants in [`connections.rs`](../crates/exchange-host/src/connections.rs) and
  [`settings.rs`](../crates/exchange-host/src/settings.rs).
- **Deployment-dependent.** The observed production Fly volume reported `encrypted: true` on
  2026-08-02. Fly documents encryption at rest as the default for volumes; an operator must verify
  the actual volume rather than infer it from [`fly.toml`](../fly.toml), which does not create the
  volume. See [Fly's volume overview](https://fly.io/docs/volumes/overview/#volume-encryption).
- **Known limitation.** Files hold application-level plaintext. There is no envelope encryption,
  passphrase, OS keychain or application-controlled key, and volume encryption does not protect
  data from the running process, platform control plane, `root`, a mounted snapshot or a copied
  store. [X-97](stories/X-97-public-credentials-leave-the-file-store.md) binds a managed secret
  backend for public deployments.
- **Known limitation.** On 2026-08-02 the production volume listed no snapshots, no retention target
  was committed, and no restore had been rehearsed. Fly now documents daily snapshots with a
  five-day default while recommending another recovery method for important single-volume data;
  [X-94](stories/X-94-persistent-state-has-a-tested-recovery-path.md) defines the tested policy. See
  [Fly's snapshot guidance](https://fly.io/docs/volumes/snapshots/).
- **Enforced in code.** Decommissioning the file store means removing its whole directory, not only
  the named file: an interrupted write may leave a complete sibling temporary. The store tests hold
  that deletion guidance beside the implementation.
- **Known limitation.** Filesystem deletion and volume destruction are not cryptographic erasure,
  and retained snapshots remain credential-bearing until their retention expires. Rotate vendor
  credentials when a store or snapshot may have escaped control.

## Execution, egress and sandbox

- **Enforced in code.** A caller names an operation and supplies only that operation's declared
  parameter object. The catalogue chooses connector, runtime, hosts and credential declarations;
  the principal chooses the tenant. `Invoker::invoke` orders catalogue, runtime and grant checks
  before credential access, then delegates the one dispatch to the operation's compiled Flux. See
  [`invoke.rs`](../crates/exchange-host/src/invoke.rs) and [`invoke.md`](designs/invoke.md).
- **Enforced in code.** Runtime is connector-declared. A multi-tenant deployment refuses socket,
  process, container and plugin runtimes because they execute with this host's identity or network
  position. An unforgeable admission witness is required to reach dispatch; see
  [`runtime.rs`](../crates/exchange-host/src/runtime.rs).
- **Enforced in code.** The server's HTTP transport denies secret references from the process
  environment and refuses private, loopback, link-local and cloud-metadata destinations, including
  public names that resolve to them. Per-connection values that would replace a whole authority are
  refused unless the connector publishes a closed allowed set; see
  [`execution.rs`](../crates/exchange-server/src/execution.rs) and
  [`connection-settings.md`](designs/connection-settings.md).
- **Enforced in code.** Invocation results are rendered through the same redactor into which the
  connector pack registered credentials, covering vendors that echo a credential in an error
  response. Counting-transport tests assert one dispatch on success and zero on every pre-dispatch
  refusal.
- **Enforced in code.** This server composition builds Flux's `System` with sandbox mode `Require`,
  no sandbox network and no extra writable path. A normal sandboxed process operation is confined
  or refused when no backend is available.
- **Known limitation.** Sandbox posture belongs to this unpublished server composition, not the
  reusable host crate. It does not cover Flux's explicitly exempt spawn paths, and a truthy
  operator-supplied `FLUX_SANDBOXED` marker tells Flux that an outer sandbox already exists. The
  precise boundary and source-scanner blind spots are recorded in
  [`execution.rs`](../crates/exchange-server/src/execution.rs) and
  [“Where the locks stop”](designs/invoke.md#where-the-locks-stop).
- **Known limitation.** The source locks prove strong properties about the published host crate but
  are not a whole-program capability proof. A new transport hidden behind an allowed transitive
  dependency or an unlisted name can escape name-based scanning; review and behavioural tests remain
  part of the boundary.

## Browser and HTTP surface

- **Enforced in code.** The console uses relative API URLs and is served by the same process and
  origin. `SameSite=Strict` session cookies are not relaxed to make split-origin hosting work. The
  SPA fallback is also fenced off `/api`, so an unknown API path cannot become a successful HTML
  response; see [`routes/mod.rs`](../crates/exchange-server/src/routes/mod.rs).
- **Enforced in code.** Every response receives a same-origin content security policy, HSTS without
  preload, `nosniff`, `no-referrer`, frame denial through CSP, and a permissions policy disabling
  camera, microphone, geolocation, payment and USB. API responses carry `Cache-Control: no-store`;
  fingerprinted static assets remain cacheable. The owning decision is
  [`public-service-hardening.md`](designs/public-service-hardening.md).
- **Enforced in code.** State-changing browser authority rides a `SameSite=Strict` cookie, and login
  CSRF is separately closed by the state-plus-binder construction. No CORS configuration is used as
  a substitute for either control.
- **Deployment-dependent.** HSTS has effect only after a browser reaches the service over TLS. Fly's
  `force_https` supplies that public transport for the current deployment; a different composition
  must provide equivalent TLS and redirect behaviour.
- **Known limitation.** The CSP permits same-origin scripts and styles; it limits where injected
  content can load from but does not make a same-origin script injection harmless. Input/output
  encoding and the absence of unsafe HTML sinks remain required.

## Availability and resource bounds

- **Enforced in code.** One process admits at most 30 OIDC authorization starts per rolling minute,
  120 invocation attempts per rolling minute, 30 invocations for each resolved principal per
  rolling minute and 16 concurrently executing invocations. Per-principal keys are derived only
  from resolved tenant/kind/id. Saturation refuses immediately with `429` and `Retry-After`; it does
  not create an unbounded queue, and health/session/administration routes do not consume invocation
  slots. Fixed-cardinality metrics and warnings expose saturation without identity labels. See
  [`traffic.rs`](../crates/exchange-server/src/traffic.rs).
- **Enforced in code.** Pending sign-ins, live sessions, tenant identifiers, credential material
  and settings have explicit memory or size bounds. Expired state is removed rather than merely
  made unreachable.
- **Deployment-dependent.** The current Fly topology is exactly one machine. This is a correctness
  constraint: the file stores coordinate only inside one process and a Fly volume belongs to one
  machine. [`remote-deployment.md`](designs/remote-deployment.md) and
  [`deploying.md`](deploying.md#why-one-machine-and-do-not-change-it) explain why horizontal scaling
  would silently create divergent state.
- **Deployment-dependent.** Fly Proxy request concurrency bounds anonymous occupancy before it
  reaches the process. The application does not read forwarding headers because it has no
  authenticated proxy-to-application address identity contract; Fly and another deployment must
  supply equivalent trusted-edge flood controls.

## Audit and incident evidence

- **Enforced in code.** Authentication, authorization, Service Account lifecycle,
  connection/credential/settings changes, grant replacement and invocation outcomes write
  versioned JSON with independent event/request ids, RFC 3339 time, stable action/outcome, resolved
  actor fields and a closed non-secret target. [`audit.rs`](../crates/exchange-server/src/audit.rs)
  cannot represent tokens, credential/setting values, OIDC material or request bodies.
- **Enforced in code.** State-changing authority is journaled as `attempted` before the handler
  touches its store or runtime, then atomically transitioned to `succeeded` or `refused`. A failed
  initial write refuses the action; a failed final write leaves the attempted row and refuses a
  false success. A reachable bind without `FLUX_EXCHANGE_AUDIT` refuses before its socket opens.
- **Enforced in code.** SQLite retains at least 30 days and supports bounded local queries by event
  id, actor or target. Authentication floods, repeated per-actor authorization refusals and every
  credential/grant change append identifier-and-count-only alerts and emit `warn` notifications.
- **Enforced in code.** Refusals distinguish operator action where it matters while avoiding an
  oracle to the caller. Provider detail and attacker-chosen key identifiers do not cross into HTTP
  responses; unreachable dependency details are logged for the operator.
- **Deployment-dependent.** The Fly volume supplies persistence. The runtime uid and Fly SSH users
  can read the journal; that uid or a Fly organization administrator able to replace/destroy the
  volume can delete it early. Tenant HTTP callers have no audit-enumeration route. The runbook names
  these powers and the read-only query command.
- **Enforced in code.** `GET /api/connections` projects the latest retained successful creation or
  rotation onto each held credential as its principal and timestamp, scoped in SQL to the resolved
  tenant. Missing evidence reads `unknown`: the credential store alone remains authoritative for
  existence and use, and evidence beside an empty address never makes a credential appear held.
  [X-60](stories/X-60-who-supplied-this-credential.md) carries the boundary.

## Supply chain and deployment provenance

- **Enforced in code.** CI builds, tests, lints and formats Rust; tests and builds both Node trees;
  audits locked Rust dependencies and both npm trees; checks the MSRV; and self-tests the action-pin
  crate-version and repository-security scanners. Every third-party action is pinned to a full
  commit SHA. See
  [`ci.yml`](../.github/workflows/ci.yml) and
  [`check-action-pins.sh`](../scripts/check-action-pins.sh). Dependabot covers the Cargo workspace,
  both Node trees and GitHub Actions; its one Flux-family group keeps engine and connector pins in
  the same update.
- **Enforced in code.** Dependency-audit exceptions are explicit and narrow. The current RSA
  advisory is ignored only because this service verifies provider signatures and generates no RSA
  keys; every other RustSec warning remains denied.
- **Enforced in code.** The runtime container contains no compiler or package manager, runs as fixed
  non-root uid `10001`, uses an exec-form entrypoint and installs the CA roots needed for OIDC TLS.
  The decisions are recorded in [`Dockerfile`](../Dockerfile).
- **Deployment-dependent.** Read-only GitHub API verification on 2026-08-03 reported private
  vulnerability reporting, Dependabot security updates, secret scanning and push protection
  enabled. Active default-branch ruleset `20297512` requires a pull request, resolved conversations,
  every established Rust/Node/site check, and an up-to-date base; it blocks deletion and force-push
  with no bypass actors. Its approval count is deliberately zero while this is a single-maintainer
  repository, because requiring an independent approval would make every pull request unmergeable;
  it must rise to one when a second maintainer can review. These are live repository settings rather
  than properties of a checkout, while [`SECURITY.md`](../SECURITY.md),
  [`dependabot.yml`](../.github/dependabot.yml) and the self-testing
  [`check-repository-security.sh`](../scripts/check-repository-security.sh) keep their committed
  contract reviewable.
- **Known limitation.** The same API verification reported validity checks and non-provider-pattern
  scanning disabled. GitHub documents both as requiring Team or Enterprise Cloud plus GitHub Secret
  Protection for an organization-owned repository, while `codewandler` reports plan `free`. X-92
  remains in progress rather than claiming those two controls; completing it requires an
  organization plan/product change before the settings can be enabled and verified.
- **Known limitation.** Production is deployed manually with `fly deploy`; the v0.13.0 deployment
  was built from a working tree containing uncommitted changes. The image therefore cannot be
  proven from an immutable reviewed commit even though the live version and headers were verified.
  [X-93](stories/X-93-production-comes-from-a-reviewed-commit.md) adds a protected, SHA-pinned
  deployment workflow and live provenance checks.
- **Known limitation.** Container base images are tag-pinned rather than digest-pinned, Cargo image
  builds do not pass `--locked`, and no image scan or SBOM is emitted. X-93 closes these as one
  provenance story.

## Ranked security roadmap

The story files are the contract; this table is only the ranked map.

| Rank | Work | Why it is next |
|---|---|---|
| P0 | [X-90 — Verify the Google organization in the signed token](stories/X-90-verify-the-google-organization-in-the-signed-token.md) | Make organization admission survive a provider-console mistake. |
| P0 | [X-91 — Signing in does not make every member an operator](stories/X-91-signing-in-does-not-make-every-member-an-operator.md) | Preserve broad authentication while narrowing administrative authority. |
| P0 | [X-92 — Private reporting and protected main](stories/X-92-private-reporting-and-protected-main.md) | Give findings a safe channel and prevent unchecked or known-secret changes reaching main. |
| P1 | [X-93 — Production comes from a reviewed commit](stories/X-93-production-comes-from-a-reviewed-commit.md) | Make a live image traceable to a gated SHA. |
| P1 | [X-94 — Persistent state has a tested recovery path](stories/X-94-persistent-state-has-a-tested-recovery-path.md) | Give the single credential-bearing volume an observed restore path. |
| P1 | [X-96 — Traffic controls are fair as well as bounded](stories/X-96-traffic-controls-are-fair-as-well-as-bounded.md) | Keep one caller from consuming the shared backstop. |
| P2 | [X-97 — Public credentials leave the file store](stories/X-97-public-credentials-leave-the-file-store.md) | Move public-deployment secrets behind a managed backend and migration contract. |

## Incident checklist

Use the smallest containment that stops further authority, record times and identifiers rather than
material, and treat every store, snapshot and log export as sensitive until proved otherwise.

### Suspected vendor-credential exposure

- **Enforced in code.** Narrow or remove the affected connector's grant first when invocation must
  stop immediately; the grant gate refuses before reading the credential.
- **Deployment-dependent.** Revoke or rotate the credential at the vendor, replace it through the
  atomic rotation surface, verify one least-privilege operation, then confirm the old value is
  unusable. Do not place either value in a ticket, command history or log.
- **Known limitation.** If a store or snapshot may have escaped, assume every credential it held was
  exposed. The durable audit journal can identify recorded suppliers and changes but cannot prove a
  copied store was unread; rotate the full set and preserve the non-secret evidence separately.

### OIDC client-secret rotation or identity-provider incident

- **Deployment-dependent.** Follow the provider's safe rotation order, update the Fly secret, and
  restart the one machine. Confirm sign-in, issuer/audience failures and security headers after the
  change; never place the client secret in `fly.toml` or repository files.
- **Known limitation.** Rotating the OIDC client secret does not revoke already-issued local
  sessions. Restart the process to invalidate all sessions when containment requires it.
- **Known limitation.** There is no dual-client-secret or per-principal session-revocation path, so
  a provider that cannot overlap secrets may impose a sign-in interruption.

### Session compromise

- **Enforced in code.** Normal logout closes the one presented session server-side. Use it when the
  affected browser still controls the session.
- **Known limitation.** For a copied session or unknown scope, restart the process to invalidate the
  in-memory session store, then investigate the underlying identity. There is no inventory or
  selective administrative revocation today.

### Snapshot restoration

- **Deployment-dependent.** Restore into a new, isolated encrypted volume; do not attach it to a
  second active exchange. Verify ownership and the `0700`/`0600` store modes before starting the
  process, then verify store reads, grants, health and headers before replacing the failed machine.
- **Deployment-dependent.** Record the snapshot timestamp as the recovery point and reconcile every
  credential, setting and grant changed after it. Treat the restored copy and the old volume as two
  credential-bearing stores until one is decommissioned.
- **Known limitation.** No restore drill or RPO/RTO evidence exists yet. X-94 targets RPO at most
  24 hours and documented RTO at most 60 minutes.

### Store decommissioning

- **Enforced in code.** Stop the process and remove the entire store directory, including sibling
  temporaries; deleting only the configured file is incomplete.
- **Deployment-dependent.** Destroy or revoke access to the old volume and every copy, record
  snapshot identifiers and retention deadlines, and rotate all vendor credentials the store held.
- **Known limitation.** A destroyed file or volume is not proof of cryptographic erasure, and Fly
  snapshots can remain restorable until retention expires. X-97 requires old directories and
  retained snapshots to age out after a managed-backend migration.
