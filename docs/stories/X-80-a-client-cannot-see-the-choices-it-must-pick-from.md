---
id: X-80
title: "A client learns a connector's permitted values only by being refused"
status: done
priority: 0
epic: connections
areas: [exchange-server, console]
note: "Decision 0002 prerequisite to X-125: publish closed choices before one generated browser/CLI connection form can render them"
---

# A client learns a connector's permitted values only by being refused

## Goal
A client can render intercom's region picker from what the settings surface publishes, without
sending a request it expects to fail.

## What happened

X-70 made a closed vendor set configurable: intercom and newrelic tenants may now supply a region,
and only a value the catalogue declares is accepted. The refusal is good — `422` naming the connector,
the field and the **choices**. The gap is on the way in:
`GET /api/connections/{connector}/settings` reports `suppliable: true` and stops there
(`connections.rs:1013` still asks `tenant_may_supply()`), so the set is discoverable only by guessing
wrong.

**This is X-46's shape, and X-46 is the precedent that says it is worth a story.** There, nothing
published what a connector declared, so the console read it out of the `422` a deliberately-empty
POST returned. Its implementor called that a workaround in the report, and it became X-46. A client
provoking a refusal to learn a fact the server already knows is the same thing again — this time on a
surface that just shipped.

## Acceptance
- [x] The settings surface publishes the declared choices for a `ChosenFrom` field.
- [x] **Failing-first test** — a client can build the permitted-value list from one `GET`, with no
      refused request. Assert it against the response, not against the catalogue.
- [x] The console renders a choice control rather than a free-text box for such a field, or a story
      says why it does not yet.
- [x] A field that is not `ChosenFrom` publishes no choices — absence must mean *free within the
      other rules*, not *no data*.

## Progress

- 2026-08-03: The successful settings read now publishes a `choices` array only for
  `HostPinning::ChosenFrom`, on both sole and labelled-instance paths. The route test builds
  Intercom's region list from that response alone; a companion test holds unrestricted Zendesk
  fields to omitting the property.
- 2026-08-03: [[X-125]] owns the console half. The current console has no settings form, and X-125's
  complete labelled plan must provide field identity, instance selection and submission target
  before a dropdown can actually write the chosen value. Its Progress records why a partial picker
  here would still leave the first-run connection incomplete.

## Notes
- X-70 named this and deferred it as C-87-shaped; its independent review confirmed it independently.
  Filing it so the deferral is a story rather than a paragraph in a closed story's Progress.
- **Decision 0002 scheduling, 2026-08-03:** promoted to priority-zero ready work and made an explicit
  prerequisite of [[X-125]]. X-125 consumes this response contract; it does not replace or weaken
  any acceptance item here.
- Related: [[X-46]] (declarations published rather than provoked), [[X-50]] (a connector that declares
  nothing).
