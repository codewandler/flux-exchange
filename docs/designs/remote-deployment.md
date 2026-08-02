# Design: a deployment a stranger can reach

**Status:** proposed · **Epic:** `remote-deployment` · **Stories:** X-82, X-83, X-84

## Why

Everything this platform does can only be seen on `127.0.0.1`. The getting-started page walks a
reader through `cargo run`, a roster handle and a console on `localhost` — and that is the whole
demonstrable surface. Owner-raised 2026-08-02: **deploy this to fly.io so it can be used and tested
fully end to end, remotely.**

Three things stand in the way, and only one of them is a packaging problem.

### 1. A reachable bind needs a real identity provider, and OIDC is the only one wired

`admit_bind` (`crates/exchange-server/src/bind.rs:50`) admits a non-loopback address only for
`IdentityBinding::Bound`. `main.rs` reaches `BoundIdentity::Real` from **one** place —
`AppState::with_oidc` (`main.rs:394`). `AppState::with_identity` exists and nothing calls it.

The development roster gets its own refusal, `ReachableBindWithDevelopmentIdentity`, deliberately
distinct from the no-identity one *because the remedy is the opposite*: there you add a provider,
here you remove the one you have. So `FLUX_EXCHANGE_BIND=0.0.0.0:8080` refuses at startup — a fly
machine that crash-loops — unless a full OIDC configuration is present **including a bound token
exchange**, since `oidc_without_a_token_exchange` also reports not-`Bound`.

**Owner-decided 2026-08-02: stand up a real OIDC provider.** That path needs no Rust change and is
already tested. [[X-58]] — local users from a config file — remains the story that makes a deployment
self-contained, and it is worth landing afterwards so a demonstration does not depend on a third
party being up. It is not on this epic's critical path.

### 2. The console has no production host, and cannot be given one on another origin

`crates/exchange-server` serves no static files at all: `tower_http` appears once, for `TraceLayer`.
There is no `ServeDir`, no embedded assets, no fallback route. The console reaches the API **only**
through the Vite dev-server proxy, which `npm run build` does not emit.

The obvious workaround — host the console somewhere else and point it at the API — **cannot work**,
and it is worth being precise about why, because it looks like a CORS problem and is not:

- `console/src/service.mts` addresses every endpoint as a same-origin relative path (`/api/…`).
- `session::host_cookie` (`session.rs:471`) issues `Path=/; Secure; HttpOnly; SameSite=Strict`
  unconditionally. **`SameSite=Strict` means the browser never attaches the session cookie to a
  request originating from another origin.** No CORS header changes that; the cookie is simply not
  sent. X-15 and X-40 chose `Strict` deliberately and a relaxation is a security decision, not a
  deployment convenience.

So the binary that answers `/api` must also answer `/`. That is [[X-83]], and it is a new capability
rather than configuration.

### 3. Nothing containerises this, and there is no precedent to copy

No `Dockerfile`, no `fly.toml`, and nothing in the family to match — `flux-connectors/providers/fly.toml`
is the fly.io **connector manifest**, a vendor description, not a deploy config. This is the first
deployment any flux-family repository has made, so [[X-84]] sets the precedent the siblings will copy,
the way `flux-connectors/web` set the precedent this repository's site followed.

## Approach

### The topology, and why it is one machine

```
browser ──https──▶ fly edge ──http──▶ machine :8080 ──▶ exchange-server
                                                          ├── /            console (X-83)
                                                          ├── /api/…       the surface
                                                          └── /health      Access::Anonymous
                                                       /data (volume)
                                                          ├── credentials  0700 dir / 0600 file
                                                          ├── agents
                                                          ├── settings
                                                          └── grants
```

**One machine, and this is a property of the store rather than a cost decision.** The credential
store rewrites and fsyncs the whole file under one mutex (X-22), and X-25 pinned an allowance race
that is only closed within a single process. A fly volume is attached to one machine, so two machines
means two independent disks holding two divergent credential stores with no reconciliation. Set the
machine count to one and say why in `fly.toml`, or somebody will scale it to two and lose credentials
silently.

