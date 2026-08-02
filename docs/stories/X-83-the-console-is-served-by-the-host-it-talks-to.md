---
id: X-83
title: "The console is served by the host it talks to"
status: ready
priority: 1
epic: remote-deployment
design: docs/designs/remote-deployment.md
areas: [exchange-server, console]
note: "the console reaches the API only through the Vite dev proxy, which npm run build does not emit — and it cannot be hosted elsewhere, because SameSite=Strict means the browser never attaches the session cookie cross-origin. Ordered before X-84, which has nothing to put in an image until this exists"
---

# The console is served by the host it talks to

## Goal
The binary that answers `/api` also answers `/`, so a browser with no checkout can use the console.

## Why this is a capability and not configuration

`crates/exchange-server` serves no static files. `tower_http` is imported once, for `TraceLayer` —
there is no `ServeDir`, no embedded assets, no fallback route. The console's only path to the API is
the **Vite dev-server proxy**, which exists in `vite dev` and not in `npm run build` output. So today
the console is reachable exactly where a developer is running two processes.

## Why it cannot be hosted somewhere else, which is the part worth reading

The obvious answer is to publish the console beside the docs site and point it at the API. **It cannot
work, and it is not a CORS problem** — which matters, because it will be misdiagnosed as one:

- `console/src/service.mts` addresses every endpoint as a same-origin relative path (`/api/…`).
- `session::host_cookie` (`crates/exchange-server/src/session.rs:471`) issues
  `Path=/; Secure; HttpOnly; SameSite=Strict` **unconditionally**.

`SameSite=Strict` means the browser does not attach the cookie to a request that originated from
another origin, whatever the server says in a CORS header. The cookie is not blocked; it is not sent.
`Strict` was chosen deliberately — X-15 is about a session arriving in a browser that did not start
it, and X-40 about a leaked token minting successors — so **relaxing it to make a deployment
convenient is a security decision wearing a packaging costume.** Do not.

## Scope
Serve the built console. **Not** a rewrite of the console, not a new screen, not the grants UI that
[[X-62]] says is still unbuilt.

## Decisions this story must make and record
- **Embedded assets or a directory read at runtime.** Embedding makes the binary self-contained and the
  image trivial; a directory keeps the published crate free of a console it has no business carrying.
  Note that `codewandler-flux-exchange-host` is the **published** artifact and `exchange-server` is
  `publish = false` — so whichever is chosen, the console must not reach the published crate.
- **The SPA fallback, and its collision with the route table.** A catch-all serving `index.html` is how
  a client-side router survives a refresh, and it is also a new route in a table that
  `the_anonymous_surface_is_only_what_was_declared_anonymous` (X-61) enumerates. Decide whether the
  fallback is a declared route or sits outside the table, and make the guard agree either way. **The
  fallback must not shadow `/api`** — an API path that falls through to `index.html` turns a 404 into
  a 200 carrying HTML, which every client will misread.

## Acceptance
- [ ] `GET /` serves the built console, and a deep link refreshed in the browser still resolves.
- [ ] **Failing-first test** — a request for an unknown `/api/...` path still refuses, rather than
      falling through to `index.html`. Write it, watch it return the SPA, then close it. This is the
      defect an SPA fallback introduces and it is silent.
- [ ] The console's own tests and build are unchanged: it is still a separate Node tree with its own
      lockfile, and nothing under `console/src/components/` is touched (those 15 components are shared
      with flux-connectors — see `AGENTS.md` § The console).
- [ ] `routes::published()` and X-61's anonymous-surface guard agree with whatever the static route is,
      by mutation rather than by reading.
- [ ] The published crate does not gain the console. `no_second_request_path.rs`'s `ALLOWED` list is
      unchanged, or the addition carries a sentence saying why it is not a transport.
- [ ] The session cookie is **not** modified by this story.

## Progress
- (not started)

## Notes
- Blocks [[X-84]]: there is nothing to put in an image until the binary can answer `/`.
- The dev proxy stays. [[X-71]] just made it follow `FLUX_EXCHANGE_BIND`, and a developer running two
  processes is still the fast loop.
