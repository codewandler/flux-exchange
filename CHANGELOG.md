# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.7.0] - 2026-08-01

### Added

- **This host executes an operation** (X-12). A caller names an operation id and **nothing else about
  the request is theirs** — not the host (the URL comes from the operation's own compiled Flux), not
  the credential (the address is derived from the resolved principal's tenant and the connector's
  declared authority), not the tenant. That is the whole confused-deputy answer, and it is what makes
  this an execution platform rather than a credential store with a catalogue.

  **The "this host builds no request of its own" rule is now enforced structurally rather than
  promised** — three locks covering different ground: the manifest's dependency list as an allow-list
  with a reason per entry, a single dispatch seam with no reachable socket (guarded by a scanner that
  self-tests against sources it must reject *and* accept), and a transport counter so a test cannot
  pass by never dispatching.

  A missing credential refuses **by address, never by value**, and is terminal — the request was
  never sent. A runtime this deployment does not admit is refused before the credential store is
  touched.

### Changed

- **`codewandler-flux-exchange-host` now carries the flux engine.** `connector-pack` and
  `flux-runtime` moved from dev-dependencies to dependencies, because the published crate executes
  now. `flux-web` did **not** — it holds the transport, and the crate that dispatches holds none.

### Known

- **Thirteen of fifty-three connectors cannot yet be invoked.** Their `base_url` is templated on a
  per-connection value and there is nowhere to supply it, so they refuse by name. It fails closed and
  says which field is missing. Tracked as X-47.

### Changed

- **The flux engine line is aligned, and `connector-pack` links** (X-11). Upstream published 0.9.0:
  `connector-pack` now requires `flux-runtime ^0.46` where it required `^0.41` against a flux line at
  0.45 — the conflict that made execution impossible from this repository. `connector-spec` (the
  compiler) is gone; its vocabulary now comes from `connector-address` 0.9.

  `connector-pack` is a **dev-dependency**, deliberately: nothing published here executes an
  operation yet, and a normal dependency would put the whole flux engine into every consumer's graph
  to satisfy a proof rather than to run code. The engine line is pinned at `0.46` in one place and a
  test refuses a second value — `flux-runtime` 0.47 exists and taking it would recreate the failure
  this removes.

  **This unblocks `invoke`, grants-gate-invoke, and per-instance connections.** Addresses are
  unchanged: `connector-address` carries an optional instance level and `CredentialRef::new` still
  elides it, asserted rather than assumed.

## [0.6.0] - 2026-08-01

### Added

- **A connector can be connected from the console** (X-44). The console could show what was wired and
  offered no way to wire anything, so an operator read their connections in a browser and created
  them with `curl` — it could do neither of the two jobs the charter gives it.

  The inputs come from **the connector's own declaration**, not a list the console keeps, so a
  connector that gains a credential gains an input with nobody editing the console. No value is ever
  rendered back: after a write the page shows addresses and whether each credential is held, through
  the same renderer the read-only listing uses. An already-connected connector points at **rotation**,
  never at delete.

- **Only a human mints an agent** (X-40). Nothing gated minting by principal kind, so once agent
  tokens authenticate, a leaked one could mint successors — and revoking the first would not kill the
  descendants. Revocation would have stopped being a remedy **invisibly**, because those descendants
  are ordinary agents with no recorded relationship to the token that was revoked.

  `Agent` and `Service` are both refused. `Service` is the interesting one: the property this
  defends holds only if every minter is itself revocable by this host's operator, and a `User` is —
  sign-in is federated — while nothing here mints, verifies, lists or revokes a *service* credential.
  Admitting it would reproduce the same defect one level further out of sight.

- **Whether this deployment can sign anyone in is a field** (X-43). The console linked to
  `/api/signin` unconditionally, so on a host with no identity provider the **Sign in** button led to
  a `503` — the operator learned the platform could not sign them in by being refused. The
  distinction existed only in a human-readable sentence, and a client branching on the wording of a
  refusal breaks when someone improves the wording.

  `GET /api/signin/availability` answers `{"sign_in_available": …}` — one key, anonymously. It is a
  **boolean and not the three internal states**, because a three-valued answer would tell a stranger
  whether this host's OIDC variables are set; the two unavailable compositions answer byte for byte
  identically, status included.

## [0.5.0] - 2026-08-01

### Added

- **An arriving agent is told what this is** (X-41). The charter calls the agent the primary caller,
  and nothing anywhere told one how to reach this service. A public page — no account needed, linked
  from the console's footer — says what the platform is, how to get an identity for an agent, and
  what that identity can and cannot do **today**.

  It is **honest by construction**: what it claims derives from the same surface declaration the
  navigation reads, so it cannot advertise a capability the console marks unbuilt. The rule is
  one-directional by design — the derivation can take a claim *off* the page, never put one *on*.
  Flipping a surface to built turns four tests red, so the wiring is checked rather than trusted.

### Added

- **An agent principal can be minted, and this host keeps only a verifier** (X-36). `docs/vision.md`
  says the primary caller is an agent, not a human — and until now `PrincipalKind::Agent` appeared
  only in its own definition, a loopback development roster, and a comment saying agents carry their
  own tokens. Nothing minted one, so the stated primary caller could authenticate only on loopback,
  in the mode that exists because it must not be exposed.

  `POST /api/agents` mints an agent for the caller's tenant and shows the token **once**. The store
  keeps a digest: a test presents every value in the file — and the whole file — back to the
  resolver, and none of them authenticates. **Reading that store is a roster disclosure; writing it
  is a full authentication bypass**, so a group- or world-writable store is refused at startup while
  a merely readable one warns.

  It **authenticates nothing yet** — binding it to the identity port is a following story, and the
  question of *who may mint* is settled before that lands.

- **A credential can be rotated in place** (X-39). The surface could create, read and delete but not
  *replace*, so rotating a credential — the remedy for a leak — meant `DELETE` then `POST`, with a
  window where the tenant had no connection at all and anything relying on it failed.

  Rotation replaces **one** credential rather than the declared set, and the reason is the north star:
  this host never hands a credential value back out, so a wholesale replace would make a caller
  re-send every value it wanted to *keep* — and an operator rotating one of two credentials has no way
  to obtain the other. It is separated from create by path, method **and** body type, so `POST`'s
  `409` on an existing connection is untouched: an upsert is still the silent overwrite the
  connections story exists to prevent.

  A refused rotation leaves the old value in place, including when it would exceed the tenant's
  allowance.

- **The console presents an execution platform** (X-34). It rendered the connector catalogue and
  nothing else, with no header and no navigation, while the service behind it grew sign-in, expiring
  sessions and a per-tenant connection surface. `docs/vision.md` gives the console two jobs — *wire
  things up* and *see what happened* — and the catalogue is neither, so it has stopped being the
  front door.

  There is now a shell: the service's name, an identity affordance (sign in, or who you are and your
  tenant), and a rail covering **every** surface with its true state. **Connections** is a read-only
  view showing addresses and whether each credential is held — never a value. **Activity**, **Invoke**
  and **Subscribe** are named, struck through and tagged `NOT BUILT`, and a test asserts they have no
  path, no route and no screen — negative-controlled, so each prong is known to fire on its own.

