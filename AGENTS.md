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

**v0.0.1. The service serves health, the catalogue, a session and a partial sign-in.** `cargo run` binds loopback and
refuses to start on a reachable address with no identity provider configured. What exists beyond
that is still the vocabulary and the rules as tested types. The [README](README.md) carries the
itemized inventory of what is *not* built, and keeping it accurate is part of the job — a page that
implies a working service costs more than an honest gap.

## Build / test / run

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

cd console && npm install && npm test && npm run build
```

Rust 1.87 or newer. The console is a separate Node build and does not participate in the Cargo
workspace.

## The dependency situation, which will bite you

- **`codewandler-connector-catalog`** has zero dependencies. **`connector-spec`** and
  **`connector-secrets`** carry no flux dependency. All three are usable today.
- **`codewandler-connector-pack` 0.8.0 requires `codewandler-flux-runtime ^0.41`**, i.e. `<0.42`,
  while the flux family is at 0.45.0. `connector_pack::pack` hands out `Arc<dyn flux_runtime::Tool>`,
  and two engine versions are two incompatible traits. **You cannot link both.** This is X-11, the
  work is upstream, and it blocks only the `invoke` epic.

Do not "solve" this with a `path` or `git` dependency on a sibling checkout. That couples a shipped
image to an unreviewed working tree, and the family has already decided against it.

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
