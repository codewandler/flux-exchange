---
id: X-74
title: "A deployment refuses a hazardous way of authenticating unless it opted in"
status: ready
priority: 2
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
areas: [exchange-host, exchange-server]
note: "ordered BEFORE X-75 for the reason X-40 was ordered before X-37: X-75 is what makes the hole reachable, and a gate that lands after it has already shipped ungated once. Unset means refuse — the production default needs no configuration to be safe"
---

# A deployment refuses a hazardous way of authenticating unless it opted in

## Goal
A production deployment cannot authenticate through a declared-hazardous acquisition, and turning
that off is one explicit statement an operator writes and an auditor can read.

## Why this lands before the thing it gates

[[X-75]] is what makes the hazard reachable. A filter written afterwards has, by definition, shipped
a release in which the hazardous path was open — which is exactly the argument that put X-40 ahead
of X-37 when X-36's implementor found the hole in the surface it had just built.

## The shape, and where each part comes from

```
FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS=resource_owner_secret_shared
```

- **Unset — every declared hazard is refused.** The safe state is the default and needs no
  configuration, which is the property `admit_bind` and `Deployment::admits` already have. This is
  fail-closed and, like X-13's grant gate, **it will look like an outage** to somebody who did not
  read the release note. Say so where they will be reading.
- **Set and naming a value this build does not recognise — the process refuses to start, and names
  the value.** It does not skip the unrecognised entry and arm the rest. `DevIdentity`'s roster
  settled this argument already: *a roster that silently lost a principal is a roster whose operator
  is debugging the wrong thing.*
- **No per-request override, and no body field or header reaches it.** Same rule as the runtime and
  the tenant: this is read at startup from configuration, and a caller cannot name it.

## Where it is enforced, which is not only at startup

At **acquisition time** — the moment a connection tries to obtain a credential through the hazardous
path — and not solely at startup. A catalogue update can introduce a hazard into a deployment that
started clean, and a check that ran only at boot would let that one through until the next restart.

The refusal **names the hazard and the connector, and never a value** (principle 5). It must be
distinguishable from "the vendor rejected these credentials", or an operator will spend the outage
re-typing a password that was correct — the defect class X-17 and X-20 exist for.

## Acceptance
- [ ] An `AuthPosture` (or equivalently-named value) in `exchange-host` decides admission from an
      `AuthHazard`, with no constructor taking caller input.
- [ ] `exchange-server` reads `FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS` **by name**, through the same
      by-name configuration path X-27 established — not a positional read, and not a fourth list of
      variable names beside the three that already describe one set.
- [ ] **Failing-first test** — an acquisition declaring `ResourceOwnerSecretShared` is refused with
      the posture unset, and admitted with it armed. Write the refusing half first against a fixture
      declaring the hazard, and watch it pass for the wrong reason (nothing declares a hazard yet)
      before pinning it against a fixture that does.
- [ ] A test that an unrecognised value in the variable **refuses at startup and names it**, rather
      than arming the recognised remainder.
- [ ] The refusal carries its own status and is distinguishable from a vendor rejection — asserted,
      following `every_refusal_states_the_status_it_answers_with`.
- [ ] `README.md` and `AGENTS.md` § Status say that this is fail-closed and what the outage looks
      like, in the same change. A gate whose refusal is undocumented is X-13's lesson repeated.

## Progress
- (not started)

## Notes
- Depends on [[X-73]] for the vocabulary. Does **not** depend on [[X-75]] — the fixture stands in for
  a declaring connector, and that is the point of landing it first.
- The upstream declaration is **C-440** in flux-connectors. Until it lands, no shipped connector
  declares a hazard, so this gate is exercised only by fixture — state that in the test's own comment
  so a reader does not mistake a green suite for coverage of a live path.
