---
id: X-87
title: "The public service has an operational security boundary"
status: done
priority: 1
epic: remote-deployment
design: docs/designs/public-service-hardening.md
areas: [ci, exchange-server]
note: "Raised from the first review after public deployment. Organization-wide Google admission is intentional; this story hardens the service around that policy rather than narrowing it."
---

# The public service has an operational security boundary

## Goal
A reachable deployment closes sessions at the server, bounds anonymous sign-in and operation work,
records successful security-relevant actions without recording secrets, supplies browser security
headers, and continuously checks its dependency trees for known vulnerabilities.

The configured Google Workspace audience remains the admission policy. Every account in that
organization being able to sign in is intentional and is not a defect this story changes.

## Acceptance
- [x] `DELETE /api/session` invalidates the presented OIDC session as well as clearing its cookie;
      replaying that cookie is refused.
- [x] Authorization starts are bounded so an anonymous flood cannot churn the entire pending-flow
      store, and invocations have both a request-rate bound and a concurrent-execution bound. A
      refusal is `429` with `Retry-After`, and health/session traffic remains available when invokes
      are saturated.
- [x] Successful sign-in, sign-out, agent minting, connection credential/settings changes, grant
      replacement and invocation emit structured audit events naming the acting principal and the
      non-secret target. Tokens and credential/setting values never enter those events.
- [x] Every response carries the browser hardening headers that are valid for this same-origin app;
      API and sign-in responses additionally carry `Cache-Control: no-store`.
- [x] CI audits Rust, console and documentation-site dependencies. A known advisory may be ignored
      only by an inline, narrow rationale; the current RSA advisory is limited to key generation and
      this service uses that dependency only for verification.
- [x] Focused tests fail before the implementation and pass afterwards, and the repository gate is
      green.
- [x] v0.13.0 is deployed to the one Fly machine; `/health` reports that version and live responses
      carry the hardened headers.

## Verification

- The first OIDC session-closure test failed before `Oidc::close_session` existed, then passed with
  the implementation. Focused traffic, audit, header and route tests pass.
- `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` pass.
- The console has 106 passing tests, a successful production build and no high-severity npm audit
  findings. The public site builds with all links checked, has 28 passing tests and no high-severity
  npm audit findings.
- `cargo audit --deny warnings` passes with the two narrowly documented transitive exceptions in CI;
  the action-pin and crate-version checks also pass.
- Fly deployment `deployment-01KZ0TZSC7C6DAGXM4XPG15QX0` updated the sole machine
  `2862de4a43e058` to machine version 3 in `fra`, with its health check passing. On 2026-08-02,
  `https://flux-exchange.fly.dev/health` returned `{"status":"ok","version":"0.13.0"}` and the
  live console and API responses carried the hardened headers; `/api/onboarding` additionally
  returned `Cache-Control: no-store`.