- **CI proves the MSRV the crate promises** (X-33), reading `rust-version` out of `Cargo.toml` rather
  than repeating it. The first real run confirmed the value reaches the toolchain (`1.88`) rather
  than silently defaulting — which would have made the job green while proving nothing.

## [0.4.0] - 2026-08-01

### Fixed

- **A partial delete reports the worst failure, and claims only what it knows** (X-29). The loop kept
  the *first* failure kind, so an unreachable store followed by a denied one answered "retrying may
  work" while a denied address sat in that same response. And `left_behind` told an operator to treat
  addresses as still usable when a connector may legitimately hold a subset of what it declares — so
  some had never held anything. The claim now hedges; the list and the safe instruction are
  unchanged. A partial `DELETE` answers `502` rather than `503` when any address failed in a way
  retrying will not fix.

- **Console tests are found at every depth** (X-32). The test script globbed one directory level, so
  a test in a subfolder never ran and the suite reported green — which became a silently-green
  pipeline once CI started running it.

- **`rust-version` was wrong, and shipped wrong in three releases** (X-30). The manifest declared
  `1.87`. It has never been true: `jsonwebtoken`, `time`, `time-core` and `time-macros` each require
  `1.88.0`, and cargo refuses before compiling anything — so `cargo +1.87 build` has failed since
  X-04 introduced `jsonwebtoken`, on the day 0.1.0 was cut. `v0.1.0`, `v0.2.0` and `v0.3.0` all
  carry the false floor.

  **`rust-version` is now `1.88`.** This is a *correction*, not a raise: no consumer can have been
  building on 1.87, because it never worked. The alternative — pinning `jsonwebtoken` and `time`
  backwards — would downgrade the library doing id-token signature verification in order to preserve
  a number nobody had verified. [X-33](docs/stories/X-33-msrv-job.md) adds the CI job that keeps it
  honest, reading the number from the manifest rather than repeating it.

