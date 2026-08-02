---
id: X-92
title: "Private reporting and protected main"
status: ready
priority: 0
areas: [ci, docs, repository]
note: "Observed 2026-08-02: private vulnerability reporting, secret scanning, Dependabot security updates and main protection are all disabled."
---

# Private reporting and protected main

## Goal
Give a security finding a monitored private route and stop unreviewed, failing or secret-bearing
changes from reaching the branch and workflows that build releases.

## Acceptance
- [ ] Enable GitHub Private Vulnerability Reporting, then add `SECURITY.md` naming that actual channel,
      supported versions, response expectations and what information a report must not include.
- [ ] Enable secret scanning, push protection, validity checks and non-provider-pattern scanning for
      the repository. Demonstrate the settings through the GitHub API without exposing a finding.
- [ ] Enable Dependabot security updates and add dependency-update configuration for the Cargo
      workspace, `console/`, `web/` and GitHub Actions, with grouping that never combines the Flux
      engine pins separately from the connector pins.
- [ ] Add a `main` ruleset requiring pull requests, all existing green checks, one approving review
      and resolved conversations; block branch deletion and force-push.
- [ ] Keep release and Pages workflows least-privilege and every third-party action SHA-pinned. Run
      the action-pin self-test before scanning any new workflow.
- [ ] Record which controls are GitHub-plan or organization dependent and fail the story rather than
      claiming a setting that could not be enabled.
- [ ] Verify the settings via read-only API calls after mutation and update `docs/security.md` from
      limitation to deployment-dependent/enforced as appropriate. This repository-only story does
      not deploy Fly unless runtime files also change.
