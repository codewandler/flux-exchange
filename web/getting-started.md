# Run it yourself

flux-exchange holds other people's credentials, so it does not start in a state where anybody can use
it. This is the shortest path from a clone to a signed-in console **on your own machine**, and it
names the two things that would otherwise stop you: where the service is allowed to listen, and what
has to be true before it will run anything at all.

> [!IMPORTANT]
> This page describes the software, not a deployment, and it makes no claim about which capabilities
> the build you clone serves. That answer is `GET /api/onboarding` — anonymous, machine-readable, and
> held to the service's own route table by a test in both directions. Where this page and the service
> disagree, the service is right.

## Start it

A Rust toolchain at or above the workspace's `rust-version`, and nothing else. No identity provider,
no container, no account anywhere.

The constraint below is inside the block because the block is what gets copied.

```sh
git clone https://github.com/codewandler/flux-exchange
cd flux-exchange

# ---- On your own machine. This is not how you deploy it. ----------------------------------
# The development identity resolves a *roster handle*: a name, with no secret in it. That is
# what makes this zero-setup, and it is exactly why flux-exchange refuses to start on any
# address but loopback for as long as one is armed — a reachable bind whose authentication is
# a name anybody can guess is worse than no authentication, because the surface in front of it
# believes every caller. It is a refusal and not a warning: no flag relaxes it.
#
# Cargo forwards everything after its `--` to the binary. This declares one tenant, `dev`, and
# one human from the startup environment: user:${USER}@dev. It also binds the complete development
# store set below the per-user state root; no store variable is required for this mode.
cargo run -- --dev
```

Development defaults are persistent rather than an in-memory fallback. An explicitly configured
store still wins, and an empty or unsafe override refuses instead of silently dropping that store.
For example, a path directly below a shared temporary directory is unsafe; placing it below a newly
created owner-only directory preserves the shared ancestor and is accepted. Every development store
left unset continues to use its default. If an explicit development roster is also present, it may
replace the implied identity, but the actual `--dev` process argument still selects this complete
development store composition.

The resulting bearer handle is the value of `$USER`. If local work needs a deliberately named tenant,
several principals, or several tenants, leave off `--dev` and arm the existing roster explicitly:

```sh
# This also carries a secretless development identity and is loopback-only. It is not how you deploy
# the service, and no flag relaxes that constraint.
export FLUX_EXCHANGE_DEV_IDENTITY="<kind:id@tenant>"
cargo run
```

That second command is configured non-development startup, not another spelling of `--dev`. Its
persistent store set is all-or-nothing: an incomplete explicit set refuses and names every missing
sibling. This prevents a stale environment override from producing a process with only some of its
state bound.

One entry is `kind:id@tenant`: `user`, `agent` or `service`; an id you choose; and the tenant that
principal is of, fixed at startup where no request field can reach it. Paste the placeholder as it
stands and the process **refuses to start**, naming the entry it could not read and the kinds it
accepts. It does not skip the bad entry and arm the rest: a roster that silently lost a principal is
a roster whose operator is debugging the wrong thing.

Then read the startup log once. It is the operator's channel, and it is more current than this page:

- the address it bound — loopback unless `FLUX_EXCHANGE_BIND` says otherwise, and refused outright if
  what that says is reachable;
- a warning, at `warn` deliberately, that a development identity is armed and that any caller
  presenting one of those handles becomes that principal;
- every route it publishes, and whether that route needs a principal and of which kind;
- every store that is **not** bound and what will refuse because of it. Under `--dev` the complete
  default set is bound; in configured non-development startup, a partial set refuses before this
  log. Nothing falls back to memory.

## Sign in

`GET /api/signin` is the authority on this. It answers for the host in front of you rather than for
the software in general, and **this section restates it for reading order — where the two disagree,
the route is right.** In its own words: present the handle of a rostered principal as a bearer token,
and `POST /api/session` exchanges it for a session cookie.

```sh
# The startup log's last line names the address it bound. This page names none: the site
# describes the software, never an instance.
exchange="<the address the log named>"

# What this host says about signing in. Started as above, it explains the mechanism rather than
# redirecting anywhere — and it names no roster entry, for the reason this page does not either.
curl "$exchange/api/signin"

# The exchange itself. With `--dev`, <handle> is the value of $USER. Loopback only, for the reason
# in the first block.
curl -X POST "$exchange/api/session" \
  -H 'Authorization: Bearer <handle>' \
  -c cookies

# Who this host resolved you to be: a kind, an id, and the tenant it took from the roster.
curl -b cookies "$exchange/api/session"
```

The cookie comes back `HttpOnly`, `Secure`, `SameSite=Strict` and `__Host-` prefixed — and that last
one is why the rest are not merely promises: a browser refuses to store a `__Host-` cookie unless it
is secure, is scoped to the whole origin and names no domain, so the client enforces them too. Two
further properties are worth noticing, because they are the difference between a session and a
credential:

