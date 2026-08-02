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
- **There is no way to point a real build at a different descriptor.** `readDescriptor()` reads the
  committed artifact and nothing else. The hypothetical builds the two demonstrations need are
  injected in-process, through VitePress's `onAfterConfigResolve` hook, which the CLI does not
  expose — see the rework note below for why an environment variable was the wrong answer.
- The web tests went in a **new file**, `web/test/status.test.mjs`, rather than into
  `site.test.mjs` — different question (where did this come from, not what may this publish), and it
  keeps the conflict surface off a file other stories are editing.
- **Adding a page requires nothing of you.** `web/test/rendered.mjs` is the one enumerator every
  suite scans through, and `web/test/coverage.test.mjs` fails if it stops covering something the
  site publishes. Do not write a second enumerator.

## Rework — what independent review found after the first merge

Both findings were real and both are fixed on top of `c84c3dc`.

1. **The capability pages escaped every content guard on the site, and the gate stayed green.**
   `site.test.mjs` enumerated pages with a non-recursive `readdirSync(dist)`. That was total coverage
   until this story published the first pages below the root of `dist` — after which
   `capabilities/invoke.html` and `capabilities/subscribe.html` were read by *none* of: no
   deployment-specific fact, nothing credential-shaped, the base-path check, and X-77's two
   family-link guards. Demonstrated rather than deduced: an IP address, a `host:port` endpoint and a
   bearer token injected into `capabilities/invoke.md` published to `dist` with **23/23 passing**.

   A scanner given fewer files does not fail; it passes sooner. So the fix is not "recurse" — that is
   only today's bug. There is now one enumerator (`web/test/rendered.mjs`) and `coverage.test.mjs`
   measures it against the markdown sources VitePress actually publishes, so a page added at any
   depth is covered by nobody remembering anything. The same non-recursive flaw was in
   `status.test.mjs`, twice, and is gone with it.

   Worse than the bug: this story's own commit *asserted* the enforcement it had just removed, in
   `web/README.md` and `AGENTS.md`. Both now describe what is actually true, and say why.

2. **The fixture escape hatch was a documented path around this story's central claim.**
   `readDescriptor()` read `process.env[FLUX_EXCHANGE_DESCRIPTOR_FIXTURE] ?? artifactPath()` — in
   production build code. A build with that set published badges derived from arbitrary JSON while
   `assertDescriptorIsCurrent()` passed, because the guard checked the committed artifact and not
   what the badges read. Not reachable from `pages.yml`, so not an incident, but the proposition here
   is *a page cannot claim a capability is live without the route table agreeing*, and that was a way
   around it shipping inside the mechanism.

   Fixed by removing it: production code reads one file. The demonstrations drive VitePress in
   process and override the resolved `transformPageData` through `onAfterConfigResolve`, a Node-API
   hook the CLI does not expose. They still exercise the production `statusFor`, and the real config
   still loads, so a hypothetical build proves the committed artifact current on the way past.

Also: the Node floor is now stated in `web/package.json`'s `engines` and checked in
`assertDescriptorIsCurrent` before the spawn, so an old runner gets a version message rather than a
TypeScript syntax error out of a subprocess.

### Round two — the same hole, one directory over

The recursion fix carried a list of directories to skip, and `walk()` was shared. Correct for the
walk that *predicts* which pages should exist — it must not descend into `node_modules` — and a hole
in the walk that *reads* which pages do exist: `dist/test/` and `dist/scripts/` went unscanned.
`coverage.test.mjs` could not see it, because both halves were blind in the same five places and so
the predicted set and the scanned set agreed. A markdown file in `web/test/` published to a public
page carrying a bearer token, with all 25 tests green.

**The rule that came out of it, and the one to keep:** a claim about what should be published is
never a licence to skip reading something that was. Excluding on the way in is a content decision;
excluding on the way out is a blind spot.

Two independent defences, because either alone has a failure mode:

- `pages()` excludes **nothing** — every `.html` in the output, any depth, any directory. Anything
  published is scanned, whatever else goes wrong.
- Nothing outside the content directories publishes at all. `srcExclude` is built from
  `.vitepress/content.mts`, which is also what the suite predicts from — **one** list, so the
  publisher and the predictor cannot drift into agreeing wrongly, which was the actual defect rather
  than a hypothetical one.

`coverage.test.mjs` holds both, and its self-test now builds a fixture whose directories are named
`test`, `scripts`, `node_modules`, `public` and `.vitepress`, so the output walk is proved to
descend into the exact names the source walk skips. That test fails against the implementation it
replaced.

Two smaller things from the same round: the docstring arguing the escape hatch was shut cited
`web/test/fixtures/hypothetical-build.mts`, a file abandoned when `--config` turned out not to exist
in VitePress 1.6.4 — it now names the real mechanism, `onAfterConfigResolve`. And the Node check
admitted 23.0–23.5, where type stripping is still flag-gated; it is two ranges now, matching
`engines`.

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
