# Deploying flux-exchange

The runbook for a reachable deployment on fly.io. Everything here has been measured against the image
in [`Dockerfile`](../Dockerfile) rather than intended — where a step exists because something refused,
the refusal is quoted.

> **This is a contributor document, not a page on the public site.** It names an app and a URL, and
> `web/test/site.test.mjs` refuses a deployment-specific fact on any published page.

## What you need first

- `flyctl`, authenticated (`fly auth whoami`).
- A reachable-safe identity: OIDC for the hosted production described here, or an owner-only local
  users verifier file for a small self-hosted deployment — see below.
- Docker, if you want to check the image locally before pushing it.

## Operator security checklist

Read the full [security posture](security.md) before exposing a machine. These checks are the short
operational gate; they do not turn a known limitation into an enforced control.

Before deploying:

- [ ] Confirm the OIDC application is still organization-internal, its redirect URI is exact, every
      configured endpoint is HTTPS, and `FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN` exactly names the signed
      Google Workspace `hd` claim this deployment admits. Never substitute an email suffix.
- [ ] Set `FLUX_EXCHANGE_OPERATOR_SUBJECTS` to the immutable OIDC subjects of the smallest operator
      set. Do not use email addresses, display names or the hosted domain; an unset policy makes
      every management route fail closed.
- [ ] Put `FLUX_EXCHANGE_OIDC_CLIENT_SECRET` and the private operator-subject policy in Fly secrets.
      The first is a credential; the second identifies real people even though it is not secret
      material. Keep both out of `fly.toml`, tickets and logs.
- [ ] Verify the actual Fly volume reports encryption enabled; do not infer this from `fly.toml`,
      because the volume is created separately.
- [ ] Verify there is exactly one machine and one attached volume. Do not scale this file-backed
      deployment horizontally.
- [ ] Verify the six store paths remain nested below `/data`, the image still runs as uid `10001`,
      and no prior store directory or snapshot is being attached accidentally.
- [ ] Select one full commit SHA already reachable from protected `main`. Production accepts no
      working tree and no branch name: `.github/workflows/production.yml` checks out exactly that
      SHA, reruns the complete gate, scans the resulting image and deploys its immutable digest.
- [ ] Confirm grants begin at the least metadata selector the intended work needs. A fresh store
      admitting nothing is the safe state.
- [ ] Confirm `snapshot watch` is green and its identifier-free evidence says the newest completed
      snapshot is at most 24 hours old with 14-day retention. A quarterly restore record, not the
      existence of a snapshot, is the evidence for the 60-minute RTO.

After deploying:

- [ ] Verify `/health` reports the intended version, sign-in completes, logout invalidates the
      presented session, and an ungranted invocation refuses.
- [ ] Verify the live console and API carry CSP, HSTS, `nosniff`, referrer and permissions headers;
      verify API responses additionally carry `Cache-Control: no-store`.
- [ ] Verify saturation answers `429` without taking health down, and inspect audit events for stable
      action/actor/target fields with no token, credential, setting value or request body.
- [ ] Query one live event by id, actor and target, restart the machine, and query the event again.
      Confirm its `timestamp` is retained and no field can hold request or credential material.
- [ ] Retain the production workflow artifact. It records source SHA, scanned image digest, release,
      machine and verification time; link an incident or exception rather than weakening a check.

## The only production deployment path

Production comes from `.github/workflows/production.yml`, either after the `ci` workflow succeeds on
protected `main`, or through a manual dispatch naming one full 40-character SHA already reachable
from `main`. The workflow checks out that exact commit three times: source selection, the complete
repository gate, then image build. A branch name, pull request SHA, fork and dirty local tree cannot
reach the production environment or its secret.

Configure the GitHub environment once:

1. Create an environment named `production`, restrict deployment branches to protected `main`, and
   keep environment administrators to maintainers. The repository has one maintainer today, so the
   branch and immutable-SHA checks are the approval boundary; do not create a circular required
   reviewer rule that nobody else can satisfy.
2. Create an app-scoped Fly deploy token with `fly tokens create deploy -a flux-exchange` and store
   the complete value as the environment secret `FLY_API_TOKEN`. Do not use an organization token:
   the workflow needs authority over this app and nothing else.
3. Keep Actions secrets unavailable to forks, and do not add a `pull_request` trigger to either the
   production or snapshot workflow.

The build pins all three base images by digest, uses `cargo build --locked` in both Docker layers,
generates an SPDX JSON SBOM, and makes every Grype finding fail the build. A vulnerability exception
belongs in `.grype.yaml` as one vulnerability/package tuple with an owner, expiry and reachability
argument. A broad severity exception is not an exception; it is switching the scan off.

