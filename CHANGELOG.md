# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **A sign-in a victim did not start cannot become a session in their browser** (X-15). Server-side
  `state` closes a *forged* callback, not login-CSRF: an attacker who starts a sign-in here honestly,
  authenticates at the provider as themselves and stops at the redirect holds a genuine `code` and a
  still-unspent `state`, and walking a victim into that callback passed every check X-04 had. The
  victim came away holding the attacker's session, inside the attacker's tenant — the north star
  inverted from the other end: the credential does not cross the boundary, the *human* does.

  A **`__Host-` binder cookie** planted at `/api/signin` is the missing tie — 256 bits from the one
  entropy path, `Secure` + `HttpOnly`, redacted in `Debug` like a session token, and never a URL
  parameter. `PendingAuthorizations::claim(state, binder)` **replaces** `take(state)` rather than
  sitting beside it: a method that spends an authorization on `state` alone *is* the hole, so the
  reliable way to stop a later story reaching for it is for it not to exist.

  A binder mismatch leaves the authorization **unspent** — the browser with the wrong binder is more
  likely the victim than the perpetrator, and a hostile callback must not cancel someone else's
  sign-in. A missing binder is refused *before* the pending store is consulted, so omitting the
  cookie neither falls through to the state-only path nor probes whether a `state` is live.
  `UnknownState`, `NoBinder` and `AnotherBrowser` are three log lines and not one, and deliberately
  indistinguishable to the caller.

  The binder is `SameSite=Lax` where the session cookie is `Strict` — deliberate, and documented at
  the definition: its whole job is to survive exactly one cross-site-initiated navigation, the
  provider's redirect back, which a `Strict` cookie would never arrive for.
- **Connections, addressed by a tenant the caller cannot name** (X-08, X-10). Create, list and
  delete a connection, scoped to the caller's tenant. The credential address is **derived** —
  `tenants/<tenant>/<authority>/<credential>`, with the tenant from the resolved principal and the
  authority from the connector's declaration — and **no route accepts an address**. A connector that
  declares no authority is refused rather than stored at a guessed one. Deleting a connection
  destroys its credentials. Tenant A cannot read, use or delete tenant B's connection, and the
  refusal names A's *own* address, never B's and never a value; 18 hostile connector ids across three
  methods were all refused.

  **A second connection to the same connector is refused (409), not silently overwritten.** The
  address has no instance dimension yet — upstream flux-connectors C-406 adds one and this repository
  cannot use it until it is published — so the refusal quotes the shape that will replace it and
  names X-14. Per-connection configuration is deferred for the same reason: a vendor subdomain is
  exactly the per-instance fact with no home until two instances can be told apart.

  The refusal is guarded across the whole probe-decide-write, because a check-then-write lost to two
  concurrent requests and produced precisely the silent overwrite it exists to prevent. **Single
  process only**, stated in the guard, the routes and the design: two replicas over one store would
  race again, and `SecretStore` has no compare-and-swap to close it properly.
- **OIDC sign-in, up to the token exchange** (X-04, partial). The authorization request is real:
  authorization-code flow with PKCE `S256`, and `state` and `nonce` bound at `/api/signin`,
  single-use and TTL-bounded. A callback carrying a `state` this host did not open is refused with
  **no session issued** — proven by committing the whole flow *without* the binding first, where the
  forged callback cheerfully answered "Signed in", i.e. a victim signed in as the attacker.

  **It cannot complete, deliberately.** Redeeming the code needs an HTTP client and verifying the id
  token needs a JOSE library; this workspace has neither, so `TokenExchange` is an unbound port and
  `/api/signin` serves an explanation rather than a redirect it could never return from. Nothing
  hand-rolls signature verification. Following X-03's precedent, a configured-but-unbound OIDC
  composition reports **`Unbound`**, so "OIDC is configured" cannot make a reachable bind legal while
  nothing can actually resolve a caller.

  The one crypto exception is a hand-written SHA-256 for the PKCE challenge, verified against
  `hashlib` over every message length 0..=600 and at the 2^32-bit boundary; it goes when `sha2` can
  be depended on. The tenant is fixed at startup rather than mapped from a claim, because some
  providers let users edit their own profile claims.
