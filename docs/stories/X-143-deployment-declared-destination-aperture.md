---
id: X-143
title: "Deployment-declared destination aperture"
pillar: "Core"
status: backlog
epic: hosted-single-org
design: hosted-single-org
note: "Decision 0019 rule 3: the deployment declares admitted egress destinations; members never select or widen; everything else stays refused post-resolution"
---

# Deployment-declared destination aperture

## Goal

The question is authority, not reachability. The invoker and the generated channel runner both
compose `PrivateNetAllow::None`, so a private or loopback destination is refused for everyone —
correct for the open internet posture, and exactly wrong for a deployment whose own infra layer
legitimately reaches an in-cluster service. Decision 0019 rule 3 opens one narrow aperture: the
deployment declares an explicit, value-free allowlist of admitted egress destinations, resolved
post-resolution (DNS-rebinding-safe) identically by request construction and the permission
subject, exactly as Decision 0008 rule 4 states for local grants. A member, model input or Service
Account can never select, widen or substitute a destination.

## Acceptance

- [ ] A deployment-declared destination list admits a named private destination for connections
      whose declaration requires it; everything undeclared stays refused post-resolution, proven
      by fixtures covering a public hostname resolving to a private address.
- [ ] The aperture exists only in the hosted single-org posture; multi-tenant refuses
      unconditionally and local single-tenant keeps the Decision 0008 runtime-grant path, both
      proven by refusal tests.
- [ ] No request field, catalogue entry or grant operation can name a destination; the allowlist
      is configuration, and receipts stay value-free.
- [ ] Both composition points (invoker and channel runner) consume one shared policy; a census
      test refuses a third composition point appearing.
