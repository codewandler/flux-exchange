---
id: X-12
title: "Invoke an operation"
status: blocked
epic: invoke
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
- Design first: this story is non-trivial and has no design doc yet. Write one under
  `docs/designs/` (`/track:design`) before implementing. The cross-repo reasoning it builds on is
  flux's [`docs/designs/ecosystem.md`](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md).
