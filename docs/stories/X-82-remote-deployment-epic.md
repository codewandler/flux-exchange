---
id: X-82
title: "A deployment a stranger can reach (epic)"
status: blocked
priority: 1
epic: remote-deployment
design: docs/designs/remote-deployment.md
areas: [exchange-server, console, ci]
note: "EPIC — production and Google OIDC sign-in are live; completion waits on connect → grant → invoke and redeploy persistence proof"
---

# A deployment a stranger can reach (epic)

## Goal
`https://<app>.fly.dev` serves this platform to somebody who has not cloned it.

## Why this epic exists

The getting-started page ([[X-69]]) walks a reader through `cargo run`, a roster handle and a console
on `localhost`, and that is the entire demonstrable surface. The public site ([[X-63]]) can now
*describe* the platform to an evaluator; nothing lets them use it.

Owner-raised 2026-08-02, and owner-decided the same day: **stand up a real OIDC provider** rather than
wait on local identity. That decision is what makes this epic two stories instead of three.

## The three delivered foundations

1. **A reachable bind has a real identity.** Production uses OIDC; X-58 also delivered
   verifier-backed local users for reachable self-hosted deployments. A reachable bind still refuses
   when neither verifier is configured.
2. **The console is same-origin** ([[X-83]]), because `SameSite=Strict` intentionally prevents a
   separately hosted console from receiving the session cookie.
3. **The service is containerised and deployed** ([[X-84]]) with one machine, one attached volume and
   fail-closed startup checks. The remaining work is live journey and redeploy evidence, not packaging.

## Children
- **X-83** — the console is served by the host it talks to. **Ordered first**: X-84 has nothing to
  put in an image until the binary can answer `/`.
- **X-84** — a container, a `fly.toml`, a volume, and the operator's first five minutes.

## Acceptance
- [ ] The union of X-83's and X-84's acceptance.
- [ ] A browser at the public URL completes: sign in → connect a connector → write a grant → invoke an
      operation → read the result.
- [ ] **No fail-closed gate is weakened to get there.** The bind rule, the grant gate, the kind gate,
      the anonymous-surface guard and the runtime gate are untouched. A diff that relaxes one is a
      blocker, not a step.
- [ ] The credential never leaves the service — asserted, not assumed.

## Progress
- 2026-08-02 — filed with [`docs/designs/remote-deployment.md`](../designs/remote-deployment.md) after
  measuring the three blockers against the tree.
- 2026-08-03 — the same-origin console and immutable Fly release are live without weakening the bind,
  grant, kind, anonymous-surface or runtime gates. After the v0.16.2 scope correction, the owner
  completed Google sign-in and reached an authenticated session. X-82 remains blocked with X-84 only
  on connect → grant → invoke and post-redeploy persistence/session-invalidation proof.

## Notes
- [[X-58]] is delivered as the verifier-backed self-hosted alternative; production deliberately uses
  OIDC for organization membership.
- The design records why hosting the console on GitHub Pages beside the docs site cannot work. It is
  the obvious idea and it fails for a reason that looks like CORS and is not.
- **It will boot and do nothing**, correctly: X-13's grant gate is fail-closed. If X-84 does not ship
  the first-five-minutes path, the first experience of the remote service is a working platform that
  appears broken.