The scanned local image is pushed once, resolved to the registry digest and deployed by that digest.
Post-deploy verification checks the application version, security headers, API `no-store`, one
running machine and that same digest. A deployment or verification failure visibly redeploys the
previous digest and leaves the workflow red. The retained artifact joins source SHA, image digest,
Fly release, machine, SBOM and scan result without including a token, secret or store identifier.

To deploy a selected merged commit manually:

```bash
gh workflow run production.yml -f source_sha="$(git rev-parse <merged-commit>)"
gh run watch --exit-status
```

Never replace this with local `fly deploy`. The local checkout is useful for image diagnostics; it
is not an attributable production source.

## Choose a reachable-safe identity binding first

`admit_bind` refuses a reachable bind unless the identity binding is federated or verifier-backed.
A deployment without either does not start degraded — it
**exits**, and fly reports a crash loop:

```
refusing to serve on 0.0.0.0:8080: it is reachable from outside this machine and no identity
provider is configured, so every caller would be anonymous. Either bind loopback
(FLUX_EXCHANGE_BIND=127.0.0.1:8080), or configure an identity provider and start again
```

`FLUX_EXCHANGE_DEV_IDENTITY` is **not** a way around this. Armed on a reachable address it produces a
*different* refusal, deliberately, because its roster handle is a name anybody can guess. The remedy
for one refusal is to add a provider and for the other is to remove what you have, which is why they
are two messages.

For a small self-hosted deployment, generate an opaque local-user secret and keep only its verifier:

```bash
cargo run -- local-user-secret alice acme
# copy the printed JSON entry into /etc/flux-exchange/users.json, then:
chmod 0600 /etc/flux-exchange/users.json
export FLUX_EXCHANGE_LOCAL_USERS=/etc/flux-exchange/users.json
```

The command shows the generated secret once. The file contains `user`, `tenant`, and `verifier`,
never a password or recoverable secret; a group/world-accessible mode or malformed entry refuses
startup and names the file or entry. The sign-in page presents the local form and issues only an
HttpOnly session cookie. Use OIDC instead when people need memorable passwords, centralized
revocation, or organizational membership policy.

Register a web application with your provider and set the redirect URI to
`https://<app>.fly.dev/api/signin/callback`. Collect: issuer, authorization endpoint, token endpoint,
JWKS URI, client id, client secret.

## First deploy

```bash
fly launch --no-deploy --copy-config      # reads the committed fly.toml; do not let it overwrite it
fly volumes create flux_exchange_data --size 1 --region fra
```

The volume is created **by hand, once**, and `fly.toml` does not create one. A deploy that silently
created a fresh empty volume would present as *every credential vanished*.

Then the configuration. Seven required OIDC values are not secret and go in the clear; Google
production also sets the optional hosted-domain hint/claim requirement. The client secret never does:

```bash
fly config env set \
  FLUX_EXCHANGE_OIDC_ISSUER='https://<provider>' \
  FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT='https://<provider>/authorize' \
  FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT='https://<provider>/oauth/token' \
  FLUX_EXCHANGE_OIDC_JWKS_URI='https://<provider>/.well-known/jwks.json' \
  FLUX_EXCHANGE_OIDC_CLIENT_ID='<client id>' \
  FLUX_EXCHANGE_OIDC_REDIRECT_URI='https://<app>.fly.dev/api/signin/callback' \
  FLUX_EXCHANGE_OIDC_TENANT='<tenant>' \
  FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN='<google workspace domain>'

fly secrets set FLUX_EXCHANGE_OIDC_CLIENT_SECRET='<client secret>'
fly secrets set FLUX_EXCHANGE_OPERATOR_SUBJECTS='<immutable OIDC subject>[,<another subject>]'
gh workflow run production.yml -f source_sha='<full commit SHA on protected main>'
```

The production workflow refuses before building an image unless Fly reports exactly one
`FLUX_EXCHANGE_OPERATOR_SUBJECTS` entry with status `Deployed`, and checks it again after rollout.
`flyctl secrets list` exposes only names, digests and deployment status: the verifier keeps that
metadata in a temporary directory, retains neither the digest nor the subjects, and records only
`operator_policy: deployed` in the production evidence.

