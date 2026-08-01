# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **The site shows how to run this and sign in** (X-69). A visitor could read what this service
  refuses to do and could not learn how to start it. Now there is a page, on the nav of every page and
  in the landing hero, and **it was verified by following it** — a clean clone, `cargo run`, the
  roster, the session cookie, and the console leg driven through headless Chrome rather than
  simulated. Doing that changed the page twice, which is the argument for doing it.

  The loopback constraint is **inside the block a reader would copy**, asserted by a test over every
  block containing `cargo run` — a roster handle is a credential with no secret in it, and a page that
  mentions loopback three screens below the command is a page that puts a secret-free roster on a
  public address. It also carries the invoke prerequisite in order (`503` → `403 not_granted` →
  the credential refusal), so nobody follows it to "you are signed in" and then falls off a cliff.

### Fixed

- ⚠ **The site's credential-shape scanner could not see inside a code block** (X-69). `textOf`
  replaced each tag with a space and the syntax highlighter puts every token in its own element, so
  `export FOO=bar` reached the rule as `export FOO = bar` and the check against a value on the
  right-hand side of an `=` never fired. Demonstrated with a throwaway page before anything was
  written: `export OTHER_PROBE=realvalue` passed the suite.

  It had never been asked, either — **this site carried no fenced code block on any page until now**,
  so a rule about what an example may contain had never met an example. The scanner now reconstructs
  each block verbatim and both scanning tests read prose *and* blocks.

## [0.11.0] - 2026-08-01

### Changed

- **Moved to the flux 0.47 engine line and the 0.10 connector catalogue** (X-67). Both pin sets in one
  commit — raising either alone puts two engine lines in one lock, and `connector_pack::pack` hands
  out `Arc<dyn flux_runtime::Tool>`, so two runtime versions are two unrelated traits. A third test
  now reads `Cargo.lock` itself, because the manifest-reading one cannot see a crate dragged in
  transitively.

  ⚠ **`intercom` is now refused for configuration, and an EU or AU tenant will read that as an
  outage.** Upstream changed its `base_url` to `https://{host}` — a bare placeholder is the whole
  destination authority — so X-47's guard refused it, exactly as designed: *a catalogue bump that
  moves a host template turns a test red rather than quietly dispatching.* A value stored before the
  bump is refused on the way **out** of the store as well.

  The refusal is right under the rule and wrong about intercom, because that `{host}` is a **closed
  set of three vendor hostnames the catalogue publishes**. X-70 is the story that admits a declared
  choice without reopening the door to a free value.

  **Census, measured rather than assumed:** 54 providers (algolia is new), 679 operations, 5
  `WholeAuthority`, 8 `PinnedTo`, 13 `OutsideTheAuthority`, and all 54 still declare HTTP — so X-48's
  runtime gate and X-13's `effects` derivation are unmoved. Four operations left the invocable surface
  (postmark and zoom operations that returned a credential) plus three from babelforce.

  Two claims in this repository turned out to be prose nothing asserted, and both were **already
  wrong** before this change: *"299 operations across 53 connectors"* appeared in two doc comments
  while the real count on the previous catalogue was 681. Corrected, and each now says where the
  number came from and that nothing checks it.

## [0.10.0] - 2026-08-01

### Added

- **You can sign in without an identity provider** (X-57). Everything this service does was reachable
  only by a signed-in principal, and the only thing that made one was an authorization-code flow
  against a configured OIDC provider. That was a lot of setup to look at a page.

  `SignIn::available()` meant *is OIDC configured*, not *can somebody sign in here* — so a deployment
  with the development identity armed, which **can** turn a caller into a principal, reported that it
  could sign nobody in. It now answers the question it is named for, and `GET /api/signin` on such a
  host serves a page explaining the mechanism instead of a refusal.

  ⚠ **Loopback only, and that is structural.** A roster handle is a credential with no secret in it,
  so `admit_bind` refuses every non-loopback address while the development identity is armed — driven
  under review with a roster and a complete OIDC environment set *simultaneously*, refused on
  `0.0.0.0`, `[::]` and a LAN address. A reachable deployment still needs a real provider. Local users
  with an actual verifier are X-58.

  Two things this corrected on the way. The premise that *the console hides its sign-in affordance*
  was **false** — it renders the anchor unconditionally and nothing reads `sign_in_available`, which
  X-43 published for exactly that purpose. And one of X-43's own assertions encoded the same
  conflation a layer up, asserting that an *available* composition answers `/api/signin` with a
  redirect.

