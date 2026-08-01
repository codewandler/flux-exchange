# AGENTS.md

Guidance for coding agents (and humans) working in this repository.

<!-- BEGIN track:agents -->
## Start here (every session) — track backlog

This project tracks work with the **track** framework: every unit of work is a markdown story in
`docs/stories/`, and the board (`docs/stories/README.md`) is generated from story frontmatter.

1. **Orient** — read the latest user request, then run `git status --short --branch`. Treat
   uncommitted changes as user-owned unless you made them.
2. **What to work on** — if the user named work, do that. Otherwise open the
   [board](docs/stories/README.md) and take the top `ready` story by `priority` (lower = higher).
3. **The contract** — read the story's `## Goal` and `## Acceptance`; Acceptance defines "done". Read
   any linked `design:`.
4. **Do the work** — set the story `in-progress`; non-trivial design goes in `docs/designs/` first;
   implement; satisfy Acceptance with a **failing-first test**; keep the gate green.
5. **On done** — set `status: done`, add a CHANGELOG entry, regenerate the board.
6. **New or unscoped work?** Create a story first, so the next agent inherits the context.

The board's status lists are generated — after any change to a story's `status`/`priority`/`title`/
`epic`, regenerate it. Story frontmatter is the single source of truth.
<!-- END track:agents -->

## What this is

