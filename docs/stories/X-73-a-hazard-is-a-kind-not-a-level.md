---
id: X-73
title: "A weakness in how a credential is obtained is a declared kind, not a rung on the risk ladder"
status: ready
priority: 1
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
areas: [exchange-host]
note: "the vocabulary the filter is written against: AuthHazard::ResourceOwnerSecretShared, citing RFC 9700 §2.4 and CWE-522. Not a fifth Risk value — a password grant buying a read-only token is Risk::Low and hazardous, so at_most(High) would admit it"
---

# A weakness in how a credential is obtained is a declared kind, not a rung on the risk ladder

## Goal
`exchange-host` carries a closed vocabulary for **named weaknesses in how a credential is obtained**,
so a deployment can refuse one by property rather than by connector name.

## Why this is not a fifth `Risk`

`Risk` is an **ordered** severity ladder — `Low < Medium < High < Destructive` — and the ordering is
load-bearing, because `Selector::at_most` compares against it. A hazard has no position on that
ladder. A password grant that buys a read-only token is `Risk::Low` *and* hazardous, so a fifth rung
placed anywhere is wrong in one direction or the other: place it high and every destructive
operation inherits a weakness it does not have; place it low and `at_most(Risk::High)` silently
admits password-grant authentication to every grant an operator has already written.

It is also not on `OperationFacts`. A hazard is a property of an **acquisition**, which happens once
per connection; an operation happens per call. Putting it there restates one fact on 389 rows.

## The name is a citation

`ResourceOwnerSecretShared`, and the doc comment carries what makes it checkable rather than a
coinage:

- **RFC 9700 §2.4** (Best Current Practice for OAuth 2.0 Security, 2025) — the resource owner
  password credentials grant **MUST NOT** be used. Three stated reasons, and they are the whole
  hazard: it exposes the resource owner's credentials to the client; it widens where those
  credentials can leak beyond the authorization server; and it cannot carry two-factor or any
  multi-step authentication.
- **RFC 6749 §4.3** — the client MUST discard the credentials once an access token is obtained.
- **CWE-522**, Insufficiently Protected Credentials, as the nearest weakness-catalogue entry.
- OAuth 2.1 drops the grant entirely.

## Acceptance
- [ ] `AuthHazard` exists in `exchange-host`, `#[non_exhaustive]`, `serde` snake_case, with
      `ResourceOwnerSecretShared` as its first and only value.
- [ ] The doc comment names RFC 9700 §2.4, RFC 6749 §4.3 and CWE-522 — a reviewer can check the claim
      without leaving the file.
- [ ] **Failing-first test** — an unknown hazard spelling **refuses at deserialization** rather than
      round-tripping to a default. Write the test against `"resource_owner_secret_sharing"` (the
      near-miss spelling), watch it fail, then close it. A typo that reads as *no hazard declared* is
      the failure mode this vocabulary exists to make impossible.
- [ ] Nothing in `Risk`, `Selector` or `OperationFacts` changes. A test or a review note states that
      `Selector::at_most` is untouched, so no existing grant's meaning moves.
- [ ] No transport, no new dependency: `crates/exchange-host/Cargo.toml`'s `[dependencies]` allow-list
      and `tests/no_second_request_path.rs` are unchanged.

## Progress
- (not started)

## Notes
- Mirror vocabulary, so the mapping rule from `OperationFacts::of` applies the day upstream declares
  one: **exhaustive match, no wildcard arm, in both directions.** A catch-all would answer a hazard it
  had never heard of with a plausible wrong one, and the filter would then admit it without anybody
  having decided to.
- [[X-74]] is the consumer. Land this first; it is the only story in the epic with no behaviour.
