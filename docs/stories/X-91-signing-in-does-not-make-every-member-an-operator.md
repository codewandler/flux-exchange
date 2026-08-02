---
id: X-91
title: "Signing in does not make every member an operator"
status: ready
priority: 0
epic: remote-deployment
areas: [exchange-host, exchange-server, identity]
note: "Preserve organization-wide authentication, but key administrative authority by immutable OIDC sub and fail closed when no operator is configured."
---

# Signing in does not make every member an operator

## Goal
Separate “may sign in as an organization member” from “may administer this tenant.” Ordinary members
keep useful authenticated access; only explicitly configured operators may change authority or
credential-bearing state.

## Acceptance
- [ ] Add a deployment-owned operator policy keyed by immutable OIDC `sub`, never email, display name
      or request input. An empty or unavailable policy admits no operator and names the configuration
      an operator must fix.
- [ ] Preserve organization-wide authentication. Ordinary members may read their session and the
      catalogue and may use grant-gated invocation; signing in alone grants no administrative role.
- [ ] Require operator authority to list/create/delete connections, supply or rotate credentials,
      edit settings, read/preview/replace grants and mint/list/revoke agents.
- [ ] Keep principal kind and operator role as separate axes: a `User` is not implicitly an operator,
      and no `Agent` or `Service` can satisfy the operator policy.
- [ ] Declare the policy at the route-table boundary so the administrative surface remains
      enumerable. Failing-first tests enumerate every operator-only route and prove an unlisted
      organization member receives `403` while an operator and grant-gated invocation still work.
- [ ] Audit both successful administrative actions and operator-policy refusals without logging the
      policy contents or session material.
- [ ] Update console refusal handling and operator documentation; produce a versioned Fly release and
      live-verify both an operator and an ordinary member.
