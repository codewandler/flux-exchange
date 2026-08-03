---
id: X-90
title: "Verify the Google organization in the signed token"
status: in-progress
priority: 0
epic: remote-deployment
areas: [exchange-server, identity]
note: "The Google app is Internal, but this process accepts any token valid for its client; Google says a Workspace boundary must verify the signed hd claim, never an email suffix."
---

# Verify the Google organization in the signed token

## Goal
Keep organization-wide Google sign-in, while making membership a signed claim this process verifies
instead of a provider-console setting whose accidental widening this host cannot see.

## Acceptance
- [x] Add an expected hosted-domain deployment setting. When set, `/api/signin` sends `hd` only as a
      Google account-selection hint; the hint is never treated as admission.
- [x] Carry the signed ID token's `hd` claim across the verified-claims seam and require byte-for-byte
      equality with the configured value. A missing or mismatched claim refuses with no session.
- [x] Never derive membership from `email` or an email suffix. Identity remains keyed by immutable
      OIDC `sub`.
- [x] Reduce the sign-in scope to `openid`; remove unused `email`/`profile` claims and fields unless
      a concrete in-tree consumer is identified in the same change.
- [x] Failing-first tests cover a matching, missing and mismatched signed claim; a mutation that
      trusts the authorization-request hint or email turns the gate red.
- [x] Update the deployment configuration and runbook without committing an organization identifier
      to the public `web/` site.
- [ ] Produce a versioned Fly release and verify live that an organization member signs in, a token
      without the expected claim is refused, and health/security headers remain intact.

## Notes
- Google requires `hd` verification when a resource is domain-restricted and explicitly says not to
  use the `email` domain: <https://developers.google.com/identity/openid-connect/reference>.