**A partial configuration refuses at startup and names the variable that is missing** (X-27), so a
typo is a clear failure rather than a mysterious `401` an hour later. Every URL must be `https` — the
scheme is checked for the back-channel *and* the browser-facing endpoints (X-19, X-23).
The hosted-domain setting is optional for non-Google OIDC providers. When set, it is both a Google
account-selection hint and a fail-closed check against the signed `hd` claim; only the latter grants
admission. The authorization request uses `openid email`: live Google evidence made `email` a
provider-protocol requirement, not an authorization input. This host does not parse the email claim;
identity remains the immutable `sub`, and organization admission remains the signed `hd` claim.

## It will start, and it will run nothing. That is correct.

This is the part that reads as an outage and is not. X-13 landed the grant gate **fail-closed**: an
operation runs only if a grant the caller's tenant holds admits it. A fresh deployment therefore
answers:

| You see | It means |
|---|---|
| `503` on invoke | No grant store is bound at all. |
| `403 not_granted` | A store is bound and this tenant holds nothing that admits the operation. |
| A credential refusal | Granted, but the connector has no credential for this tenant yet. |

`fly.toml` binds `FLUX_EXCHANGE_GRANTS`, so a deployment made from it starts in the **second** state.
Nothing is wrong; nobody has been granted anything.

### The first five minutes

1. Open `https://<app>.fly.dev` and sign in. The console is served by the same origin that answers
   `/api` — it has to be, because the session cookie is `SameSite=Strict` and a browser never attaches
   one cross-origin (X-83).
2. **Write a grant.** `GET/PUT /api/grants` edits them and `POST /api/grants/preview` shows what a
   selector would admit *before* it is saved (X-62), which is the endpoint to use first. A grant
   selects by declared metadata — risk, effects, idempotency — and never by a list of operation names.
   A read-only start:

   ```json
   { "grants": [ { "connector": "github", "selector": { "max_risk": "low" } } ] }
   ```

   The console's Grants step previews and writes this policy for a signed-in human; `curl` remains
   useful for deployment automation.
3. **Connect a connector** from the console. The credential goes to the store and is never returned by
   any route — that is the platform's whole claim, not a detail.
4. **Invoke an operation.** `POST /api/operations/{operation}/invoke`. If it refuses, the table above
   says which of the three states you are in.

## What a redeploy does and does not keep

**Keeps** — credentials, Service Accounts, settings, grants, workflows, run activity and application
audit evidence. All are on the volume; audit rows younger than 30 days are retained across restarts.

**Loses** — every session. `SessionStore` is a `Mutex<HashMap<…>>` in memory, so a deploy signs
everyone out and they sign in again. This is worth stating because it looks like a bug and is not; a
session that outlived the process would need a store, and nothing binds one.

## Why one machine, and do not change it

`min_machines_running = 1`, and the ceiling matters more than the floor. The credential store rewrites
and fsyncs the whole file under a single mutex (X-22), and the per-tenant allowance race X-25 closed is
closed only *within one process*. A fly volume attaches to exactly one machine — so two machines is two
independent credential stores, diverging with no reconciliation and no way to know which one a caller
wrote to. Scaling horizontally needs a shared store behind the `SecretStore` port first. The port
exists for that; nothing binds such an implementation.

## Things that were measured, not assumed

- **The store paths are nested one directory deep.** A fresh volume mounts its root `0755`, and the
  credential store refuses a parent wider than `0700` rather than tightening it (X-09):
  *"the secret store refused access to /data: its mode is 0755"* — the process exits. Pointed one level
  down it creates its own parent `0700` and its file `0600` and boots with no manual `chmod`. Do not
  flatten those paths to tidy them.
- **The uid is fixed at 10001.** The store's files are owned by whoever created them, and a deploy that
  changed uid would find a store it cannot read. The number is part of the contract.
- **`ca-certificates` is installed in the runtime image.** Without it the OIDC token exchange fails on
  the certificate chain, which reads as *the provider refused us* — the exact confusion X-17 split
  apart.
- **The entrypoint is exec form**, so the binary is pid 1 and receives fly's `SIGTERM` directly. That
  is what `with_graceful_shutdown` waits for; behind a shell the signal reaches the shell and the
  server is killed mid-write, on a store that rewrites the whole file at once.
- **`force_https = true`.** The session cookie is `Secure`, so a browser only stores it over https. A
  deployment answering plain http serves a sign-in whose cookie is silently discarded — which presents
  as *sign-in does nothing*, with no error on either side.

## Checking the image without deploying

