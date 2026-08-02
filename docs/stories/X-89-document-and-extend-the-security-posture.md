---
id: X-89
title: "The security posture is discoverable and its gaps are tracked"
status: done
priority: 0
areas: [ci, docs, exchange-host, exchange-server]
note: "Document the controls X-87 hardened, label deployment assumptions and limitations honestly, and turn the remaining security work into ranked stories."
---

# The security posture is discoverable and its gaps are tracked

## Goal
Give contributors and operators one authoritative map of the assets, attackers, trust boundaries,
enforced controls, deployment assumptions and known security limitations without changing runtime
behaviour. Turn every material gap found while writing it into ranked work rather than leaving it in
prose.

Organization-wide Google sign-in remains intentional. Authentication and operator authorization
are separate decisions; X-91 is where the latter changes.

## Acceptance
- [x] A failing-first repository test requires `docs/security.md`, links to it from `README.md` and
      `docs/README.md`, and requires it to reference the authoritative identity, invoke, public
      hardening and remote-deployment designs.
- [x] `docs/security.md` covers the threat model, protected assets, trust boundaries, assumed
      attackers, authentication/session controls, authorization/tenancy, credential storage,
      execution/egress, browser controls, availability bounds, audit evidence and supply chain.
- [x] Every posture claim is labelled **Enforced in code**, **Deployment-dependent**, or **Known
      limitation**, and points to the source or design that owns the detail.
- [x] The document contains a ranked security roadmap and an incident checklist for credential
      rotation, OIDC-secret rotation, session invalidation, snapshot restoration and store
      decommissioning.
- [x] `docs/deploying.md` contains an operator security checklist, and `SECURITY.md` is not added
      before a private reporting channel exists.
- [x] X-90 through X-97 record the prioritized follow-up work, including Google hosted-domain
      verification, explicit operators, repository protections/reporting, reviewed deployments,
      recovery, durable audit evidence, fair traffic controls and a managed credential backend.
- [x] Every relative link resolves, documentation contains no credential-shaped value, the complete
      Rust/console/web gates and dependency audits pass, and `git diff --check` is clean.
- [x] X-89 is marked done, an Unreleased CHANGELOG entry records it and the generated board is
      current. This documentation-only tranche does not bump or deploy a release.

## Progress
- Recovered the decision-complete plan from Codex session
  `019fc175-a2b8-7a30-ad11-e1ee765c6fb5`.
- Added the repository coverage test first and observed it fail because `docs/security.md` did not
  exist; it passes after the document and both index links were added.
- Verified all relative links in the new document and story set. The documentation contains no
  credential value; the deploy runbook retains only its existing explicit `<client secret>`
  placeholder.
- The complete gate is green: 418 Rust tests, 113 console tests, 28 public-site tests, both Node
  production builds and dependency audits, Clippy, formatting, RustSec with the two documented
  exceptions, action pins, crate versions and `git diff --check`.

## Notes
- Existing agent-authentication work stays under X-35. It must not become live without listing and
  revocation; this story does not duplicate that work.
- Every later story that changes runtime security must produce a versioned Fly release and live
  verification. `docs/` is excluded from the current image, so X-89 itself does not deploy.
