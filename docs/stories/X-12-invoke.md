---
id: X-12
title: "Invoke an operation"
status: blocked
epic: invoke
design: docs/designs/invoke.md
note: "the caller names an operation id and nothing else about the request is theirs — not the host, not the credential, not the tenant. That is the whole confused-deputy answer"
---

# Invoke an operation

## Goal
A caller names an operation and gets a result. This host resolves the credential, and
`connector_pack` builds the request from the operation's own compiled Flux.

## Acceptance
- [ ] A route runs one catalogue operation for the caller's tenant and returns the result.
- [ ] **This host constructs no request of its own.** Every path ends in `connector_pack`. Assert it
      structurally — a test that fails if a second request-building path appears.
- [ ] **Failing-first test** — no request field lets a caller influence the destination host.
- [ ] **Failing-first test** — a missing credential refuses by **address**, never by value, and is
      terminal rather than retryable: the request was never sent.
- [ ] The credential is registered with the redactor **before** the request is built, and the
      registration is verified to have taken.
- [ ] A connector whose declared runtime this deployment does not admit is refused via
      `Deployment::admits`, not executed.

## Progress
- (blocked on X-11)

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
