---
id: X-84
title: "A container, one machine, and the operator's first five minutes"
status: ready
priority: 2
epic: remote-deployment
design: docs/designs/remote-deployment.md
areas: [ci, exchange-server]
note: "the first deployment any flux-family repository has made, so it sets the precedent the siblings copy. One machine deliberately — the credential store fsyncs the whole file under one mutex and a fly volume is per-machine, so two machines is two divergent stores with no reconciliation"
---

# A container, one machine, and the operator's first five minutes

## Goal
`fly deploy` puts this service on a public URL, and an operator who has never seen it can get from
that URL to an invoked operation.

## Scope
A `Dockerfile`, a `fly.toml`, a volume, the secrets, and the runbook. **Not** a CI deploy pipeline —
that is worth a separate story once a deploy has been done by hand more than once.

## What the container must get right

- **MSRV is 1.88** (`Cargo.toml`), read from the manifest rather than restated — the same rule X-33's
  `msrv` job follows. Do not pin a base image that silently floats below it.
- **The published binary is `exchange-server`**, which is `publish = false`. The image carries it and
  the built console ([[X-83]]); it does not carry a toolchain.
- **The store modes survive the container.** The credential store wants `0700` on the directory and
  `0600` on the file, and it **refuses a widened mode rather than tightening it** (X-09). A volume with
  a permissive default, or a process running as a different uid than the one that created the files, is
  a startup refusal. Choose the uid deliberately and write down why.

## What `fly.toml` must get right

- **`min_machines_running` and the machine count are one, deliberately.** The credential store rewrites
  and fsyncs the whole file under a single mutex (X-22) and X-25's allowance race is closed only within
  one process. A fly volume attaches to one machine, so two machines is **two independent credential
  stores that silently diverge**. Put the reason in the file, not just the number — a comment is what
  stops somebody scaling it during an incident.
- **`/health` is the check.** It is `Access::Anonymous` by design (`routes/health.rs:19`) so an operator
  can ask whether the process is up before it can tell them anything else.
- **https only.** The `Secure` session cookie needs the *browser* on https. A service that also accepts
  plain http serves a sign-in whose cookie the browser then refuses to store, and that presents as
  "sign-in silently does nothing" — a support question with no error message anywhere.
- **No secret in the file.** `fly.toml` is committed. `FLUX_EXCHANGE_OIDC_CLIENT_SECRET` goes through
  `fly secrets set`, on the same rule that governs `CARGO_REGISTRY_TOKEN`.

## The environment, in full
`FLUX_EXCHANGE_BIND=0.0.0.0:8080` · `FLUX_EXCHANGE_CREDENTIALS` · `FLUX_EXCHANGE_AGENTS` ·
`FLUX_EXCHANGE_SETTINGS` · `FLUX_EXCHANGE_GRANTS` (all four on the volume) · the seven
`FLUX_EXCHANGE_OIDC_*` variables, with `..._REDIRECT_URI=https://<app>.fly.dev/api/signin/callback`.
`FLUX_EXCHANGE_DEV_IDENTITY` must be **unset** — armed, it forces `admit_bind` to refuse.

## The first five minutes, which is the half most likely to be skipped

X-13 landed the grant gate fail-closed and said plainly it **will look like an outage**. A fresh
deployment answers `503` with no grant store bound and `403 not_granted` with one bound and empty. If
this story ships a URL and no path through that, the first experience of the remote service is a
correctly-working platform that appears broken — which is how this epic gets judged a failure.

So the runbook covers, in order: the grants file and one grant that admits something; connecting one
connector; invoking one operation; and what each of `503`, `403 not_granted` and the credential refusal
means. `POST /api/grants/preview` shows what a selector would admit before it is saved ([[X-62]]) and
belongs in that walkthrough.

## Acceptance
- [ ] `fly deploy` produces a running machine that answers `/health` and serves the console at `/`.
- [ ] A browser at the public URL signs in through the configured provider and reaches an authenticated
      screen.
- [ ] **Verified by walking it**, the way [[X-69]] verified its page rather than intending it: sign in →
      connect → grant → invoke → result, on the deployed URL, from a machine with no checkout.
- [ ] The credential store's files are `0600` in a `0700` directory on the volume, asserted after a
      restart rather than at first boot.
- [ ] A redeploy preserves credentials, agents, settings and grants, and **signs everyone out** —
      sessions are in memory (`session.rs:173`). Stated in the runbook so it does not read as a bug.
- [ ] No secret appears in any committed file. `scripts/check-action-pins.sh` still passes if any
      workflow is touched.
- [ ] `FLUX_EXCHANGE_DEV_IDENTITY` is provably unset in the deployed environment — the one variable
      whose presence turns the whole bind rule off.

## Progress
- (not started)

## Notes
- Blocked on [[X-83]]: nothing to serve at `/` until it lands.
- **This is the family's first deployment.** `flux-connectors/providers/fly.toml` is the fly.io
  *connector manifest* and not a deploy config, so there is nothing to copy — and whatever this story
  writes is what `flux` and `flux-connectors` will copy later. Worth the extra hour on the comments.
- A public URL is a target and nothing here rate-limits. Worth its own story before the URL is shared
  widely; recorded in the design's risks.