```bash
docker build -t flux-exchange:local .

# The bind rule refuses, which is the correct answer:
docker run --rm -e FLUX_EXCHANGE_BIND=0.0.0.0:8080 flux-exchange:local

# Loopback with the stores bound, to watch it come up:
docker volume create flux-exchange-local
docker run --rm -v flux-exchange-local:/data \
  -e FLUX_EXCHANGE_BIND=127.0.0.1:8080 \
  -e FLUX_EXCHANGE_CREDENTIALS=/data/credentials/store.json \
  -e FLUX_EXCHANGE_CONNECTIONS=/data/connections/store.json \
  -e FLUX_EXCHANGE_GRANTS=/data/grants/store.json \
  -e FLUX_EXCHANGE_WORKFLOWS=/data/workflows \
  -e FLUX_EXCHANGE_AUDIT=/data/audit/events.sqlite3 \
  flux-exchange:local
```

## Reading and deleting audit evidence

The journal is not an HTTP surface. Tenant principals cannot enumerate it. A Fly organization
member with SSH access can run the binary's read-only query command inside the machine:

```bash
fly ssh console --command 'flux-exchange audit-query --event-id <event-id>'
fly ssh console --command 'flux-exchange audit-query --actor codewandler/user/<principal-id> --limit 100'
fly ssh console --command 'flux-exchange audit-query --target invocation/<operation-id> --limit 100'
```

Each result is one JSON object per line. The command accepts exactly one query shape and bounds
`--limit` to 1–1000. The `exchange` uid can append, finish and age rows out after the 30-day minimum.
A Fly organization member with SSH can read them because SSH reaches that uid. Early deletion
requires that runtime uid to alter the database or a Fly organization administrator to replace or
destroy the volume; neither power is available to an Exchange tenant principal.

The fixed alert policies are retained records as well as `warn` events in stdout: 20 authentication
refusals in five minutes, 10 authorization refusals for one actor in five minutes, and every
credential or grant change. Fly log search is the notification stream, not the retention source;
the SQLite journal remains authoritative when shorter-lived platform logs expire.

## The process traffic boundary

X-87 bounds the two paths that allocate or spend outside the handler: 30 OIDC authorization starts
per rolling minute, 120 invocation attempts per rolling minute, and 16 concurrently executing
invocations. X-96 adds a 30-per-minute rolling budget for each resolved `(tenant, kind, id)` while
retaining those process-wide ceilings. The key is constructed only after authentication from the
resolved principal; no request header, query or body field participates. A saturated path refuses
immediately with `429` and `Retry-After` rather than growing a queue. Health, session and
administration routes do not consume invocation slots.

These are application backstops, not a substitute for edge flood protection. The committed Fly
service declares request concurrency at the Fly Proxy, the configured immediate edge, with a soft
limit of 64 and a hard limit of 96. It bounds anonymous request occupancy before work reaches the
process; Fly can answer an over-hard-limit request with `503`, while application budget saturation
continues to answer `429`. The process deliberately does not read `X-Forwarded-For`, `Fly-Client-IP`
or another forwarding header: neither address is an identity and there is no application-level
authenticated proxy contract that would make a caller address a safe bucket key.

Fly scrapes `/metrics` on the internal service port. Its series have only fixed `work`, `outcome` and
`limit` values and expose sign-in/invocation admissions and refusals plus active invocations; they
never label by tenant, principal, route, operation or a caller value. Alert on a sustained increase
of the anonymous/global sign-in refusal series and on any sustained invocation refusal series. The
process also emits one fixed-label warning at each 20 refusals, with no token or request body.

## Recovery objectives and the writes at risk

The production volume has a **24-hour RPO** and a **60-minute RTO**. Fly creates one automatic
snapshot daily and retains new snapshots for 14 days. Restoring the newest completed snapshot can
lose every persistent write after its `created_at`: credential creation and rotation, connection
labels and metadata, Service Accounts, settings, grants, workflow drafts and versions, run activity,
and audit rows. Sessions are already process-local and are lost on every restart.

`fly.toml` declares policy for volumes Fly may create later. It does not repair the existing volume.
Apply and immediately verify the live setting as a separate protected operation:

```bash
volume_id="$(fly volumes list -a flux-exchange --json |
  jq -er '[.[] | select(.name == "flux_exchange_data" and .state == "created")][0].id')"
fly volumes update "$volume_id" -a flux-exchange \
  --scheduled-snapshots=true --snapshot-retention=14
TMPDIR=/path/to/private/scratch ./scripts/verify-fly-snapshot.sh
unset volume_id
```

Do not paste the volume or snapshot id into an issue, workflow summary or recovery record. Both are
handles to credential-bearing state. The verifier emits only timestamps, age, retention, encryption
and scheduling. `.github/workflows/snapshot-watch.yml` runs it daily; a missing, stale, unencrypted or
misconfigured recovery point opens one private issue, and a later healthy run closes that issue.

