---
id: X-63
title: "A site exists, builds, and publishes"
status: done
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web]
note: "the scaffold and the pipeline, matched to flux-connectors/web: VitePress in web/, pages.yml with SHA-pinned actions, building on PRs as a gate and deploying only from main"
---

# A site exists, builds, and publishes

## Goal
A public URL serves a flux-exchange documentation site, and a broken site cannot reach it.

## Scope
The scaffold and the pipeline. **Three pages, not twenty** — overview, the credential-boundary
argument, and a placeholder index of the surface. Volume comes later, after [[X-64]] makes it safe.

## Match the sibling, do not reinvent

`flux-connectors/web` is the reference and its settled decisions come with reasons worth reading
before changing any of them:

- VitePress in `web/`, `npm ci` / `npm run build`, `node --test test/*.test.mjs`.
- `.github/workflows/pages.yml`: **SHA-pinned** actions (`scripts/check-action-pins.sh` enforces this
  repo-wide and will fail on a tag), pinned Node, `cache-dependency-path: web/package-lock.json`,
  build on `pull_request` as a gate, deploy only on push to `main`.
- `ignoreDeadLinks: false` — a dead internal link fails the build rather than publishing.
- `srcExclude: ['README.md']` — the contributor readme is not a page.
- `base: '/flux-exchange/'`. ⚠ **Do not set it to `/` because a CNAME file exists.** flux-connectors
  paid for this: the Pages API still reported `"cname": null` while the file was committed, and `'/'`
  404s every asset. Flip only once `gh api repos/codewandler/flux-exchange/pages` reports the cname.

## Acceptance
- [x] `web/` builds with `npm run build`, and the build is a **required gate on pull requests**.
- [x] **Failing-first test** — a dead internal link fails the build. Add one, watch it fail, remove it.
- [x] `pages.yml` passes `scripts/check-action-pins.sh` — every third-party action pinned to a SHA.
- [ ] The site is reachable at its published URL, and the one-time repository setting it needs
      (Settings → Pages → Source = GitHub Actions) is **written in the workflow header**, the way
      flux-connectors' is, because a workflow cannot do it for itself.
- [x] `AGENTS.md`'s gate section names the site build, so it is not a check only CI knows about.
- [x] Nothing deployment-specific and nothing credential-shaped appears on any page.

## Notes
- Keep the page count low deliberately. The interesting work is [[X-64]]; this story is plumbing, and
  plumbing that ships three honest pages is finished.

## Progress

Scaffold, pipeline and three pages are in. The one unticked box is unticked on purpose and is the
only thing left in this story.

**What is done**

- `web/` — VitePress 1.6.4, one dependency, its own lockfile and its own `.gitignore`. Three pages:
  `index.md` (overview), `boundary.md` (the credential-boundary argument), `surface.md` (the
  vocabulary index). `web/README.md` is the contributor readme and is `srcExclude`d.
- `.github/workflows/pages.yml` — builds on `pull_request` as a gate, deploys only from `main`, all
  four actions pinned to a commit SHA with the version as a trailing comment, Node pinned to 22,
  `cache-dependency-path: web/package-lock.json`.
- `AGENTS.md` § Build / test / run gained `cd web && npm ci && npm run build && npm test` plus why
  the build is a gate and not a formality.
- `web/test/site.test.mjs` — 8 guards over the **built** site. Each was verified to fail against a
  real violation, not merely written: base flipped to `'/'` → "404.html links to /assets/…, which is
  outside the deployed base"; an IP on a page → "surface.html publishes an IP address (127.0.0.1)";
  a fake token → "publishes what looks like a bearer token"; `srcExclude` removed → "web/README.md
  rendered into the site".

**The failing-first test.** `ignoreDeadLinks: false`. Appending `[a page that does not
exist](/channels)` to `surface.md` and running `npm run build` gives

