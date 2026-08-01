---
id: X-71
title: "The console's dev server follows the address the service was told to bind"
status: ready
priority: 3
areas: [console]
note: "found by X-69 while walking its own page, 2026-08-01: console/vite.config.ts hard-codes the default bind, so a reader who sets FLUX_EXCHANGE_BIND gets a console that cannot reach the service"
---

# The console's dev server follows the address the service was told to bind

## Goal
Following the getting-started page with a non-default bind produces a working console.

## How it was found

X-69 walked its own page end to end, hit a port already in use, moved `FLUX_EXCHANGE_BIND` — and the
console's dev server kept proxying `/api` to the default. It had to move the bind back to finish the
walkthrough.

`console/vite.config.ts` hard-codes the address. The public page now names the constraint instead,
which is honest and is not a fix: the first thing a reader does when a port is taken is change it.

## Acceptance
- [ ] The dev server's proxy target follows `FLUX_EXCHANGE_BIND` when it is set, and falls back to the
      default when it is not.
- [ ] **Failing-first test** — the resolved proxy target is derived from the environment, asserted
      without starting a server.
- [ ] `web/getting-started.md`'s note about the constraint is removed or rewritten in the same change,
      so the page and the tool do not disagree.

## Notes
- Small, and it is on the path of the first thing a stranger does with this project, which is why it
  is worth a story rather than a comment.
