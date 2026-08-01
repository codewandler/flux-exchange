---
id: X-42
title: "An agent can fetch what it needs instead of reading a page"
status: ready
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
- [ ] **Failing-first test** — the descriptor is served, anonymously, and the route appears in the
      declared anonymous surface. It must fail before the route exists.
- [ ] **Nothing tenant-specific, asserted adversarially.** No connector list, no principal, no
      address, no counts, no configuration values, no endpoint this host was configured to talk to.
      A test drives it with two tenants connected and asserts the answer is byte-identical.
- [ ] The descriptor names, at minimum: what this service is, the auth scheme, and **which
      capabilities are live** — derived from the same source of truth, not restated.
- [ ] **The page and the descriptor agree**, asserted by a test that compares them rather than
      checking each separately. Two renderings that can drift are the failure this epic's design
      names explicitly.
- [ ] It answers correctly on a deployment with **no identity provider configured** — that host still
      exists and still serves `/health` and the catalogue, and its descriptor must say sign-in is
      unavailable rather than pretending.

## Notes
- Read `docs/designs/agent-onboarding.md` §3 before choosing a shape. "Similar to a skill" is the
  brief: small, stable, fetchable, naming endpoints and capabilities.
- **Do not invent a standard.** If an existing one fits (a well-known URI, a small JSON document),
  prefer it and say why. If none does, keep it minimal and version it.
- The hard part is not the format. It is that this is a **public** endpoint on a credential-holding
  service, so every field is a decision about what a stranger may learn.
