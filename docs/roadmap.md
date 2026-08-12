# flux-exchange — roadmap & status

What is delivered, what is next, and the epics that group the work. The operational detail lives on
the [board](stories/README.md); this document is the narrative around it.

For work named by `../flux-roadmap/programs/`, the cross-repository schedule and Decision 0001 take
precedence over this local narrative. Repo-local story Goal and Acceptance remain the definition of
done.

## Status

_As of 2026-08-12:_ **v0.18.1 — the catalog-artifact adoption line: document-backed settings, the served catalogue pack and
instance-bound channels.**
`cargo run -- --dev` is the loopback single-tenant shorthand; reachable binds still require a real
identity provider. Complete OIDC sign-in, per-tenant connections and settings, metadata grants,
ordinary connector invocation, immutable workflow publication, durable value-free workflow run
records, multiple labelled connections per connector with explicit invocation selection, canonical
Service Account lifecycle and bearer identity, persistent generated WebSocket
channels and authenticated live subscriptions are built.
The authenticated effective Service Account catalogue, webhook channels, durable event
replay/inboxes and leases-in-anger remain unbuilt.

`crates/exchange-host` carries the vocabulary and rules, the credential/settings/grant bindings,
ordinary invocation and the tenant workflow runtime. A workflow is a stored Flux program rather
than another interpreter: publication freezes operation contracts and execution repeats both its
entry grant and every nested connector grant before credential resolution.

`crates/exchange-server` composes the transports, identity and durable stores. `console/` guides an
operator through Connect → Grant → Invoke, provides Workflows and Activity views backed by the
upstream Flux editor schema, and manages the declared event subsets of persistent Channels.

**The credential never crosses the boundary; the authority does.**

## The engine line

`connector-pack` 0.19 and the engine crates are aligned on Flux 0.54 for X-125. They remain one atomic
pin set: two engine versions are two incompatible `Tool` traits even when their names are identical.
The manifest, resolved-lock and compile-time seam tests keep that rule executable.

## Epics

### Rich connector runtimes through Exchange

HTTP invocation and generated connector WebSocket channels are delivered slices, not the hosted
boundary. The accepted family direction moves Docker, Kubernetes, SQL, observability, secret stores,
collaboration tools, and every other official integration into flux-connectors. Exchange executes
every declared connector address under tenant-derived authority; it does not invent its own vendor
request or adapter path. Flux contributes guarded substrate and an embedded client, not a second
official execution placement or fallback.

[[X-111]] tracks the program and [[X-124]] fixes its cross-repository execution contract. The first
independently shippable milestone is [[X-113]]: an authenticated effective Service Account catalogue
with stable generation identity beside the existing one-shot HTTP invoke. Streams, cancellation and
terminal outcomes stay in [[X-117]], and leases stay in [[X-118]], so neither blocks that useful path.

After the HTTP milestone, [[X-114]] dispatches connector-declared runtime plans through Exchange;
[[X-115]] binds Flux's guarded substrate in local single-tenant Exchange; [[X-119]] installs only
digest-pinned connector artifacts through the connector/Exchange pipeline; and [[X-120]] runs the
accumulated migration corpus through that local Exchange. [[X-116]] separately adds fail-closed
per-tenant isolation for a hosted multi-tenant deployment and does not block the local proof.
Delivered [[X-107]] Service Account authentication and X-101–X-105 channel work are prerequisites,
not duplicate stories. Design:
[`docs/designs/rich-connector-runtimes.md`](designs/rich-connector-runtimes.md).

### A public documentation site

`flux` and `flux-connectors` each publish a VitePress site from `web/`; this repository publishes
nothing, so an evaluator can read what the engine does and browse the catalogue, and then hits a gap
exactly where *"what holds my credentials, and what will it let an agent do?"* gets answered.

