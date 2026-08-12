---
id: X-146
title: "Adopt the released connector 0.21 line, which does not move the engine"
pillar: "Core"
status: done
areas: [build, exchange-host, exchange-server, docs]
design: docs/designs/released-domain-audit.md
note: "connector-only bump: connector-pack 0.21 requires flux-runtime ^0.54, the same as 0.20, so no codewandler-flux-* pin moved and ENGINE_LINE stayed 0.54. Adopting flux 0.58+ needs its own story, gated on a connector-pack release that asks for it"
---

# Adopt the released connector 0.21 line, which does not move the engine

## Goal

Move Exchange onto the published `codewandler-connector-*` 0.21 line and re-issue the
released-domain audit for it, **without touching the flux engine line**, because 0.21 did not move
it. `connector-pack` 0.21.0 requires `codewandler-flux-runtime ^0.54` — byte for byte what 0.20.0
required. Raising the flux pins alongside it would put two `flux_runtime::Tool` traits in one lock:
the exact X-11 failure [`AGENTS.md`](../../AGENTS.md) § The dependency situation exists to prevent.

## The correction this story carries

**This story was filed as "Adopt the released flux 0.58 and connector 0.21 lines", and that premise
was wrong.** Its first acceptance criterion asserted that connector-pack 0.21 requires
`flux-runtime ^0.58`. It does not. Read from the crates.io sparse index before any manifest was
edited:

| | requires `codewandler-flux-runtime` | `flux-core` / `flux-lang` | `flux-spec` |
|---|---|---|---|
| `connector-pack` 0.20.0 | `^0.54` | `^0.54` | `^1.3` |
| `connector-pack` 0.21.0 | `^0.54` — **unchanged** | `^0.54` | `^1.3` |

`codewandler-flux-runtime`'s newest published version is 0.59.3, and that is irrelevant. **The
engine line is set by what `connector-pack` requires, never by what is newest** — the same rule that
kept the line at 0.46 through X-11 while 0.47 was published, and at 0.52 until X-101. The connector
line and the engine line are one compatibility unit only in the sense that they must not *diverge*;
a connector release that leaves the engine alone is a connector-only bump, and moving flux to match
a story title is how the divergence gets created rather than avoided.

Two consequences:

- **No `codewandler-flux-*` pin changed and `ENGINE_LINE` is still `0.54`.** The engine crates
  resolve at 0.54.4 exactly as before; the lockfile diff is the four connector packages and one new
  dependency edge, nothing else.
- **The C-531 `DispatchId` / `flux_lang::sink::FlowSink` criterion is out of scope and is left
  unticked below.** It is a flux 0.58 concern, and this workspace is not on flux 0.58. It belongs to
  the future engine-adoption story, not here.

**Adopting flux 0.58+ needs its own story, and it is gated on a `connector-pack` release that
requires it.** Filing it now with no such release to point at would recreate exactly the pressure
this one had to refuse.

## Preflight — run before the first manifest edit

Read directly from `https://index.crates.io/`, not from `Cargo.lock` and not from a sibling
checkout:

```
newest non-yanked:
  codewandler-connector-address    0.21.0
  codewandler-connector-catalog    0.21.0
  codewandler-connector-pack       0.21.0
  codewandler-connector-secrets    0.21.0
  codewandler-flux-runtime         0.59.3   <- not adopted; see above
  codewandler-flux-spec            1.4.0    <- not adopted; 0.21 still asks for ^1.3

connector-pack 0.21.0 normal dependencies:
  catalog ^0.21.0   connector-address ^0.21.0   connector-secrets ^0.21.0
  flux-core ^0.54   flux-lang ^0.54             flux-runtime ^0.54
  flux-spec ^1.3    async-trait ^0.1            serde_json ^1        thiserror ^2
```

All four connector crates are published and unyanked at 0.21.0. `connector-catalog` 0.21.0 still has
**zero** dependencies. `connector-pack` 0.21.0 now names `connector-address` directly, which 0.20.0
reached only through `connector-secrets` — that is the single added edge in the lock.

## Acceptance

- [x] The registry preflight verifies, before the first manifest edit, that `connector-pack` 0.21.0
      requires `flux-runtime ^0.54` — **not** `^0.58` — and that all four `codewandler-connector-*`
      crates are published at 0.21.0. Recorded above.
- [x] All four `codewandler-connector-*` pins move 0.20 → 0.21 in one commit with `Cargo.lock`, and
      **no `codewandler-flux-*` pin and no `ENGINE_LINE` value changes**. The manifest, compile-time
      seam and lockfile engine-line tests all pass and prove there is still one engine line.
- [x] The independent `flux-spec` floor is raised only if the resolved graph requires it. It does
      not: 0.21 asks for `^1.3` as 0.20 did, the manifest floor stays `1.2.1`, and the lock still
      resolves 1.3.0. `flux-spec` 1.4.0 is published and deliberately not adopted.
- [x] `the_lock_carries_the_exact_registry_connector_021_line` pins the four 0.21.0 crates.io archive
      checksums, read from the sparse index rather than copied out of the lock under test.
- [ ] ~~The `flux_lang::sink::FlowSink` implementation adopts the C-531 `DispatchId` parameters.~~
      **Out of scope — deliberately unticked.** C-531 is a flux 0.58 change and this workspace stays
      on flux 0.54. It moves to the engine-adoption story.
