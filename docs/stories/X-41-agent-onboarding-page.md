---
id: X-41
title: "An agent arriving at this service is told how to connect"
status: done
epic: agent-onboarding
design: docs/designs/agent-onboarding.md
areas: [console]
note: "owner-raised 2026-08-01, high priority: the charter's primary caller is an agent and nothing anywhere tells one how to reach this service. Public, linked from the footer, and honest about what it cannot yet do"
---

# An agent arriving at this service is told how to connect

## Goal
Someone bringing an agent to this deployment can find out — without an account — what it is, how to
get an identity for their agent, and what that identity can and cannot do today.

## Why this is priority 1

`docs/vision.md`'s second sentence calls the agent the **primary caller**. Everything built so far
serves the other one: a human signs in, wires up a connection, reads a catalogue. **An agent arriving
here is told nothing** — no page, no descriptor, no route answers "what is this and how do I connect".

X-36 just made it possible to mint an agent principal and hand it a token. Nothing tells anyone that.

## Acceptance
- [x] **Failing-first test** — the onboarding surface renders, is reachable **without a session**, and
      fails before it exists. Follow `console/test/shell.test.mjs`'s server-rendered precedent.
- [x] Reachable from the **footer** (`console__foot` in `App.vue`), not the main rail — it is a
      reference an agent author reaches for once, not a place an operator works.
- [x] **Readable signed out.** A test asserts it renders with no session and attempts no
      tenant-scoped read. An agent that must authenticate to learn how to authenticate is a closed
      loop.
- [x] **Nothing tenant-specific appears** — no connector list, no principal, no address, no count.
      It describes the shape of the service, never its contents. Assert this, do not intend it.
- [x] **Honest by construction:** what the page says an agent can do is derived from the same
      `surfaces.mts` declaration the navigation uses. A test asserts the page cannot present a
      capability whose surface is marked `built: false` — so today it must say an agent can be
      **minted** and cannot yet **authenticate** or **invoke**.
- [x] It states the concrete next step that actually works today: how a signed-in human mints an
      agent (`POST /api/agents`) and that the token is shown **once**.
- [x] Light and dark through `tokens.css`; no second colour vocabulary; no file under
      `console/src/components/` modified.

## Notes
- Read `docs/designs/agent-onboarding.md` first — especially §2, which is why this is derived rather
  than written. Copy describing a platform this young is false within a release.
- The tone to match is the README's: it states what is *not* built without apology and is the reason
  this repository's inventory is trustworthy. Do not write marketing.
- **Do not widen the server's anonymous surface in this story.** This is a console page reading what
  the console already knows. Publishing a machine-readable descriptor from the service is X-42, and
  that one is a disclosure decision.
- The tutorial an agent needs is short: what this is, what it holds for you, how you get an identity,
  what you can call. If it is long, it is describing something that does not exist yet.

## Progress
- **Done 2026-08-01.** Console 33 -> 42 tests; Rust unchanged at 44 + 213; build clean.
- **The derivation was falsified at integration, not taken on report.** Flipping `invoke` to
  `built: true` in `surfaces.mts` turns **four** tests red, including
  `the_derivation_is_live_and_not_a_coincidence`. The wiring is real; the page cannot claim a
  capability the rail marks unbuilt.
- **The rule is one-directional, and the code says so.** `identity` being built is not proof that
  `POST /api/agents` exists. What the derivation guarantees is that it can take a claim **off** the
  page, never put one **on** — the direction that protects a reader. Stated in `onboarding.mts`'s
  module doc rather than left to be discovered.
- **`authenticate` is withheld structurally, not by a sentence.** It is a step whose backing surface
  is `null`, and the implementor stated the rule one notch stronger than the Acceptance asked:
  *nothing may claim to work unless a **built** surface backs it*. X-37 will have to add a surface
  the navigation also shows in order to change that.
- **The path is `/connect`, not `/agents`** — X-38 needs `/agents` for a listing of a tenant's
  agents, which is real tenant data. Naming a page whose whole discipline is holding no tenant
  records after the collection it must never show is a trap worth not laying.
- **A deliberate future failure:** `available => call !== null` is asserted, so the day `invoke` or
  `subscribe` flips to built, this suite goes red until someone writes the instruction. That is
  intended, and it will look like an unrelated failure to whoever lands X-12 — recorded here so it
  reads as a prompt rather than a break.
- **Carried forward:** the `identity` surface backs the mint step. Repurposing `identity` in
  `surfaces.mts` would move the mint claim with it silently — the test asserts the derivation, not
  the mapping's judgement.
