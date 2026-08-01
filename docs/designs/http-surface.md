---
story: X-02
status: accepted
---

# The HTTP surface: axum, per-module routers, and a bind that refuses

The decisions behind `crates/exchange-server` — which framework serves the API, how routes are
assembled, and the one rule that decides whether the process is allowed to listen at all.

## The framework is axum

**Decision: [axum](https://github.com/tokio-rs/axum) 0.8.**

The reasons, in the order they mattered:

1. **It is `tower` all the way down.** Everything this service will need is a `tower` layer, not an
   axum feature: the principal guard, request tracing, rate limits, timeouts, body limits. That
   matters more here than in an ordinary web service, because the thing we most need to be able to
   state precisely is *which routes are guarded*, and a `Layer` applied to a route is a claim a test
   can check. A framework with its own middleware vocabulary would make that claim framework-shaped
   instead of type-shaped.

2. **The async runtime is already `tokio`,** which `AGENTS.md` fixes. axum is the tokio project's own
   HTTP framework; there is no second reactor, no bridging layer and no runtime to argue with.

3. **Extractors put "what a handler is allowed to read" in the type signature.** This repository's
   central rule is that the tenant comes from the resolved principal and from *nothing a caller
   controls*. A handler that takes a `Principal` and no `Path`/`Header` tenant is a handler whose
   signature carries the rule. That is worth a great deal in a codebase whose whole point is a
   boundary.

4. **It was already the workspace's declared dependency.** `axum`, `tower` and `tower-http` are in
   `Cargo.toml` and in the lockfile. Choosing otherwise would have meant a dependency change to
   justify, and nothing below was hard enough to justify it.

Alternatives, briefly. **`actix-web`** is fast and mature but brings its own actor runtime and its own
middleware model, which buys nothing here and costs the tower ecosystem. **`poem`** and **`salvo`**
are pleasant and smaller-community; neither offers something axum lacks for this shape of service.
**Raw `hyper`** would mean writing routing and extraction by hand — exactly the kind of bespoke code
principle 6 ("we construct no request of our own") suggests we should be buying, not building.
Nothing about this decision is hard to reverse: the surface is a route table plus handlers, and the
framework touches ~40 lines.

## Routers compose per module

**Decision: each feature module owns its routes; the app is assembled from them at one merge site.**

`crates/exchange-server/src/routes/` holds one module per feature area — `health` today, identity
(X-03) and the catalogue (X-06) next. A module declares a `Module` with its own routes, and
`routes::app` folds every module in `MODULES` into the served `Router`.

The reason is scheduling as much as taste. If every route lived in one `routes.rs`, two stories that
each add routes would collide on that file and have to run one after the other. With a module each,
the only shared line is the entry in `MODULES`. A story that adds a surface writes a new file and
one line.

### Why a module hands over a table rather than an opaque `Router`

The obvious spelling is `health::router() -> Router`, merged by the assembly. This design does
something slightly different — a module declares its routes **as data** (path, access class,
handler) and its `Router` is *derived* from that declaration — for one reason:

**axum's `Router` cannot be asked what it answers.** There is no introspection API. If each module
built its router privately, a module could publish a route reachable without a principal and no test
could see it. The Acceptance for X-02 requires that "health is the only route reachable without a
principal" be asserted *by enumerating routes*, and an enumeration that can only see the modules it
was told about is an enumeration of its own assumptions.

Declaring routes as data makes `routes::published()` the whole surface **by construction**, so the
enumeration test covers a module added in a future story on the day it lands. The composition seam is
unchanged — per-module ownership, one merge line — only the direction of the dependency moved.

The same declaration is what wires the guard: `Access::Principal` is what applies the
`require_principal` layer. A route is *not* guarded by its handler remembering to ask for a
principal, because that is a thing a handler can forget. The test
`the_declared_access_is_what_decides_the_answer` runs the mechanism against a route it must refuse
and one it must admit — the same shape `console/test/components.test.mjs` uses for the component
scanner.

## A reachable bind with no identity is refused at startup

**Decision: if the bind address is not loopback and no `Identity` port is bound, the process refuses
to start.** Not a warning, not a default, not a flag that turns it off.

This is `crate::bind::admit_bind`, and it is deliberately copied rather than invented.
[flux's own HTTP server](https://github.com/codewandler/flux) refuses a non-loopback bind without a
token for the same reason: the daemon auto-approves tools, so an open listener is remote code
execution. The reasoning transfers with *credentials* in place of *tools*, and it gets worse in
transfer — an RCE ends when the process does, whereas a leaked vendor credential outlives it and is
not ours to rotate.

Three details that carry the weight:

- **The default bind is loopback** (`127.0.0.1:8080`), overridable with `FLUX_EXCHANGE_BIND`. A
  credential-holding service that is reachable *by default* is reachable before anyone decided it
  should be.
- **The unspecified addresses are reachable.** `0.0.0.0` and `::` are not loopback, and they are what
  an operator actually reaches for. A rule that missed them would be decorative;
  `bind::tests::the_unspecified_addresses_are_reachable` is what keeps it honest.
- **The check runs before the socket opens.** A listener that opens and then closes has still been
  open.

The refusal names both things that would have worked — bind loopback, *or* configure an identity
provider — because from the outside an operator cannot tell which half of the pair we think they
meant. That is `AGENTS.md` § Invariants, "Refuse; never repair": name the address, never the value,
and never leave the reader to guess the remedy.

Note what is *not* claimed. With no identity provider bound (the state today, until X-03), every
route but health answers `401`. That is the honest answer rather than a hole: the host cannot resolve
a principal, so it cannot attribute a request, so it refuses. `IdentityError::Rejected` and
`IdentityError::Unreachable` are already kept distinct out to the caller — `401` and `503` — because
an operator answers a bad token and an outage in opposite ways.

## What this leaves for later

- Binding a real identity provider, and the session a request carries — X-03.
- A `WWW-Authenticate` challenge header on the `401`. Correct HTTP, no security consequence, and out
  of this story's scope.
- TLS. Terminated in front of this process for now; if that ever stops being true, the bind rule
  above is where the decision belongs.
