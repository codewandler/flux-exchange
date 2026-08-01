# Design: agent onboarding

**Status:** accepted · **Epic:** `agent-onboarding` · **Stories:** X-41, X-42

## Why

`docs/vision.md` says the thing this epic exists to act on:

> Its primary caller is an **agent, not a human.** People sign in to wire things up and to see what
> happened; agents are what call operations all day. … **the API is the product and the console is
> the admin surface**, not the other way round.

Everything built so far serves the *second* caller. A human can sign in, wire up a connection, rotate
a credential and look at a catalogue. **An agent arriving at this service is told nothing.** There is
no page, no descriptor and no route that answers "what is this, and how do I connect to it?" — and
the caller the charter calls primary is the one with no way in.

X-36 sharpens this. An agent principal can now be minted and handed a token. Nothing tells anyone
that, or what to do with it.

## The awkward thing this design has to resolve

**An onboarding *page* is a human artifact.** An agent does not read a hero section; it fetches
something and acts. So "onboarding instructions for agents" has two audiences that must not drift
apart:

- **A human operator**, deciding whether to point their agent at this deployment, and needing to know
  what to hand it.
- **The agent itself**, needing something fetchable and unambiguous.

The resolution is **one truth, two renderings** — and the truth is not prose.

## Approach

### 1. It is public, and that is a decision

The page is reachable **without signing in**. An agent that must already be authenticated to learn
how to authenticate is a closed loop, and a human evaluating the platform should not need an account
to read what it offers.

`Access::Anonymous` already exists and `routes::tests::the_anonymous_surface_is_only_what_was_declared_anonymous`
guards it, so widening the anonymous surface is a deliberate, tested act rather than a default. What
is published must therefore contain **nothing tenant-specific**: no connector list for a tenant, no
principal, no address. It describes the *shape* of the service, not its contents.

### 2. Honest by construction, not by editing

This is the part that decides whether the page is worth having in six months.

The state of this platform is unusual and changing weekly: an agent can be **minted** (X-36) and
**cannot yet authenticate** (X-37), and can invoke nothing at all (X-12, blocked upstream). Prose
describing that will be false within a release, and `docs/vision.md` principle 7 makes a page that
implies a working service worse than an honest gap.

**So the instructions derive from the same surface declaration the console's navigation uses.** X-34
established `surfaces.mts` with a `built: bool` per surface and a test asserting nothing not-built is
reachable. Onboarding must read from that same source, so a capability cannot appear in the tutorial
while the nav marks it unbuilt. Honesty becomes a property of the wiring rather than of whoever last
edited the copy.

An onboarding page that says *"you can be issued an identity; you cannot yet use it, and here is
what will change when you can"* is genuinely useful — it tells an agent author exactly where the
platform is. That is the page to build.

### 3. A descriptor, not only a document

"Similar to a skill" is the right instinct: what an agent wants is a small, stable, fetchable
artifact naming the endpoints, the auth scheme and the capabilities — the same facts the page
renders, in a form something can parse. Deriving both from one source is what keeps them from
disagreeing, which is the failure mode every "docs plus SDK" pair eventually has.

### 4. Where it hangs

The footer. X-34's shell already has one (`console__foot` in `App.vue`). A footer link is right for
this: it is not a surface an operator works in daily, it is a reference an agent author reaches for
once, and putting it in the main rail would imply it is a place to do work.

## Alternatives considered

- **Hand-written prose in the README only.** Rejected: an agent cannot fetch a README section, and a
  human evaluating the deployment is looking at the console, not GitHub. It also rots silently, which
  is the specific failure this repository keeps having to correct.
- **Gate it behind sign-in.** Rejected above — a closed loop.
- **Generate it from the OpenAPI-ish route table.** Attractive, and premature: the route surface is
  declared (`routes::Module`) but says nothing about *auth scheme* or *what an agent should do
  first*, which is most of what onboarding is. Worth revisiting once `invoke` exists.
- **Wait until X-37 so the instructions describe a working flow.** Rejected, and this is the crux: the
  platform will *always* be mid-build, and a rule of "document it once it is finished" produces a
  service that is never documented. The honest version is publishable today.

## Risks & open questions

- **The anonymous surface widens.** That is a security-relevant change and the reason the tests
  guarding it exist. Whatever is published must be reviewed as a disclosure, not as copy.
- **Two renderings can still drift** if the descriptor and the page are wired to the source
  separately rather than sharing it. The test to write is that they agree, not that each is correct.
- **"How to authenticate" is a moving target** across X-37 and X-40. The page must be built to change
  cheaply — which is the argument for deriving rather than writing.

## Acceptance / done

The union of X-41 and X-42. In short: an agent author who has never seen this service can reach a
public page from the footer, learn what the platform is, what it can and cannot do **today**, and
fetch a machine-readable form of the same facts — and no part of it can claim a capability the
console marks unbuilt.