The brief is to scaffold the **whole** future surface — channels, `subscribe`, leases, workflows —
not just what is built. That runs straight into principle 7, and it is worth being blunt about why:
this repository corrected **five renderings of one false claim** in a single week, each written
honestly, each stale within a release. A docs site is a factory for that failure.

So the epic is ordered around the mechanism rather than the volume. [[X-63]] is the scaffold and the
pipeline, matched to flux-connectors. [[X-64]] makes every page's status **derived** from the same
descriptor artifact whose `live` flags X-42 and X-52 hold to the route table — the one honesty device
here that has survived review. Only then do [[X-65]] and [[X-66]] add the surface and the argument.

Done looks like: a visitor reaches a public URL, understands why the credential never crosses the
boundary, sees the whole intended platform including channels and workflows, and can tell built from
planned at a glance — with that answer coming from the route table rather than from an author's
memory.

Design: [`docs/designs/public-docs-site.md`](designs/public-docs-site.md).


### Local identity

**You cannot use this console without standing up an OIDC provider.** Everything shipped through
v0.8.0 is reachable only by a signed-in principal, and the only thing that makes a principal is an
authorization-code flow against a configured provider. That is a lot of setup to look at a page, and
it stands between this platform and its own operators.

The wall is not where it looks. A development identity already exists and works; `SignIn::available()`
simply returns `true` only for OIDC, so a host with that identity armed tells the console it cannot
sign anyone in. That conflation is [[X-57]] and it is priority 0 — nothing else in the epic is
reachable until it lands.

The rest is two **orthogonal axes**, which the original request framed as alternatives:
authentication ([[X-58]] — users from a config file, carrying a real verifier so the binding may
listen on a reachable address) and tenancy ([[X-59]] — one tenant, named at startup, with the
credential address unchanged). Authentication is what unblocks the console; tenancy is a convenience.

Done looks like: clone, write one config file, start the server on your own network, open the
console, sign in, wire up a connection, invoke an operation — with no identity provider, and with no
mode in which authentication is a name anybody can guess.

Design: [`docs/designs/local-identity.md`](designs/local-identity.md).


### A deployment a stranger can reach

Everything this platform does can only be seen on `127.0.0.1`. [[X-69]]'s page walks a reader through
`cargo run`, a roster handle and a console on localhost, and that is the entire demonstrable surface;
[[X-63]]'s site can now *describe* the platform to an evaluator and nothing lets them use it.

Owner-raised 2026-08-02: deploy to fly.io so it can be used and tested end to end, remotely. Three
things stand in the way and only one is packaging. **A reachable bind needs a bound identity**, and
`main.rs:394` is the sole route to one — the development roster is refused by name, with its own
refusal variant, because a roster handle is a name anybody can guess. Owner-decided the same day:
stand up a real OIDC provider, which needs no Rust change at all. **The console has no production
host**, and cannot be given one on another origin: it addresses the API by same-origin relative paths
and the session cookie is `SameSite=Strict`, so a browser would never attach it cross-origin. That is
[[X-83]] and it looks like a CORS problem while being nothing of the kind. **Nothing containerises
this** — [[X-84]] — and no flux-family repository has ever deployed, so what that story writes is the
precedent `flux` and `flux-connectors` copy.

One machine, deliberately: the credential store fsyncs the whole file under a single mutex and a fly
volume attaches to one machine, so two machines is two divergent credential stores with no
reconciliation.

The half most likely to be skipped is the operator's first five minutes. X-13's grant gate is
fail-closed and **will look like an outage** — `503` with no store bound, `403 not_granted` with one
bound and empty. A URL shipped without a path through that is a working platform that appears broken.

Done looks like: a stranger opens the public URL, signs in, connects a connector, writes a grant,
invokes an operation and reads the result — with no checkout, and with no fail-closed gate weakened to
get there.

Design: [`docs/designs/remote-deployment.md`](designs/remote-deployment.md).


### Credential acquisition, and a labelled weak one

