# web — the public documentation site

The [VitePress](https://vitepress.dev) site published to
<https://codewandler.github.io/flux-exchange/> by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) on every push to `main`. Pull
requests build it too, and the build is a gate: a broken site fails the check rather than publishing.

**The Node toolchain is contained here.** `package.json`, `package-lock.json` and `node_modules` all
live under `web/`; nothing about the Rust workspace at the repository root knows or cares that this
directory exists, and neither does `console/`, which is a separate Node tree with a separate lockfile.

## Build

Requires Node 22+.

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
| `.vitepress/config.mts` | Site config — title, nav, sidebar, and the Pages **base path**. |
| `index.md` | Overview: what flux-exchange is, and where the current answer about capabilities lives. |
| `getting-started.md` | Clone, arm a local identity, sign in, reach the console — and what must be true before anything runs. |
| `boundary.md` | The credential-boundary argument. |
| `surface.md` | An index of the vocabulary — the things this service deals in. |
| `test/` | Guards over the built site: the base path, and the two content rules below. |

Four pages. Three were X-63's deliberate floor; `getting-started.md` is
[X-69](../docs/stories/X-69-run-it-yourself.md), and it is **not** the volume that story deferred —
it describes how to run the software rather than claiming what a build can do, so it does not wait on
the derived badge. That volume still comes later, and the ordering is the whole point of the epic:
[X-64](../docs/stories/X-64-status-is-derived-not-written.md) makes each page's status a **derived**
fact before [X-65](../docs/stories/X-65-the-whole-surface-is-on-the-page.md) writes the pages that
would otherwise carry it as prose.

`getting-started.md` is the site's first page with a fenced example, and it is why `test/` grew a
code-block reader: the highlighter puts every token in its own element, so the scan that looks for a
value on the right-hand side of an `=` could not see inside a code block at all. It can now, and the
one exemption — `FOO=<a placeholder>` is a grammar rather than a value — is held to its width by a
test of its own.

## What this site must not publish

It is a public page about a service that holds other people's credentials.

- **No deployment-specific fact.** The site describes the software, never an instance. No hostname,
  no address, no port, no environment's configuration.
- **Nothing that is not already public.** `GET /api/onboarding` is anonymous and its disclosure list
  was reviewed field by field; that list is the ceiling, not a starting point.
- **No configuration example containing anything credential-shaped**, however obviously fake. A
  copyable example is a copied example.
- **No claim that a capability is or is not live.** Not until [X-64] makes that a derived fact. The
  live answer is the descriptor, and the site links to it rather than restating it — this repository
  corrected five renderings of one stale capability claim in a single week, and a public site is a
  factory for that failure.

`test/site.test.mjs` enforces the first and third of those mechanically over the built HTML. The
second and fourth are review's job, and the test says so in place rather than implying coverage it
does not have.

[X-64]: ../docs/stories/X-64-status-is-derived-not-written.md

## The base path

`.vitepress/config.mts` sets `base: '/flux-exchange/'`, because that is where GitHub actually serves
a *project* Pages site. Renaming the repository, or moving to a custom domain, means changing that
value too — or every bundled asset resolves a level too high and 404s.

This repository publishes no `CNAME`, and the sibling's experience is why the config carries a
warning about adding one: in `flux-connectors` the base was flipped to `'/'` on the strength of a
committed CNAME file and shipped an unstyled site, because a committed CNAME is a *request* for a
custom domain and GitHub had not accepted it. Flip the base **only** once
`gh api repos/codewandler/flux-exchange/pages --jq .cname` reports the domain.
