# flux-exchange — roadmap & status

What is delivered, what is next, and the epics that group the work. The operational detail lives on
the [board](stories/README.md); this document is the narrative around it.

## Status

_As of 2026-08-01:_ **v0.4.0 — a platform that holds credentials and can sign a human in.**
`cargo run` binds loopback and refuses a reachable address with no identity provider. Complete OIDC
sign-in, sessions that end when the identity behind them does, a per-tenant connection surface with
credential bounds, and the connector catalogue. **It still executes nothing** — `invoke`,
`subscribe` and execution records are unbuilt, so the platform can be wired up but not yet used.

`crates/exchange-host` carries the vocabulary and the rules as tested types (32 tests):
`Principal`/`Tenant`, `Grant`/`Selector`, `Runtime`/`Deployment`, `Lease`, and the `Identity` port.
Four rules are executed rather than described — a traversing tenant id refused at construction, a
multi-tenant deployment refusing every locally-executing runtime, deny beating allow in a selector,
and a lease requiring the same principal rather than merely the same tenant.

`crates/exchange-server` prints which runtimes each deployment shape would serve, and exits.
`console/` reads the live catalogue from the service; the fixture banner is gone with the fixtures.

**It holds credentials, binds a port and answers requests. It does not yet run an operation.**

## The blocker, and what it does not block

`codewandler-connector-pack` 0.8.0 requires `codewandler-flux-runtime ^0.41`. For a `0.x` crate
Cargo reads that as `>=0.41.0, <0.42.0`, and the flux family is at **0.45.0**. Because
`connector_pack::pack` hands out `Arc<dyn flux_runtime::Tool>`, two engine versions are two
incompatible traits — so this repository cannot link the pack and current flux together.

**That blocks the `invoke` epic and nothing else.** Measured on crates.io the same day:
`connector-catalog` has **zero dependencies**, and `connector-spec` and `connector-secrets` carry
**no flux dependency at all**. The HTTP surface, sign-in, the catalogue and the credential store are
buildable today, which is why the first wave is eight stories rather than one.

X-11 tracks the alignment. The work is upstream, in flux-connectors.

## Epics

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

### Connections and credentials — X-08 · ✅ **DONE** (X-14, X-21 open on the same slug)

An operator connects a provider; a tenant's credentials are reachable only by that tenant.

**X-09 is the story most likely to be got wrong in a comfortable direction.** Every acceptance
criterion on it is a refusal: a widened file mode is refused rather than repaired, a store path
inside the working directory is refused, a bad store configuration is a startup error with no
fallback to memory. The last one matters most — a host that fell back would start successfully,
serve every route correctly, look exactly like a working one, and lose everything on the next
restart.

### Agent access — X-35 · 🔄 **X-36, X-40 done; X-37, X-38, X-45 open**

The charter's second sentence calls the agent the **primary caller**, and for most of this project's
life `PrincipalKind::Agent` existed as a type that nothing could produce: the only ways to become a
principal were federated sign-in (a human) and the loopback development roster.

X-36 made minting real — a token shown once, with the store keeping only a digest — and then found
the hole in itself: nothing gated *who* may mint, so a leaked agent token would mint successors and
revocation would stop being a remedy invisibly. X-40 closed that before X-37 could make it
reachable, and refused `Service` as well as `Agent`, because the property holds only if every minter
is itself revocable by this operator.

**Still open, and stated plainly rather than implied:** an agent token **authenticates nothing yet**
(X-37), and it authorises nothing beyond any principal until grants land (X-13). See
[`docs/designs/agent-access.md`](designs/agent-access.md).

### Agent onboarding — X-41, X-42 · 🔄 **READY**

The charter's second sentence calls the agent the **primary caller**, and everything built so far
serves the other one. A human can sign in, wire up a connection and read a catalogue; **an agent
arriving at this service is told nothing** — no page, no descriptor, no route answers "what is this
and how do I connect to it". X-36 made it possible to mint an agent principal and hand it a token,
and nothing tells anyone so.

Two renderings of one truth: a public page reachable from the console's footer (X-41), and a
fetchable descriptor for the caller that does not read pages (X-42). Both derive what they claim from
the same surface declaration the navigation uses, so neither can advertise a capability the console
marks unbuilt — which matters more than usual here, because the honest answer today is *an agent can
be issued an identity and cannot yet use it*.

**Done** looks like: an agent author who has never seen this deployment can reach the page without an
account, learn what it can and cannot do today, and fetch the same facts in a parseable form. See
[`docs/designs/agent-onboarding.md`](designs/agent-onboarding.md).

### Invoke — X-11…X-13 · 🔄 **UNBLOCKED, X-12 in progress**

Where the confused-deputy answer becomes code: the caller names an operation id, and nothing else
about the request is theirs to choose. Not the host — the URL comes from the operation's own compiled
Flux. Not the credential — the address is derived. Not the tenant — it comes from the session.

**X-12's hardest criterion is structural, not behavioural:** a test that fails if a second
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

flux needs no new concept to consume this: `flux-channels` already has a generic `connector` channel
kind, and a `mode = "remote"` setting opens a stream instead of binding a listener. The event names
come from the same manifest either way, so `trigger { on = … }` is unchanged.

## Not yet filed

Deliberately, because filing a story that cannot be started manufactures work rather than scoping it:

- **`subscribe`** — inbound events, and the other verb of the same remote connector binding as
  `invoke`. **Its stated blocker is gone**: the inbound confused-deputy argument is now written down
  (flux's ecosystem design, and restated in [`vision.md`](vision.md#north-star) — *a subscriber
  cannot name a binding it has not been granted; a subscription is a projection of the connections
  that tenant already has*), and it needed an authenticated principal, which shipped in v0.1.0.
  What it still waits on is a **grant model to scope a subscription with**, i.e. X-13 — which is
  blocked upstream. Filing it now would produce a story nobody can start.
- **Leases** — the type is tested and nothing holds one. It needs a runtime that keeps state open,
  which means the runtime axis beyond `http`.
- **Workflows** — stored, versioned, per-tenant `flux-app` Programs, never a second execution model
  and never an interpreter here (see [`vision.md`](vision.md) principle 8). Furthest out, and
  dependent on the composition path in flux-connectors.

  **Check the pin, not flux's HEAD.** The prerequisite was `http.request` returning a record rather
  than a flat string, so a composite operation can read a field out of a previous step's response.
  That landed in flux **v0.43.0** — but flux-connectors pins `codewandler-flux-web` **0.41.0**, where
  it is still flat, so its `Graph` lowering still refuses composites *correctly for the version in
  its lockfile*. The unblock is real, it is upstream, and it reaches the connector compiler on a
  flux-web bump and not before.
- **Execution records** — after X-12, since there is nothing to record until something executes.

**Unblocked 2026-08-01.** flux-connectors published 0.9.0: `connector-pack` requires
`flux-runtime ^0.46` where it required `^0.41` against a flux line at 0.45. X-11 landed the upgrade
and proved `connector-pack` **links** — `crates/exchange-host/tests/engine_line.rs` packs a real
connector into a `flux_runtime::ToolRegistry` through `flux_web`'s `HttpRequestTool`. X-12 is in
progress; X-13 follows it, and X-14 (per-instance addresses) unblocked with it because the same
release carries the instance dimension.

Two decisions X-11 made that the rest of the epic inherits: `connector-pack` is a **dev-dependency**
until X-12 promotes it, so a published crate does not carry the flux engine to satisfy a proof; and
the engine line is pinned at **0.46, not the newest** — `flux-runtime` 0.47 exists and taking it
would recreate the two-incompatible-types failure that blocked this epic in the first place.
