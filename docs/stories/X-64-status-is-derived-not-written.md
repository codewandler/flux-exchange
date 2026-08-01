---
id: X-64
title: "Every page's \"is this built\" is derived, not written"
status: in-progress
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
- [x] Every capability page's status comes from the descriptor artifact. **No page states its own
      liveness in prose.** `web/.vitepress/descriptor.mts:statusFor` reads the committed artifact and
      `config.mts:transformPageData` stamps it; the pages carry a `capability:` key and no verdict.
- [x] **Failing-first test** — flipping a capability's `served` flag flips the rendered badge, with no
      edit to any page. Demonstrate it, the way X-52 demonstrated its mutations.
      `web/test/status.test.mjs::flipping a capability's served flag flips the rendered badge` drives
      `served` in `console/src/surfaces.mts` through `descriptorJson()`, rebuilds the whole site
      against the result, and reads `data-live` off the rendered `subscribe` page.
- [x] **Failing-first test** — a page for a capability the descriptor does not name fails the build,
      rather than rendering with no badge. A missing status must not read as "fine".
      `web/test/status.test.mjs::a page for a capability the descriptor does not name fails the build`
      builds against a descriptor with `invoke` deleted and asserts a non-zero exit naming the page.
- [x] The status is visible in the **page chrome**, not in a paragraph. The five renderings went wrong
      because the caveat and the claim drifted apart on the page. Rendered by
      `web/.vitepress/theme/index.mts` into the default theme's `doc-before` slot; asserted to appear
      before the page's own `<h1>` by `the status is in the page chrome, above the prose`.
- [x] The site build depends on the descriptor being current — the console suite already fails on
      drift, and the site must not assume that check ran. `assertDescriptorIsCurrent()` runs at config
      load and re-derives from `console/src/descriptor.mts` via `web/scripts/derive-descriptor.mjs`.

## Notes
- Derive the **status**; write the prose. The descriptor names capabilities and endpoints, and most of
  what a docs site is for — *why the credential never crosses the boundary* — is not in it and should
  not be.
- What this does **not** hold is a claim written in prose on a page. Say so on the site's contributing
  page rather than letting the next author assume the badge covers them.

## Progress

**Done.** The chain is `served` (`console/src/surfaces.mts`) → `available()` (`onboarding.mts`) →
`live` (the committed descriptor) → `data-live` in the page chrome. Nothing between the two ends is
written by an author.

How it is wired, for whoever picks up [[X-65]]:

- **`web/capabilities/` is the directory that means "this page is about a capability"**, and a page
  in it declares `capability: <id>` in frontmatter and nothing else about status. Both refusals live
  in `web/.vitepress/descriptor.mts:statusFor` — no key, or a key the descriptor does not publish.
- **Only a descriptor capability can carry a derived badge.** X-65's page list includes leases and
  workflows, which the descriptor does not name and cannot answer for — see the finding below before
  putting them under `capabilities/`.
- **Two pages, deliberately.** `invoke` (live) and `subscribe` (not), so both branches of the badge
  are rendered by a real page rather than by a component nobody has executed. That is the mechanism's
  floor, not the intended set; X-65 writes the volume.
- `FLUX_EXCHANGE_DESCRIPTOR_FIXTURE` is test-only plumbing that swaps the document badges derive
  from. It deliberately does **not** swap the one `assertDescriptorIsCurrent()` reads, so a fixture
  build can never switch off the staleness guard.
- The web tests went in a **new file**, `web/test/status.test.mjs`, rather than into
  `site.test.mjs` — different question (where did this come from, not what may this publish), and it
  keeps the conflict surface off a file other stories are editing.

**Two findings, neither fixed here:**

1. **X-65's Acceptance is not satisfiable as written.** It asks that every page in its list carry a
   derived status ([[X-64]]), but *leases*, *workflows* and *connections and credentials* are not
   capabilities in the agent descriptor — there is no `live` flag to derive from, and inventing one
   would mean writing into `surfaces.mts` a surface the route table cannot hold it to, which is the
   opposite of this mechanism. X-65 should either scope "derived status" to descriptor capabilities
   and put the rest outside `capabilities/`, or first extend the descriptor — a much larger change
   that reaches the Rust route-table guard.
2. **Prose tense on a planned page is unsettled.** `capabilities/subscribe.md` describes designed
   behaviour in the present tense, which is what the design intends (a planned page describes the
   surface as designed, and the chrome carries "not built"). It reads correctly *with* the badge
   above it and this is the arrangement the design asks for — but X-65 is about to write several more
   such pages, and a convention decided once beats six authors deciding separately.
