---
id: X-77
title: "A reader following a family link lands on a documentation site, not on a repository"
status: in-progress
priority: 1
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web]
note: "owner-raised 2026-08-02, the day the site went live: index.md sends a reader to github.com for flux and flux-connectors, so following the family link drops them out of the docs and into a source tree. Both sibling sites are published and neither is linked"
---

# A reader following a family link lands on a documentation site, not on a repository

## Goal
Following flux-exchange's link to flux or flux-connectors keeps a reader inside the family's
documentation, so three sites read as one product rather than three repositories.

## The measurement

`web/index.md` says flux-exchange is *"the platform layer of the flux family"* and then sends the
reader to a source tree:

- `index.md:30` — `[flux](https://github.com/codewandler/flux)`
- `index.md:32` — `[flux-connectors](https://github.com/codewandler/flux-connectors)`

Both siblings publish. Verified 2026-08-02: `https://codewandler.github.io/flux/` and
`https://codewandler.github.io/flux-connectors/` each answer **200**, and `gh api …/pages` reports no
`cname` for either — so the github.io URL is the address, not a redirect to somewhere better.

This only became worth fixing on the day this site started serving. Before that, a github.com link was
the only honest destination.

## Which links change, and which deliberately do not

**Change** — a link whose subject is *what the sibling is or does*: the two in `index.md`, and a nav
or footer entry so the family is reachable from every page rather than only from the overview.

**Leave** — a link whose subject is *the repository itself*. `getting-started.md:22`'s `git clone`,
`surface.md:91`'s pointer to the itemized inventory in the README, `index.md:70`'s deep link to
`#what-exists-today`, and the config's `Releases (GitHub)` entry all mean the repository and are
correct. The rule is the subject, not the hostname — swapping those would send a reader looking for a
clone URL to a landing page.

## The guard, because the build will not catch this

`ignoreDeadLinks: false` is what makes a broken link fail `npm run build`, and it checks **internal**
links. An external link to the wrong host is not dead and will never fail a build. So the rule needs a
test in `web/test/site.test.mjs`, which already asserts over the built `dist` and already holds eight
guards of exactly this shape.

## Acceptance
- [x] `index.md`'s flux and flux-connectors links point at the published sites, and the family is
      reachable from the nav or footer on every page.
- [x] **Failing-first test** — a guard in `web/test/site.test.mjs` asserting that a family *link*
      resolves to the sibling **site** and not to `github.com`. Point one back at github.com, watch it
      fail with a message naming the page and the URL, then fix it. A guard that has not been seen
      firing is not evidence.
- [x] The guard distinguishes the two categories above, so a legitimate repository link — the clone
      URL, the releases entry — does not trip it. State the discriminator in the test's own comment;
      the next person to add a github.com link will read that comment and not the story.
- [x] `npm run build && npm test` green, and the deployed base path guard still passes.

## Progress
- **Done.** `index.md:30,32` now link `https://codewandler.github.io/flux/` and
  `https://codewandler.github.io/flux-connectors/`, and `.vitepress/config.mts` carries a
  `The flux family` nav dropdown with both — VitePress renders a flyout's items into the static HTML,
  so the links are on every page of the built site rather than only in the client bundle. `404.html`
  is the one page without them and it is the one page VitePress renders without the theme shell.
- **Three guards, not one**, all in `web/test/site.test.mjs`:
  `a link about a sibling project goes to that project’s site, not to its repository` is the rule;
  `the family is reachable from every page, not only from the overview` is the nav half, which a
  subject rule alone cannot express; and `the subject rule admits a repository link and still catches
  a family link` pins the discriminator in both directions, following the shape the
  environment-variable rule already uses here.
- **The discriminator is two questions, and it lives on `subjectIsTheProject`.** A URL addressing
  anything *inside* the repository — a path, a fragment, a releases page — is a repository link and no
  site serves it; a bare repository URL is judged by the anchor's own words, and only the project's own
  name makes it a link about the project. That admits `surface.md:91`'s `repository`, `index.md:70`'s
  `what exists today`, the `Releases (GitHub)` entry, the wordless social icon and
  `getting-started.md:22`'s clone URL, which is not an anchor at all.
- Seen firing twice: at the merge base against the real `index.md` links, and again with the nav entry
  deliberately pointed back at `https://github.com/codewandler/flux`, where it named `boundary.html`
  and the URL — proof it covers pages the overview does not.
- `the site links off-site only to the flux family` had a one-host allow-list and would have refused
  the new links; `codewandler.github.io` was added to it with the reason, since that test asks a
  different question (is this host ours) from the new one (is this link's subject the project).
- `web/README.md` gained a `Where a family link goes` section, because a contributor reads it before
  the test file.

## Notes
- **The same gap exists upstream, and it is not ours to close here.** `flux-connectors/web/index.md:34`
  links flux by github.com too. The consistent experience the owner asked for needs the sibling repos
  to make the reciprocal change; file it there rather than working around it here.
- Cheap, self-contained, and touches only `web/`. It conflicts with nothing else on the board, which
  makes it a good passenger in a wave alongside work in `crates/` or `console/`.
