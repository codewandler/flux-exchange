---
layout: home

hero:
  name: flux-exchange
  text: The credential never crosses the boundary
  tagline: The platform layer of the flux family — a service that holds credentials, terminates channels, runs operations for many callers, and records what happened.
  actions:
    - theme: brand
      text: The credential boundary
      link: /boundary
    - theme: alt
      text: The surface
      link: /surface

features:
  - title: Its primary caller is an agent
    details: People sign in to wire things up and to see what happened. Agents are what call operations all day, so the API is the product and the console is the admin surface.
  - title: A caller names an operation, never a secret
    details: Not a host, not a credential, not a tenant. Each of those is derived rather than accepted, which is what makes a stolen agent token bounded.
  - title: What a build can do is a question you can ask it
    details: Every deployment answers GET /api/onboarding anonymously, describing what it can and cannot do. That answer, not this site, is the current one.
---

## What this is

flux-exchange is the platform layer of the [flux](https://github.com/codewandler/flux) family.

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
flux runs it. Neither holds a credential on anybody's behalf, and neither should — that is a third
job, and this is it.

The family divides by one interrogative each, so that the boundary does not need taste to hold:

| Ask | Whose job |
|---|---|
| Does it change what happens when an effect executes? | **flux**, the engine |
| Is it true of the vendor regardless of who runs it? | **flux-connectors** |
| **Does it require holding a credential or knowing a tenant?** | **flux-exchange** |

This row owns principals, connections, credentials, channels, leases, stored programs and execution
records. Anything that fails the test belongs upstream or downstream.

## What it is not

- **It is not required.** flux must never *need* flux-exchange. A program loading a connector module
  on a laptop is a complete path and stays one. Trading distribution pain for service lock-in would
  be a bad trade made twice.
- **It is not a proxy that injects credentials.** Every execution path ends in the operation's own
  compiled Flux. A second request-building path is the thing the family already rejected.
- **It is not an interpreter, and not a second execution model.** A workflow here is a stored,
  versioned program — see [the surface](/surface).

## Where the honest answer lives

This site describes the software. It does not describe any particular deployment, and it does not
tell you which capabilities a given build serves.

**That question has a machine-readable answer already: `GET /api/onboarding`.** It is anonymous, its
disclosure list was reviewed field by field, and its claims are held to the service's route table by
a test in both directions — a capability cannot even name an endpoint that does not serve it. Ask
the build you are talking to; it will tell you what it can and cannot do today.

Per-capability status **on this site** — a badge on each page, derived from that same descriptor
rather than typed by an author — is a tracked change (story X-64) and is not here yet. Until it
lands, this site deliberately makes no claim about whether any capability is live. The repository
under [what exists today](https://github.com/codewandler/flux-exchange#what-exists-today) carries the
itemized inventory, including what is *not* built.

That gap is on purpose. This repository has already corrected five separate renderings of one stale
claim about its own capabilities — each written honestly, each believed by somebody. A documentation
site is a factory for exactly that failure, so the pages come after the mechanism that keeps them
true, not before it.
