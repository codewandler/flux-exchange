---
id: X-63
title: "A site exists, builds, and publishes"
status: ready
priority: 1
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
- [ ] `web/` builds with `npm run build`, and the build is a **required gate on pull requests**.
- [ ] **Failing-first test** — a dead internal link fails the build. Add one, watch it fail, remove it.
- [ ] `pages.yml` passes `scripts/check-action-pins.sh` — every third-party action pinned to a SHA.
- [ ] The site is reachable at its published URL, and the one-time repository setting it needs
      (Settings → Pages → Source = GitHub Actions) is **written in the workflow header**, the way
      flux-connectors' is, because a workflow cannot do it for itself.
- [ ] `AGENTS.md`'s gate section names the site build, so it is not a check only CI knows about.
- [ ] Nothing deployment-specific and nothing credential-shaped appears on any page.

## Notes
- Keep the page count low deliberately. The interesting work is [[X-64]]; this story is plumbing, and
  plumbing that ships three honest pages is finished.
