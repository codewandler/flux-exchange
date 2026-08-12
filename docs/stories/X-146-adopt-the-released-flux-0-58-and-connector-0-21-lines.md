---
id: X-146
title: "Adopt the released flux 0.58 and connector 0.21 lines"
pillar: "Core"
status: blocked
areas: [build, exchange-host, exchange-server, docs]
design: docs/designs/released-domain-audit.md
note: "blocked: connector 0.21 is NOT published — crates.io's newest connector line is 0.20.0. Both pin sets move as one unit once it ships; two compile breaks are already known (C-531 DispatchId on FlowSink, and ConfigField gaining also_services)"
---

# Adopt the released flux 0.58 and connector 0.21 lines

## Goal

Move Exchange onto the published connector 0.21 / flux 0.58 graph as one compatibility unit and
re-issue the released-domain audit for that line. This is scheduled after the v0.18.0 release
train: X-134, X-139 and X-126 build and freeze evidence on the 0.54 line, and nothing they consume
exists only in 0.55–0.58. Going first is impossible anyway — connector-pack 0.20 requires
`flux-runtime ^0.54`, so a lone flux bump would put two `Tool` traits in one lock, the exact
failure X-11 removed.

## Blocked on: connector 0.21 does not exist yet

Verified 2026-08-12 against the crates.io sparse index:

| crate | newest published |
|---|---|
| `codewandler-connector-catalog` / `-pack` / `-secrets` / `-address` | **0.20.0** |
| `codewandler-flux-runtime` / `-lang` | 0.59.3 (0.58.0 is published) |

The flux side is available; **the connector side is not**. The 0.21 metadata this story and
[[X-147]] read — `catalog::Acquisition::OAuth2`, `catalog::Subject`, `catalog::OAuthGrant`,
`catalog::OAuthRedirect`, `catalog::OperationDirection` — exists only in the flux-connectors working
tree (`main` @ `428938cd`, `crates/catalog/src/lib.rs`), which is itself still versioned `0.20.0`.
Nothing is released, so there is no pin to move to. **This does not become unblocked by using a
`path` or `git` dependency on that checkout** — [`AGENTS.md`](../../AGENTS.md) § The dependency
situation refuses that outright.

Nor can the flux side go first: `connector-pack` 0.20 requires `flux-runtime ^0.54`, so raising only
the engine puts two `Tool` traits in one lock.

**Unblocks when:** flux-connectors publishes the 0.21 line to crates.io.

## Preflight already done — two known compile breaks, not zero

An early claim that this bump is "non-breaking for exchange" is **half right**, and the wrong half is
a compile error. Checked against released 0.20.0 versus the flux-connectors tree:

- ✅ **Exchange never constructs `catalog::Credential`.** It has its own `exchange_host::DeclaredCredential`
      (`crates/exchange-host/src/connections.rs:592` and friends). `Credential` gaining
      `subject: Subject` is therefore harmless here, despite the type not being `#[non_exhaustive]`.
- ✅ **Every `Acquisition` use is a `matches!` with `{ .. }`** — so a new `OAuth2` variant does not
      break them. Note there are **two**, not one:
      `crates/exchange-host/src/settings.rs:437` and `:499`, both
      `matches!(credential.acquire, Acquisition::BasicJoin { .. })`.
- ❌ **Exchange *does* construct `ConfigField`**, at
      `crates/exchange-server/src/routes/connections/plan.rs:1835` inside a `#[cfg(test)]` module
      (`a_noncredential_secret_is_visible_but_never_routed_to_settings`). `ConfigField` is **not**
      `#[non_exhaustive]` (`connector-catalog` `lib.rs:477`) and the 0.21 line adds
      `also_services: &'static [&'static str]`. This is a missing-field compile error in the test
      suite the moment the pin moves.
- ❌ The C-531 `DispatchId` parameter on the `flux_lang::sink::FlowSink` impl, already recorded below.

## Acceptance

- [ ] The registry preflight verifies connector-pack 0.21 requires `flux-runtime ^0.58` and every
      `codewandler-flux-*` engine crate this workspace pins resolves at 0.58.0 before the first
      manifest edit.
- [ ] Failing first: changing only `ENGINE_LINE` to 0.58 makes
      `the_engine_line_is_recorded_in_exactly_one_place` reject every stale pin by name.
- [ ] Both pin sets — all `codewandler-flux-*` engine pins and all four `codewandler-connector-*`
      pins — move together in one commit; the manifest, compile-time seam and lockfile engine-line
      tests prove there is no second line, and the independent `flux-spec` floor is raised to the
      published 1.4 line only if the resolved graph requires it.
- [ ] The `flux_lang::sink::FlowSink` implementation adopts the C-531 `DispatchId` parameters and
      pairs tool call/timing/result events by dispatch id rather than by op name.
- [ ] `EDITOR_SCHEMA_VERSION` and the console-served workflow schema are verified unchanged, or
      the schema bump is treated as a console contract change with its own changelog entry.
- [ ] The catalogue-derived safety censuses are re-audited against the 0.21 connector artifacts,
      and `docs/designs/released-domain-audit.md` is re-issued for the 0.58/0.21 line per its own
      update rule.
- [ ] The `no_second_request_path` dependency allow-list is unchanged, or every added name carries
      its written reason; `"flux_system"` remains a banned host source string.
- [ ] The `ConfigField` literal in
      `crates/exchange-server/src/routes/connections/plan.rs` compiles against the 0.21 shape, or the
      test is rewritten to stop depending on an exhaustive literal.
- [ ] The Rust workspace gate, console tests/build and public-site build/tests pass; the changelog
      records the dependency move as a Changed entry for exchange-host consumers.

## Progress

- 2026-08-12: Confirmed the connector 0.21 line is unpublished — crates.io tops out at 0.20.0 for all
  four `codewandler-connector-*` crates. Status moved `backlog` → `blocked`, since the dependency
  cannot be satisfied at all rather than merely being scheduled later. Preflight run early and
  recorded above: the bump is **not** non-breaking; two compile breaks are already identified.
  [[X-147]] and [[X-148]] queue behind this.
