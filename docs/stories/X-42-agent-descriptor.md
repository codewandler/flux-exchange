---
id: X-42
title: "An agent can fetch what it needs instead of reading a page"
status: in-progress
priority: 3
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
- **Carried forward:** two `withheld` strings reach the wire in console vocabulary ("there is no
  screen to call an operation from", "there is no Activity screen"). That is deliberate — the design
  requires the gap be stated in the surface's own words, once — but if a later story wants
  API-shaped prose there, the place to change it is `surfaces.mts`, and the page changes with it.