### Added

- **Every `ExchangeError` is pinned against the refusal it becomes** (X-31). The status mapping was
  guarded variant by variant, but nothing guarded the edge *before* it — a new exchange error folded
  into an existing refusal would have inherited its status and silently undone the operator-vs-caller
  split, without touching the mapping any test was watching.

- **CI checks the action pins and the version pairing** (X-30). Both checkers **self-test before
  they scan**, so one that has stopped catching violations cannot report there are none. The pin
  scanner classifies YAML rather than grepping, because a comment or a `run:` block containing an
  example pin will fool a line-wise grep — and the sibling repository's own error hint is such a
  line.

## [0.3.0] - 2026-08-01

### Added

- **A tenant's allowance holds against its own concurrent creates** (X-25). X-22's occupancy bound
  was read and written under a claim keyed per `(tenant, connector)`, so one tenant's concurrent
  creates to *different* connectors each read an occupancy the others had not written yet. A second
  claim keyed on the tenant closes it. `DELETE` deliberately stays outside that claim — it only frees
  allowance, and the case a delete exists for is revoking a leaked secret, which must not wait.

  A client firing several creates for one tenant in parallel now sees a retryable `409` where it
  previously got a `201` and an allowance that did not hold. Different tenants still do not contend.

### Changed

- **OIDC configuration is read by name, not by position** (X-27). The read pulled values out of a
  vector positionally, and three lists described one set of variables — so adding a variable to one
  and not another silently shifted every value after it. That drift had already shipped once. The
  parallel lists are gone: both are now derived from the read itself, so the same mistake is a
  compile error rather than a host that starts up with a blank client secret. No refusal, order or
  message changed.

- **A sign-in refusal carries its own status** (X-26). The refusal-to-status mapping moved from
  inline in the callback route onto `SignInRefusal`, beside `caller_facing()` — where the argument
  for it already lived. Every status on the wire is unchanged and now pinned variant by variant.

- **CI gates every push and pull request** (X-28). This repository had one workflow and it fired on
  a version tag, so a red `main` was invisible until someone tried to release, and the console had
  never been built by CI at all. `ci.yml` now runs the whole Rust gate and builds and tests the
  console in its own job. The release workflow **keeps** its own inline gate: a tag can be pushed at
  a commit no CI run ever covered, and publishing is the irreversible path.

## [0.2.0] - 2026-08-01

## [0.1.0] - 2026-08-01

### Added

- **One tenant cannot make every other tenant's writes slow** (X-22). Nothing bounded a credential's
  size or how much of the store one tenant could occupy, and the file store rewrites and `fsync`s a
  single file under one mutex on every write — so one tenant's data set the latency of every other
  tenant's writes. That is shared fate between tenants in the service whose central claim is that
  tenants share nothing.

  Two bounds, because they answer different questions. **8 KiB per credential** is about *kind*: a
  credential is a token or a signing secret, and at the largest an RSA-4096 PEM is ~3.2 KiB, so a
  value that does not fit is not a credential that grew. **64 KiB per tenant** is the one that
  protects the neighbours — a per-value bound alone leaves a ceiling that grows every time upstream
  publishes another connector. An oversized value is `413`; an exhausted allowance is `409`, because
  the remedy is to disconnect something rather than to send less.

