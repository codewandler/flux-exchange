---
id: X-83
title: "The console is served by the host it talks to"
status: done
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
- [x] `GET /` serves the built console, and a deep link refreshed in the browser still resolves.
      → `routes::app_with_console`; verified against the real binary and the real
      `console/dist`: `/` → `200 text/html`, `/assets/index-*.js` → `200 text/javascript`,
      `/connections/zendesk` → `200` and the entry point.
- [x] **Failing-first test** — a request for an unknown `/api/...` path still refuses, rather than
      falling through to `index.html`.
      → `an_unknown_api_path_refuses_rather_than_serving_the_console`, and **it earned its keep
      immediately**: it went red *after* the wildcard catch-all was already in place, because
      `/api/{*unmatched}` matches one segment or more and so left `/api/` — trailing slash, nothing
      after — falling through to the console with a `200`. Three routes now refuse: the wildcard,
      `/api` and `/api/`. Confirmed on the running binary, which answers
      `{"error":"no such route on this host"}` with `404` for both bare forms.
- [x] The console's own tests and build are unchanged: it is still a separate Node tree with its own
      lockfile, and nothing under `console/src/components/` is touched.
      → nothing under `console/` is in the diff at all. The console is read as a **directory at
      runtime** rather than embedded, which is what keeps that true.
- [x] `routes::published()` and X-61's anonymous-surface guard agree with whatever the static route is.
      → the static route is **not** declared, and `app()` — which every guard walks — is now
      `app_with_console(state, None)` and `#[cfg(test)]`. So the enumeration walks exactly the router
      a checkout serves. `a_bound_console_shadows_no_declared_route` is the other half: with a console
      bound, every route in `published()` answers the same status it answers without one.
- [x] The published crate does not gain the console.
      → `crates/exchange-host` is not in the diff. `tower-http`'s `fs` feature is added in the root
      manifest and used only by `exchange-server`, which is `publish = false`;
      `no_second_request_path.rs` passes 11/11 with its `ALLOWED` list untouched.
- [x] The session cookie is **not** modified by this story.
      → `session.rs` is not in the diff. Observed on the wire, unchanged:
      `__Host-flux_exchange_session=…; Path=/; Secure; HttpOnly; SameSite=Strict`.

## Progress
- **Done.** `routes::app_with_console(state, Option<&Path>)` is the one production entry point;
  `app(state)` survives as `#[cfg(test)]` and is what every surface guard walks, so the enumeration
  tests describe the router a checkout actually serves.
- **`ServeDir` reading a directory, not embedded assets.** Keeps `console/` a separate Node tree that
  shares nothing, keeps the console out of the published crate, and leaves `cargo run` working with no
  console built — `FLUX_EXCHANGE_CONSOLE` unset means no static route, which is exactly the prior
  behaviour.
- **The trailing-slash case is the finding.** A wildcard matches one segment or more, so the obvious
  `/api/{*unmatched}` still let `/api/` reach the console's entry point with a `200`. The
  failing-first test found it after the catch-all was written, which is the argument for having
  written the test rather than reviewing the route.
- **An absent console directory does not refuse at startup**, deliberately, and the reason is written
  at `configured_console`: every other store here refuses because starting degraded hides data loss,
  while a mistyped console path is a `404` at `/` with the whole API answering correctly. Refusing
  would let a cosmetic setting take the platform down.
- Verified end to end against the real binary and the real `console/dist`, not a fixture: signed in
  through `POST /api/session`, read `/api/catalogue/connectors` with the resulting cookie (`200`), and
  saw `/api/connections` answer `503` — no credential store bound, which is the correct fail-closed
  answer rather than a defect.

## Notes
- Blocks [[X-84]]: there is nothing to put in an image until the binary can answer `/`.
- The dev proxy stays. [[X-71]] just made it follow `FLUX_EXCHANGE_BIND`, and a developer running two
  processes is still the fast loop.
