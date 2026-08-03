---
id: X-123
title: "Production refuses an operatorless deployment"
status: done
epic: remote-deployment
areas: [operations, identity]
note: "The deploy checks Fly's value-free secret metadata before building and after rollout, so an absent operator policy cannot ship silently."
---

# Production refuses an operatorless deployment

## Goal
Make the production pipeline prove that its deployment-owned operator policy exists without reading
or publishing the policy contents.

## Acceptance
- [x] A failing-first operations-contract test rejects a production workflow that does not run the
      operator-policy preflight, or weakens its required Fly status from `Deployed`.
- [x] The preflight refuses an absent, duplicate or not-yet-deployed
      `FLUX_EXCHANGE_OPERATOR_SUBJECTS` entry using only `flyctl secrets list` metadata.
- [x] Production runs the preflight before building an image, and post-deploy evidence records only
      that the policy was deployed — never its digest or subjects.
- [x] The deployment runbook distinguishes the OIDC client credential from private operator-subject
      metadata and names both required Fly settings without implying either belongs in `fly.toml`.

## Progress
- 2026-08-03: Filed from the live v0.16.2 incident after OIDC sign-in succeeded but every
  administrative route correctly refused because the Fly app had no operator policy.
- 2026-08-03: A failing-first production-operations check refused the missing verifier. The
  self-tested preflight now runs before image construction and again after rollout; a live v0.16.2
  verification retained only `operator_policy: deployed`.

## Notes
- Extends X-91's operator boundary and X-93's attributable production workflow; neither policy
  contents nor Fly's secret digest belongs in retained production evidence.
