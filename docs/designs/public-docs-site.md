# Design: a public documentation site

**Status:** accepted · **Epic:** `public-docs-site` · **Stories:** X-63, X-64, X-65, X-66

## Why

`flux` and `flux-connectors` each publish a VitePress site from `web/`, deployed to GitHub Pages by
an Actions workflow. This repository publishes nothing. Somebody evaluating the flux family can read
what the engine does and browse the connector catalogue, and then finds a gap exactly where the
question "what holds my credentials, and what will it let an agent do?" gets answered.

The brief is also to **scaffold the whole future surface** — webhooks, channels, triggers — rather
than documenting only what is built.

## The tension, which is the whole design

`docs/vision.md` principle 7:

> **Say what is not built.** A page or a type that implies a working service costs more than an
> honest gap.

This session alone corrected **five separate renderings** of one false claim — that `invoke` was not
built — across the onboarding page, the mint screen, the shell inventory, the machine-readable
descriptor and the catalogue explorer, plus a sixth in a README. Each was written honestly, went
stale, and was believed by somebody.

**A documentation site is a rendering factory for exactly that failure.** Twenty pages describing
webhooks, channels and triggers is twenty more places to be wrong, published at a public URL, read by
people with no way to check.

So the requirement is not *document the future surface* or *only document what is built*. It is:

> **Scaffold the whole surface, and make every page's status a derived fact rather than a sentence
> somebody wrote.**

## Approach

### 1. Status is derived, and it is derived from the thing that already works

X-42 built `GET /api/onboarding`: a machine-readable descriptor of what this build can and cannot do,
whose `live` flags are held to the route table by a Rust test in **both** directions (X-52 tightened
it so a capability cannot even name a route that does not serve it). That is the only honesty
mechanism in this repository that has survived a review.

**The site derives its status badges from the same source.** The descriptor is generated from
`console/src/onboarding.mts` into a committed artifact; the site reads that artifact. A page for a
capability that is not live renders as *planned*, and it does so because the route table says so —
not because an author remembered.

This is the reason to build the site now rather than after the platform settles: the mechanism that
makes a large speculative site safe already exists and is tested.

### 2. Three kinds of page, and they are visually distinct

- **Built** — behaviour that exists, with the endpoint, the refusals, and what a caller may not name.
- **Planned** — the surface as designed, marked as not built, with the story that would build it.
- **Principle** — the charter material, which is true regardless of what ships.

A reader must never have to guess which they are looking at. **Planned pages carry the marker in the
page chrome, not in a paragraph three screens down**, because the way the five renderings went wrong
was that the caveat and the claim drifted apart.

### 3. What the future surface actually is

Not invented for the site. The vocabulary is `docs/vision.md`'s and it is already settled:

- **`invoke` and `subscribe` are two verbs of one remote connector binding**, not two features
  (`vision.md:58`).
- **The three lifetimes** (`vision.md:62-71`), which exist because conflating them produces real bugs
  — *a webhook endpoint that dies when an agent's session ends*:

  | | scoped to | direction | ends when |
  |---|---|---|---|
  | **Session** | a caller's conversation | — | the conversation does |
  | **Channel** | a deployment | pushes | the operator removes it |
  | **Lease** | a caller's grant | pulls | the holder releases it, or TTL |

- **A webhook is a Channel**, not a trigger and not a session artifact. The site should use one word
  per thing, which is what the vision asks for and what makes the pages worth writing.
- **Workflows are stored, versioned `flux-app` Programs** (`vision.md:106`) — triggers, conditions and
  schedules live there, not in a bespoke engine here. **Documenting triggers as a flux-exchange
  feature would be the largest untruth on the site**, so the page that covers them says where they
  actually live.

### 4. Style, matched rather than reinvented

Follow `flux-connectors/web` exactly: VitePress in `web/`, `npm ci` / `npm run build`, deployed by a
`pages.yml` using SHA-pinned actions, building on pull requests as a gate and deploying only from
`main`. Take its settled decisions with their reasons:

- `ignoreDeadLinks: false` — a broken internal link fails the build rather than publishing.
- `srcExclude: ['README.md']` — the contributor readme is not a page.
- `base: '/flux-exchange/'`, because a project Pages site is served under the repository name.
  flux-connectors' config carries a hard-won comment about this: a CNAME file landing does **not**
  mean the custom domain is live, and flipping `base` to `'/'` early 404s every asset. Flip only once
  `gh api repos/codewandler/flux-exchange/pages` reports the cname.

### 5. What the site must not publish

It is a public page about a service that holds other people's credentials.

- **No deployment-specific fact.** The site describes the software, never an instance.
- **Nothing that is not already public.** `GET /api/onboarding` is anonymous and its disclosure list
  was reviewed field by field (X-42); that list is the ceiling, not a starting point.
- **No configuration example containing anything credential-shaped**, even obviously fake. A copyable
  example is a copied example.

## Alternatives considered

- **Publish `docs/` as-is.** Rejected: those are working documents — designs with corrections in
  place, stories with review findings — and their value depends on being written for people building
  this. A visitor needs a different document, and flattening the two would make the internal ones
  worse.
- **Write the future surface as prose and keep it accurate by review.** Rejected, and the evidence is
  in this repository: five renderings, each written honestly, each stale within a release. Review is
  what caught them; it is not what prevented them.
- **Wait until the platform is built.** Rejected. A platform will always be mid-build, and "document
  it once it is finished" produces a service that is never documented — the same argument
  `agent-onboarding.md` made and was right about.
- **Generate every page from the descriptor.** Attractive and too far: the descriptor names
  capabilities and endpoints, and most of what a docs site is for — *why the credential never crosses
  the boundary* — is not in it and should not be. Derive the **status**, write the prose.

## Risks & open questions

- **The committed descriptor artifact is the seam.** If the site reads it and nobody regenerates it,
  the site is stale in the one place it claimed not to be. The console suite already fails on drift;
  the site's build must depend on the same check rather than assuming it ran.
- **A site is a maintenance surface.** Four pages that stay true beat twenty that rot. The stories are
  ordered so the mechanism lands before the volume.
- **`base` and the custom domain.** Documented above; the sibling repo has already paid for this
  mistake once.
- **Two audiences again.** X-42's design split the human page from the machine descriptor and made a
  test hold them together. This site is a **third** rendering of overlapping facts. That is
  acceptable only because its status comes from the same artifact — but a claim written in prose on
  the site is not held by anything, and that limit should be stated on the site's own contributing
  page rather than discovered.

## Acceptance / done

A visitor can reach a public URL, learn what flux-exchange is, understand the credential-boundary
argument, see the whole intended surface including channels and workflows, and — for any capability —
know whether it is built **today**, with that answer derived from the route table rather than written
by hand.
