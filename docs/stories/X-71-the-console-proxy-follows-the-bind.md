---
id: X-71
title: "The console's dev server follows the address the service was told to bind"
status: in-progress
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
- [x] The dev server's proxy target follows `FLUX_EXCHANGE_BIND` when it is set, and falls back to the
      default when it is not.
- [x] **Failing-first test** — the resolved proxy target is derived from the environment, asserted
      without starting a server.
- [x] `web/getting-started.md`'s note about the constraint is removed or rewritten in the same change,
      so the page and the tool do not disagree.

## Notes
- Small, and it is on the path of the first thing a stranger does with this project, which is why it
  is worth a story rather than a comment.

## Progress

**Done.** `console/vite.proxy.mts` resolves the target; `console/vite.config.ts` calls it; five tests
in `console/test/proxy.test.mjs` assert it with no dev server running.

- **The environment arrives as an argument.** `apiProxyTarget(env)` takes a
  `Record<string, string | undefined>` and `vite.config.ts` calls it with no argument, at which point
  it reads `process.env` off `globalThis`. That is what settles the objection the old comment in
  `vite.config.ts` raised — reading the environment needs no `@types/node`, because nothing here
  reaches for a global the console's `tsconfig` does not have. `vue-tsc --noEmit` is green.
- **The last test is the one that matters.** Four assert the resolver; the fifth sets the variable,
  imports the real `vite.config.ts`, and reads `server.proxy['/api'].target` off the object the dev
  server would load. Without it a resolver nobody called would pass. It is also what failed at the
  base with `actual 'http://127.0.0.1:8080'` against `expected 'http://127.0.0.1:9091'` — the module
  imports in the file are deliberately dynamic so a missing resolver does not take that assertion
  down before it runs.
- **The address is used as written, and blank is the only exception.** `0.0.0.0` is not rewritten to
  loopback and a malformed value is not corrected: repairing a configured value here would either
  reach a service the operator did not ask for, or quietly agree with a bind the service itself
  refused to start on. A whitespace-only value reads as "not set", because `FLUX_EXCHANGE_BIND=` is
  how a shell clears a variable rather than how it names a host.
- **The page now describes the behaviour** rather than the constraint, and names no address — the
  site's `no page publishes a deployment-specific fact` rule forbids one. It also says the setting is
  read at dev-server startup, which is the part a reader would otherwise discover by changing it and
  seeing nothing happen.
- **Left alone deliberately:** `DEFAULT_BIND` is still spelled in two trees — here and in
  `crates/exchange-server/src/bind.rs`. A test tying them together would have to read the Rust
  source, and every spelling of that check that was tried would pass while the two disagreed, so a
  doc comment naming the other file stands in for it.