**What a redeploy destroys.** Sessions are `Mutex<HashMap<…>>` in memory (`session.rs:173`) and are
not persisted, so every deploy signs everyone out — acceptable, and it must be *written down* or it
reads as a bug. Agents (`FLUX_EXCHANGE_AGENTS`), credentials, settings and grants are all on disk and
survive, provided the volume is mounted before the process starts.

### Transport, which fly gets right by accident of the design

fly terminates TLS at the edge and forwards plain HTTP. Three things could have broken and do not:

- **The `Secure` cookie** needs the *browser* on https, not the server. The edge provides it.
- **The OIDC URL checks** (`oidc/config.rs`, X-19/X-23/X-27) require `https` or `http` on loopback for
  the issuer, both back-channel URLs **and** the two browser-facing ones. The provider is external
  https; the redirect URI is `https://<app>.fly.dev/api/signin/callback`. Both pass.
- **Nothing reads `Host` or `X-Forwarded-*`**, so the edge rewriting neither is invisible to the app.

The one thing to verify rather than assume is that the app is reached over https *only* — a fly
service that also accepts plain http would serve a login whose cookie the browser then refuses to
store, which presents as "sign-in silently does nothing".

### Secrets

`FLUX_EXCHANGE_OIDC_CLIENT_SECRET` goes in `fly secrets set`, never in `fly.toml`, which is committed.
That is the same rule as `CARGO_REGISTRY_TOKEN`: a credential in a committed file is a credential in
the history. The config module refuses a partial OIDC configuration by name (X-27), so a missing
secret is a startup refusal naming the variable rather than a mysterious 401 later.

### It will boot and do nothing, and that is correct

X-13 landed the grant gate fail-closed and said plainly that it *will look like an outage*. A fresh
deployment answers `503` with no grant store bound and `403 not_granted` with one bound and empty.
The deployment story must therefore ship the operator's first five minutes — the grants file, the
first grant, and what each refusal means — or the first experience of the remote service is a
correctly-working platform that appears broken. This is the single most likely way this epic is
judged a failure.

## Alternatives considered

- **Host the console on GitHub Pages beside the docs site.** Rejected: `SameSite=Strict` (see §2). It
  would look like it nearly worked, which is worse than not trying.
- **Relax the session cookie to `SameSite=Lax` to allow a split origin.** Rejected as a deployment
  convenience overriding a security decision. X-15's whole subject is a session arriving in a browser
  that did not start it.
- **Arm the development roster on fly and accept the risk.** Rejected — `admit_bind` refuses it, and
  the refusal exists precisely because a roster handle is a name anybody can guess. Deleting that
  check to ship a demo is the single worst change available in this repository.
- **Deploy the API only and drive it with `curl`.** Considered and declined by the owner: the brief is
  to use it fully end to end, and the console is where connecting, granting and invoking actually
  happen.

## Risks & open questions

- **This is the first time this service is exposed to the internet.** Every fail-closed gate stops
  being a test fixture and starts being load-bearing. The bind rule, the grant gate, the kind gate on
  connections (X-54), the anonymous-surface guard (X-61) and the runtime gate are the things standing
  between a public URL and a credential store. None of them should be touched in this epic.
- **A public URL is a target.** X-87 subsequently added process-wide sign-in and invocation rate
  bounds plus an invocation concurrency bound. They are application backstops; deployment-edge flood
  protection remains a separate operational layer.
- **The `0600`/`0700` store modes must survive the container.** The store refuses a widened mode rather
  than tightening it (X-09), so a volume mounted with a permissive default, or a process running as a
  different uid than the one that created the files, is a startup refusal. Decide the uid deliberately.
- **`fly.toml` is committed and the site publishes no deployment-specific facts** — the app name and
  URL belong in `fly.toml` and in this design, and must not leak onto a `web/` page, where
  `site.test.mjs` would catch them anyway.

## Acceptance / done

A stranger opens `https://<app>.fly.dev`, signs in through the configured provider, connects a
connector, writes a grant, invokes an operation, and sees the result — with the credential never
leaving the service, and with no step requiring a checkout.