- **An operator can see and change what their tenant may run** (X-62). v0.9.0 gated invocation by
  grant and shipped no way to write one, so a deployment ran nothing until somebody hand-wrote a
  file. `GET/PUT /api/grants` and `POST /api/grants/preview` (signed-in humans only), plus a console
  screen.

  **A grant is a selector, never a list of operation ids** — this connector, at most this risk, these
  effects — and an id is refused by two independent mechanisms: a recursive key scan that runs
  *before* serde, and `deny_unknown_fields` on the selector. That is the property the gate was built
  around: X-13 decides from what an operation declares, and a surface writing ids back would undo it.

  **The preview is the point.** The screen will not offer to save until it has been told what the
  draft would admit, and changing a bound asks again — a grant nobody can evaluate before saving is a
  grant somebody sets too wide.

  Two refusals beyond what was asked for, both deliberate: a `PUT` is refused when the tenant's
  existing grant carries id exceptions this surface cannot express, rather than dropping them
  silently; and two grants for one connector are refused rather than resolved by an unstated
  precedence. ⚠ The first blocks exactly today's population — anyone who already hand-wrote a grants
  file with a deny.

- **A public documentation site** (X-63). VitePress in `web/`, published to GitHub Pages by a
  workflow that builds on every pull request and deploys only from `main` — so a broken site cannot
  reach the URL. A dead internal link fails the build.

  Three pages, deliberately. The epic is ordered around the mechanism rather than the volume: this
  repository corrected **five renderings of one false claim** in a single week, and a documentation
  site is a factory for that failure. So these pages claim nothing about what is or is not live —
  they route that question to `GET /api/onboarding`, and per-capability status arrives with X-64,
  derived from the same descriptor whose `live` flags are held to the route table.

  Eight guards run over the **built** site — base-prefix drift, an IP address on a page, a
  token-shaped string, the contributor readme rendering — each verified against a real violation.

  ⚠ One repository setting is still required and no workflow can perform it: **Settings → Pages →
  Source = GitHub Actions.**

## [0.9.0] - 2026-08-01

### Added

- **An operation runs only if a grant admits it** (X-13). v0.8.0 gated invocation by identity alone
  and published that fact anonymously: *any principal this host resolves may run any operation in the
  catalogue against its own tenant's connections.* That sentence is now false, and the descriptor says
  what replaced it.

  **The decision is derived, never listed.** A grant is a selector over what an operation *declares* —
  its risk, its effects, its idempotency — not a set of operation ids. The route's own three
  projections of those facts were deleted in favour of the one the gate uses, with a test pinning the
  **served bytes** against it, so the catalogue cannot describe an operation differently from the
  thing that decides on it.

  **Both gates are a compile error to skip.** `admit_grant` consumes the runtime gate's `Admitted` and
  returns `Granted`; `Granted::resolve` is the only route to the resolver and `Admitted::resolve` is
  gone.

  ⚠ **Fail-closed, and it will look like an outage.** A deployment that upgrades runs nothing until
  `FLUX_EXCHANGE_GRANTS` names a file and grants are written into it, and **no surface writes one
  yet** — expect `503` if no store is bound, `403 not_granted` if one is bound and the tenant holds
  nothing. That is the intended posture; the surface that fixes the ergonomics is X-62.

- ⚠ **Breaking, `codewandler-flux-exchange-host`:** `Invoker::new` takes a sixth argument
  (`Arc<dyn Grants>`) and `Admitted::resolve` is replaced by `Granted::resolve`. An
  `Option<Arc<dyn Grants>>` was rejected deliberately: its only plausible `None` behaviour is *admit
  everything*, which is the exposure this closed.

- **Only a signed-in human may supply or rotate a credential** (X-54). `POST /api/connections/{connector}`
  and `PUT /api/connections/{connector}/credentials/{credential}` are gated by principal *kind*. At
  v0.8.0 an agent could do both — measured, not inferred: an agent `POST` answered `201` and left its
  own value at the tenant's credential address.

  Neither route hands a value out, so the literal reading of *"an agent's token grants access to an
  operation, never to a credential"* is not what they break. What they grant is the **credential
  position** — a caller deciding which vendor account every operation the tenant runs will reach has
  been granted it, value or no value. Two properties settle it: nothing records who supplied a
  credential, so a listing reads identically for a planted value; and revoking the token does not take
  the value back out.

  `DELETE` stays open to every kind, deliberately. It destroys tenant data inside the tenant the
  caller already belongs to, an operator can see and undo it, and no authority survives it. Whether an
  agent should reach a destructive route is the *grant*-shaped question, not the kind-shaped one.

