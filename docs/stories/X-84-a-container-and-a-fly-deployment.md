---
id: X-84
title: "A container, one machine, and the operator's first five minutes"
status: blocked
epic: remote-deployment
design: docs/designs/remote-deployment.md
areas: [ci, exchange-server]
note: "BLOCKED on a human Google OIDC sign-in and public connect → grant → invoke walkthrough; deployment and storage artifacts are already live"
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
- [x] `fly deploy` produces a running machine that answers `/health` and serves the console at `/`.
      → deployed at `https://flux-exchange.fly.dev` and re-verified after X-85's v0.12.0 rollout:
      Fly release v2 passes its machine check, `/health` names v0.12.0 and `/` serves the console.
- [ ] A browser at the public URL signs in through the configured provider and reaches an authenticated
      screen.
- [ ] **Verified by walking it**, the way [[X-69]] verified its page rather than intending it: sign in →
      connect → grant → invoke → result, on the deployed URL, from a machine with no checkout.
- [x] The credential store's files are `0600` in a `0700` directory on the volume.
      → verified in the built image on a mounted volume: `drwx------ /data/credentials` holding
      `-rw------- store.json`. **This is where the measurement changed the design** — see Progress.
- [ ] A redeploy preserves credentials, agents, settings and grants, and **signs everyone out** —
      sessions are in memory (`session.rs:173`). Stated in the runbook so it does not read as a bug.
- [x] No secret appears in any committed file.
      → `fly.toml` carries the six non-secret OIDC values as placeholders and names the seventh only to
      say it belongs in `fly secrets set`. No workflow was touched.
- [x] `FLUX_EXCHANGE_DEV_IDENTITY` is provably unset — the one variable whose presence turns the whole
      bind rule off.
      → absent from `fly.toml` and the image, with the reason written where somebody would think to add
      it. Confirmed in the container: a reachable bind with no identity exits, quoting the refusal.

## Progress
- **The artifacts are deployed, not sketched.** `Dockerfile` (three stages, no
  toolchain in the runtime layer), `.dockerignore`, `fly.toml`, and `docs/deploying.md` as the runbook.
  `docker build` succeeds and the image was driven through both the failure and the success path.
- **The measurement that changed the design: a fresh volume mounts `0755`, and the credential store
  refuses rather than tightening** (X-09). The obvious `FLUX_EXCHANGE_CREDENTIALS=/data/credentials`
  does not start:

  ```
  the secret store refused access to /data: its mode is 0755, and a credential store must be
  no wider than 0700 — run `chmod 700 /data` once you are satisfied nobody else has read it
  ```

  Pointed one level deeper — `/data/credentials/store.json` — the store **creates its own parent
  `0700`** and its file `0600`, and the service boots with no manual `chmod` on a volume nobody has
  touched. All four store paths are nested for that reason, and `fly.toml` carries the quoted refusal
  beside them: flattening them back to `/data/*.json` looks like tidying and is a deployment that will
  not start.
- **A second finding from the same run:** the agent store makes the same complaint about `0755` as a
  *warning* rather than a refusal — it discloses which agents exist and when their tokens expire, and no
  token — so it would have started and quietly disclosed that. Nesting silences it too.
- **The bind rule was verified inside the container**, which had to be true before the rest was worth
  writing: `FLUX_EXCHANGE_BIND=0.0.0.0:8080` with no identity exits and quotes the refusal, so a
  misconfigured machine crash-loops with the reason in its log rather than serving anonymously.
- **`ca-certificates` is in the runtime image deliberately.** Without it the OIDC token exchange fails
  on the certificate chain and reads as *the provider refused us* — the confusion X-17 exists to split
  apart, reintroduced by a missing package.
- **OIDC is registered and the service is deployed.** `/api/signin` redirects to Google with PKCE,
  but the authenticated browser and sign-in → connect → grant → invoke walkthrough remain unverified;
  those require completing the provider interaction as a real account, not another code change.
- **2026-08-03 — status reconciled.** The same external verification remains: a human must complete
  Google OIDC and walk the public flow, then redeploy and confirm persistence/session invalidation.
  No repository implementation is active while that evidence is unavailable, so this is blocked
  rather than in progress.

## Notes
- Blocked on [[X-83]]: nothing to serve at `/` until it lands.
- **This is the family's first deployment.** `flux-connectors/providers/fly.toml` is the fly.io
  *connector manifest* and not a deploy config, so there is nothing to copy — and whatever this story
  writes is what `flux` and `flux-connectors` will copy later. Worth the extra hour on the comments.
- A public URL is a target and nothing here rate-limits. Worth its own story before the URL is shared
  widely; recorded in the design's risks.