- **A browser-facing OIDC endpoint is refused in cleartext too** (X-23). X-17's refusal covered only
  the token endpoint and the key set, on the argument that a browser enforces the transport of the
  addresses it navigates. That does not cover the authorization URL carrying `state`, `nonce` and the
  PKCE challenge readable and modifiable in flight, nor an operator who typed `http` and was told
  nothing. All four `FLUX_EXCHANGE_OIDC_*` endpoints are now checked; loopback stays exempt, private
  ranges do not.

  **Upgrading:** this is a refusal, so a deployment with an `http` authorization endpoint or redirect
  URI on a non-loopback address will stop offering `/api/signin` at startup, naming the variable.
  `/health` and the catalogue keep serving. Look for `InsecureEndpoint` in the startup log.

- **An operator can tell their own misconfiguration from a refused credential** (X-17).
  `ExchangeError::Rejected` collapsed four causes, one of which was *this host's own client secret
  being wrong* — logged as "the provider refused the authorization code", which sends an operator to
  check a caller's credential instead of their own configuration. Four variants now, and **one**
  caller-facing answer: the split is in the log only, and the guard that the caller learns nothing
  about the provider stays green. Same shape X-15 established on the front channel.

- **A cleartext back channel is refused at startup, naming the variable** (X-17). An
  `http://` token endpoint sent this host's client secret as HTTP Basic credentials in the clear,
  with no refusal at all. **Loopback is exempt** — a local test IdP is a real workflow, and
  forbidding it pushes operators toward disabling verification or testing against production, while
  loopback packets never reach an interface. **Private ranges are not exempt**: "it's only the
  internal network" is exactly the assumption that makes a cleartext secret worth taking. An absent
  or unrecognised scheme is refused rather than guessed.

### Fixed

- **A sign-in reads the clock once** (X-24). X-16 consolidated the wall clock to one function, but
  one function is not one reading: `complete` read it for `admit` and the session store read it
  again, so a token expiring between the two was admitted and then refused — the caller seeing a
  `503` "cannot open a session" for what was really an expired credential, and the log saying the
  same. The reading is now taken once and spent on both decisions.

  It is still taken **after** the token exchange rather than at the top of the call. Moving it
  earlier reads plainer and fails open: the deadline is measured from it, so a reading taken before
  a slow token endpoint would let the session outlive the token by the round-trip.

- **A create the store refuses keeps its kind** (X-20). `partly_written` flattened every
  store-failure kind to `503` "retrying may work", so a create refused because the store *denied this
  host access* sent the operator to retry instead of to fix the permission — the same defect X-18
  fixed on the delete side. A partly-written create is now `502` for denied, backend and layout
  failures. The three existing caller-facing sentences are pinned byte for byte, so the shared
  mapping cannot be reworded by accident.

- **The cleartext check now parses an authority the way the client that dials it does** (X-19).
  X-17's refusal read `http://evil.example\@127.0.0.1/token` as loopback and admitted it, while the
  `url` crate reqwest actually dials with ends the authority at the backslash and resolves the host
  to `evil.example` — so the client secret would have gone out as Basic credentials, in cleartext,
  to a remote host, past the check built to stop precisely that. Operator-supplied configuration
  only, never caller-reachable.

  The agreement is now **measured**: 475,270 generated spellings through the old parser, the new one
  and real `url` 2.5.8. The old parser admitted 15 endpoints `url` dials remotely over `http`; the
  new one admits none. The doc no longer claims it cannot admit a cleartext endpoint — it promises
  one direction, names the working configurations it refuses, and says the agreement is measured
  rather than proved.

- **A delete that fails half way says what it destroyed** (X-18). `DELETE` looped over a
  connection's credentials and returned a generic `503` on the first error, leaving some destroyed
  and some live while telling the operator only "retrying may work" — so a *live* vendor credential
  could survive a delete, which is the worst possible outcome for the case a delete exists for:
  revoking a leaked secret. The refusal now names what was destroyed and what is still held.

  **Rollback is not available in this direction** — a destroyed credential cannot be put back,
  because this host never held the plaintext — so the answer is honest reporting rather than a copy
  of create's rollback. The loop is best-effort rather than stopping at the first failure, since a
  delete is a revocation and destroying two of three beats destroying one. The store failure's kind
  survives into the refusal instead of being flattened, because answering a "denied" with "retrying
  may work" would be a fresh instance of the same misinformation.