### Fixed

- **The catalogue explorer stops badging operations this service runs as "not live yet"** (X-53).
  The **fifth** rendering of one falsehood — after the onboarding page, the mint screen, the shell
  inventory and the descriptor — this one in `console/README.md` as well as in the explorer itself.

  `works` now means *this service runs this operation*: a build fact, identical for every caller, and
  derived from the same `served` flag the server holds to its route table. The two tenant-specific
  readings were rejected because this explorer is **anonymous** — a badge from either turns a public
  page into a report on somebody's connections. The "could this principal invoke it" reading was
  rejected because `admitted` is three-valued and its `null` is not `false`; folding it into a
  boolean would make a public badge move with who is looking.

- **The agent descriptor's guard checks what its name claims** (X-52). X-42's liveness test compared
  a capability's `live` flag against *does the mapped route exist*, and never against the endpoint the
  capability itself publishes — so republishing `be-minted` at `/api/session` passed 253 tests. And
  `call.method` was pinned by nothing: changing the catalogue's `GET` to `DELETE` left the whole gate
  green. Both are held now, and both were demonstrated by mutation before being fixed.

  ⚠ **The obvious test for the method does not work, which is worth knowing.** Driving each endpoint
  anonymously and asserting the answer is not `405` fails to distinguish anything: on a guarded route
  axum's `route_layer` runs *before* the method router, so an anonymous request answers `401` first.
  That test would have passed for `DELETE /api/agents` — the exact defect. It was caught by the
  test's own control rather than by review.

## [0.8.0] - 2026-08-01

### Changed

- ⚠ **Breaking, `codewandler-flux-exchange-host`:** `admit_runtime` returns
  `Result<Admitted, RuntimeRefusal>` rather than `Result<(), RuntimeRefusal>` (X-48). `?;`,
  `.is_ok()` and `.expect_err()` all still compile; a caller binding the unit — `let () =
  admit_runtime(…)?` — does not. `Deployment::admits` is unchanged.

  **The type is the point.** The deployment gate is an invariant with deliberately no override, and
  it was held by a test that read `Invoker::invoke`'s source for the substring `admit_runtime(`.
  Three mutations defeated it with every test green: a discarded result, an `if false` branch, and a
  **string literal that merely mentioned the gate**. `Admitted` has a private field, no public
  constructor, no `Default` and no `Clone`, and `Admitted::resolve` is the only route from `invoke`
  to the resolver — so all three are now compile errors. It is a method on the witness rather than an
  ignored parameter because an ignored parameter is what the next person deletes as dead weight.

### Fixed

- **The invoke path's safety claims are as strong as its code** (X-48). Four findings from an
  independent review of X-12, all one shape: *the code said something stronger than it did.* The
  sandbox posture is written out field by field (`SandboxMode::Require`) instead of inheriting
  `System::new`'s disabled default — in the same function that already wrote two other settings
  longhand to avoid exactly that. A comment claiming no process could be spawned is replaced by what
  is true.

  **Lock 2 stopped chasing accessor spellings.** A first attempt refused `.system(`; the review then
  demonstrated `ctx.workspace_context().active()` reaching process spawn while naming nothing
  forbidden, and pointed out that the accessor the comment cited does not exist. The rule now bounds
  where the capability *handle* may live: only the two files that may name `Egress` may name
  `ToolContext`. A file that cannot name the handle has nothing to call an accessor on, whatever
  upstream renames next.

  **Lock 1's allow-list was overstated for a reason worth repeating:** it had no self-test, while
  lock 2's rules have had one since X-12. Its parser matched the literal line `[dependencies]`, so a
  `[dependencies.reqwest]` table escaped it entirely. Both directions are now self-tested.

### Added

