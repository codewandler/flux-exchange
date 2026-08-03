---
id: X-92
title: "Private reporting and protected main"
status: in-progress
priority: 0
areas: [ci, docs, repository]
note: "Private reporting, protected main and baseline scanning are live; GitHub Free cannot enable validity checks or non-provider patterns."
---

# Private reporting and protected main

## Goal
Give a security finding a monitored private route and stop unchecked, failing or secret-bearing
changes from reaching the branch and workflows that build releases.

## Acceptance
- [x] Enable GitHub Private Vulnerability Reporting, then add `SECURITY.md` naming that actual channel,
      supported versions, response expectations and what information a report must not include.
- [ ] Enable secret scanning, push protection, validity checks and non-provider-pattern scanning for
      the repository. Demonstrate the settings through the GitHub API without exposing a finding.
- [x] Enable Dependabot security updates and add dependency-update configuration for the Cargo
      workspace, `console/`, `web/` and GitHub Actions, with grouping that never combines the Flux
      engine pins separately from the connector pins.
- [x] Add a `main` ruleset requiring pull requests, all existing green checks and resolved
      conversations; block branch deletion and force-push. While the repository has one maintainer,
      require zero approvals rather than making every change unmergeable; raise this to one when a
      second maintainer can review independently.
- [x] Keep release and Pages workflows least-privilege and every third-party action SHA-pinned. Run
      the action-pin self-test before scanning any new workflow.
- [x] Record which controls are GitHub-plan or organization dependent and fail the story rather than
      claiming a setting that could not be enabled.
- [x] Verify the settings via read-only API calls after mutation and update `docs/security.md` from
      limitation to deployment-dependent/enforced as appropriate. This repository-only story does
      not deploy Fly unless runtime files also change.

## Implementation status — 2026-08-03

Read-only GitHub API calls after mutation report private vulnerability reporting and Dependabot
security updates enabled, with secret scanning and push protection enabled. Repository ruleset
`20297512` (`Protect main`) is active on the default branch with no bypass actors and holds the pull
request, conversation, status-check, deletion and non-fast-forward rules above. Its approval count
is deliberately zero while the repository has only one maintainer; the required pull request and
checks still prevent direct or failing changes from reaching `main`.

The same API evidence reports `secret_scanning_validity_checks` and
`secret_scanning_non_provider_patterns` disabled. The `codewandler` organization reports plan
`free`; GitHub documents both controls as requiring an organization-owned repository on Team or
Enterprise Cloud with GitHub Secret Protection. The API accepted the requested settings but did not
enable them. This story therefore remains in progress and its combined secret-scanning acceptance
item remains unchecked until the organization enables that product; the repository does not claim
the unavailable controls.
