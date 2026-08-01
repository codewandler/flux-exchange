---
id: X-70
title: "A setting whose values are a closed vendor set is not the caller naming a host"
status: in-progress
priority: 1
epic: connections
design: docs/designs/connection-settings.md
areas: [exchange-host]
note: "found by X-67, 2026-08-01: intercom is now refused because its base_url is `https://{host}`, but that {host} is a closed set of three vendor hostnames the catalogue publishes — host_pinning reads the template and never the choices, so it errs closed on a value that could not name anything"
---

# A setting whose values are a closed vendor set is not the caller naming a host

## Goal
A tenant on intercom's EU or AU region can configure their connection.

## What happened

X-67 moved to catalogue 0.10 and X-47's guard did exactly what it was built to do: it turned red,
naming a **fifth** connector whose templated value is the whole destination authority.

```
left:  [docusign/…, freshdesk/…, intercom/endpoint.host ({host}), newrelic/…, okta/…]
right: [docusign/…, freshdesk/…, newrelic/…, okta/…]
```

Upstream C-225 changed intercom's `base_url` to `https://{host}`. A bare placeholder **is** the whole
authority, so `host_pinning` answers `WholeAuthority` and the setting is refused.

**That refusal is correct under the rule and wrong about intercom.** The same upstream change shipped
`config_choices`, and intercom's `{host}` is a **closed set of three vendor hostnames**:

```
crates … /codewandler-connector-catalog-0.10.0/src/generated/intercom.rs
  base_url: "https://{host}"
  Choice { value: "api.intercom.io",    label: "United States" }
  Choice { value: "api.eu.intercom.io", … }
  Choice { value: "api.au.intercom.io", … }
```

A caller choosing among three hostnames the **vendor** published is not a caller naming a
destination. It is the same act as choosing a region from a dropdown, and it cannot reach
`evil.example` because the value is not free.

## Why this is not simply "relax the rule"

X-47's rule is deliberately about the **template, never the value** — *"a value rule would be a
blocklist, and a blocklist catches only what somebody enumerated."* That reasoning is why the guard
found four connectors where a hand-written list would have found two, and it must survive this story.

The distinction that makes this safe is that a closed choice set is **not a value rule**: it is a
second piece of *declared catalogue data*, published by the same source the host rule is derived
from. Admitting a value because it is one of the choices the catalogue declares is still deciding
from the catalogue — the property X-47 exists to keep. Admitting it because it *looks* fine is the
thing that must stay refused.

So: **`host_pinning` gains a fourth answer**, and the guard admits only a value that is **exactly**
one of the declared choices. Not a prefix of one, not a suffix match, not a case-insensitive
comparison — equality against a set the tenant cannot influence.

## Acceptance
- [x] **Failing-first test** — a tenant may set intercom's `endpoint.host` to `api.eu.intercom.io`,
      and the connection dispatches there. It is refused today.
      → `connection_settings.rs::a_tenant_may_choose_intercoms_declared_region_and_the_request_goes_there`
- [x] **Failing-first test** — a value that is *not* one of the declared choices is refused, including
      one that merely contains or extends a choice (`api.eu.intercom.io.evil.example`,
      `API.EU.INTERCOM.IO`, ` api.eu.intercom.io`). The refusal names the address, not the value.
      → `connection_settings.rs::only_an_exactly_declared_choice_may_be_supplied`, and over HTTP in
      `routes::connections::tests::no_route_lets_a_tenant_supply_a_value_the_catalogue_does_not_declare`
- [x] The rule stays **derived**: the admitted set comes from `connector_catalog`'s published choices
      and from nothing written in this repository. **Failing-first test** — a connector whose
      choice set is empty or absent is still refused as `WholeAuthority`.
      → `connection_settings.rs::what_a_tenant_may_supply_is_read_off_the_catalogues_own_choices`
      walks the whole catalogue in both directions;
      `settings.rs::host_pinning` reads `Provider::choices_for` and this repository enumerates no
      hostname anywhere
- [x] Enforced at `ConfigStore::get` as well as at `set`, like every other answer of `host_pinning` —
      a value that reached the file some other way is still checked against the choices.
      → `HostPinning::admits` is asked at both points;
      `connection_settings.rs::a_planted_value_is_admitted_only_when_it_is_a_declared_choice`
- [x] The catalogue-wide census is re-run and the story records the new split. If any connector other
      than intercom moves, that is a finding. → **`newrelic` moved too — see Progress.**
- [x] `docs/designs/connection-settings.md` §4 gains this as a further correction, in the shape §4
      already uses — it now carries two. → § *The third correction: a closed set the catalogue
      publishes is not the caller choosing (X-70)*; §4 now carries three.

## Progress

**Implemented on `impl/X-70`.** `host_pinning` gained a fourth answer, `HostPinning::ChosenFrom(Vec<String>)`,
and the value question moved to one method, `HostPinning::admits(value)`, asked at both enforcement
points. The refusal for a wrong value is a new variant, `SettingsRefusal::NotADeclaredChoice`, which
quotes the catalogue's choices and never the value; the route answers it `422` with a `choices` array.

**The census moved two connectors, not one — `newrelic` is the finding.** Upstream C-225 shipped
`config_choices` for `newrelic` as well as for `intercom`: two region hosts, `api.newrelic.com` and
`api.eu.newrelic.com`. A rule derived from the catalogue admits it for the same reason it admits
intercom, and special-casing intercom would have been exactly the enumerated list this story says
must not exist. Recorded rather than smoothed over, because it is a **widening of the write surface
nobody asked for in the story**.

The new split, measured (catalogue 0.10, 54 providers, 19 declaring a per-connection value):

| answer | settings |
| --- | --- |
| `WholeAuthority` (refused) | `docusign/endpoint.account_host ({account_host})`, `freshdesk/endpoint.domain ({domain})`, `okta/endpoint.domain ({domain})` |
| `ChosenFrom` (new) | `intercom/endpoint.host` (3 regions), `newrelic/endpoint.host` (2 regions) |

Configurable connectors: **51 of 54**, up from 49. `no_shipped_connector_lets_a_tenant_supply_its_whole_authority`
now pins **both** lists with the declared values written out, so a widening arrives as a failing test.

Falsified rather than reported — each of these was made and the gate went red:

- `get` asking `tenant_may_supply` instead of `admits` → `a_planted_value_is_admitted_only_when_it_is_a_declared_choice`
  **and** the older `a_planted_whole_authority_value_is_refused_on_the_way_out` fail, the second by
  dispatching to `https://evil.example` with the tenant's `X-Api-Key`.
- the `set`-side membership check deleted → `only_an_exactly_declared_choice_may_be_supplied` and
  `a_setting_cannot_become_the_destination_authority` fail.
- equality loosened to substring / trimmed / case-insensitive → both the `set` and `get` tests fail.

Not done, and deliberately out of scope: `GET /api/connections/{connector}/settings` still does not
publish the choices for a `ChosenFrom` row, so a client cannot render the dropdown without provoking
a `422` first. That is C-87-shaped and belongs in its own story.

## Notes
- ⚠ **This is a change to the safety surface** and it is the third time this file has been revised on
  a credential path. It gets an independent review with the value-equality edges above driven, not
  read.
- Until it lands, an intercom tenant in the EU or AU cannot configure their connection at all, and a
  tenant who had configured one before the 0.10 catalogue is refused on the way **out** of the store
  by `a_planted_whole_authority_value_is_refused_on_the_way_out`. That is fail-closed and correct, and
  it will read as an outage.