### Quarterly isolated restore drill

Run once per calendar quarter and time from stopping the active machine until the replacement answers
all checks. Use a maintainer-only terminal with history disabled. A restored volume may exist detached
while production runs, but it is never attached until the active writer is stopped. The drill machine
has no public service registration, and production is not restarted until the drill machine is
destroyed. These are the locks that prevent two copies from accepting writes.

1. Privately select the newest completed snapshot, confirm its age is no more than 24 hours and create
   a **new encrypted** volume from it in `fra`. Give it a quarter-specific name and 1-day retention.
   Record only the snapshot timestamp in durable evidence.

   ```bash
   fly volumes snapshots list "$production_volume_id" -a flux-exchange --json >"$private_snapshots"
   snapshot_id="$(jq -er '[.[] | select(.status == "created")] | sort_by(.created_at) | last.id' "$private_snapshots")"
   fly volumes create "flux_exchange_recovery_${quarter}" -a flux-exchange -r fra -s 1 \
     --snapshot-id "$snapshot_id" --snapshot-retention 1 --scheduled-snapshots=false --yes
   ```

2. Save the current machine JSON in the same private scratch directory. Record the start time, stop
   the active machine, and verify it is stopped **before** attaching the restored copy. Derive a drill
   config from the stopped machine: replace its mount with the restored volume, delete every service,
   set `restart.policy` to `no`, add `recovery_drill=<quarter>` metadata, and keep the production image
   and store environment unchanged. Never hand-copy an OIDC secret into the file; app secrets remain
   managed by Fly.

   ```bash
   fly machines list -a flux-exchange --json >"$private_machines"
   active_machine="$(jq -er '[.[] | select(.state == "started")][0].id' "$private_machines")"
   fly machine stop "$active_machine" -a flux-exchange
   # Build drill-machine.json from .[0].config as described above, then review it before creation.
   fly machine run "$(jq -er '.[0].config.image' "$private_machines")" -a flux-exchange \
     --machine-config "$private_drill_config" --skip-dns-registration --detach
   ```

3. A successful start is the first store check: the host opens and parses credentials, connections,
   Service Accounts, settings, grants, workflows and the audit database before serving. Through SSH,
   assert uid `10001`, each store directory is `0700`, each file is `0600`, and the known paths are
   readable; print only path, owner and mode. Do not print, copy or count store values. Check startup
   logs for a refusal, then confirm grants and connection metadata routes return structured responses
   to the drill operator without placing either response in the evidence record.

4. Use the drill machine's private IPv6 address with `fly proxy`, never a public service. Fetch
   `/health`, `/` and `/api/onboarding`; assert the expected version, CSP, HSTS, `nosniff`, referrer
   and permissions policies on console and API, plus `Cache-Control: no-store` on the API. Stop the
   proxy immediately after the checks.

5. Destroy in this order even when a check fails: stop and destroy the drill machine, destroy the
   restored volume, delete private scratch files, then start the original machine. Verify the public
   health/security policy and one-machine/one-volume topology again. If elapsed time exceeds 60
   minutes, the drill failed the RTO even when the data opened eventually.

The operator should install an `EXIT` trap after both machine identifiers are known so an interrupted
shell attempts the same decommission order. If automated cleanup fails, keep production stopped and
finish cleanup manually; starting it beside a surviving drill writer converts a recovery exercise into
a split-brain incident.

Record a dated quarter, source snapshot time, elapsed restore minutes, version/image, pass/fail for
ownership/modes, all stores opened, grant/connection metadata, health/security policy, drill machine
destroyed, drill volume destroyed, and original machine healthy. Machine, volume and snapshot ids and
all store contents stay in the private scratch record and are deleted after decommission.

### Independent backup, because a Fly snapshot is not enough

Daily Fly snapshots share the volume provider's failure domain. Once per day, create an on-demand
snapshot, restore it detached, stop the production writer, and attach the copy only to an unregistered
backup machine. Stream one archive of `/data` directly through `age` to object storage in a different
provider account with object lock and 30-day retention; no plaintext archive touches disk. The age
recipient is public, while the private key is kept offline outside Fly and GitHub. Verify the uploaded
ciphertext digest and a quarterly test decryption into the same isolated drill path, then decommission
the helper and resume production in the order above.

This independent copy is also a 24-hour objective, but it does not replace the Fly snapshot: the
snapshot is the fast path that meets the 60-minute RTO, and the separately administered ciphertext is
the last-resort path when Fly's volume and snapshot plane are both unavailable.
