---
id: X-42
title: "An agent can fetch what it needs instead of reading a page"
status: in-progress
epic: agent-onboarding
design: docs/designs/agent-onboarding.md
areas: [exchange-server, console]
note: "the other half of onboarding: a page is a human artifact, and the charter's primary caller does not read pages. One truth, two renderings — and a test that they agree"
---

# An agent can fetch what it needs instead of reading a page

## Goal
An agent can obtain the facts on the onboarding page in a form it can parse.

## Why this is separate from X-41

X-41 renders a page from what the console knows. **This story publishes the same facts from the
service**, which is a different act: it widens the **anonymous surface**, and that is a security
decision rather than a copy decision.

`routes::tests::the_anonymous_surface_is_only_what_was_declared_anonymous` exists precisely so
widening it is deliberate and tested. Whatever is published here must be reviewed as a **disclosure**.

## Acceptance
- [x] **Failing-first test** — the descriptor is served, anonymously, and the route appears in the
      declared anonymous surface. It must fail before the route exists.
- [x] **Nothing tenant-specific, asserted adversarially.** No connector list, no principal, no
      address, no counts, no configuration values, no endpoint this host was configured to talk to.
      A test drives it with two tenants connected and asserts the answer is byte-identical.
- [x] The descriptor names, at minimum: what this service is, the auth scheme, and **which
      capabilities are live** — derived from the same source of truth, not restated.
- [x] **The page and the descriptor agree**, asserted by a test that compares them rather than
      checking each separately. Two renderings that can drift are the failure this epic's design
      names explicitly.
- [x] It answers correctly on a deployment with **no identity provider configured** — that host still
      exists and still serves `/health` and the catalogue, and its descriptor must say sign-in is
      unavailable rather than pretending.

## Notes
- Read `docs/designs/agent-onboarding.md` §3 before choosing a shape. "Similar to a skill" is the
  brief: small, stable, fetchable, naming endpoints and capabilities.
- **Do not invent a standard.** If an existing one fits (a well-known URI, a small JSON document),
  prefer it and say why. If none does, keep it minimal and version it.
- The hard part is not the format. It is that this is a **public** endpoint on a credential-holding
  service, so every field is a decision about what a stranger may learn.

## Progress

- **`GET /api/onboarding`**, `Access::Anonymous`, served by `crates/exchange-server/src/routes/onboarding.rs`.
  Console 70 tests, Rust 52 + 247 + integration suites, gate green.
- **No well-known URI, and that is the answer to "do not invent a standard".** RFC 8615 keeps a
  registry and nothing in it means "how an agent authenticates to this service". Neighbouring
  ecosystems *do* serve agent cards under `/.well-known/`, which is the argument against borrowing
  one of those names rather than for it: a client that knows such a standard would parse this
  document against a schema it does not follow and act on the result. So a private, self-naming,
  self-versioning document under `/api`, and the module documentation carries the argument.
- **What is published, and why a stranger may learn each of it.** `descriptor`, `version`,
  `endpoint`, `service.{name,summary}`, `authentication.{scheme,header,capability,live}`, and per
  capability `{id,title,summary,live,call{method,endpoint,caller,note,warn},withheld}`. Every one of
  those is a `&'static` fact about **the build** — identical bytes in every deployment of a version,
  read from no composition, no store, no catalogue and no request. The auth scheme is one the guard
  already answers to anybody who sends a request.
- **The one deployment-varying field is `sign_in_available`, and it was chosen because this surface
  already publishes it** at `/api/signin/availability` (X-43). Embedding a fact a caller could get
  with one more request is not a widening; inventing a second one would have been. It is what makes
  the Acceptance's last item honest rather than a technicality: on a host with no identity provider,
  the mint capability's caller is still "a signed-in human", so silence there would read as a host
  somebody could be minted on.
- **The types are `deny_unknown_fields`, and that is the disclosure control.** The artifact is
  generated; a field added to the console model and regenerated does **not** quietly appear on the
  anonymous surface — it fails to parse until somebody adds it to `Onboarding` next to the argument
  for publishing it. `an_artifact_this_host_cannot_read_refuses_rather_than_serving_half_a_document`
  drives that with a `tenant` field spliced in.
- **The agreement test compares the two artifacts, not the two models.** It renders the page and
  reads the JSON the service compiles in, then walks it field by field in both directions. Falsified
  twice at implementation: dropping the `warn` line from the page turns it red, and editing one
  title in the served document turns it red.
- **A real defect the regression case caught.** `descriptor()` first spelled `call` as "whatever the
  step carries", which agrees with the page today only because no withheld step carries one. Under a
  surface regressing to unbuilt the page drops the instruction and the document would have kept
  publishing the endpoint — a capability marked not-live still inviting a call. It is now gated on
  `live`, which is the page's own rule.