- **An agent can fetch what this service is instead of reading a page** (X-42). `GET /api/onboarding`
  answers anonymously with what the platform is, the auth scheme, and which capabilities are live —
  the same facts the console's onboarding page renders, from the same source.

  **The first attempt published a falsehood, and how it happened is the useful part.** It told the
  caller the vision calls primary that `invoke` was not built, while
  `POST /api/operations/{operation}/invoke` had been in the published surface since v0.7.0. The
  page-and-descriptor agreement test was green the whole time, because the two renderings agreed
  **with each other** while both were wrong. Deriving from one source protects against drift, not
  against the source being false.

  The cause was one flag answering two questions: `built` means *has this console a screen*, and the
  document asks *does this service do this*. Those had the same answer for every surface until
  `invoke` shipped a route with no screen. They are now two fields, and liveness is held to
  `routes::MODULES` by a test that runs in **both** directions — a capability cannot be published as
  not-live while a route serving it is in the surface, and a route cannot be published without either
  being a capability or carrying a written argument for why not.

  The falsehood was in three renderings. All three are corrected.

  **It publishes one real exposure deliberately**: this build gates invocation by identity alone, so
  any principal it resolves may run any catalogue operation against its own tenant's connections.
  That is a fact about the software rather than a deployment, and publishing the endpoint while
  withholding it would be the dishonest half of a disclosure.

- **The two branches X-46 opened are pinned** (X-49). Publishing declarations changed how a connector
  that declares nothing renders — it used to arrive as `refused` and now arrives as `ready` with an
  empty list — and nothing exercised the branch it took. Both are held by tests now, and each was
  proved by removing the thing it pins.

  The catalogue guard is the one worth reading: `the_existing_catalogue_answers_gained_no_field`
  asserted key sets inside a loop over the catalogue with **no non-vacuity check**, so an empty
  catalogue would have passed it without comparing a single set. It was non-vacuous in fact, which is
  exactly why it could stop being so silently. The proof runs the counterfactual — the same emptied
  walk with the counter removed reports `ok`.

- **A connector with a templated host can be invoked** (X-47). Invoke landed and immediately showed
  that a large minority of connectors could not run at all: their `base_url` is templated on a
  per-connection value and there was nowhere for a tenant to supply it. **Seventeen connectors** —
  the count is derived by rehearsing the shipped catalogue rather than scanning `base_url`, because
  five carry their configuration variables elsewhere in the operation's compiled Flux.

  **Configuration is not a credential and is not stored as one**: its own file, its own port, and
  bounds never summed with the credential allowance. Values are not read back out.

  **Only a signed-in human may supply a setting.** An independent re-review measured a tenant's
  credential on the wire at an origin the *caller* chose — because a suffix pin constrains which
  **vendor** a request reaches, not **whose account** at that vendor, and `*.zendesk.com`,
  `*.atlassian.net`, `*.myshopify.com`, `*.supabase.co` and `*.my.salesforce.com` are all
  self-service registrable. An earlier draft of this entry claimed the four refusals below were the
  security property; they were half of it. The write is now gated by principal *kind*, so an agent's
  token cannot become delivery of its tenant's credential to an origin it named, and the value is
  refused on the way **out** of the store as well as on the way in.

  What is *not* closed is stated in the design rather than left to be found: a human of the tenant
  who did not supply the credential can still read it out this way, because values are write-only
  here. That needs an operator-scoped surface, which does not exist yet.

  **Four connectors are deliberately refused**: for
  `newrelic`, `docusign`, `okta` and `freshdesk` the templated value *is the entire destination
  authority*, so supplying it would have been a way for a caller to name a host — and the tenant's
  credential would have travelled there. The rule is about the **template**, not the value, and it is
  enforced on read as well as on write, so a value that reached the file some other way is still
  refused. The listing says which connectors are unconfigurable and why, rather than letting them
  read as broken.

- **An operator can mint an agent from the console** (X-45), and the token is shown **once**. The
  store keeps a verifier, so this host genuinely cannot show it again — the screen says so, and
  offers no affordance implying otherwise. The token is held in the view's own scope rather than the
  application root, so navigating away is the state ceasing to exist rather than something
  remembering to clear it, and that is asserted through a real component lifecycle.

- **A connector's declared credentials are published** (X-46). `GET
  /api/catalogue/connectors/{id}/credentials` names what a connector requires. Before this, nothing
  published the fact, so the console discovered it by issuing a create it knew would be refused and
  reading the refusal — which coupled it to an error body. The declaration only: names, authority
  and leaf, **never whether anyone holds them**.

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
