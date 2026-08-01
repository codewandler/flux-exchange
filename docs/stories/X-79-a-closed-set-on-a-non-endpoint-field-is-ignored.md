---
id: X-79
title: "A catalogue-declared closed set is only honoured on an endpoint field"
status: backlog
epic: connections
areas: [exchange-host]
note: "found by X-70's independent review, 2026-08-02: the SettingKind::Username short-circuit at settings.rs:507 returns before the choices_for lookup ten lines later, so a closed set declared on a non-endpoint field is silently ignored. Nothing shipped is affected and the census test would fire — but the rule reads as general and is not"
---

# A catalogue-declared closed set is only honoured on an endpoint field

## Goal
`host_pinning` honours a declared closed set wherever the catalogue declares one, or says in the code
which kinds it reads it for.

## What the review found

X-70 made a catalogue-declared closed set admissible: a value is accepted when it is **exactly** one
the catalogue publishes. The lookup sits after a `SettingKind::Username` short-circuit that returns
first, so the set is consulted only for an endpoint field.

**Nothing shipped is affected.** Catalogue 0.10 declares exactly two `ConfigChoices` — intercom and
newrelic — and both are `kind: "endpoint"`, verified across all 54 generated sources during X-70's
review. And the day upstream publishes one on a username field,
`no_shipped_connector_lets_a_tenant_supply_its_whole_authority` fires: the census is pinned in both
directions, so this cannot arrive silently.

So this is not a hole. It is a **rule that reads more general than it is**, which is the category this
repository keeps paying for — see [[X-52]] and [[X-78]].

## The decision, which is the actual work
Either honour the set for every kind, or state in `host_pinning` that a closed set is read for
endpoint fields only and why. The second may well be right: a username is not an authority, and
admitting a value because the vendor listed it is a different argument there. **Do not widen it
reflexively** — X-70's whole rule is that admitting a value must still be deciding from the
catalogue, and the reason it is safe for a host is that a host is a *destination*.

## Acceptance
- [ ] Decide, and say which in the code rather than here.
- [ ] **Failing-first test** — if honouring it for every kind, a fixture declaring a closed set on a
      username field is enforced. Watch it be ignored first.
- [ ] `docs/designs/connection-settings.md` agrees with whichever answer is chosen.

## Progress
- 2026-08-02 — found by the independent review of [[X-70]], classified MINOR and non-blocking.
