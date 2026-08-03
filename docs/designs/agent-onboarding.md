# Design: agent onboarding

**Status:** accepted · **Epic:** `agent-onboarding` · **Stories:** X-41, X-42, X-52

> **Vocabulary note, 2026-08-03.** This design predates X-107 and uses “agent” for the non-human
> bearer caller now named a **Service Account**. Its anonymous descriptor and human onboarding
> decisions remain delivered; current references to a hosted **Managed Agent** mean Flux's model +
> authored loop + bounded capabilities. Historical wording below is retained to preserve the design
> record, not as a live principal spelling.

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

#### Correction (X-42): deriving from `surfaces.mts` was right; deriving from `built` was not

This section's promise — *honesty becomes a property of the wiring rather than of whoever last
edited the copy* — held exactly as written, and wired the page to the wrong construct. `built` in
`surfaces.mts` answers **"does this console have a screen"**; the page and the descriptor are
answering **"does this service do this"**. Those had the same answer for every surface when X-41 was
written, so nothing distinguished them. Then `POST /api/operations/{operation}/invoke` shipped in
v0.7.0 with no screen behind it, and three renderings — the navigation, the onboarding page and the
mint screen — each began reporting that this deployment could call nothing, while the route sat in
the published surface running operations.

The prose above is stale in the same way: *"can invoke nothing at all (X-12, blocked upstream)"* was
true when it was written and is not now.

So `surfaces.mts` answers the two questions separately — `built` and `served` — and `served` is not
kept true by remembering: `routes::onboarding::tests` in the server crate measures every published
capability against `routes::MODULES`, and refuses to let a published route go unaccounted for. That
second half is what would have caught this on the day `invoke` landed. **The lesson to carry: a
derivation is only as honest as the construct it derives from, and a test that two renderings agree
cannot see that both are wrong.**

#### Correction (X-52): a guard is only as strong as the field it reads

The correction above holds. What its own review found is that the guard it produced —
`a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it` — read one field of the
document and was named for two. It compared `capability.live` against *does the `SERVED_BY` path
exist* and never against the capability's own `call.endpoint`, so republishing `be-minted` at
`/api/session` (a real route, and one the same file argues is **not** a capability) left every test
in the server crate green. All three live endpoints were pinned, but by a hand-written line in a
console test belonging to another story. **The pattern to carry: a test whose name names two facts
has to assert both, and the way to find out is to mutate each one separately.**

The `method` field was pinned by nothing at all, and the obstacle is worth recording because it
recurs. `Route` carries a `fn() -> MethodRouter`; a `MethodRouter` cannot be asked what it answers,
which is the same property §"How a module hands over a table rather than a `Router`" in
`routes/mod.rs` is built around. Two shapes were on the table:

- **Declare the method on `Route`, beside the `method_router`.** Rejected. It makes the document
  agree with a declaration and leaves the declaration agreeing with nothing — two values in one
  struct that can disagree, with the guard standing on the wrong one.
- **Drive each published call and assert the answer is not `405`.** Taken. It asks the surface the
  question an agent will ask it.

Two things about that are worth knowing before trusting it. It proves the method **reaches a
handler**, not that it is the only method the route serves — narrower than "the method is correct",
and the test is named for the narrower claim. And it only works against a **resolved** caller: on a
guarded route the `route_layer` runs before the method router, so an anonymous probe answers `401`
for every method and cannot tell one from another. The first spelling of the test drove it
anonymously and would have passed for `DELETE /api/agents`; its own control caught that, which is
the argument for the control.

The third finding has no test and should not pretend to. The `authenticate` withholding ended *"The
only principals a deployment resolves today are humans who signed in through its identity
provider"*, which is false on a development-identity deployment — the roster resolves
`service_account:` and `service:` handles, and
`dev_identity::tests::a_handle_resolves_to_the_principal_the_roster_armed`
drives one. The operative claim was true and the sentence around it was not. **A wrong argument in a
withholding, or in a `NOT_A_CAPABILITY` line, is invisible to a test**; the only thing holding those
is that they are written next to the code they describe and read when it changes.

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