Every credential this service has ever held arrived the same way: **a human pasted it in.**
`Acquisition` ships one value, `Static`, and babelforce's own `[[auth]]` block spells out the
consequence — the token is *"minted outside flux and supplied through the environment"*. A babelforce
user has an email address and a password, not a token. So the connector with 389 catalogued
operations is the one nobody can connect.

Owner-raised 2026-08-01: use the **OAuth2 password grant**, and mark it as the weaker thing it is.
The marking is the interesting half, because it is principle 3 turned on authentication — *select by
declared metadata, not by name*. A deployment forbidding password-grant authentication says so once,
about a property, rather than keeping a list of connector names that is wrong the moment the
catalogue grows a 55th provider.

The hazard is a **kind**, not a level: [[X-73]] adds `AuthHazard::ResourceOwnerSecretShared`, citing
RFC 9700 §2.4 (which says the grant MUST NOT be used, and why) and CWE-522 — a fifth `Risk` rung would
make `Selector::at_most(High)` silently admit it, because a password grant buying a read-only token is
`Risk::Low` *and* hazardous. [[X-74]] is the opt-in filter and lands **before** [[X-75]] performs the
grant, for the reason X-40 preceded X-37.

[[X-76]] is the rule that keeps the vocabulary from bloating, and it earned its place by being wrong
first. It was filed as *"the owner says a TTL parameter exists, the specification has no such field,
ask the API owners"* — and the vendor's implementation says both halves were true: `expires_in` is
read straight out of `params`, which is why no generated document shows it, **with different meaning
on every grant** (a hard cap on one, ignored on another, the difference between an hour and forever on
a third). Plus `account_id` on a refresh, which switches the account. Owner-decided 2026-08-02:
**a behaviour no document declares is a quirk of one endpoint, never a field the other fifty-three
providers are assumed to honour.**

Nothing here adds an operation. *An authentication endpoint is never a connector operation* is
owner-stated and upstream-enforced; the manifest declares and this host performs, which is what
flux-connectors' authentication contract already says the host is for. The declaration is **C-440**
in that repository.

Done looks like: an operator connects babelforce with a username and a password on a deployment that
explicitly opted in; what is stored is a token with an expiry; the password is on no disk and in no
log; and the same attempt without the opt-in is refused by name before a request leaves the process.

Design: [`docs/designs/credential-acquisition.md`](designs/credential-acquisition.md).


### The HTTP surface — X-01 · ✅ **DONE**

Turn the binary that prints a matrix into a service. The load-bearing story is **X-02**, and it is
load-bearing for one reason: a credential-holding service that starts on a reachable address without
a way to authenticate is not a bug to fix later. flux's own server takes the same position — a
non-loopback bind without a token is refused *at startup*, because the daemon auto-approves tools and
an open listener is RCE. Substitute credentials for tools and the argument is unchanged.

**X-03** is where the north star stops being prose. The tenant must come from the resolved principal
and from nothing a caller controls, and the story asks for that asserted three times — once for a
path segment, once for a body field, once for a header.

### The catalogue surface — X-05 · ✅ **DONE** (X-46 open on the same slug)

Serve what exists, so the console stops rendering fixtures. Unblocked by construction:
`connector-catalog` is static data with no dependencies, no IO and no runtime.

Two decisions worth making deliberately rather than by accident. **The catalogue must carry `risk`,
`effects` and `idempotency`** — without them nothing but the server can predict what a `Selector`
admits, and the grant model becomes folklore. And **it must not be silently filtered by grant**: an
agent that cannot see an operation it lacks cannot report that it was refused.

### Connections and credentials — X-08, X-14 · ✅ **LIVE** (X-21 open on the same slug)

An operator connects a provider; a tenant's credentials are reachable only by that tenant.

**X-09 is the story most likely to be got wrong in a comfortable direction.** Every acceptance
criterion on it is a refusal: a widened file mode is refused rather than repaired, a store path
inside the working directory is refused, a bad store configuration is a startup error with no
fallback to memory. The last one matters most — a host that fell back would start successfully,
serve every route correctly, look exactly like a working one, and lose everything on the next
restart.