- **Identity, bound — with a dev principal that cannot open the door** (X-03). The `Identity` port is
  wired: a request carries a session, the host resolves it to a `Principal`, and every tenant is read
  from *that* and from nothing a caller controls — asserted three times, once each for a path
  segment, a body field and a header, against a route that genuinely declares `/{tenant}` so the
  claim is delivered and then ignored rather than never parsed.

  The load-bearing decision is that a development identity is a **third** bind state, not "bound". It
  resolves principals, so counting it as bound would have made `0.0.0.0` legal — but a roster handle
  is a credential with no secret in it, which is worse than an unauthenticated port, because
  everything downstream believes the principal. Arming it therefore confines the process to loopback,
  and the refusal names the opposite remedy from the unbound one.

  Sessions are a `__Host-` cookie with `Secure`/`HttpOnly`/`SameSite=Strict` and 32 bytes from
  `/dev/urandom`, refusing rather than falling back if the CSPRNG is unavailable. **A session token is
  returned in the body only to a caller that presented a readable credential**, so the route cannot
  turn an unreadable credential into a readable one — without that rule `HttpOnly` was a control that
  only appeared to exist, since script could POST with the ambient cookie and read an equally
  powerful token out of the response. The store is bounded and **refuses at the bound rather than
  evicting**, because evicting signs out a caller who did nothing wrong. No expiry yet, stated rather
  than implied.
- **The connector catalogue, served and read** (X-05, X-06, X-07). `GET /api/catalogue/connectors`
  and `/api/catalogue/connectors/{id}/operations` publish 53 connectors and 299 operations with the
  metadata a `Selector` is written over — `risk`, `effects`, `idempotency` — so the grant model stops
  being server-only folklore. The response distinguishes **what exists** from **what a principal may
  call**: nothing is filtered by grant, and `admitted: null` says so on the wire rather than omitting
  an operation a caller lacks, because an agent that cannot see an operation cannot report being
  refused. `effects` is *derived* (`network` iff the operation declares hosts, since the catalogue
  declares no effects) and carries `effects_derived: true` so an inference is never read as a
  declaration. Adding a connector needs no change to the route.

  The console now reads that catalogue live; `console/src/fixtures/catalog.ts` and its banner are
  deleted in the same change. An unreachable service renders an error **naming the endpoint** — "zero
  connectors" and "cannot reach the server" must not look alike. The 15 explorer components carried
  from flux-connectors are untouched; four findings against them were reported upstream
  (flux-connectors C-408) rather than patched locally.
- **A credential store, honest about what protects it** (X-09). `exchange_host::CredentialStore`
  binds `connector-secrets`' file-backed store — `0600` in a `0700` directory, modes set in the
  create call and re-checked at open, a widened mode **refused rather than tightened**, and atomic
  writes through temp + `fsync` + `rename(2)`. What this host adds is startup honesty: a path inside
  a working tree is refused (one `git add -A` from a committed credential), a configuration naming
  no path is a startup error naming what would have worked with **no fallback to memory**, and the
  banner reads its path back off the store that was actually bound. The README states what does
  *not* protect a value there: the file mode and nothing else.
- **An HTTP surface that refuses an open bind** (X-02). `cargo run` binds `127.0.0.1:8080` and
  answers `GET /health`. Startup on a reachable address with no identity provider configured is
  **refused before the socket opens**, and the refusal names what would have worked — a daemon
  holding credentials behind an open listener is the failure this exists to prevent, so it does not
  start-and-warn. Routes are declared as data per feature module and the `Router` is derived from
  them, so `routes::published()` is the whole surface by construction and a test can enumerate it;
  an opaque per-module `Router` would have let a module publish an unauthenticated route no test
  could see. Framework choice and its reasons: `docs/designs/http-surface.md`.

- **The backlog** — vision, roadmap, and thirteen stories across four epics (X-01…X-13), plus the
  operating contract in `AGENTS.md`. The first wave is eight ready stories: the HTTP surface,
  sign-in, the catalogue and the credential store.

## [0.0.1] - 2026-08-01

### Added

- **The charter, and the rules as tested types.** `crates/exchange-host` carries `Principal`/`Tenant`,
  `Grant`/`Selector`, `Runtime`/`Deployment`, `Lease` and the `Identity` port, with 19 tests. Four
  rules are executed rather than described: a tenant id that would traverse its credential-address
  prefix is refused at construction; a multi-tenant deployment refuses every locally-executing
  runtime, naming what would have worked; a grant selects by declared metadata with deny beating
  allow; and a lease requires the same principal, not merely the same tenant.
- **A binary that reports and exits**, deliberately not a service.
- **A console** over the 15 framework-free explorer components carried from flux-connectors,
  rendering fixture data behind a banner that says so, with the components' no-framework-import
  invariant ported and strengthened.
