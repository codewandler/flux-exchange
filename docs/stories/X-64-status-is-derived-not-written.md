---
id: X-64
title: "Every page's \"is this built\" is derived, not written"
status: ready
priority: 1
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web, console]
note: "the mechanism that makes a large speculative site safe — status badges read the same descriptor artifact whose live flags are held to the route table by X-42/X-52's tests"
---

# Every page's "is this built" is derived, not written

## Goal
A page cannot claim a capability is live, or not live, without the route table agreeing.

## Why this is the story that matters

This repository corrected **five separate renderings** of one false claim in a single week — that
`invoke` was not built — across the onboarding page, the mint screen, the shell inventory, the
descriptor and the catalogue explorer, plus a sixth in a README. Every one was written honestly. Every
one went stale. Each was caught by a review, not by a mechanism.

**A documentation site is a factory for that failure**, and [[X-65]] is about to add a page per
planned capability. This story is what makes that safe, and it must land first.

The mechanism already exists and is the only honesty device here that has survived review: X-42's
`GET /api/onboarding` descriptor, generated from `console/src/onboarding.mts` into a committed
artifact, whose `live` flags are held to `routes::MODULES` in both directions —
`a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`, tightened by X-52 so a
capability cannot even name a route that does not serve it.

## Acceptance
- [ ] Every capability page's status comes from the descriptor artifact. **No page states its own
      liveness in prose.**
- [ ] **Failing-first test** — flipping a capability's `served` flag flips the rendered badge, with no
      edit to any page. Demonstrate it, the way X-52 demonstrated its mutations.
- [ ] **Failing-first test** — a page for a capability the descriptor does not name fails the build,
      rather than rendering with no badge. A missing status must not read as "fine".
- [ ] The status is visible in the **page chrome**, not in a paragraph. The five renderings went wrong
      because the caveat and the claim drifted apart on the page.
- [ ] The site build depends on the descriptor being current — the console suite already fails on
      drift, and the site must not assume that check ran.

## Notes
- Derive the **status**; write the prose. The descriptor names capabilities and endpoints, and most of
  what a docs site is for — *why the credential never crosses the boundary* — is not in it and should
  not be.
- What this does **not** hold is a claim written in prose on a page. Say so on the site's contributing
  page rather than letting the next author assume the badge covers them.