```
(!) Found dead link /channels in file surface.md
x Build failed in 1.03s
build error: [vitepress] 1 dead link(s) found.
```

and exit 1; removing it builds clean. The link was removed, and `site.test.mjs` guards the setting
that makes that failure possible, since a one-word edit turns it off.

**What is deliberately not on the pages.** No claim that any capability is or is not live. The three
pages link to `GET /api/onboarding` as the live answer and say plainly that per-capability status
here arrives with [[X-64]]. That is an honest gap rather than a sentence that would be wrong in a
release — this repository corrected five renderings of one such sentence in a week, which is the
whole reason the epic is ordered mechanism-before-volume.

**The remaining box needs two things this branch cannot do**, and neither is a code change:

1. a push to `main`, so `deploy` runs for the first time; and
2. **Settings → Pages → "Build and deployment" → Source = GitHub Actions**, clicked once by a
   repository admin. Until it is, `build` still runs and still fails on a broken site, but `deploy`
   errors with "Get Pages site failed" / "Not Found". The instruction is in the `pages.yml` header,
   which is the only place it can live.

The URL will be `https://codewandler.github.io/flux-exchange/`, which is why `base` is
`'/flux-exchange/'`. No `CNAME` is committed — flipping `base` to `'/'` is warranted only once
`gh api repos/codewandler/flux-exchange/pages --jq .cname` reports a domain, and that value is pinned
in the test as well as the config so the flip takes two deliberate edits.

## Closed 2026-08-01 — done as far as a diff reaches, and one box left honestly unticked

Gate green: site builds, **8/8** site guards, 366 Rust, 16 actions pinned.

**One Acceptance item is not ticked and should not be**: *the site is reachable at its published URL*.
That needs a push to `main` **plus a repository admin setting Settings → Pages → "Build and
deployment" → Source = GitHub Actions**, which no workflow can do for itself. The instruction lives in
`pages.yml`'s header — the only place it can — and a test asserts the header survives. The implementor
declined to tick the box on the strength of intending it, which is the right call.

⚠ **Still to be clicked, once, by an admin:** Settings → Pages → Source = **GitHub Actions**. Until
then `build` runs and still fails on a broken site, but `deploy` errors with *"Get Pages site failed"*.
The site will serve at `https://codewandler.github.io/flux-exchange/`.

**Each of the eight guards was verified against a real violation rather than merely written** — base-
prefix drift (`404.html links to /assets/… which is outside the deployed base`), an IP address on a
page, a token-shaped string, and `srcExclude` removed so the contributor readme renders. That is the
discipline this epic needs, since the site's whole justification is not repeating the five renderings.

**The three pages claim nothing about what is live.** They route that to `GET /api/onboarding` and say
per-capability status arrives with [[X-64]] — the ordering the design insists on.

### Found on the way, and fixed at integration rather than filed

`README.md` carried **three** stale claims on the page a visitor reads first: `Status: v0.7.0`,
`# 167 tests`, and `Rust 1.87 or newer`. The last is the sharpest — **1.87 was false through three
releases**, which is exactly what X-30 corrected in the manifest, and this line was missed. All three
corrected, with the MSRV one carrying its own history so it is not quietly re-broken.

`AGENTS.md` said `v0.8.0` and still described invocation as *gated by identity alone, no grant model*,
which X-13 changed in v0.9.0 — and it is the file every agent reads first.

### Carried
- **`deploy` is untested and untestable from here.** Its first real run is the first push to `main`
  after the Pages source is set; if it fails it will be the environment or `path: web/.vitepress/dist`.
- **`npm test` alone is meaningless** — the guards read `dist/`, so the suite must follow
  `npm run build`. It fails loudly rather than silently, but the ordering is now in `AGENTS.md` and
  `pages.yml` for a reason.
- The off-site link allow-list is `github.com` only; a page linking to `vitepress.dev` fails the
  suite. Deliberate, one line to widen, and it will surprise whoever hits it first.
