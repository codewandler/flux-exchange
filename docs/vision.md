# flux-exchange — vision & principles

This document states *why* flux-exchange exists and the principles that decide how it is built. It
is the **tie-breaker** when a design choice is unclear.

## What flux-exchange is

The platform layer of the [flux](https://github.com/codewandler/flux) family: **a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.**

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
Flux supplies the language, agent loop and guarded runtime substrate. Exchange executes every
official external integration while holding the tenant's credential; neither Flux nor the connector
declaration holds one on anyone's behalf.

### The test that decides what belongs here

The family divides by one interrogative each, and
[flux's ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
states why: *a boundary that requires taste is a boundary that erodes.*

| Domain | Test |
|---|---|
| **flux** (engine) | Does it change what happens when an effect executes? |
| **flux-connectors** | Is it true of the integration regardless of who runs it? |
| **flux-exchange** | **Does it require holding a credential or knowing a tenant?** |
| a downstream product | Is it true only of one company's customers? |

Our row owns: **principals, connections, credentials, channels, installed apps, datasource and
trigger bindings, event deliveries, model profiles, leases, stored programs and execution
records.** Anything that fails our test belongs upstream or downstream — and
**no flux-family repository names a downstream company**, which is the fourth row applied to the
documentation itself.

Every official integration is therefore a connector, including Docker, Kubernetes, SQL,
observability, secret stores, and other protocol-rich systems. flux-connectors owns the declaration
and any vendor-specific runtime artifact; Flux owns generic guarded mechanisms but is not a second
official-integration execution placement. Exchange executes the connector address: it owns tenant
authority, runtime admission and execution, isolation, streams, leases and audit. If Exchange is
unavailable, the official external tool is unavailable; there is no local vendor/plugin fallback.

## Who it is for

**Its primary caller is non-human, not a human.** People sign in to wire things up and to see what
happened; Service Accounts and hosted Managed Agents call operations all day. A **Service Account**
is the non-human bearer principal; an **Agent** remains model + authored loop + bounded capabilities.
The v0.16 `/api/agents` create alias was compatibility surface, not a second definition, and is
removed in v0.17.
This inverts the usual assumption for a
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

4. **A Service Account token grants access to an operation, never to a credential.** A stolen token
   yields a bounded operation set against one tenant's connections — never a vendor secret. A
   Managed Agent receives the same bounded authority through its installed App revision without
   receiving the credential value.

5. **Refuse; never repair.** A missing credential, an unbound config value, an unknown runtime: each
   refuses by name. A partial substitution is a request to a *different* host, which that host
   answers with a `200`.

6. **We construct no request of our own.** Every execution path ends in `connector_pack`, evaluating
   the operation's own compiled Flux. A second request path is how this becomes the credential-
   injecting proxy the family already rejected.

7. **Say what is not built.** A page or a type that implies a working service costs more than an
   honest gap. The README carries an itemized inventory and it is expected to stay accurate.

8. **A workflow is a stored Program, not a second execution model or a formal domain type.** What an
   operator informally calls a workflow — triggers, conditions, schedules, flows of operations — is
   a stored, versioned, per-tenant Flux Program installed as an App. A visual editor emits IR; the IR
   lowers to Flux. The simplified
   schema an editor wants is a *projection*, never a second model.

   That buys determinism, replay, fork/diff, approval gates, typing and risk derivation for free, and
   makes a composed operation **indistinguishable from a vendor one** — same catalogue entry, same
   gating, same address. An agent cannot tell whether an operation came from an OpenAPI document or
   from someone dragging boxes, which is exactly what makes an editor useful to agents and not only
   to humans.

## Non-goals

- **Being required for the language or core tools.** Flux remains useful without Exchange for the
  language, agent loop and core tools. Official external integrations do require Exchange: the
  embedded binding has no helper executable, installed pack or local vendor/plugin fallback.
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

- **Does this console reuse flux-connectors' explorer components?** **No.** X-86 retired that copied
  surface after the two hosts diverged in exactly the way the boundary predicted: the documentation
  site publishes request paths, generated Flux, hosts, credentials and inbound declarations, while
  this service deliberately publishes a thinner anonymous catalogue. Rendering the richer UI here
  produced blank facts, and changing it required synchronising two repositories. The exchange now
  owns one finder over what its own API actually serves. The useful seams survived — data still
  arrives as props and colour still comes from one token layer — without shared source ownership.
- **Does `subscribe` ship before or after multi-tenant sign-in?** **After.** Complete OIDC sign-in
  shipped in v0.1.0, and X-101–X-105 now deliver authenticated `/api/subscribe` for generated socket
  channels with closed declared event sets. Webhooks, polls, arbitrary streamed operation output,
  replay, and lease liveness remain in the X-111 rich-runtime program.

---

See [`docs/roadmap.md`](roadmap.md) for status and what is next, and
[flux's ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
for how the three projects divide.