The platform layer of the [flux](https://github.com/codewandler/flux) family: a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.

**Read [`docs/vision.md`](docs/vision.md) before your first change.** It is the tie-breaker, and its
north star is the sentence every design decision here answers to:

> **The credential never crosses the boundary; the authority does.**

## Status — read this before believing anything else

**v0.9.0. The service serves health, the catalogue, a session, a complete OIDC sign-in,
`POST /api/operations/{operation}/invoke` (X-12) which runs one catalogue operation for the caller's
tenant, per-connection settings gated to signed-in humans (X-47), and — since X-42 —
`GET /api/onboarding`, an anonymous machine-readable descriptor of what this build can and cannot
do.** `cargo run` binds loopback and refuses to start on a reachable address with no identity
provider configured. What exists beyond that is still the vocabulary and the rules as tested types.

**Signing in without an identity provider works on loopback** (X-57, v0.9.0+). Arm
`FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme` and `GET /api/signin` tells you how — present the roster
handle as a bearer token, `POST /api/session` exchanges it for a cookie. `sign_in_available` now means
*this deployment can turn a caller into a principal*, not *OIDC is configured*.

⚠ *This paragraph used to say the console hid its sign-in affordance. That was never true — it renders
the anchor unconditionally and nothing reads `sign_in_available`. The link led nowhere useful; that is
what X-57 fixed.* A **reachable** deployment still needs a real provider: the roster has no secret, so
`admit_bind` refuses every non-loopback address while it is armed. Local users with an actual verifier
are X-58.

⚠ **Invocation is gated by identity *and by grant*** (X-13, v0.9.0), and since X-62
`GET/PUT /api/grants` edits them (`User` only) with `POST /api/grants/preview` showing what a selector
would admit before it is saved. **A console screen for it is still unbuilt.** An operation runs only if a
grant the caller's tenant holds admits it, decided from what the operation declares — its risk, its
effects, its idempotency — never from a list of names. **This is fail-closed and it will look like an
outage**: a deployment runs nothing until `FLUX_EXCHANGE_GRANTS` names a file and grants are written
into it. Expect `503` with no store bound,
`403 not_granted` with one bound and the tenant empty. The [README](README.md) carries the
itemized inventory of what is *not* built, and keeping it accurate is part of the job — a page that
implies a working service costs more than an honest gap.

## Build / test / run

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

cd console && npm install && npm test && npm run build

cd web && npm ci && npm run build && npm test
```

**`web/` is the public documentation site (X-63), and its build is a gate rather than a formality.**
`.vitepress/config.mts` sets `ignoreDeadLinks: false`, so a dead internal link fails `npm run build`
instead of publishing a broken page — that failure is the whole reason a broken site cannot reach the
public URL. [`.github/workflows/pages.yml`](.github/workflows/pages.yml) runs it on every pull
request and deploys only from `main`; run it locally before you touch a page or a link. `npm test`
comes **after** the build because it asserts over `web/.vitepress/dist`, and it guards the deployed
base path plus the two content rules the site publishes under — no deployment-specific fact, nothing
credential-shaped. It is a third Node tree: `web/`, `console/` and the Cargo workspace share nothing,
including lockfiles.

Rust 1.88 or newer — that is the floor `jsonwebtoken` and `time` impose, not a number we chose. It
lives in `Cargo.toml` as `rust-version`, and since X-33 CI's `msrv` job builds the workspace on
whatever that says, reading the number out of the manifest rather than restating it. **Do not raise
`rust-version` to make that job green.** It said 1.87 through `v0.1.0`, `v0.2.0` and `v0.3.0` while
the tree had not built on 1.87 since X-04; correcting it was a repair, and raising it again is a
compatibility break for consumers that belongs in the CHANGELOG as a decision. The console is a
separate Node build and does not participate in the Cargo workspace.

## The dependency situation, which will bite you

**X-11 closed the engine-line conflict; X-67 moved the line.** The connector crates are on 0.10 and
`connector-pack` links here. What is left is one rule, and it is the one that bites:

- **The flux engine line is `0.47`, and it is written down once** — in `[workspace.dependencies]`
  in the root `Cargo.toml`, under the `ENGINE_LINE` marker. Every `codewandler-flux-*` pin carries
  that value, and no member manifest pins one at all.
- **It is set by what `connector-pack` requires, never by what is newest.** `connector-pack` 0.10.0
  requires `codewandler-flux-runtime ^0.47`, and that is the whole reason 0.47 is allowed now — it
  had been published for some time while this repository stayed on 0.46, because 0.9.0 required
  `^0.46`. `connector_pack::pack` hands out `Arc<dyn flux_runtime::Tool>`, and two engine versions
  are two incompatible traits with identical names — Cargo resolves both happily and the failure
  lands at type-check somewhere else entirely.
- **Both pin sets move in one commit.** `codewandler-flux-*` and `codewandler-connector-*` are one
  bump, never two. Raising either alone is exactly what puts two engine lines in one lock.
- Three tests in `crates/exchange-host/tests/engine_line.rs` keep this true rather than review:
  one links `connector_pack::pack` against `flux_web::http::HttpRequestTool` so a divergence that
  touches the seam is a compile error, one reads the manifests so a divergence that does not touch
  it is still caught, and one reads `Cargo.lock` — because a manifest stating one line proves
  nothing about what *resolved*, and a transitive requirement a line behind is invisible in the
  manifest. That third one is ported from flux-connectors' `flux_engine_line.rs`.
- **`connector-address` is the address vocabulary; `connector-spec` is the compiler.** Upstream
  C-407 extracted the first out of the second, so this repository names `connector-address` and no
  longer pulls a compiler it never called. `connector-secrets` re-exports the same vocabulary, so
  `CredentialRef` from either is one type. `DEFAULT_SERVICE` is the exception — it lives at
  `connector-address`'s root, not in its `credential` module, so `connector-secrets` does not
  re-export it.
- **`connector-pack` and `flux-runtime` are ordinary dependencies since X-12**, and that was a
  deliberate decision rather than a formality: the published `codewandler-flux-exchange-host` now
  puts the flux engine into every consumer's graph, because it now *runs* operations rather than
  merely proving it could link the thing that does. **`flux-web` did not come with them** — it holds
  `HttpRequestTool`, and the crate that dispatches holds no transport.
- **`crates/exchange-host/Cargo.toml`'s `[dependencies]` table is an allow-list**, read by
  `tests/no_second_request_path.rs`. Adding a dependency there means adding it to `ALLOWED` in that
  test **with a sentence saying why it is not a transport**. That test being annoying is the design;
  deleting it is a blocker.
- **`codewandler-connector-catalog`** still has zero dependencies.

Do not "solve" an engine-line conflict with a `path` or `git` dependency on a sibling checkout. That
couples a shipped image to an unreviewed working tree, and the family has already decided against it.

## Invariants — do not regress these

Each is stated in `docs/vision.md` and several are already enforced by tests in
`crates/exchange-host`. A change that weakens one is a blocker, not a nit.

- **The tenant comes from the resolved principal and from nothing a caller controls.** Not a path
  segment, not a body field, not a header. `Identity` says so; the routes must make it true.
- **This host constructs no request of its own.** Every execution path ends in `connector_pack`,
  evaluating the operation's own compiled Flux. A second request-building path is how this becomes
  the credential-injecting proxy the family rejected.
- **The runtime is declared by the connector, never chosen by the caller.** There is deliberately no
  constructor on `Runtime` that takes caller input; keep it that way.
- **A multi-tenant deployment refuses every locally-executing runtime.** `Deployment::admits` decides
  this from the manifest. Do not add an override.
- **Grants select by declared metadata, not by name**, and an explicit `deny` beats an explicit
  `allow`.
- **An agent's token grants access to an operation, never to a credential.**
- **Refuse; never repair.** A missing credential, a widened file mode, an unbound config value: each
  refuses and names the address, never the value. A store that falls back to memory, or a mode that
  is quietly tightened, hides the thing you needed to know.

## Conventions

- Errors: `thiserror` in the library, and every variant refuses. Distinguish failures an operator
  responds to differently — "rejected" and "unreachable" are not the same event.
- No `unwrap()` in non-test code on fallible IO.
- Async is `tokio`.
- Doc comments on public items, and a comment that explains *why* rather than restating the code.
- Match the surrounding style — comment density, naming, module layout.

## The console

`console/src/components/` holds 15 Vue components **shared with flux-connectors**. They import only
Vue, a sibling component, and `catalog.mts` — an invariant enforced by
`console/test/components.test.mjs`, which is itself guarded by a test that runs the scanner against
sources it must reject and accept.

**Do not modify a carried component to make local work easier.** If one genuinely needs a change,
that is a finding to report, because the change belongs upstream where the component is shared.

## Publishing

`codewandler-flux-exchange-host` is the reusable artifact — routes, tenancy, the grant model and the
runtime registry behind traits, so a product composes them into its own binary with its own identity
provider. The server binary is `publish = false`.

That trait boundary is what keeps downstream concerns out of this repository, and it is structural
rather than disciplinary: the public crate has no downstream dependency to leak through. **No
flux-family repository names a downstream company.**

### Publishing contract

**Publishing to crates.io is CI-only. Never run `cargo publish` by hand** — not locally, not with
`--allow-dirty`, not "just to test". A published version cannot be withdrawn or corrected: a burned
version number is burned, and a wrong `description`, `readme` or `keywords` is fixable only in the
*next* version. `--dry-run` is the only form of `cargo publish` anyone runs outside CI.

- A release is a consequence of pushing a `vX.Y.Z` tag.
  [`.github/workflows/crates-io.yml`](.github/workflows/crates-io.yml) does the rest, via
  [`scripts/publish-crates-io.sh`](scripts/publish-crates-io.sh). It holds a `concurrency` group so
  two runs cannot race, and `workflow_dispatch` resumes a run that died partway.
- It needs one secret, **`CARGO_REGISTRY_TOKEN`**, checked before anything is packaged so a missing
  token fails the run rather than surfacing as an upload failure. It is an **org-level secret on
  `codewandler` with SELECTED visibility**, shared with `flux` and `flux-connectors` — this
  repository is on its allow-list, and a fork or a renamed repository will not be.
- **The gate runs inside that workflow, and still does now that `ci.yml` exists.**
  [`ci.yml`](.github/workflows/ci.yml) gates every push to `main` and every pull request, but that
  is not evidence about the *tagged* commit — a tag can be pushed at a commit no run ever covered.
  Publishing an artifact nobody tested is the worst thing to make permanent, so the release path
  proves the gate for itself. **Do not delete it from `crates-io.yml`** on the grounds that CI now
  covers it.
- The publish is **idempotent**: a version already on crates.io is skipped, so a failed run can be
  re-run or the tag re-pushed. That is what makes a partial release recoverable, given that what is
  already up cannot be withdrawn.
- The tag must match `[workspace.package].version`; the workflow refuses otherwise. Bump the version
  and the `exchange-host` pin in `[workspace.dependencies]` together — they are two places holding
  one number, and a publish is where they being out of step first hurts. Since X-30 that pairing is
  checked at PR time by [`scripts/check-crate-versions.sh`](scripts/check-crate-versions.sh), so the
  mismatch surfaces where it is free to fix. The tag check stays regardless: a tag can be pushed at a
  commit no pull request touched.

## Supply chain — checked, not trusted

**Every third-party action in `.github/workflows/` is pinned to a full 40-char commit SHA, with its
human-readable version as a trailing comment.** A movable tag hands whoever controls it the code
running in our workflows, and `crates-io.yml` carries `CARGO_REGISTRY_TOKEN` — publish rights to a
crate whose versions cannot be withdrawn.

Since X-30 this is enforced by [`scripts/check-action-pins.sh`](scripts/check-action-pins.sh) in
CI's `action-pins` job, not by review. Both checkers run `--self-test` before they scan, following
`../flux`: a checker that has not just proved it catches a violation is not evidence there are none.
Keep that ordering if you touch them.

Note for anyone writing a workflow comment: the scanner classifies lines before judging them, so a
comment or a `run: |` example that mentions the step keyword is not mistaken for a real reference.
Do not "fix" a comment to work around a grep — fix the grep.
