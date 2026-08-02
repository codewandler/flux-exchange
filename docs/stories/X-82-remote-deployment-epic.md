---
id: X-82
title: "A deployment a stranger can reach (epic)"
status: ready
priority: 1
epic: remote-deployment
design: docs/designs/remote-deployment.md
areas: [exchange-server, console, ci]
note: "EPIC — owner-raised 2026-08-02: everything this platform does can only be seen on 127.0.0.1. Three blockers, and only one is packaging: OIDC is the sole path to a reachable bind, the console has no production host and cannot be given one on another origin, and nothing containerises this"
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

## The three blockers, and which are ours

1. **A reachable bind needs a bound identity, and OIDC is the only one wired.** `main.rs:394` is the
   sole route to `BoundIdentity::Real`. Everything else refuses at startup, which on fly is a
   crash-loop. **Resolved by configuration, not code** — the path is already tested.
2. **The console has no production host** and cannot be given one on another origin, because
   `SameSite=Strict` means the browser never attaches the session cookie cross-origin. That is
   [[X-83]], and it is a new capability.
3. **Nothing containerises this**, and no flux-family repository has ever deployed. That is [[X-84]],
   and it sets the precedent the siblings copy.

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

## Notes
- [[X-58]] stays worth landing **after** this, so a demonstration stops depending on a third party
  being up. It is not on the critical path now that OIDC is the decided route.
- The design records why hosting the console on GitHub Pages beside the docs site cannot work. It is
  the obvious idea and it fails for a reason that looks like CORS and is not.
- **It will boot and do nothing**, correctly: X-13's grant gate is fail-closed. If X-84 does not ship
  the first-five-minutes path, the first experience of the remote service is a working platform that
  appears broken.
