# flux-exchange

The platform layer of the [flux](https://github.com/codewandler/flux) family: a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.

Its primary caller is an **agent**, not a human. People sign in to wire things up and to see what
happened; agents are what call operations all day. That inverts the usual assumption and shapes
everything below.

The contributor and operator [security posture](docs/security.md) maps the threat model, enforced
controls, deployment assumptions, known limitations, security roadmap and incident checklist.
The [domain vocabulary](docs/concepts.md) gives Connector, Connection, Datasource, Trigger, App,
Managed Agent and Service Account one meaning across the Flux family and labels which bindings are
live versus target architecture.

> [!WARNING]
> **Status: v0.16.0 — Service Accounts, gated operations, versioned workflows, and generated channels.**
>
> `cargo run -- --dev` binds `127.0.0.1:8080`, derives `user:${USER}@dev`, and serves health, the
> connector catalogue and a session without OIDC setup. The ordinary composition supports a
> **complete** OIDC sign-in and refuses to start on a reachable address while no identity provider
> is configured. The authorization code is redeemed back-channel and the id token's signature is
> verified against the provider's published keys, so `/api/signin` redirects to a real provider —
> configure the eight `FLUX_EXCHANGE_OIDC_*` variables and it works end to end. Connections can be
> created, listed, **rotated** and deleted per tenant. A signed-in human can create, list and revoke
> **Service Accounts**, whose `fxsa_…` bearer tokens authenticate at the same guarded API boundary
> as browser sessions. **`invoke` runs**: `POST /api/operations/{operation}/invoke` executes one
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
>
> See [What exists today](#what-exists-today) for the honest inventory before planning around any
> of this.

## Why it exists

flux runs on your machine with credentials in your environment. That stops working the moment you
want a team to share an integration, an agent to use it unattended, or an auditor to ask what
happened.

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
flux runs it. Neither of them holds a credential on anybody's behalf, and neither should — that is
a third job, and this is it.

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
whose runtime is declared upstream. This service will execute or delegate those same connector
addresses through the X-111 program; it will not grow a second Exchange-specific adapter catalogue.
The current generated WebSocket channel path is the first hosted rich-protocol slice, while general
socket/process/container/plugin dispatch, streamed results and leases remain planned work.

**A grant selects operations by declared metadata, not by name.** A grant written as a list of ids
is a list somebody maintains, and it stops covering a connector the moment that connector gains an
operation. `risk <= low` covers the new one correctly on the day it lands. An agent's token grants
access to *operations*, never to credentials — so a stolen token yields a bounded operation set
against one tenant's connections, not a vendor secret.

## What exists today

| | |
|---|---|
| `crates/exchange-host` | Principal-derived tenancy, grants, runtime admission, credential/settings/channel stores, ordinary invocation, zero-I/O generated channel planning, and tenant-scoped workflow drafts plus immutable published versions. Workflow and channel execution still end in Flux and `connector_pack`; this crate holds no transport of its own. |
| `crates/exchange-server` | Health, catalogue, complete OIDC sign-in, per-tenant connections and grants, Service Account lifecycle and bearer authentication, ordinary invocation, workflow authoring/publication, durable SQLite run records and typed 30-day audit evidence, channel supervision and authenticated live event fan-out. It is the **only crate here that holds transports**, and deliberately never names `connector_pack` — tests assert both halves. |
| `console/` | A Vue 3 **admin surface**, not a catalogue browser: Connect → Grant → Invoke plus Workflows, Activity and Channels. The workflow editor uses the upstream Flux graph contract, protects unsaved drafts, retains exact source, and paints durable value-free run events back onto nodes. Failed reads name their endpoint and can be retried — never an empty answer or false "signed out". |

**Not built, despite being described in the design:** rich outbound runtime-plan dispatch, a second connection to one
connector (the address has no instance dimension until upstream publishes one), webhook channels,
durable event replay/inboxes, general operation streams, isolated per-tenant workers,
leases-in-anger, runtime artifact installation/attestation, and the catalogue loader. Stored workflows,
workflow execution records and generated WebSocket channels moved off this list in X-98 and X-101.
The credential store has moved off this
list and is described below, and X-47 moved
per-connection configuration off it too — but the honest replacement claim is narrower than "done":
a tenant can now **supply**, over HTTP, every admitted catalogue-declared connection value — and
**four are refused on purpose**: `asterisk`, `okta`, `docusign` and `freshdesk` template their whole
destination authority, so a tenant-supplied value would *be* the origin this host sends their
credential to. Those four stay uninvocable and say so. The design is ahead of the code
on purpose; the gap is stated here so nobody has to discover it.

### Generated WebSocket channels are live and fail closed

`FLUX_EXCHANGE_CHANNELS` names the owner-only persistent channel file. With that store, the
credential store and the grant store bound, a signed-in human can create, edit and remove a channel
from the console. The mutation names only a catalogue connector, one of its generated socket
bindings and a closed event subset. Tenant, connection, endpoint, credential and placement are all
derived or operator-owned; none is accepted from the request body.

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

`POST /api/service-accounts` creates a Service Account for the authenticated human's tenant and
returns an `fxsa_…` token **once**. `GET /api/service-accounts` lists ids and expiries without token
or verifier material; `DELETE /api/service-accounts/{id}` revokes one. The host keeps only
`SHA-256(token)` in the owner-only file named by `FLUX_EXCHANGE_SERVICE_ACCOUNTS`.

The token is presented as `Authorization: Bearer …` and resolves to `kind: service_account` in its
original tenant until expiry or revocation. Authentication grants nothing by itself: the same
metadata-selected grants bound invocation and inbound subscriptions, and a Service Account cannot
edit connections, settings or grants, create another principal, or read a credential.

For v0.16 only, `POST /api/agents` and `FLUX_EXCHANGE_AGENTS` remain compatibility spellings. The
route emits deprecation, successor and v0.17-removal headers; differing canonical and legacy store
paths refuse startup. Existing unprefixed tokens keep resolving without rewriting stored material.
The [migration design](docs/designs/service-accounts.md) owns the removal checkpoint.

### The credential store, and what does not protect it

`exchange_host::CredentialStore` binds the file-backed store from `connector-secrets` rather than
reimplementing one: a `0600` file in a `0700` directory, both modes set in the `open(2)`/`mkdir(2)`
call and **re-checked every time the store is opened**. A widened mode is refused, never quietly
tightened — the file already had that mode while it held values, so tightening it would hide the
exposure instead of reporting it. A path inside a working tree is refused outright, because a
credential under a checkout is one `git add -A` from being committed — and the path is resolved
through every symlink and every `..` before that check, so what is inspected is where the store
would land rather than how it was spelt. A write is a whole-file
rewrite through a sibling temporary, `fsync` and `rename(2)`, so a crash mid-write leaves the
previous file whole rather than truncated, and a delete rewrites immediately, so a revoked
credential does not come back on restart.

**What protects a value there is that file mode and nothing else.** There is no encryption at rest,
no passphrase, no OS keychain integration, and no protection from `root` or from a backup that
copies the file. That makes it the right store for a single-operator deployment and the wrong one
for a shared machine, where `connector-secrets`' Vault-backed store is the answer. Nothing ever
silently selects the in-memory store instead: a configuration naming no path is a **startup error**
naming what would have worked, because a host that fell back would start, serve every route
correctly, look exactly like a working one, and lose every credential on restart.

To decommission a store, remove the **directory**, not the file — a write interrupted between the
`fsync` and the `rename(2)` can leave a complete copy of every credential in a sibling temporary
that `rm` on the store file alone does not touch.

The binary binds it when `FLUX_EXCHANGE_CREDENTIALS` names a path; unset, the connection routes
refuse and name the setting rather than pretending a store exists.

## Try it

```bash
cargo run -- --dev              # user:${USER}@dev on 127.0.0.1:8080; no OIDC setup
cargo test --workspace
cd console && npm install && npm run dev
```

`--dev` belongs to the binary, so Cargo's first `--` is the argument-forwarding boundary. An
explicit `FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme,...` roster remains available when local work
needs named tenants or more than one principal. With `--dev`, follow **Sign in** and choose
**Continue as the local development user**; the host establishes the sole implied user's browser
session and returns to the console.

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
