---
id: X-125
title: "Generate one complete labelled connection plan for browser and CLI"
status: ready
priority: 0
epic: connections
areas: [console, exchange-server, catalogue]
depends_on: [X-80]
note: "Milestone 1 — one declaration-driven contract asks for the label, every secret and setting, and exposes incomplete or partial state honestly to browser and Flux CLI"
---

# Generate one complete labelled connection plan for browser and CLI

## Goal

A first-time operator can create and maintain a named connection from either the Exchange console or
Flux CLI without knowing Exchange's credential, setting and instance routes. Exchange publishes one
machine-readable plan derived from connector declarations, and both clients execute that same
contract without moving a credential across the Exchange boundary.

## Why this is Milestone 1 work

The server already has separate surfaces for labelled instances, credentials and non-secret
settings, but a client cannot assemble a complete form from them. The console currently omits facts
such as Jira's site and Zendesk's subdomain, so a connection can appear created while every invocation
will refuse. Flux's proposed `integration connect <connector> --name <name>` would repeat that defect
if it invents a second vendor-specific schema.

The plan is a projection of connector-owned configuration metadata, not a list maintained by
Exchange. It describes which fields must be supplied and where Exchange accepts them; it never
contains a supplied secret value. The operator-chosen name is the existing tenant-scoped connection
label from X-14, not a host, authority, credential address, instance UUID or runtime selector.

## Acceptance

- [ ] An authenticated human can fetch a machine-readable connection plan generated from the
      connector's declared credentials and configuration metadata. **Failing-first contract test:**
      adding a required declared field to a fixture makes the plan test fail until that field is
      projected; no Exchange-maintained connector or vendor field list may satisfy the test.
- [ ] The plan asks for `name` first and maps it to X-14's existing label/instance lifecycle. It can
      create a labelled connection, list its existing labels, select one for editing, and rename a
      label without moving the host-minted instance UUID, credential or settings.
- [ ] Every required secret and non-secret setting is present exactly once, with stable field
      identity, service, human label, required/optional status, input kind and submission target.
      A declared required field that cannot be rendered or routed makes the plan visibly incomplete;
      it is never silently omitted.
- [ ] **Failing-first census test:** the generated contract includes Jira Cloud's declared site,
      account email and API token, and Zendesk's declared subdomain/domain, account email and API
      token. The test derives those expectations from the shipped connector declarations and fails
      if either browser or CLI projection drops one.
- [ ] When the upstream GitLab declaration supports an operator-pinned custom HTTPS endpoint, the
      same generic projection exposes it without a GitLab-only Exchange schema. The connector owns
      its API path and the host's operator policy admits the origin; neither model input nor a
      Service Account can choose or change the origin.
- [ ] A closed declared set carries its permitted choices in the plan and both clients render it as
      a choice/dropdown rather than unrestricted text. [[X-80]] is a prerequisite and retains its
      complete acceptance: clients learn choices from one successful read, while fields without a
      closed set publish no choices.
- [ ] Secret field descriptors may be returned, but secret **values** travel directly from the
      human-controlled input to an Exchange-owned write surface. They never appear in the plan, any
      response, URL/query, Flux argument or configuration, application log, activity record, or
      browser navigation/history. Non-interactive Flux CLI use does not accept a vendor secret on
      argv; secure stdin/prompt or an Exchange-owned browser handoff is required.
- [ ] Non-secret settings use the existing tenant-, connector-, service- and label-scoped settings
      route and persistence from X-47/X-14. Reloading the plan reports whether each required field is
      set without returning the stored value, and a missing setting keeps the connection visibly
      incomplete.
- [ ] Applying the composite plan cannot report a misleading complete connection after only some
      writes land. Either one checked atomic operation commits label, settings and credentials
      together, or the API publishes an explicit ordered apply plan with per-step outcomes,
      retry/compensation semantics and an overall incomplete/partial state until every required step
      succeeds. A failing-first persistence test drives a refusal in the middle and proves the
      reported state matches what survived.
- [ ] The Exchange console and Flux CLI consume the same versioned plan response and submission
      semantics. A contract fixture is exercised through both clients; neither client maintains
      vendor-specific required-field, routing or completion logic. Interactive prompts and flags
      such as `--endpoint`, `--site` or `--domain` are aliases onto declared field identities, not a
      second schema.
- [ ] The console renders existing labels and allows a human to rename them, shows required versus
      optional fields and choice controls, and distinguishes complete, incomplete, refused and
      partially-applied states without displaying any stored value.

## Progress

- 2026-08-03: Filed from cross-repository Decision 0002 after the first-run tutorial exposed that a
  runtime-ready GitLab/Jira path still cannot be established from either client. X-80 was promoted to
  priority-zero ready work as this story's explicit response-contract prerequisite.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0002-declaration-driven-connection-onboarding.md`.
- Reuse X-14's label-to-host-minted-UUID lifecycle and X-47's setting storage. Do not create another
  connection identity or configuration store for the form.
- A custom origin is configuration selected by an operator under deployment policy, never request
  input to `invoke`. Request construction and permission subjects must resolve the same pinned
  origin; the connector story owns that upstream declaration and safety proof.
- This contract does not hand vendor credentials to Flux. Flux is a plan consumer and may arrange a
  secure Exchange-owned handoff; Exchange remains the only credential holder and integration
  executor.
