---
id: X-12
title: "Invoke an operation"
status: done
epic: invoke
areas: [exchange-host, exchange-server]
design: docs/designs/invoke.md
note: "the caller names an operation id and nothing else about the request is theirs — not the host, not the credential, not the tenant. That is the whole confused-deputy answer"
---

# Invoke an operation

## Goal
A caller names an operation and gets a result. This host resolves the credential, and
`connector_pack` builds the request from the operation's own compiled Flux.

## Acceptance
- [x] A route runs one catalogue operation for the caller's tenant and returns the result.
- [x] **This host constructs no request of its own.** Every path ends in `connector_pack`. Assert it
      structurally — a test that fails if a second request-building path appears.
- [x] **Failing-first test** — no request field lets a caller influence the destination host.
- [x] **Failing-first test** — a missing credential refuses by **address**, never by value, and is
      terminal rather than retryable: the request was never sent.
- [x] The credential is registered with the redactor **before** the request is built, and the
      registration is verified to have taken.
- [x] A connector whose declared runtime this deployment does not admit is refused via
      `Deployment::admits`, not executed.

## Progress
- **In progress 2026-08-01** — dispatched to an implementor after X-11 unblocked the engine
  line. The story promotes `connector-pack` from a dev-dependency to a dependency, which is the
  decision X-11 deliberately left to it.

## Notes
- `crates/exchange-host/src/runtime.rs` already carries the refusal and its tested message.
- flux's redactor silently ignores values under six trimmed characters; a credential too short to
  redact must be **refused rather than sent**.
- **Design: [`docs/designs/invoke.md`](../designs/invoke.md)** — route shape, the enumeration of what
  a caller cannot name, the three-lock mechanism for the "no second request path" criterion, the
  redaction obligations, where `Deployment::admits` sits, and the terminal/retryable taxonomy. Read
  it before implementing. The cross-repo reasoning it builds on is flux's
  [`docs/designs/ecosystem.md`](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md).
- Two findings from the design that change the shape of the work: `catalog::Provider` has **no
  runtime field**, so `ConnectorSurface::runtime` must be derived and the `Deployment::admits`
  refusal has no shipped connector that exercises it; and `connector-catalog` currently sits in
  `exchange-server` while the invoke path needs it in `exchange-host`.
- **The invoke function lives in `exchange-host`, not `exchange-server`.** The axum handler stays in
  the server as a thin adapter. This moves `connector-catalog` into the host crate's dependencies,
  which overlaps X-02's work — structure the router accordingly rather than retrofitting it.
- **`http.request` returns the record `{status, headers, body}`.** Use the current line; do not port
  `connectors-api/src/exec.rs`'s flat-string handling, which predates it.
- Catalogue gap filed upstream as flux-connectors **C-405**: `catalog::Provider` publishes no runtime,
  so `ConnectorSurface::runtime` must be derived (`Http`) until it does — and no shipped connector
  exercises the `Deployment::admits` refusal, so that test builds a fixture.

## Unblocked, 2026-08-01

X-11 landed: `connector-pack` 0.9.0 links against the flux 0.46 engine line, proved by
`crates/exchange-host/tests/engine_line.rs`, which packs a real connector into a
`flux_runtime::ToolRegistry` through `flux_web`'s `HttpRequestTool`. **The thing that made this
impossible is gone.**

Two facts X-11 established that this story inherits:

- **`connector-pack` is a `dev-dependency` today.** X-11 argued that a published crate should not put
  the whole flux engine into every consumer's graph to satisfy a proof. **This story is what promotes
  it** — three lines from `[dev-dependencies]` to `[dependencies]` — and that is a real decision about
  the published artifact's weight, not a formality. Say so in the report.
- **The engine line is pinned at `0.46` in one place**, marked `ENGINE_LINE` in
  `[workspace.dependencies]` and enforced by a test that refuses a second value. `flux-runtime` 0.47
  exists and **must not be taken**: `connector-pack` 0.9.0 requires `^0.46`, and reaching for the
  newest recreates the two-incompatible-types failure X-11 just removed.

Also inherited, and it is the thing to be careful with: `connector-address` 0.9 carries C-406's
**instance dimension** — `CredentialRef` gained an optional `@instances/<uuid>` level, and
`CredentialRef::new` still elides it. X-14 is the story that uses it. Invoke must not start
resolving credentials at instanced addresses by accident.
- **Done 2026-08-01.** Gate green: 291 Rust tests (48 + 3 + 10 + 5 + 225), clippy clean, fmt clean.
  **This host executes.** Genuine merge-base failure — the test could not resolve the symbols it
  names.
- **The no-second-request-path invariant is enforced structurally, in three locks covering different
  ground**, not promised: the manifest's `[dependencies]` as an allow-list with a reason per entry;
  one dispatch seam with no reachable socket, guarded by a scanner that **self-tests** against
  sources it must reject and accept; and a transport counter, so a test cannot pass by never
  dispatching. The scanner was proved **on the real tree** — a planted file naming `reqwest` made it
  fail.
- **`connector_pack::resolve`, not `pack`**, and the reason is upstream's: C-413 split the seams
  after the design was written, and `pack` is **model-facing — it withholds every `expose = false`
  operation**, so an execute route built on it would silently refuse operations callers are entitled
  to run. Lock 2 counts `resolve` and separately **forbids** `pack`.
- **`flux-system` is deliberately not in `exchange-host`.** Building a `ToolContext` needs it and it
  dials, so writing "not a transport" beside it in the allow-list would have been false. `Contexts`
  is a port the composition implements, and lock 2 forbids naming `flux_system` in host sources.
- **One place fidelity was lost deliberately and documented rather than smoothed over:** every
  `connector_pack::Error` arrives as `flux_core::Error::Config(String)`, so this host **cannot tell a
  missing credential from an unreachable store**. It reports the conservative answer for both — not
  sent, not retryable — rather than string-matching upstream's prose. The bias never causes a
  duplicated write. **Re-measure this on any `connector-pack` or `flux-web` bump.**
- **A design premise turned out false in our favour:** C-405 landed, so `catalog::Provider` now
  publishes its runtime and `ConnectorSurface` *reads* it rather than deriving `Http`, with an
  exhaustive no-wildcard mapping making a new upstream runtime a compile error here.
- **The published crate now carries the flux engine.** `connector-pack` and `flux-runtime` were
  promoted to `[dependencies]`; `flux-web` was **not**, because it holds the transport and lock 1
  says the crate that dispatches holds none. Every consumer of
  `codewandler-flux-exchange-host` now pays for the engine — that is the weight this story sanctions.
- **Filed as [X-47](X-47-per-connection-settings.md):** thirteen of fifty-three connectors declare a
  templated `base_url` with nowhere to put the value, so the shipped surface runs 40 of 53. It fails
  closed and names the field, which is right — and it is still a story.
- **Carried forward:** `invoke` is in the console navigation and **inert**; the backend it was
  waiting for now exists.
