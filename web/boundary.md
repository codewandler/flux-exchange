# The credential boundary

> **The credential never crosses the boundary; the authority does.**

That sentence is the north star every design decision in flux-exchange answers to. This page is the
argument behind it, because a service that holds other people's credentials is worth exactly as much
as the reason it can be trusted to.

It is an argument about the *software*, not a report on any deployment, and it is not an inventory of
what is built — see [where the honest answer lives](/#where-the-honest-answer-lives) for that.

## Outbound: a caller names an operation and gets a result

Three things a caller might expect to name, and does not:

| It cannot name | Because | So it comes from |
|---|---|---|
| the **host** | a caller who can pick the destination can point a credential at their own | the operation's own compiled Flux |
| the **credential** | a caller who can pick the secret is a caller who has one | the tenant, plus the connector's declared authority |
| the **tenant** | a caller who can pick the tenant is every other tenant's problem | the resolved principal, and nothing a caller controls |

The tenant one is the sharpest, and it is stated as an invariant rather than a habit: **the tenant is
read from the resolved principal — not from a path segment, not from a body field, not from a
header.** There is no request shape that carries it, so there is no request shape that forges it.

The same reasoning removes the choice of runtime. **The runtime is declared by the connector, never
chosen by the caller**, and there is deliberately no constructor that takes caller input for it: a
caller who can pick the runtime is a caller who can pick an effect.

## Inbound: a subscriber cannot name a binding it was not granted

This is the mirror of the outbound argument, and it is the half that is easiest to skip.

A vendor's signed payload is verified at this boundary and the caller receives a typed, declared
event. A subscription is **not** a request for events from a source the caller names — it is a
*projection of the connections that tenant already has*, scoped by the same tenant derivation as a
credential address.

`invoke` and `subscribe` are not two features. They are **two verbs of one remote connector
binding**, and the symmetry is the design: a connector declares both directions, so a host can serve
either one remotely.

In neither direction does a caller come to hold a value it did not already have.

## Authority is granted by property, not by name

A grant selects operations **by their declared metadata** — risk, effects, idempotency — rather than
by a list of identifiers. The difference is not stylistic:

- a grant written as a list of names is a list somebody maintains, and it stops covering a connector
  the moment that connector gains an operation;
- a grant written as `risk <= low` covers the new operation correctly on the day it lands.

An explicit deny beats an explicit allow, and **an agent's token grants access to an operation, never
to a credential**. That is what bounds the blast radius of a stolen token: it yields a bounded
operation set against one tenant's connections, and never a vendor secret.

## Refuse; never repair

A missing credential, an unbound configuration value, an unknown runtime: each **refuses, and names
the address rather than the value**.

The temptation is always to substitute something reasonable and carry on. A partial substitution is a
request to a *different* host, and that host answers it with a `200`. So a store that quietly falls
back to memory, or a file mode that is quietly tightened rather than reported, hides the one thing
you needed to know — the file already had the wrong mode while it held values, and tightening it
conceals the exposure instead of reporting it.

The same rule decides how this service starts: a configuration that names no credential store is a
startup error naming what would have worked, not a host that starts, serves every route correctly,
looks exactly like a working one, and loses every credential on restart.

## Some runtimes cannot be shared, and the refusal is mechanical

HTTP is shareable because the effect leaves the machine. Process spawning, container exec and raw
sockets consume the host's own identity, network position and filesystem — so they cannot be safely
multi-tenant in one process, however carefully they are configured.

A shared deployment therefore **refuses** them, decided from the connector's manifest rather than by
an operator noticing, and the refusal names what would have worked instead. There is no override, and
adding one would be a regression rather than a feature.

## What this page does not tell you

Whether the build you are talking to serves any particular one of these paths. That answer is
`GET /api/onboarding` — anonymous, machine-readable, and held to the route table by a test — and it
is the current one in a way that prose on a public page can never be.
