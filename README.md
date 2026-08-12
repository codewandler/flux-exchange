# flux-exchange

The platform layer of the [flux](https://github.com/codewandler/flux) family: a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.

Its primary caller is a **Service Account** or hosted **Managed Agent**—not a human at the console.
People sign in to wire things up and to see what happened;
non-human callers invoke operations all day. That inverts the usual assumption and shapes everything
below.

The contributor and operator [security posture](docs/security.md) maps the threat model, enforced
controls, deployment assumptions, known limitations, security roadmap and incident checklist.
The [domain vocabulary](docs/concepts.md) gives Connector, Connection, Datasource, Trigger, App,
Managed Agent and Service Account one meaning across the Flux family and labels which bindings are
live versus target architecture.

> [!WARNING]
> **Status: v0.18.0 — the catalog-artifact adoption line: document-backed settings, the served catalogue pack, and artifact-composed authorizations.**
>
> `cargo run -- --dev` binds `127.0.0.1:8080`, derives `user:${USER}@dev`, and binds the
> complete durable local composition below the conventional per-user state directory without any
> storage setting. It serves health, the connector catalogue and a session without OIDC setup. The ordinary composition supports both a
> verifier-only local users file and **complete** OIDC sign-in, and refuses to start on a reachable
> address while neither safe binding is configured. The authorization code is redeemed back-channel and the id token's signature is
> verified against the provider's published keys, so `/api/signin` redirects to a real provider —
> configure the eight `FLUX_EXCHANGE_OIDC_*` variables and it works end to end. Connections can be
> created, listed, **rotated** and deleted per tenant. With `FLUX_EXCHANGE_CONNECTIONS` bound, one
> tenant can hold several labelled instances of the same connector; invocation selects one with
> `?connection=<label>` and refuses ambiguity rather than choosing an account. A signed-in human
> can fetch one declaration-driven `exchange.connection-plan.v2` contract that names the label,
> every credential and setting target, choices and CLI aliases without disclosing secret presence:
> every secret field's live `set` fact is `null`. Mutations use the bounded local-management
> ceremony rather than placing vendor values in ordinary JSON. The console renders that same
> contract and reports incomplete and refused outcomes honestly. GitLab's
> connector-declared custom HTTPS origin is a revisioned proposal until a configured operator
> reviews and approves it; proposed and revoked values cannot reach request projection.
> A signed-in human
> can create, list and revoke
> **Service Accounts**, whose `fxsa_…` bearer tokens authenticate at the same guarded API boundary
> as browser sessions. An authenticated `GET /api/catalogue/effective` returns exactly the
> connected and granted operation bindings that Service Account can use, with a stable generation
> identity for turn-boundary refresh. **`invoke` runs**:
> `POST /api/operations/{operation}/invoke` executes one
> catalogue operation for the caller's tenant, with the request built by `connector_pack` from the
> operation's own compiled Flux — **gated by a grant** since X-13, and limited to the forty
> connectors whose base URL needs no per-tenant configuration. An operation runs only if one of the
> caller's tenant's grants admits it, decided from the operation's declared `risk`, `effects` and
> `idempotency` rather than from a list of ids; a host with no grant store bound
> (`FLUX_EXCHANGE_GRANTS`) runs nothing at all. Since X-62 a **signed-in human of the tenant reads
> and edits those grants over HTTP** — `GET`/`PUT /api/grants`, with `POST /api/grants/preview`
> answering which operations a proposed grant *would* admit before it is saved — stating a connector
> and at most a risk level, never a list of operation ids, which the route refuses. The console now
> guides a person through Connect → Grant → Invoke. A signed-in human can also author and validate
> a Flux workflow, publish immutable versions, and inspect or cancel durable runs. The workflow
> entry operation and every nested connector call are independently grant-gated before credentials
> are resolved; `FLUX_EXCHANGE_WORKFLOWS` names the durable definitions-and-runs directory. The same
> boundary now works inbound: an operator can persist a connector's generated WebSocket
> binding and a principal can subscribe only to the closed declared event set an inbound grant
> admits. `FLUX_EXCHANGE_CHANNELS` names the durable channel declarations; the built-in binary runs
> sockets locally only under the single-tenant `--dev` composition and refuses to invent a local
> placement for a multi-tenant deployment.
> With `FLUX_EXCHANGE_APPS` bound, an operator can install the curated immutable Slack-bot-style
> App Package against a labelled Slack Connection, freeze its reviewed operation/model/risk/scope
> authority, and chat with its Managed Agent. Declared events enter a durable inbox before Flux
> executes them; Sessions, Runs and value-free Activity are folded from tenant/App-isolated Flux
> event logs, and retry refuses whenever frozen effects cannot prove it safe.
>
> Authentication acquisition is also **fail-closed**. A connector declaring a hazardous way to
> obtain a credential is refused unless the deployment explicitly names that hazard in
> `FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS` (currently
> `resource_owner_secret_shared`). Unset permits none; an unknown name refuses startup rather than
> being skipped. X-75's injectable acquisition seam exists, but no released connector declares it
> until upstream C-440 lands, so production binds no live acquisition performer today. Once one is
> bound, forgetting the opt-in looks like a connection outage: the host answers `403` and names the
> connector and hazard before contacting the vendor. A vendor rejection remains a different failure.
>
> See [What exists today](#what-exists-today) for the honest inventory before planning around any
> of this.

## Why it exists

flux runs on your machine with credentials in your environment. That stops working the moment you
want a team to share an integration, an agent to use it unattended, or an auditor to ask what
happened.

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
Flux supplies the language, agent loop and guarded runtime substrate. Exchange executes every
official external integration while holding the tenant's credential; neither Flux nor the connector
declaration holds one on anybody's behalf.

The boundary, one question each:

- Does it change what happens when an effect executes? → **flux**
- Is it true of the vendor regardless of who runs it? → **flux-connectors**
- Does it require holding a credential or knowing a tenant? → **flux-exchange**

## The rules this repository enforces

Three claims from the [ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
are executed here rather than merely written down. Each has tests.

**The credential never crosses the boundary; the authority does.** Outbound, a caller names an
operation and gets a result — it cannot name a host (the URL comes from the operation's own compiled
Flux), cannot name a credential (the address is derived from the session's tenant and the
connector's declared authority), and cannot name a tenant (that is read from the resolved principal
and from nothing a caller controls). Inbound, a vendor's signed payload is verified here and the
caller receives a typed, declared event. In neither direction does a caller come to hold a value it
did not already have.

**The runtime is declared by the connector, never chosen by the caller.** A caller who can pick the
runtime is a caller who can pick an effect. There is deliberately no constructor on `Runtime` that
takes caller input.

**A locally-executing runtime cannot be safely multi-tenant in one process.** HTTP is shareable
because the effect leaves the machine; process spawning, container exec and raw sockets consume this
host's own identity, network position and filesystem. So a shared deployment **refuses** them, and
the refusal names what would have worked:

```
SingleTenant
  serves:  http, socket, process, container, plugin, remote
  refuses: nothing
MultiTenant
  serves:  http, remote
  refuses: socket, process, container, plugin  (they execute on this host)
```

HTTP is the first delivered outbound runtime, not the product boundary. Every official integration
— including Docker, Kubernetes, SQL, Prometheus and other rich protocols — is moving to a connector
whose runtime is declared upstream. Exchange is the sole official-integration execution placement;
it executes or delegates those connector addresses through the X-111 program and will not grow a
second vendor-adapter catalogue. Flux contributes the guarded substrate and embeds the Exchange
client, but it has no local vendor/plugin fallback. The current generated WebSocket channel path is
the first rich-protocol slice, while general socket/process/container/plugin dispatch, streamed
results and leases remain planned work.

**A grant selects operations by declared metadata, not by name.** A grant written as a list of ids
is a list somebody maintains, and it stops covering a connector the moment that connector gains an
operation. `risk <= low` covers the new one correctly on the day it lands. An agent's token grants
access to *operations*, never to credentials — so a stolen token yields a bounded operation set
against one tenant's connections, not a vendor secret.

**Authentication hazards are admitted by declared property, not connector name.** The production
default allows none. `FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS=resource_owner_secret_shared` is an explicit
deployment-level exception for an acquisition that presents the resource owner's secret to this
host; it is read only at startup and cannot be overridden by a request. The policy is applied again
when acquisition is attempted, so a catalogue update cannot bypass a check that happened only at
boot. No released connector declares a hazard yet; upstream C-440 will make the first path live.

## What exists today

| | |
|---|---|
| `crates/exchange-host` | Principal-derived tenancy, grants, connection-instance naming, runtime admission, credential/settings/channel stores, ordinary invocation, immutable App Packages, atomic installed-App bindings and durable Event Deliveries, plus tenant-scoped workflow drafts and versions. Execution still ends in Flux and `connector_pack`; this crate holds no transport of its own. |
| `crates/exchange-server` | Health, catalogues, identity, generated labelled connection plans, connections/grants, Service Accounts, invocation, workflows, channel supervision, and installed Flux App supervision over tenant/App-isolated durable Flux event logs. It is the **only crate here that holds transports**, and deliberately never names `connector_pack` — tests assert both halves. |
| `console/` | A Vue 3 **admin surface**: declaration-driven labelled connection setup, Connect → Grant → Invoke, Workflows, Activity, Channels and Apps. The Apps surface installs the Slack-bot-style template, freezes Connection/access/model/risk/scope choices, chats with its Managed Agent and inspects activation Activity. Failed reads name their endpoint and can be retried — never an empty answer or false "signed out". |

**Not built, despite being described in the design:** rich outbound runtime-plan dispatch, webhook
channels, general channel replay,
general operation streams, isolated per-tenant workers, leases-in-anger, and runtime artifact
installation/attestation. Stored workflows,
workflow execution records, generated WebSocket channels and installed App inboxes moved off this list in X-98, X-101 and X-108.
The credential store has moved off this
list and is described below, and X-47 moved
per-connection configuration off it too — but the honest replacement claim is narrower than "done":
a tenant can now **supply**, through the bounded operator-onboarding transport, every admitted
catalogue-declared connection value — and
**four are refused on purpose**: `asterisk`, `okta`, `docusign` and `freshdesk` template their whole
destination authority, so a tenant-supplied value would *be* the origin this host sends their
credential to. Those four stay uninvocable and say so. The design is ahead of the code
on purpose; the gap is stated here so nobody has to discover it.

### Generated WebSocket channels are live and fail closed

`FLUX_EXCHANGE_CHANNELS` names the owner-only persistent channel file. With that store, the
credential store and the grant store bound, a signed-in human can create, edit and remove a channel
from the console. The mutation names only a catalogue connector, an operator connection label, one
of its generated socket bindings and a closed event subset. The host resolves that mutable label to
the held instance's immutable id inside the principal's tenant; no tenant, UUID, endpoint,
credential address or placement is accepted from the request body. Renaming the connection changes
what the channel displays without retargeting it, and deleting a connection is refused while a
durable channel still binds it.

The vendor socket is supervised independently of subscribers and restored after restart. An
authenticated `GET /api/subscribe` WebSocket multiplexes opaque channel ids and returns
request-correlated acknowledgements or non-enumerating refusals. Events are live and at-most-once:
each subscriber has a bounded queue, a slow subscriber is disconnected without stopping the vendor
channel, and there is deliberately no cursor, replay or retained payload inbox.

The built-in composition admits local socket execution only when `--dev` selected
`Deployment::SingleTenant`. With OIDC or an explicit development roster it is multi-tenant and
channel placement refuses before credentials are read; a product embedding `exchange-host` must
bind its own operator-selected remote placement to run those channels safely.

### Service Accounts are the non-human API identity

`POST /api/service-accounts` accepts only the Service Account's non-secret id and expiry and returns
exactly one FXSA binary handoff body rather than a JSON credential. `GET /api/service-accounts` lists
ids and expiries without token or verifier material; `DELETE /api/service-accounts/{id}` revokes one.
The host keeps only
`SHA-256(token)` in the owner-only file named by `FLUX_EXCHANGE_SERVICE_ACCOUNTS`.

The token is presented as `Authorization: Bearer …` and resolves to `kind: service_account` in its
original tenant until expiry or revocation. Authentication grants nothing by itself: the same
metadata-selected grants bound invocation and inbound subscriptions, and a Service Account cannot
edit connections, settings or grants, create another principal, or read a credential.

The v0.16 `POST /api/agents`, `FLUX_EXCHANGE_AGENTS`, `agent` principal and `#/agents` compatibility
spellings are removed in v0.17. Existing unprefixed tokens keep resolving from the unchanged
verifier-keyed store without rewriting stored material. The
[migration design](docs/designs/service-accounts.md) records the completed checkpoint.

An authenticated `GET /api/catalogue/effective` is the non-human discovery surface. It returns only
operations for connectors this tenant has connected, whose required non-secret settings are
present, and that one of this tenant's grants admits.
Each operation carries its declared input schema and the existing tenant-local connection label to
bind when invoking; it carries no tenant, credential address, endpoint, runtime or instance UUID.
The top-level `generation` is a SHA-256 content identity over that complete projection: identical
content is stable across requests and restarts, while a relevant declaration, connection or grant
change produces a new value.

### The credential store, and what does not protect it

`exchange_host::CredentialStore` binds the portable file-backed store from `connector-secrets`
rather than reimplementing one. Exchange consumes its Linux implementation: it creates a `0600`
file below an owner-only directory and checks the effective owner, object kind and modes every time
the store opens. The provider remains portable, but non-Linux provider support is not an Exchange
server or release claim. Native local startup also walks the complete path from the authenticated
account boundary: symlinks, foreign ownership and an ancestor writable by an untrusted account
refuse. Unsafe existing metadata is never quietly narrowed — the
object already had that metadata while it held values, so repairing it would hide the exposure
instead of reporting it. A path inside a working tree is refused outright, because a credential
under a checkout is one `git add -A` from being committed. A write is a whole-file
rewrite through a sibling temporary, `fsync` and `rename(2)`, so a crash mid-write leaves the
previous file whole rather than truncated, and a delete rewrites immediately, so a revoked
credential does not come back on restart.

**What protects a value there is the platform filesystem boundary and nothing else.** There is no
encryption at rest, no passphrase, no OS keychain integration, and no protection from Linux `root`
or a backup that copies the file. That makes it the right store for a
single-operator deployment and the wrong one
for a shared machine, where `connector-secrets`' Vault-backed store is the answer. Nothing ever
silently selects the in-memory store instead: a configuration naming no path is a **startup error**
naming what would have worked, because a host that fell back would start, serve every route
correctly, look exactly like a working one, and lose every credential on restart.

To decommission a store, remove the **directory**, not the file — a write interrupted between the
`fsync` and the `rename(2)` can leave a complete copy of every credential in a sibling temporary
that `rm` on the store file alone does not touch.

The binary binds it when `FLUX_EXCHANGE_CREDENTIALS` names a path; unset, the connection routes
refuse and name the setting rather than pretending a store exists.

### Several connections to one connector

`FLUX_EXCHANGE_CONNECTIONS` names the durable label-to-UUID registry. It contains operator labels
and host-minted UUIDs, never credential or setting values, and its path must be outside every working
tree. With no registry bound, the source-compatible sole legacy connection surface remains live.

A signed-in human labels an existing sole connection with
`PUT /api/connections/{connector}/label`, then creates another at
`POST /api/connections/{connector}/instances/{label}`. Management, settings and credential rotation
have matching label-scoped resources. Invocation uses
`POST /api/operations/{operation}/invoke?connection={label}`; the JSON body remains exactly the
operation's parameter object. Omitting the label is valid only when the tenant holds exactly one
connection; zero is `disconnected`, and several are `ambiguous_connection` rather than a guessed
default.

Existence comes from credential addresses, not from this registry. The file store can enumerate a
tenant/authority scope and apply the first-to-second address migration as one checked atomic batch.
A backend that cannot prove both operations—currently including the Vault binding—continues to
support sole legacy connections but refuses plural management. Exchange never falls back to a
point-by-point credential move.

## Try it

```bash
cargo run -- --dev              # user:${USER}@dev on 127.0.0.1:8080; no OIDC setup
cargo run -- local-user-secret alice acme  # mint a reachable-safe local login once
cargo test --workspace
cd console && npm install && npm run dev
```

`--dev` belongs to the binary, so Cargo's first `--` is the argument-forwarding boundary. An
explicit `FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme,...` roster remains available when local work
needs named tenants or more than one principal. With `--dev`, follow **Sign in** and choose
**Continue as the local development user**; the host establishes the sole implied user's browser
session and returns to the console.

### Flux-supervised local launch

`flux-exchange compatibility --json` is the side-effect-free release/protocol query: it opens no
store and binds no listener. The [verified local binary release runbook](docs/local-binary-releases.md)
documents its exact JSON identity, two supported Linux targets, fixed signed channel origin, offline-root
and delegated-signer trust, monotonic rollback/expiry behavior, and equivalent online/offline
verification paths. The server archives are separate Exchange product artifacts—not crates.io
artifacts, Flux release artifacts, official integration plugins or connector runtimes—and the
runbook does not treat a staged workflow as proof that the production channel is live.

`--supervised` is a separate machine-only launch mode for the Flux
local supervisor. It accepts only an OS-selected loopback port (`FLUX_EXCHANGE_BIND` may be absent
or exactly `127.0.0.1:0` or `[::1]:0`) and emits one bounded canonical
readiness object after every store/safety check and the one listener bind have succeeded. Its
`exchange.supervisor-ready.v2` identity carries the complete eight-protocol compatibility inventory
described by the release runbook. The record goes only to the inherited
one-shot readiness capability; stdout and stderr remain ordinary process output, the HTTP listener
carries application traffic, and later control is not part of this channel.

On Linux the complete ABI is `flux-exchange --supervised`: FD 3 is the readiness pipe's write end and
FD 4 is the liveness pipe's read end, with no other inherited nonstandard descriptor. A native
thread exits the process without unwinding when liveness reaches EOF,
receives a byte or fails, so supervisor death cannot leave Exchange or its port behind even when the
async runtime is wedged. Readiness is not logged or copied to HTTP, no PID file is emitted, and
`/health` becomes useful only after Flux has validated the one-shot process/start identity.

Production discovers the default root from the authenticated operating-system account, not from an
inherited process environment. Linux calls `getpwuid_r(geteuid())` and uses
`.local/state/flux-exchange` below that account's home. `HOME`, `XDG_STATE_HOME` and equivalent
inherited variables do not participate. `FLUX_EXCHANGE_STATE` remains
an explicit operator override, and every individual storage setting remains authoritative, but a
non-development persistent composition is all-or-nothing: setting only some of credentials,
settings, grants, connections, channels, workflows, audit and Service Accounts refuses before the
server listens and names every missing sibling.

The complete path is validated before use. Linux rejects symlinks, a foreign owner, a root or
descendant wider than owner-only, and any shared ancestor writable by an untrusted account; new
Exchange roots are `0700` and files are `0600`. Existing unsafe metadata is refused and never has
its modes narrowed, ownership changed or any other repair applied. In particular, an explicit store directly below a shared
directory such as `/tmp` is
unsafe; create an owner-only child and place the store below it instead of narrowing the shared
ancestor.

The owner bootstrap exists only inside this supervised, single-user composition. It is an
owner-only Unix socket below the native state root. The server authenticates the peer with
`SO_PEERCRED` as the same operating-system account and
maps that fact to local
operator authority only inside the native-management dispatcher. It never installs that account as
an HTTP identity, so loopback HTTP, hosted operation routes, another account and `--dev` cannot
reproduce the bootstrap. The endpoint is also not readiness, liveness or lifecycle control: those
remain separate, value-free supervisor capabilities.

The verified `flux-exchange` executable, not Flux, mediates native secret entry. Flux supplies a
closed, non-secret request capability; the helper re-derives and authenticates the owner endpoint,
reads a requested vendor value directly from `/dev/tty`, and sends it only to
the running Exchange process. Flux receives one value-free receipt or refusal, never the secret,
its transaction identity or its requested-field ordering. If no secret is required, the helper does
not open a terminal. Service Account mint uses a distinct one-way writer capability: Exchange sends
the one-time credential directly to that writer while ordinary JSON, argv, environment, stdout,
stderr, readiness, liveness and diagnostics remain value-free.

The supported local targets are exactly `aarch64-unknown-linux-gnu` and
`x86_64-unknown-linux-gnu`. CI compiles and exercises the Linux owner-bound runtime natively on each
target; cross-compilation is not treated as runtime support. A non-Linux server build refuses in its
build script, and the release tooling refuses a target outside this set before staging or signing.

For a reachable self-hosted console without OIDC, put the generator's JSON entry in an owner-only
file (`0600`) and set
`FLUX_EXCHANGE_LOCAL_USERS` to it. The file stores only a verifier; the opaque
secret is shown once and submitted through `/api/signin`'s same-origin form. This is a separate
secret-backed identity state, not a relaxation of the loopback-only development roster.

Rust 1.88 or newer — the floor `Cargo.toml`'s `rust-version` states. ⚠ *This said 1.87 through
three releases and was false the whole time; `jsonwebtoken` and `time` both require 1.88. X-30
corrected the manifest and this line was missed.*

## Embedding it in your own product

`codewandler-flux-exchange-host` publishes the host as a **crate**, not only a binary. Identity, the
secret store, the transport and the runtime registry are all **ports**, so a product composes them
into its own service with its own identity provider and its own runtimes — without forking anything,
and without that product's concerns reaching the shared code.

One implementation ships behind one of those ports: `CredentialStore`, the file-backed store above.
It is a default, not a fixture — the port is `SecretStore`, a deployment that wants Vault or its own
backend binds that instead and never constructs the type, and the store it wraps comes from
`connector-secrets`, a flux-family crate, rather than from any product.

That is the point of the boundary, not a side effect of it: the public crate has no **downstream**
dependency to leak through. Traits are how that is kept true, and a default a composing binary can
decline does not spend it.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