X-14 adds several labelled instances of one connector without changing the sole legacy address.
The host mints the UUID, existence remains derived from credentials, migrations are checked atomic
batches, and invocation refuses ambiguity rather than choosing an account. X-122 follows with the
separate durable binding generated channels need to survive connection-label renames.

### Service Account access — X-35, X-107 · ✅ **LIVE** (legacy spelling removal X-121)

The old Agent-access stories delivered the durable non-human principal in slices: one-time token
minting, human-only lifecycle control, bearer authentication, listing and revocation. X-107 gives
that resource its canonical name: **Service Account**. It authenticates an API caller; it is not the
model + loop + bounded capabilities that Flux calls an Agent.

A Service Account receives no authority merely by existing. Invocation and inbound subscriptions
remain bounded by the tenant's grants, selected from declared metadata, and a token never yields a
credential. The v0.16 Agent-named aliases are removed in v0.17 without invalidating verifier-keyed
tokens. See
[`docs/designs/service-accounts.md`](designs/service-accounts.md).

### Machine onboarding — X-41, X-42 · ✅ **LIVE**

The anonymous onboarding descriptor and public capability pages tell an App, Agent or automation
what this deployment can do without exposing deployment-specific or credential-shaped data. The
descriptor derives capability status from the same declared surface the server exposes, including
canonical Service Account creation and bearer authentication.

This is discovery, not authority: learning that a capability exists does not grant it. An installed
App and hosted Agent remain target architecture, defined in [`docs/concepts.md`](concepts.md).

### Invoke — X-11…X-14 · ✅ **LIVE**

Where the confused-deputy answer becomes code: the caller names an operation id and, when several
connections exist, one tenant-scoped operator label. Nothing else about the request is theirs to
choose. Not the host — the URL comes from the operation's own compiled Flux. Not the credential —
the address is derived. Not the tenant — it comes from the resolved principal.

**X-12's hardest criterion is structural, not behavioural:** a test fails if a second
request-building path ever appears. This host constructs no request of its own, and that is the
property that keeps it from becoming the credential-injecting proxy the family already rejected.

## Transport, decided upstream

Recorded here because it is an architectural commitment this repository will be held to, not a
preference to be re-litigated per story:

- **HTTP for everything one-shot** — catalogue reads, connection and credential management,
  stateless `invoke`, the whole management surface. That is what exists today.
- **One websocket per connected agent** for the three things that do not fit request/response, which
  are all the same shape — a long-lived authenticated bidirectional frame stream: inbound events
  (`subscribe`), streamed operation output (`logs -f`, process stdout, a socket read loop), and
  **lease liveness**, because this host must learn that a holder died in order to release what it is
  holding for them.

Flux's embedded Exchange binding consumes both transports. `flux-channels` already has a generic
`connector` channel kind, and its Exchange-backed mode opens the authenticated stream instead of
binding a vendor listener. Event names still come from the connector manifest, so `trigger { on =
… }` is unchanged; the placement is always Exchange for an official external integration.

## The formerly unfiled platform work is now owned

Generated `subscribe` shipped in X-101–X-105 and workflows plus execution records shipped in X-98.
The remaining general work is no longer an unscoped direction: the effective catalogue and one-shot
HTTP contract are X-113; streams and cancellation are X-117; leases are X-118; rich runtime dispatch
and placement are X-114–X-116; artifact trust is X-119; and X-120 holds the local single-tenant
migration-corpus proof. Webhook/poll hosting and durable replay remain outside this runtime epic and
still require their own designs before implementation.

**Current dependency boundary, 2026-08-04.** connector v0.19 and Flux v0.54.4 move as one registry-only
graph. `connector-pack` and `flux-runtime` are ordinary host dependencies because Exchange executes
operations. The manifest, resolved lock and compile-time seam tests prove one engine line; neither a
path nor a Git override participates in a release.
