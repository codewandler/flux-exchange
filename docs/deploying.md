# Deploying flux-exchange

The runbook for a reachable deployment on fly.io. Everything here has been measured against the image
in [`Dockerfile`](../Dockerfile) rather than intended — where a step exists because something refused,
the refusal is quoted.

> **This is a contributor document, not a page on the public site.** It names an app and a URL, and
> `web/test/site.test.mjs` refuses a deployment-specific fact on any published page.

## What you need first

- `flyctl`, authenticated (`fly auth whoami`).
- **An OIDC provider.** This is not optional and it is not a preference — see below.
- Docker, if you want to check the image locally before pushing it.

## Why a provider is required, before you spend time on anything else

`admit_bind` refuses a reachable bind unless the identity binding is `Bound`, and
`AppState::with_oidc` is the only path to that. A deployment without OIDC does not start degraded — it
**exits**, and fly reports a crash loop:

```
refusing to serve on 0.0.0.0:8080: it is reachable from outside this machine and no identity
provider is configured, so every caller would be anonymous. Either bind loopback
(FLUX_EXCHANGE_BIND=127.0.0.1:8080), or configure an identity provider and start again
```

`FLUX_EXCHANGE_DEV_IDENTITY` is **not** a way around this. Armed on a reachable address it produces a
*different* refusal, deliberately, because its roster handle is a name anybody can guess. The remedy
for one refusal is to add a provider and for the other is to remove what you have, which is why they
are two messages. Local users with a real verifier are [X-58](stories/X-58-static-users-from-a-config-file.md),
and until that lands a remote deployment means a provider.

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

Then the configuration. Six OIDC values are not secret and go in the clear; the seventh never does:

```bash
fly config env set \
  FLUX_EXCHANGE_OIDC_ISSUER='https://<provider>' \
  FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT='https://<provider>/authorize' \
  FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT='https://<provider>/oauth/token' \
  FLUX_EXCHANGE_OIDC_JWKS_URI='https://<provider>/.well-known/jwks.json' \
  FLUX_EXCHANGE_OIDC_CLIENT_ID='<client id>' \
  FLUX_EXCHANGE_OIDC_REDIRECT_URI='https://<app>.fly.dev/api/signin/callback' \
  FLUX_EXCHANGE_OIDC_TENANT='<tenant>'

fly secrets set FLUX_EXCHANGE_OIDC_CLIENT_SECRET='<client secret>'
fly deploy
```

**A partial configuration refuses at startup and names the variable that is missing** (X-27), so a
typo is a clear failure rather than a mysterious `401` an hour later. Every URL must be `https` — the
scheme is checked for the back-channel *and* the browser-facing endpoints (X-19, X-23).

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

   There is **no console screen for grants yet** (X-62 shipped the API and said so), so this step is
   `curl` or the preview endpoint until one exists.
3. **Connect a connector** from the console. The credential goes to the store and is never returned by
   any route — that is the platform's whole claim, not a detail.
4. **Invoke an operation.** `POST /api/operations/{operation}/invoke`. If it refuses, the table above
   says which of the three states you are in.

## What a redeploy does and does not keep

**Keeps** — credentials, agents, settings and grants. All four are on the volume.

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
  -e FLUX_EXCHANGE_GRANTS=/data/grants/store.json \
  flux-exchange:local
```

## Not covered, and worth knowing before the URL is shared

**Nothing here rate-limits anything.** X-22's bounds are about one tenant's cost to another, not about
an anonymous flood, and there is no brute-force protection on any endpoint. A public URL is a target.
That is a story, not a gap in this runbook.

There is also **no CI deploy pipeline** — a deploy is `fly deploy` from a working tree. Worth
automating once it has been done by hand more than once.
