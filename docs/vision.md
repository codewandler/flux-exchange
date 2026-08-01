# flux-exchange — vision & principles

This document states *why* flux-exchange exists and the principles that decide how it is built. It
is the **tie-breaker** when a design choice is unclear.

## What flux-exchange is

The platform layer of the [flux](https://github.com/codewandler/flux) family: **a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.**

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
flux runs it. Neither holds a credential on anyone's behalf, and neither should — that is a third
job, and this is it.

## Who it is for

**Its primary caller is an agent, not a human.** People sign in to wire things up and to see what
happened; agents are what call operations all day. That inverts the usual assumption for a
credential-holding web service and it shapes everything: the API is the product and the console is
the admin surface, not the other way round.

## North star

**The credential never crosses the boundary; the authority does.**

Outbound, a caller names an operation and gets a result. It cannot name a host — the URL comes from
the operation's own compiled Flux. It cannot name a credential — the address is derived from the
session's tenant and the connector's declared authority. It cannot name a tenant — that is read from
the resolved principal and from nothing a caller controls.

Inbound, a vendor's signed payload is verified here and the caller receives a typed, declared event.

In neither direction does a caller come to hold a value it did not already have. Any proposal that
weakens this is wrong, however convenient.

## Principles

1. **The runtime is declared by the connector, never chosen by the caller.** A caller who can pick
   the runtime is a caller who can pick an effect.

2. **A locally-executing runtime cannot be safely multi-tenant in one process.** HTTP is shareable
   because the effect leaves the machine; process, container and raw-socket runtimes consume this
   host's own identity and network position. A shared deployment refuses them — mechanically, from
   the manifest, not by an operator noticing.

3. **Grants select operations by declared metadata, not by name.** A grant written as a list of ids
   is a list somebody maintains, and it stops covering a connector the moment that connector gains
   an operation. `risk <= low` covers the new one correctly on the day it lands.

4. **An agent's token grants access to an operation, never to a credential.** A stolen agent token
   yields a bounded operation set against one tenant's connections — never a vendor secret.

5. **Refuse; never repair.** A missing credential, an unbound config value, an unknown runtime: each
   refuses by name. A partial substitution is a request to a *different* host, which that host
   answers with a `200`.

6. **We construct no request of our own.** Every execution path ends in `connector_pack`, evaluating
   the operation's own compiled Flux. A second request path is how this becomes the credential-
   injecting proxy the family already rejected.

7. **Say what is not built.** A page or a type that implies a working service costs more than an
   honest gap. The README carries an itemized inventory and it is expected to stay accurate.

## Non-goals

- **Being required.** flux must never *need* flux-exchange. A `.flux` program loading a connector
  module on a laptop is a complete path and stays one. Trading plugin-binary distribution pain for
  service lock-in would be a bad trade made twice.
- **A second request path.** See principle 6.
- **Holding credentials an operator did not choose to give it.** A tenant's credentials are reachable
  only by that tenant's own authenticated principals.
- **Reimplementing the engine.** Execution primitives come from `flux-system`; the safety vocabulary
  comes from `flux-spec`. Where flux already answers a question, we bind rather than restate.

## How success is measured

- An agent can call a vendor operation without its operator's credential ever reaching it.
- A grant can be read and its effect predicted without consulting a list of operation names.
- Every execution is explainable: who asked, which grant admitted it, what was called, what came back.
- The published inventory of what is *not* built stays true.

---

See [`docs/roadmap.md`](roadmap.md) for status and what is next, and
[flux's ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
for how the three projects divide.
