---
id: X-149
title: "Adopt the current vite line in the console"
status: ready
priority: 3
areas: [console, build]
note: "vite 6 -> 8 and @vitejs/plugin-vue 5 -> 6 are one change: neither resolves without the other, and vite 8 swaps the bundler from esbuild to rolldown"
---

# Adopt the current vite line in the console

## Goal

Move `console/` onto the current `vite` line, as one reviewed change rather than two dependabot
bumps that cannot land separately.

## Why this is a story and not a version bump

Dependabot filed the two halves as **#54** (vite 6.4.3 → 8.2.1) and **#5** (`@vitejs/plugin-vue`
5.2.4 → 6.0.8). Neither can merge alone:

- **#54 alone does not install.** `npm ci` fails before it reaches any test or audit step:

  ```
  npm error ERESOLVE unable to resolve dependency tree
  npm error peer vite@"^5.0.0 || ^6.0.0" from @vitejs/plugin-vue@5.2.4
  ```

- **#5 alone is harmless but pointless.** plugin-vue 6.0.8 peers `vite ^5 || ^6 || ^7 || ^8`, so it
  is compatible with today's vite 6 *and* is the prerequisite that makes #54 resolvable.

So they are one commit. That much is mechanical. What makes it a story is the second fact:

**vite 8 replaces the bundler.** esbuild gives way to rolldown. That is a change to how the console
is built and to what it emits, not a version number — it deserves its own review, its own changelog
line, and a look at the built output rather than a green check.

## Acceptance

- [ ] `vite` and `@vitejs/plugin-vue` move to the current published line **in one commit**, with
      `console/package.json` and `console/package-lock.json` in step.
- [ ] `cd console && npm install && npm test && npm run build` passes — the exact sequence
      [`AGENTS.md`](../../AGENTS.md) § Build / test / run specifies. All 125 console tests stay green.
- [ ] `npm audit --audit-level=high` reports zero vulnerabilities, so the gate that
      [[X-92]] relies on stays meaningful.
- [ ] The built output is **inspected, not just produced**: the emitted bundle still loads, the
      console still reaches `/api` same-origin, and the rolldown output is compared against the
      esbuild output for anything that changed shape rather than size.
- [ ] `node scripts/agent-descriptor.mjs` is re-run if anything under `console/src/` changed, so the
      committed descriptor cannot go stale — the public-site build reads it and turns red otherwise.
- [ ] The public site (`web/`) build and tests still pass. It is a separate Node tree and shares no
      lockfile, so it should be unaffected — confirm rather than assume.
- [ ] The changelog records the bundler change as a Changed entry, naming esbuild → rolldown.

## Progress

- 2026-08-12: Filed. Both dependabot PRs left open and commented with the evidence rather than
  closed, so this story has something to point at. #7 (typescript 7) was closed separately and is
  **not** part of this: it fails for an unrelated reason — `vue-tsc` resolves
  `typescript/lib/tsc`, and TypeScript 7's `exports` map does not define that subpath.

## Notes

- Do not fold #8 (`vue-tsc` 2.2.12 → 3.3.9) in here on the assumption that it pairs with a
  TypeScript bump — it does not. vue-tsc 3.3.9 still resolves `typescript/lib/tsc` and its alias
  handling caps at `@typescript/typescript6`, so it is compatible with the current TypeScript 5.9.3
  and stands on its own.
- `console/` is a separate Node tree from `web/`; they share no lockfile and no dependency.