- **A failing key set can no longer be hammered once per sign-in** (X-17). The refetch floor gated
  only unknown-`kid` refetches and was written after a *successful* parse, so while the JWKS endpoint
  was down every callback provoked a fresh outbound fetch. The floor now gates going out at all, and
  the rate-limited branch answers "provider unreachable" rather than "unpublished key" when no
  current key set is held — without which the fix would have made an outage read as a refused
  credential.

- **A session ends when the identity behind it does** (X-16). Deferred twice — X-03 left it to X-04
  on the grounds that an id token carries an `exp` worth binding to, and X-04 deferred it again
  because no composition could produce an id token. X-04 removed that reason, and the position was
  then worse than before: the host knew when an identity expired and discarded it.

  `Oidc::complete` passes the id token's `exp` to the session store **verbatim**. A five-minute token
  yields a five-minute session; this host invents no lifetime, because one it invented would outlive
  the credential it was shown.

  **An `exp` already past, or further out than thirty days, refuses the sign-in rather than being
  clamped.** Clamping would issue a session neither the provider nor this host described, and would
  leave the misconfigured provider in place for nobody to find. An expired session is **removed**
  rather than left unresolvable, so expiry cannot become a back door through the store's bound, and
  it answers exactly as a session that never existed.

  One wall clock, not two: `now()` moved to `session.rs`, because `admit` decides whether a token has
  expired and the store decides how long a session may live, and two clocks could admit a token and
  then refuse it a session. The development identity keeps its process-lifetime session — a roster
  handle carries no secret and no expiry, so any lifetime there would be invented, and that port
  already forces a loopback bind.

- **OIDC sign-in completes** (X-04, closing the `PARTIAL` this story shipped as). The owner took the
  dependency decision on 2026-08-01 — `reqwest`, `jsonwebtoken`, `sha2` — and `TokenExchange` is no
  longer an unbound port. The authorization code is redeemed back-channel with `client_secret_basic`
  and the id token's signature is verified against the provider's published keys, so `/api/signin`
  redirects to a real provider instead of serving an explanation, and the composition reports
  `Bound`. Configure the eight `FLUX_EXCHANGE_OIDC_*` variables and sign-in works end to end.

  **The permitted algorithms are derived from the JWK's key type and never from the token header**,
  which is what closes `alg: none` and RSA/HMAC algorithm confusion — the two attacks a *caller* of
  a JOSE library can still get wrong. Both are tested, the confusion case forged with both the
  published PEM and the JWK modulus spelling, because a vulnerable verifier passes whichever bytes
  it happens to hold. An unpublished `kid` is refused rather than falling back to trying keys until
  one verifies; a token with no `kid` resolves only when the provider publishes exactly one key.

  **Signature verification only.** Every claim check — `iss`, `aud`, `exp`, `nonce`, `sub` — stays in
  `Oidc::admit`, where it was already tested, so an expired token is refused as `Expired` rather than
  collapsing into a generic rejection. Two independent reviews verified that split claim by claim
  against `admit` rather than taking the comment on trust.

  `sha2` retires the hand-written `oidc/sha256.rs`, which existed only because no digest crate was
  allowed in. RFC 7636 Appendix A's vector is unchanged and still passes, which is what makes the
  swap checkable rather than merely plausible.

  Two endpoints are configured rather than discovered — `FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT` and
  `_JWKS_URI`. Discovery stays rejected, now on a different argument: with an HTTP client available
  it is a choice, and it keeps which keys can mint a session here legible from the environment
  rather than from a document re-fetched at runtime.

  **Note for deployers:** reqwest's `rustls` feature resolves to `aws-lc-rs`, so this build now
  compiles C and assembly. A container with no C toolchain that built this repository before will
  fail. OpenSSL and a second TLS stack are genuinely absent.

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