- **`POST /api/session` reads no field of the request.** There is nothing a caller could say about who
  it is: the principal was resolved before the handler ran, and its tenant came from the roster.
- **A caller that authenticates with the cookie gets no token back.** A session a script cannot read
  must never be exchangeable for one it can, or `HttpOnly` would be decorative.

## Reach the console

```sh
cd console
npm install
npm run dev
```

The console is a separate Node build that shares nothing with the Cargo workspace, and its dev server
prints where it is serving. It proxies `/api` to the service, which is a separate process: in a
deployment the console is served by the same host that answers those routes, and a dev server is the
one context where that is false — so it proxies to wherever `FLUX_EXCHANGE_BIND` says the service
listens, and to the same default the service uses when that says nothing. Moving the bind therefore
moves the console with it. The setting is read once, when the dev server starts, so export it in the
shell you run `npm run dev` from and restart that server if you change it.

You will arrive **signed out**. A browser cannot put an `Authorization` header on a navigation, and a
sign-in form that would take a handle is a tracked change (story X-58). Until it lands, the browser
makes the same exchange once, from its own devtools console, on the page the dev server is serving:

```js
// Same origin as the page, so the dev server proxies it and the session cookie that comes back
// is one this browser carries from here on. Loopback only — see the first block.
await fetch('/api/session', {
  method: 'POST',
  headers: { Authorization: 'Bearer <handle>' },
}).then((response) => response.json())
```

Reload. The shell names the principal and the tenant where it offered a sign-in link before — the
tenant too, because every credential address on the page is derived from it.

What you can do from there is the console's own answer rather than this page's: it names its
surfaces, marks the ones it has no screen behind, and reads what this tenant holds from the service.
The one thing it will not tell you is why an invocation refuses, which is the next section.

## Before anything will run

> [!WARNING]
> **Being signed in is not being allowed to do anything.** This is the step whose absence looks like
> an outage, and skipping it is how a first run ends in a refusal with nothing to act on.

An invocation is admitted by a **grant**: a selection over what an operation *declares* — its risk,
its effects, its idempotency — and never a list of operation ids, so a grant covers a connector's next
operation correctly on the day it lands. A tenant starts holding none. That is the safe state you
reach by doing nothing, rather than a default somebody has to remember to turn off.

In the order you will meet them:

- **Neither store bound.** `FLUX_EXCHANGE_GRANTS` names where a decision about what a tenant may run
  lives; `FLUX_EXCHANGE_CREDENTIALS` names what operations run with. Without both there is no invoker
  at all, and the grant surface and the invoke route each refuse with `503`, naming both settings.
  That is why both are in the first block.
- **Both bound, the tenant holding nothing.** An invocation refuses with `403 not_granted`. The
  refusal names the operation and *not* the axis that turned it down: an agent that learned which
  predicate refused it could enumerate a tenant's policy one call at a time.
- **A grant that admits it, and no credential stored.** The refusal moves on rather than disappearing
  — it names the address the credential was looked for at, says the request was not sent, and points
  at where to supply it.

`GET /api/catalogue/connectors` is anonymous, so you can read what this build carries before signing
in at all; `/api/catalogue/connectors/{id}/operations` gives each operation's risk, effects and
idempotency, which are the three axes a selector speaks. A grant is then written per tenant and
whole-set — what a tenant may do, entire, because a revoke beside a grant is a sequence nobody can see
the end state of:

```sh
# What a proposed grant would admit, decided but not stored. The answer lists the operations it
# covers, derived by the same function the gate decides with rather than by a second copy of it.
curl -X POST "$exchange/api/grants/preview" -b cookies \
  -H 'content-type: application/json' \
  -d '{"connector":"<a connector id from the catalogue>","selector":{"max_risk":"low"}}'

# Save the set. `GET` on the same path reads back what the tenant holds, with what each admits.
curl -X PUT "$exchange/api/grants" -b cookies \
  -H 'content-type: application/json' \
  -d '{"grants":[{"connector":"<the same connector id>","selector":{"max_risk":"low"}}]}'
```

The three grant routes — read, replace, preview — answer a signed-in **human** and nothing else, the
read included: whoever may edit a grant decides what the tenant runs, which is strictly more authority
than supplying a credential, and a read open to every kind would hand a tenant's whole policy to an
agent in one request.

## What this path is not

- **It is not a deployment.** A host other machines can reach needs an identity whose credential has a
  secret in it, and the development identity is deliberately not one — which is why the bind rule
  refuses that combination outright instead of warning about it. Local users with a real verifier are
  a tracked change (story X-58).
- **It is not a security model a setting widens.** Every refusal above is a refusal: nothing here
  warns and serves anyway, and nothing falls back to memory when a store is missing.
- **It is not the whole picture.** What each word here means is [the surface](/surface); why a caller
  names an operation and never a credential is [the credential boundary](/boundary); what a given
  build actually serves is that build's own `GET /api/onboarding`.
