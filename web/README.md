# web — the public documentation site

The [VitePress](https://vitepress.dev) site published to
<https://codewandler.github.io/flux-exchange/> by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) on every push to `main`. Pull
requests build it too, and the build is a gate: a broken site fails the check rather than publishing.

**The Node toolchain is contained here.** `package.json`, `package-lock.json` and `node_modules` all
live under `web/`; nothing about the Rust workspace at the repository root knows or cares that this
directory exists, and neither does `console/`, which is a separate Node tree with a separate lockfile.

The site build does read two files from outside this directory, and both are about the
[status badge](#the-status-badge-is-derived): the committed agent descriptor at
`crates/exchange-server/src/routes/onboarding.json`, and `console/src/descriptor.mts`, which it
re-derives to check that the first one is current. Neither is a *dependency* — no package, no
lockfile entry, nothing installed — and the console derivation is pure TypeScript that Node's own
type stripping runs, so `web/` still shares no build with either tree.

## Build

Requires Node 22.18+ — 22+ for VitePress, and the `.18` for the type stripping that lets the
descriptor check run `console/src/descriptor.mts` without a compiler or a dependency.

```bash
cd web
npm ci          # or `npm install` on first setup, to create the lockfile
npm run build   # static site into web/.vitepress/dist
npm test        # the site's own guards — run after `npm run build`
```

Other scripts: `npm run dev` for a local dev server with hot reload, `npm run preview` to serve the
built output with the base path applied.

`npm test` is Node's built-in runner over `test/*.test.mjs`; it adds no dependency. It reads the
built HTML in `.vitepress/dist`, so it must follow a build — see the comment at the top of
[`test/site.test.mjs`](test/site.test.mjs) for what it asserts and, more usefully, what it cannot.

## Layout

| Path | What it is |
|---|---|
| `.vitepress/config.mts` | Site config — title, nav, sidebar, the Pages **base path**, and the hook that stamps each page's derived status. |
| `.vitepress/descriptor.mts` | Where a status badge comes from: reads the agent descriptor, checks it is current, refuses a page it cannot answer for. |
| `.vitepress/theme/` | The default theme plus one slot — the badge, rendered in the page chrome. |
| `scripts/derive-descriptor.mjs` | Prints what `console/src/descriptor.mts` derives, so the build can compare. |
| `index.md` | Overview: what flux-exchange is, and where the current answer about capabilities lives. |
| `getting-started.md` | Clone, arm a local identity, sign in, reach the console — and what must be true before anything runs. |
| `boundary.md` | The credential-boundary argument. |
| `surface.md` | An index of the vocabulary — the things this service deals in. |
| `capabilities/` | One page per capability, each carrying a **derived** status. See below. |
| `test/` | Guards over the built site: the base path, the content rules below, where a family link goes, and the derivation. |

Three pages were X-63's deliberate floor; `getting-started.md` is
[X-69](../docs/stories/X-69-run-it-yourself.md), and it is **not** the volume that story deferred —
it describes how to run the software rather than claiming what a build can do, so it did not wait on
the derived badge.

[X-64](../docs/stories/X-64-status-is-derived-not-written.md) is the badge, and `capabilities/`
holds its first two users: `invoke` and `subscribe`, the two verbs of one binding, one served by this
build and one not. **Two is the mechanism's floor rather than the intended set** — one live and one
planned, so both answers a badge can give are rendered by a real page and neither branch is a
component nobody has executed.
[X-65](../docs/stories/X-65-the-whole-surface-is-on-the-page.md) writes the rest of the surface on
top of it. That ordering is the whole point of the epic: the mechanism lands before the volume that
would otherwise carry status as prose.

`getting-started.md` is the site's first page with a fenced example, and it is why `test/` grew a
code-block reader: the highlighter puts every token in its own element, so the scan that looks for a
value on the right-hand side of an `=` could not see inside a code block at all. It can now, and the
one exemption — `FOO=<a placeholder>` is a grammar rather than a value — is held to its width by a
test of its own.

## Where a family link goes

A link about **what `flux` or `flux-connectors` is** goes to the site that project publishes, not to
its repository, so that the three sites read as one product
([X-77](../docs/stories/X-77-the-family-links-go-to-the-family-sites.md)). The nav carries both on
every page, and `test/site.test.mjs` holds the rule — the build cannot, because `ignoreDeadLinks`
resolves internal links only and a link to the wrong external host is not dead.

**The discriminator is the link's subject, not its hostname.** `getting-started`'s clone URL,
`surface`'s pointer to the itemized inventory in the README, `index`'s `#what-exists-today` deep link
and the `Releases (GitHub)` nav entry all mean the repository, and github.com is the right address
for every one of them. The comment on `subjectIsTheProject` in `test/site.test.mjs` is the statement
of record, including what that rule deliberately does not catch.

## What this site must not publish

It is a public page about a service that holds other people's credentials.

- **No deployment-specific fact.** The site describes the software, never an instance. No hostname,
  no address, no port, no environment's configuration.
- **Nothing that is not already public.** `GET /api/onboarding` is anonymous and its disclosure list
  was reviewed field by field; that list is the ceiling, not a starting point.
- **No configuration example containing anything credential-shaped**, however obviously fake. A
  copyable example is a copied example.
- **No page states its own liveness.** Whether a capability is built is a derived fact, and the
  mechanism below is how. Writing it into a sentence is the failure this site was built expecting.

`test/site.test.mjs` enforces the first and third of those mechanically over the built HTML. The
second is review's job, and the test says so in place rather than implying coverage it does not
have. The fourth has a mechanism and a gap, and the gap is stated below because assuming it is
covered is worse than knowing it is not.

**Every one of those rules is a loop over the built pages, so which pages get looped over is itself
load-bearing.** `test/rendered.mjs` is the single enumerator all the suites share, and its `pages()`
**excludes nothing whatsoever** — every `.html` file the build produced, at any depth, in any
directory. If it is published, it is read.

That is not defensive tidying, and it took two rounds of review to get right. X-64 added
`capabilities/`, the first pages this site ever published below the root, while the enumerator was a
single non-recursive `readdirSync(dist)`: those pages were scanned by *none* of the rules above, and
an IP address, a `host:port` endpoint and a bearer token reached the live public site with the full
gate green. The fix recursed — and still carried a list of directories to skip, shared with the walk
that predicts which pages *should* exist. Correct there, a hole here: `dist/test/` and
`dist/scripts/` went unread, and the coverage check could not see it because both halves were blind
in the same five places, so the predicted set and the scanned set agreed.

The rule that came out of it, worth keeping in mind before adding any filter: **a claim about what
should be published is never a licence to skip reading something that was.** Excluding on the way in
is a content decision; excluding on the way out is a blind spot.

Two independent defences now, and `test/coverage.test.mjs` holds both:

- **Nothing outside the content directories publishes at all.** `srcExclude` is built from
  `.vitepress/content.mts`, which is the same constant the suite predicts from — one list, so the
  publisher and the predictor cannot drift into agreeing wrongly. A markdown file in `test/`,
  `scripts/`, `.vitepress/`, `public/` or `node_modules/` is not a page.
- **Anything that publishes anyway is still scanned.** Because `pages()` filters nothing.

So **adding a page anywhere under `web/` requires nothing of you.** If it renders and the suite is
not reading it, `coverage.test.mjs` goes red.

## The status badge is derived

This repository corrected **five separate renderings** of one false claim in a single week — that
`invoke` was not built — each written honestly, each stale within a release, each caught by a review
rather than by a mechanism. A documentation site is a factory for that failure, so no page here
answers "is this built" for itself.

The answer comes from `GET /api/onboarding`, the anonymous machine-readable descriptor whose `live`
flags are held to the service's own route table **in both directions** by a Rust test — a route
landing or leaving turns that gate red until `console/src/surfaces.mts` agrees with it. The site
reads the committed artifact that descriptor is served from.

**To write a page whose subject is a capability:** put it in `capabilities/` and name the capability
in its frontmatter. Nothing else.

```md
---
capability: subscribe
---
```

`.vitepress/config.mts` resolves that id against the descriptor at build time and stamps the answer
onto the page; `.vitepress/theme/` renders it above the page's own heading. **The chrome, not a
paragraph** — the five renderings went wrong partly because the caveat and the claim drifted apart
on the page, and a badge an author types into prose is that failure waiting again.

Two things fail the build rather than rendering:

- a page in `capabilities/` that declares no `capability:`, and
- a page naming a capability the descriptor does not publish — either it describes something this
  service does not have, or the capability has left the descriptor and this page is the last thing
  on the site still claiming it.

Both refuse for one reason: **a missing status must not read as "fine".** Absence is exactly how a
stale claim survives.

The build also **re-derives the descriptor from `console/src/descriptor.mts` and fails if the
committed artifact is stale**, rather than trusting that the console suite ran. `web/` and `console/`
are separate trees that can be tested apart, and a site deriving badges from a stale copy would be
wrong in the one place it advertises as derived. When that fails, run
`node scripts/agent-descriptor.mjs` from `console/`.

### What the badge does not hold

**It holds the badge, and nothing else on the page.** A sentence in your prose claiming a capability
works, or does not, is held by nobody — no regex separates "this service invokes" from a definition
of what invoking is, which is why `test/site.test.mjs` says in place that it does not try.

So the rule for an author is not "the badge has me covered". It is: **state the status nowhere, and
let the chrome say it.** Describe what a capability *is*, why the boundary is where it is, what a
caller may not name — the things the descriptor does not carry and should not. If you catch yourself
writing "not yet built" into a paragraph, that belongs in `console/src/surfaces.mts`, where flipping
it moves the badge on every page at once.

## The base path

`.vitepress/config.mts` sets `base: '/flux-exchange/'`, because that is where GitHub actually serves
a *project* Pages site. Renaming the repository, or moving to a custom domain, means changing that
value too — or every bundled asset resolves a level too high and 404s.

This repository publishes no `CNAME`, and the sibling's experience is why the config carries a
warning about adding one: in `flux-connectors` the base was flipped to `'/'` on the strength of a
committed CNAME file and shipped an unstyled site, because a committed CNAME is a *request* for a
custom domain and GitHub had not accepted it. Flip the base **only** once
`gh api repos/codewandler/flux-exchange/pages --jq .cname` reports the domain.
