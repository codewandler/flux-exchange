---
id: X-65
title: "The whole intended surface is on the page, including what is not built"
status: ready
priority: 2
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web]
note: "channels, subscribe, leases, workflows — the brief's \"scaffold the whole future surface\", safe to write only once X-64 makes status derived"
---

# The whole intended surface is on the page, including what is not built

## Goal
Somebody evaluating this platform can see the whole shape of it, and can tell built from planned at a
glance.

## The vocabulary is settled — use it, do not invent it

All of this is `docs/vision.md`'s and none of it is new:

- **`invoke` and `subscribe` are two verbs of one remote connector binding**, not two features
  (`vision.md:58`). Pages that present them as separate products get the model wrong at the top.
- **The three lifetimes** (`vision.md:62-71`) exist because conflating them produces real bugs — the
  vision's own example is *a webhook endpoint that dies when an agent's session ends*:

  | | scoped to | direction | ends when |
  |---|---|---|---|
  | **Session** | a caller's conversation | — | the conversation does |
  | **Channel** | a deployment | pushes | the operator removes it |
  | **Lease** | a caller's grant | pulls | the holder releases it, or TTL |

- **A webhook is a Channel.** One word per thing. A page that calls the same object a webhook, a
  trigger and a subscription depending on the paragraph is the thing this table exists to prevent.
- ⚠ **Triggers, conditions and schedules are not flux-exchange features.** `vision.md:106` says a
  workflow is a stored, versioned `flux-app` Program. **Documenting them as capabilities of this
  service would be the largest untruth on the site** — the page that covers them says where they
  actually live and why that boundary is where it is.
- **The inbound confused-deputy argument** — *a subscriber cannot name a binding it has not been
  granted* (`vision.md:50`) — is the mirror of the outbound one and is the reason `subscribe` is
  interesting rather than routine.

## Acceptance
- [ ] A page for each of: connections and credentials, `invoke`, `subscribe` and channels, leases,
      grants, agents, workflows-and-where-they-live.
- [ ] Every one carries a derived status ([[X-64]]) — **no page asserts its own liveness**.
- [ ] The three lifetimes appear **once**, as the table, linked from anything that names one. Not
      restated per page, because restating is how two copies disagree.
- [ ] Planned pages name the story that would build them, so a reader can follow the actual work.
- [ ] **Failing-first test** — no page describes a trigger or schedule as something this service runs.
- [ ] Nothing deployment-specific; nothing beyond what `GET /api/onboarding` already discloses
      publicly. X-42's reviewed field list is the ceiling.

## Notes
- Depends on [[X-64]]. Writing these pages before status is derived is how the sixth rendering gets
  published, at a public URL, to readers with no way to check.
- Prefer fewer, truer pages. Four that stay right beat twenty that rot, and the ordering of this epic
  says so.
