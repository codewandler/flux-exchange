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

### The HTTP surface — X-01 · 🔄 **READY (X-02…X-04)**

Turn the binary that prints a matrix into a service. The load-bearing story is **X-02**, and it is
load-bearing for one reason: a credential-holding service that starts on a reachable address without
a way to authenticate is not a bug to fix later. flux's own server takes the same position — a
non-loopback bind without a token is refused *at startup*, because the daemon auto-approves tools and
an open listener is RCE. Substitute credentials for tools and the argument is unchanged.

**X-03** is where the north star stops being prose. The tenant must come from the resolved principal
and from nothing a caller controls, and the story asks for that asserted three times — once for a
path segment, once for a body field, once for a header.

### The catalogue surface — X-05 · 🔄 **READY (X-06, X-07)**

Serve what exists, so the console stops rendering fixtures. Unblocked by construction:
`connector-catalog` is static data with no dependencies, no IO and no runtime.

Two decisions worth making deliberately rather than by accident. **The catalogue must carry `risk`,
`effects` and `idempotency`** — without them nothing but the server can predict what a `Selector`
admits, and the grant model becomes folklore. And **it must not be silently filtered by grant**: an
agent that cannot see an operation it lacks cannot report that it was refused.

### Connections and credentials — X-08 · 🔄 **READY (X-09, X-10)**

An operator connects a provider; a tenant's credentials are reachable only by that tenant.

**X-09 is the story most likely to be got wrong in a comfortable direction.** Every acceptance
criterion on it is a refusal: a widened file mode is refused rather than repaired, a store path
inside the working directory is refused, a bad store configuration is a startup error with no
fallback to memory. The last one matters most — a host that fell back would start successfully,
serve every route correctly, look exactly like a working one, and lose everything on the next
restart.

### Invoke — X-11…X-13 · ⛔ **BLOCKED on the engine line**

Where the confused-deputy answer becomes code: the caller names an operation id, and nothing else
about the request is theirs to choose. Not the host — the URL comes from the operation's own compiled
Flux. Not the credential — the address is derived. Not the tenant — it comes from the session.

**X-12's hardest criterion is structural, not behavioural:** a test that fails if a second
request-building path ever appears. This host constructs no request of its own, and that is the
property that keeps it from becoming the credential-injecting proxy the family already rejected.

## Not yet filed

Deliberately, because filing a story that cannot be started manufactures work rather than scoping it:

- **`subscribe`** — inbound events. It needs the confused-deputy argument made for the *inbound*
  direction first ("a subscriber cannot name a binding it has not been granted"), and that argument
  is only sound once an authenticated principal exists. After X-03.
- **Leases** — the type is tested and nothing holds one. It needs a runtime that keeps state open,
  which means the runtime axis beyond `http`.
- **Workflows** — stored `flux-app` programs. Furthest out, and dependent on the composition path
  in flux-connectors, which is itself waiting on a flux-web bump.
- **Execution records** — after X-12, since there is nothing to record until something executes.
