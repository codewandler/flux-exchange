---
id: X-73
title: "A weakness in how a credential is obtained is a declared kind, not a rung on the risk ladder"
status: in-progress
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
- [x] `AuthHazard` exists in `exchange-host`, `#[non_exhaustive]`, `serde` snake_case, with
      `ResourceOwnerSecretShared` as its first and only value.
      → `crates/exchange-host/src/acquisition.rs:67-69` (the attributes) and `:96` (the variant);
      re-exported at `crates/exchange-host/src/lib.rs:122`.
- [x] The doc comment names RFC 9700 §2.4, RFC 6749 §4.3 and CWE-522 — a reviewer can check the claim
      without leaving the file.
      → `crates/exchange-host/src/acquisition.rs:78-89`, and pinned by
      `the_doc_comment_carries_its_citations` so a later tidy-up cannot quietly drop one.
- [x] **Failing-first test** — an unknown hazard spelling **refuses at deserialization** rather than
      round-tripping to a default. Write the test against `"resource_owner_secret_sharing"` (the
      near-miss spelling), watch it fail, then close it. A typo that reads as *no hazard declared* is
      the failure mode this vocabulary exists to make impossible.
      → `tests/auth_hazard.rs::an_unknown_hazard_spelling_refuses_at_deserialization`. Watched to
      fail at `3ee3698` with ``no `AuthHazard` in the root``.
- [x] Nothing in `Risk`, `Selector` or `OperationFacts` changes. A test or a review note states that
      `Selector::at_most` is untouched, so no existing grant's meaning moves.
      → `src/grant.rs` is not in this diff at all, and
      `a_hazard_is_neither_a_rung_on_the_risk_ladder_nor_a_fact_about_an_operation` says so in a way
      that cannot rot: it names every field of `OperationFacts` and `Selector` in struct literals, so
      adding a hazard to either stops compiling, and it asserts `Selector::at_most(Risk::High)` still
      admits a `Risk::Low` operation.
- [x] No transport, no new dependency: `crates/exchange-host/Cargo.toml`'s `[dependencies]` allow-list
      and `tests/no_second_request_path.rs` are unchanged.
      → neither file is in the diff; `serde` and `serde_json` were already present, and the lock-1
      suite passes 9/9.

## Progress
- **Done, pending review.** `AuthHazard` lands in a new module `src/acquisition.rs` rather than in
  `connections.rs`: that module answers *where a credential lives*, and this is *how it got there*.
  It is also where §2's `AuthPosture` and §3's acquisition port go, so [[X-74]] extends this file
  rather than moving the type.
- **`Ord` is derived, and the doc comment says why it is not a ladder.** An allow-list of hazards
  wants to be a `BTreeSet`, which is the same reason `Effect` derives it. Since this story's entire
  argument is *kind, not level*, the derive carries an explicit note that the ordering is declaration
  order and nothing may read severity into it. If a reviewer would rather X-74 add it at the point of
  need, dropping it here is a one-line change with no other caller.
- **`#[non_exhaustive]` from the start**, unlike `HostPinning` and `SettingsRefusal` (X-70), which
  gained variants without it. The doc comment records the thing that looks contradictory and is not:
  `#[non_exhaustive]` binds *consumers*, and a match written inside this crate is still exhaustive
  with no wildcard arm, per `OperationFacts::of`'s rule.
- No behaviour, by design — nothing consumes this yet. [[X-74]] is the consumer.

## Notes
- Mirror vocabulary, so the mapping rule from `OperationFacts::of` applies the day upstream declares
  one: **exhaustive match, no wildcard arm, in both directions.** A catch-all would answer a hazard it
  had never heard of with a plausible wrong one, and the filter would then admit it without anybody
  having decided to.
- [[X-74]] is the consumer. Land this first; it is the only story in the epic with no behaviour.
