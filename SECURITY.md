# Security policy

## Reporting a vulnerability

Report a suspected vulnerability through this repository's
[private vulnerability reporting form](https://github.com/codewandler/flux-exchange/security/advisories/new).
Do not open a public issue for an unpatched vulnerability. The private advisory is the monitored
channel for coordinating triage, remediation and disclosure with the maintainers.

Use synthetic or redacted evidence. Do not include real credentials, bearer tokens, session cookies,
OIDC material or other secrets. Do not include customer data, tenant data, personal data, production
payloads or unnecessary identifying information. Do not exploit a live tenant or service to prove a
finding; describe the smallest safe reproduction instead.

## Supported versions

Security fixes are made on the latest released version and on `main`. Older releases are unsupported;
an advisory will say explicitly if a fix is backported. The current release is listed in
[`CHANGELOG.md`](CHANGELOG.md).

## Response expectations

Maintainers aim to acknowledge a report within two business days, provide an initial triage result
within five business days and send at least weekly updates while remediation is active. These are
response targets, not a promise that every report can be fixed or disclosed on that schedule.

The report should state the affected surface and version, security impact, safe reproduction steps
and any suggested mitigation. Please say whether you plan to publish and coordinate a disclosure date
through the private advisory before making details public.