- **Carried forward:** one `withheld` string reaches the wire in console vocabulary ("there is no
  Activity screen"). Deliberate — the design requires the gap be stated in the surface's own words,
  once — but if a later story wants API-shaped prose there, the place to change it is
  `surfaces.mts`, and the page changes with it.

## Progress — rework, round 1

**The descriptor published a false statement about `invoke`, and the coordinator caught it by
measuring against the route table.** `POST /api/operations/{operation}/invoke` shipped in v0.7.0,
is in `routes::MODULES`, and is gated `Access::Principal`; the document said
`"live": false, "withheld": "Nothing in this deployment can be called."`

- **The root cause was the source of truth, not the derivation.** `surfaces.mts`'s `built` answers
  *does this console have a screen*; the page and the descriptor ask *does this service do this*.
  The two agreed for every surface until `invoke` shipped a route with no screen. So the wiring was
  honest and pointed at the wrong construct — and the page/descriptor agreement test could not see
  it, because both renderings agreed **with each other** while both were wrong.
- **`surfaces.mts` now answers the two questions separately**: `built` (a screen) and `served` (the
  service publishes it). `invoke` is `built: false, served: true`. Nothing about the navigation
  changed — no surface's `built` moved — so the rail is untouched.
- **The enforcement is in Rust, against `routes::MODULES`**, which is the part worth more than the
  agreement test: `a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it` (a capability
  is live exactly when a route serves it) and
  `every_published_route_is_a_capability_or_is_argued_not_to_be` (every published route is offered
  as a capability or listed with an argument for why an agent author is not told about it). The
  second would have gone red on the day `invoke` landed. Both demonstrated red before the fix.
- **All six capabilities re-checked against the route table**, not against `surfaces.mts`:
  `read-the-catalogue` → `/api/catalogue/connectors` ✓ live; `be-minted` → `/api/agents` ✓ live;
  `invoke` → `/api/operations/{operation}/invoke` ✓ **now live** (was the falsehood); `authenticate`
  → no route, and the claim is about the identity port, pinned by
  `nothing_this_host_binds_resolves_an_agent_token`; `subscribe` → no route ✓; `read-what-happened`
  → no route ✓. Only `invoke` was wrong.
- **The same falsehood was in three renderings, and all three are corrected**, because fixing one
  leaves them disagreeing: the onboarding page (X-41), the mint screen's token standing (X-45), and
  the shell's "Not built: … it calls nothing" inventory sentence (X-34). The rail's per-entry tag is
  now `no screen` rather than `not built`, which is the distinction that caused this.
- **X-45's deliberate trip-wire fired, and was answered rather than silenced.** `minting.mts`'s
  `authorisation()` withdrew itself the moment a token-holder step became available, with a note
  saying whoever lands it must "say what is true instead". Withdrawing would have blanked the
  paragraph exactly when it acquired teeth, so the sentence now states the exposure — a token that
  can be presented will be admitted to every operation in the catalogue for its tenant, because
  invocation is gated by identity alone — and the withdrawal is re-keyed to the event that makes it
  a genuine grant question: a token that can actually be **presented** (`authenticate`).
- **The "no parameters in an endpoint" rule was the right instinct one notch too tight.** It would
  have meant omitting the one route that runs an operation, or describing it in prose to dodge a
  test. The rule is now "no **value**": path parameters must be catalogue keys, written out as a
  list (`operation`), so admitting a second is a decision. `/api/connections/{connector}` is still
  refused. The Rust side additionally requires every endpoint named to be a route this host
  actually publishes.
- **Found while re-checking, not fixed:** `service.mts` still maps every catalogue operation to
  `works: false` with the comment *"nothing in flux-exchange can be invoked yet"*, so the explorer
  reads "not live yet" on operations the service can run. Same root cause, different surface
  (X-07/X-46), and deciding what `works` should mean per operation is that story's call.

## Carried into X-37 — a trap this story leaves one story out

`nothing_this_host_binds_resolves_an_agent_token` (`onboarding.rs:708`) pins `authenticate`'s
`live: false` by asserting `AppState::without_identity().with_agents(store).identity().is_none()`.

That holds today. **It only fails at X-37 if X-37 makes `with_agents` set the identity port.** If
X-37 instead composes an `Identity` that consults the agent store, this pin and `SERVED_BY`'s
`("authenticate", None)` row both stay green **while the document becomes false** — the same
*internally consistent, externally false* shape as this story's round 1, arriving one story later.

X-37 must land with an assertion that flips both. This is recorded here and on the epic so it is not
discovered by an agent author.