- [x] `EDITOR_SCHEMA_VERSION` and the console-served workflow schema are verified unchanged.
      `exchange_host::EDITOR_SCHEMA_VERSION` is `flux_lang::editor::EDITOR_SCHEMA_VERSION`, `flux-lang`
      did not move, and the value is still `1`. No console contract change, so no changelog entry for
      one.
- [x] The catalogue-derived safety censuses are re-audited against the 0.21 connector artifacts and
      `docs/designs/released-domain-audit.md` is re-issued for the 0.21 connector line at engine
      0.54, per its own update rule.
- [x] The `no_second_request_path` dependency allow-list is unchanged — `crates/exchange-host/Cargo.toml`
      is untouched by this story — and `"flux_system"` remains a banned host source string.
- [x] The `ConfigField` fixture in `crates/exchange-server/src/routes/connections/plan.rs` no longer
      depends on an exhaustive struct literal: it starts from the provider's own shipped declaration
      and overrides the one flag under test, so the next catalogue release does not break it.
- [x] The Rust workspace gate, console tests/build and public-site build/tests pass; the changelog
      records the dependency move as a Changed entry for exchange-host consumers.

## What actually broke, and what did not

**One compile break, exactly as predicted.** `catalog::ConfigField` gained
`also_services: &'static [&'static str]` and is not `#[non_exhaustive]`, so the exhaustive literal in
`a_noncredential_secret_is_visible_but_never_routed_to_settings` stopped compiling. It is now derived
from jira's own shipped `endpoint.site` declaration with `secret: true` overridden — the fixture was
never making a claim about the struct's shape, only about one flag, and it should not have been
written as though it were. `ConfigField` staying exhaustive is right for a *consumer*, which does
want to be told a member appeared.

**Verified inert rather than assumed inert:**

- `catalog::Credential` gained `subject: Subject` and `hazard: Option<AuthHazard>` and is likewise
  not `#[non_exhaustive]` — but Exchange constructs its own `exchange_host::DeclaredCredential` and
  never that type. The proof is that the workspace compiles: a construction would have failed the
  same way `ConfigField` did.
- Both `Acquisition` uses are `matches!(credential.acquire, Acquisition::BasicJoin { .. })`
  (`crates/exchange-host/src/settings.rs:437`, `:499`) and the new `OAuth2` variant does not reach
  them. The set of `BasicJoin` credentials is identical across 0.20 and 0.21 — confluence, twilio,
  jira, asterisk and zendesk's two — so the settings census answers the same thing it did.
- `Operation` gained `direction: OperationDirection`, but that type is `#[non_exhaustive]` and
  Exchange only reads it.

## Progress

- 2026-08-12: **Preflight corrected the story's premise.** connector-pack 0.21.0 requires
  `flux-runtime ^0.54`, identical to 0.20.0 — the flux 0.58 half of this story was never true. The
  title, Goal, `note:` and Acceptance are rewritten for what this actually is: a connector-only
  bump. `ENGINE_LINE` stays 0.54 and no `codewandler-flux-*` pin moved. **Adopting flux 0.58+ is now
  future work with no story filed, because there is nothing to gate it on: it needs a
  `connector-pack` release that requires it, and 0.21 is not that release.** File it when one ships.
- 2026-08-12: Bumped the four connector pins, updated `Cargo.lock`, fixed the one `ConfigField`
  compile break and re-issued the released-domain audit. All six `engine_line.rs` tests pass,
  including the three that matter here — manifest, compile-time seam and lockfile. Full Rust gate,
  console `npm test`/`build` and public-site `build`/`test` green; **neither Node tree needed a
  change**, because the connector catalogue reaches the console over HTTP at runtime rather than as a
  committed artifact.
- 2026-08-12: **[[X-147]] and [[X-148]] were blocked on this line reaching crates.io, and it has.**
  0.21.0 publishes `Acquisition::OAuth2`, `Subject`, `OAuthGrant`, `OAuthRedirect` and a gitlab
  `gitlab.oauth_token` credential with three `oauth.*` config bindings. Their status is theirs to
  move; this story does not move it for them.
- 2026-08-12: **A released connector now declares an authentication hazard for the first time.**
  babelforce's `babelforce.access_token` moved `Static` → `OAuth2` and carries
  `hazard: Some(AuthHazard::ResourceOwnerSecretShared)`, which is upstream C-440 landing. It changes
  nothing at runtime yet — Exchange reads a hazard from the injected `AcquisitionBinding`, not from
  the catalogue, and production still composes an empty `AcquisitionBindings` — but the X-74/X-75
  prose that said "no released connector declares the acquisition" is now false and has been
  corrected in `AGENTS.md` and in `crates/exchange-host/tests/auth_posture.rs`.

## Notes

- The `oauth.client_id`, `oauth.redirect_uri` and `oauth.client_secret` bindings gitlab 0.21 adds are
  a **new binding namespace Exchange does not recognise**. `DeclaredSetting::parse` returns `None`
  for them, so the two non-secret ones render visible-but-unroutable with the reason
  "binding `oauth.client_id` is not accepted by the existing settings surface", and the secret one
  takes the older refusal path that keeps a deployment-owned secret out of the tenant settings store.
  Both are the fail-closed answer and neither is a regression. [[X-147]] owns building the surface.
- `also_services: &["login"]` on gitlab's `origin` field is inert here for a structural reason worth
  recording: every gitlab *operation* is on service `default`, and `login` exists only as the OAuth2
  endpoint reference. Exchange composes no URL for it because Exchange runs no catalogue-declared
  grant. The moment [[X-147]] does, this field becomes load-bearing — a host that ignores it reports
  an unbound placeholder for a value the operator already supplied.
