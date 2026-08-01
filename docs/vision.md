# flux-exchange — vision & principles

This document states *why* flux-exchange exists and the principles that decide how it is built. It
is the **tie-breaker** when a design choice is unclear.

## What flux-exchange is

The platform layer of the [flux](https://github.com/codewandler/flux) family: **a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.**

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
flux runs it. Neither holds a credential on anyone's behalf, and neither should — that is a third
job, and this is it.

### The test that decides what belongs here

The family divides by one interrogative each, and
[flux's ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
states why: *a boundary that requires taste is a boundary that erodes.*

| Domain | Test |
|---|---|
| **flux** (engine) | Does it change what happens when an effect executes? |
| **flux-connectors** | Is it true of the vendor regardless of who runs it? |
| **flux-exchange** | **Does it require holding a credential or knowing a tenant?** |
| a downstream product | Is it true only of one company's customers? |

Our row owns: **principals, connections, credentials, channels, leases, stored programs and
execution records.** Anything that fails our test belongs upstream or downstream — and
**no flux-family repository names a downstream company**, which is the fourth row applied to the
documentation itself.

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
The mirror of the outbound argument, and the half that is easiest to skip: **a subscriber cannot name
a binding it has not been granted.** A subscription is not a request for events from a source the
caller names — it is a *projection of the connections that tenant already has*, scoped by the same
tenant derivation as the credential address.

In neither direction does a caller come to hold a value it did not already have. Any proposal that
weakens this is wrong, however convenient.

`invoke` and `subscribe` are not two features. They are **two verbs of one remote connector binding**,
and the symmetry is the design: a connector declares both directions, so a host can serve either one
remotely.

## The three lifetimes

Routinely conflated, and conflating them produces real bugs — a webhook endpoint that dies when an
agent disconnects, or a lease that outlives the grant that opened it.

| | Scope | Direction | Dies when |
|---|---|---|---|
| **Session** | a conversation | — | it is closed or expires; resumable |
| **Channel** | a deployment | pushes | the operator removes it |
| **Lease** | a caller's grant | pulls | the holder releases it, or TTL |

`lease` is deliberately not called "session": flux's `session` already means an event-sourced
conversation, and the two have opposite lifetimes and opposite owners. One name each, so a sentence
about one can never be misread as a sentence about the other.

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

8. **A workflow is a stored program, not a second execution model.** What an operator calls a
   workflow — triggers, conditions, schedules, flows of operations — is a stored, versioned,
   per-tenant `flux-app` Program. A visual editor emits IR; the IR lowers to Flux. The simplified
   schema an editor wants is a *projection*, never a second model.

   That buys determinism, replay, fork/diff, approval gates, typing and risk derivation for free, and
   makes a composed operation **indistinguishable from a vendor one** — same catalogue entry, same
   gating, same address. An agent cannot tell whether an operation came from an OpenAPI document or
   from someone dragging boxes, which is exactly what makes an editor useful to agents and not only
   to humans.

## Non-goals

- **Being required.** flux must never *need* flux-exchange. A `.flux` program loading a connector
  module on a laptop is a complete path and stays one. Trading plugin-binary distribution pain for
  service lock-in would be a bad trade made twice.
- **A second request path.** See principle 6.
- **Holding credentials an operator did not choose to give it.** A tenant's credentials are reachable
  only by that tenant's own authenticated principals.
- **Reimplementing the engine.** Execution primitives come from `flux-system`; the safety vocabulary
  comes from `flux-spec`. Where flux already answers a question, we bind rather than restate.
- **Shipping an interpreter.** Follows from principle 8, and it is what keeps flux-connectors' own
  north star — *a connector is compiled, never interpreted* — intact across the whole family.

## How success is measured

- An agent can call a vendor operation without its operator's credential ever reaching it.
- A grant can be read and its effect predicted without consulting a list of operation names.
- Every execution is explainable: who asked, which grant admitted it, what was called, what came back.
- The published inventory of what is *not* built stays true.

## Two questions the ecosystem design defers to this charter

flux's ecosystem design lists both as open and hands them here. Answering them is this document's
job, so they are answered rather than left hanging:

- **Does this console reuse flux-connectors' explorer components?** **Yes, and it already does.**
  Fifteen components are carried in `console/src/components/`, importing only Vue, a sibling
  component and `catalog.mts` — an invariant enforced by `console/test/components.test.mjs`, which is
  itself guarded by a scanner self-test. They take everything they render as props, which is the
  property that let them be lifted without a rewrite. **They are not modified here**; a change one
  needs is a finding to report upstream, where the component is shared.
- **Does `subscribe` ship before or after multi-tenant sign-in?** **After, and sign-in has now
  landed.** The inbound confused-deputy argument is sound only once a principal exists; until it did,
  a loopback bind stood in for one exactly as it does for `invoke`. Complete OIDC sign-in shipped in
  v0.1.0, so the stated blocker is gone — what `subscribe` still waits on is a grant model to scope a
  subscription with, which is why it is not yet a story.

---

See [`docs/roadmap.md`](roadmap.md) for status and what is next, and
[flux's ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
for how the three projects divide.
