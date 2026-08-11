---
story: X-03
status: accepted
---

# Binding the Identity port: a development identity, and the session a caller carries

How `Identity` stops being a documented rule and becomes an enforced one. Extends
[`http-surface.md`](http-surface.md), which built the route table and the `require_principal` guard
this story fills in.

The rule everything below answers to is `AGENTS.md` § Invariants, first entry:

> **The tenant comes from the resolved principal and from nothing a caller controls.** Not a path
> segment, not a body field, not a header.

## The development identity lives in the binary, not in the host

**Decision: `DevIdentity` is `crates/exchange-server/src/dev_identity.rs`, and `exchange_host` never
learns it exists.**

`exchange_host` is the crate a product embeds, and it deliberately carries no identity provider —
that is the whole reason the boundary is a trait. A development identity inside it would be one that
every downstream composition links whether it wanted one or not, and the only thing standing between
that and a production hole would be each of those products remembering not to arm it. Security that
depends on every downstream remembering something is not a boundary.

The `CredentialStore` precedent in `lib.rs` does not extend to this, and the difference is stated in
that module's own words: a default a composing binary **can decline** puts no product concern in
shared code. A product cannot decline a hole it did not know it linked.

One thing did move the other way. `exchange_host` now re-exports `async_trait`, because `Identity`
is an `#[async_trait]` trait and an implementor that resolves a *different* version of that macro
gets a `resolve` whose desugared lifetimes do not match, and an error that names lifetimes rather
than the actual problem. A port that cannot be implemented without guessing a dependency version is
not much of a port.

## Arming is opt-in, and the refusal is a bind refusal

**Decision: one environment variable holds a roster; unset binds nothing; armed forces loopback.**

```
FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme,service_account:triage-bot@globex
```

Three properties, in the order they matter:

1. **The tenant is fixed at startup, by the operator, in the roster.** This is why the shape is a
   roster and not "trust what the caller claims". There is no request field that reaches a tenant
   because there is no code path from a request to one — the tenant is parsed by `Tenant::new` when
   the process starts and is thereafter only ever *read* off a resolved `Principal`.

2. **Unset is the default, and the default binds nothing.** That is already the state
   `admit_bind` refuses a reachable bind in, so nothing has to be turned *off* to be safe — only
   turned on to be convenient. A safety property that depends on a setting staying at its default is
   one setting away from gone.

3. **Set-and-wrong refuses to start.** Malformed entry, unknown kind, unusable tenant, duplicate
   handle, and — the one worth calling out — a variable that is *set but empty*. Treating that as
   "unset" would arm nothing while the operator believed they had armed something, which is a
   failure with a silent success mode. Every refusal names the entry, because a handle, a kind and a
   tenant are not secrets and naming them is the fastest route to the fix.

### `IdentityBinding::Development` is a third state, not `Bound`

This is the load-bearing decision of the story, and it is a decision *against* the obvious spelling.

A development identity resolves principals. So it satisfies a bind rule that asks only "can anything
authenticate a caller" — and `admit_bind` asks exactly that. Arming it would therefore have made
`FLUX_EXCHANGE_BIND=0.0.0.0:8080` legal, and the resulting service is **worse than an
unauthenticated one**: a roster handle is a credential with no secret in it, so every caller on the
network becomes any principal by naming it, and everything downstream believes the principal it is
handed. An open door is at least visibly an open door; this is a door with a lock that opens for
anyone who reads the label.

So `IdentityBinding` gained a third variant, and `admit_bind` refuses it on any non-loopback address.
The two reachable-bind refusals are kept distinct because **their remedies are opposite** — one tells
the operator to add an identity provider, the other tells them to remove the one they have — and a
single refusal covering both would tell half its readers to do the wrong thing.

`AppState::with_development_identity` takes `Arc<DevIdentity>` rather than `Arc<dyn Identity>`, so
`Development` cannot be claimed by a port that is not the development one, and cannot be avoided by
one that is. The three constructors are the only way to build an `AppState`, which is what keeps the
port and its binding from drifting apart.

Two levels are tested, because they are two claims and only one of them was pinned at first:

- **The enum** — `admit_bind(_, Development)` refuses a reachable address, in `bind::tests`.
- **The wiring** — `AppState::with_development_identity(…)` → `identity_binding()` → `admit_bind`,
  in `state::tests`. This is the path `main` actually runs, and it is where a future
  "simplification" lands: collapsing `identity_binding` to report `Bound` for the development port
  leaves every test in `bind` green while making `FLUX_EXCHANGE_BIND=0.0.0.0:8080` serve a
  credential-free identity to the network. Pinning the enum is not pinning the wiring.

## There is no anonymous sign-in route

**Decision: every route in the identity module requires a principal, including the one that mints
the session.**

The obvious shape is an anonymous `POST /api/session` that takes a credential and returns a session.
It was not built, because it would have widened the set of routes answering a caller with no
principal, and this module adds nothing to that set. The enumeration test in `routes::tests` makes
every entry an argument somebody had to write down; a *session* route is the last one that should be
appearing there without one.

Instead the caller presents whatever its identity port understands, the guard resolves it, and
`POST /api/session` **exchanges** an already-resolved principal for a session a browser can carry.
Signing in requires already being resolvable. For the development port the presented thing is a
roster handle; for X-04 it will be a federated token.

The route sits under `/api` for a reason that is not aesthetic: the console's dev server owns the
origin under `vite dev` and proxies `/api` to the service, so a session route outside that prefix
would be answered by the SPA fallback rather than by this host — the failure X-07 already hit once.

X-04 genuinely does need anonymous routes — an OIDC `/signin` redirect and a `/callback` cannot
present a credential they have not obtained yet. That story should widen the anonymous set
deliberately, with its own justification, rather than inherit a hole opened here on its behalf.

## One session, two ways to carry it

An agent sets `Authorization: Bearer <token>`; a browser sends the cookie it was given. Both arrive
at `routes::presented` as an opaque string, and the identity port never learns which. **The header
wins when both are present**: an `Authorization` header is something the caller deliberately
attached, a cookie is ambient and attached by the browser on the caller's behalf, and when they
disagree the deliberate one is what was meant.

The port does not learn which, but the *guard* records it, as `routes::Carrier` in the request
extensions. Exactly one route reads it, for the reason in the next section.

### A cookie session cannot be exchanged for a readable token

**Decision: `POST /api/session` mints a token only for a caller that presented a readable
credential.**

> **A session token is returned in the body only to a caller that already held one it could read.
> This route can never turn an unreadable credential into a readable one.**

This is what makes the `HttpOnly` claim below true, and it was **not** true in the first version of
this story — the review caught it, and the shape of the mistake is worth recording because it is
easy to make again.

`HttpOnly` stops script *reading* the cookie. It does not stop script *using* it: same-origin
`fetch` has the browser attach the cookie ambiently, and `SameSite=Strict` does not apply to a
same-origin request. So an XSS could `POST` here carrying a credential it could not read and receive
one it could — and since nothing expires, that token outlives the page, survives sign-out elsewhere
and travels off the machine. The attribute was doing nothing, while the design note claimed it was
doing everything. A control that only appears to exist is worse than an absent one, because it stops
anybody looking.

The fix is a branch, but the thing to keep is the invariant: a cookie-carried caller mints nothing,
because a session cookie *is* already the session and there is nothing to exchange. It is answered
with its principal and no new credential enters the world.
`a_cookie_session_cannot_be_exchanged_for_a_readable_token` drives every method a script could reach
holding only the cookie and asserts nothing token-shaped comes back — matching on the 64-hex *shape*
rather than on a field called `token`, so a rename or a nesting cannot quietly reopen it.
`a_readable_credential_still_mints_a_readable_token` is its counterweight, since "never mint
anything" would satisfy the first assertion and break every agent.

### The cookie

`__Host-flux_exchange_session`, with `Path=/; Secure; HttpOnly; SameSite=Strict`.

- **`Secure`** — never travels in clear text. Browsers treat `http://localhost` as a secure context,
  so this is compatible with the loopback development bind rather than in tension with it.
- **`HttpOnly`** — script cannot read it. On its own that buys less than it appears to; what makes
  the exfiltration claim true is the rule above, not the attribute.
- **`SameSite=Strict`** — not sent on any cross-site request, which is what stops another origin
  spending it. `Strict` and not `Lax` because this surface has no cross-site entry flow to preserve;
  X-04's OIDC redirect is where that question gets asked, and it is the one thing there that may
  need to move.
- The **`__Host-` prefix** is the part a test cannot give you: a browser *refuses* such a cookie
  unless it is `Secure`, has `Path=/` and carries no `Domain`. The attributes become enforced by the
  client rather than only asserted by us, and a sibling subdomain cannot plant a session for this
  host.

### No cookie crate, and why that is not a hand-rolled parser

The workspace carries none, and the two directions sit on opposite sides of the difficulty line.
`Set-Cookie` is only ever **written**, which is string formatting. The `Cookie` *request* header is
parsed, and its entire grammar is `cookie-pair *( ";" SP cookie-pair )` — no attributes, no dates, no
quoting rules, because attributes travel the other way. `session::from_cookie_header` implements that
grammar **completely** rather than approximately, in four lines with its own tests. A dependency was
not refused to save a dependency; it was not needed.

### The token

32 bytes from `/dev/urandom`, hex encoded. If the entropy source is unreadable the request refuses —
there is no weaker fallback worth having, since a token from a predictable source is a session anyone
can guess, and a host that quietly downgraded to one would look exactly like a host that had not.

`SessionToken` does not implement `Display` and its `Debug` redacts. It is a bearer credential, and
one in a log line is a session anyone reading the log can use.

## Addendum, X-16 — sessions expire, where there is something to expire with

The first bullet below is superseded and is kept as the reasoning of the time. `SessionStore::open`
now takes an `Expiry` that every caller must name:

- **`Expiry::Credential { expires_at }`** — the OIDC port, carrying the id token's `exp` verbatim.
  The session ends when the identity behind it does, and an expired entry is *removed* rather than
  left unresolvable, so it cannot occupy the bound described two bullets down. An `exp` already past
  or further out than thirty days refuses rather than being clamped.
- **`Expiry::WhileTheProcessLives`** — the development port, and unchanged behaviour. What is
  presented there is a roster handle: a name with no secret and no expiry in it, so there is nothing
  to bind a session's end to, and any lifetime named would be one this host invented. That is the
  same repair the OIDC side refuses. Arming this port already forces a loopback bind, which is where
  the protection actually is.

The cookie still carries no `Max-Age`, and the argument in the **first** bullet below still holds —
a second copy of the deadline, in a place this host cannot correct, buys nothing now that the server
enforces the first.

## What this deliberately does not do

- **Sessions do not expire.** One lives until it is closed or until the process does, and the cookie
  is a session cookie for the same reason. An expiry the browser honours but the server does not is a
  lie that reads as a security control. Binding a session's lifetime to the credential that opened it
  belongs in X-04, where there is an id token with an `exp` to bind to.
- **Sessions do not survive a restart.** They are in a `Mutex<HashMap>` in the development port. A
  shared store is a real design question — it decides whether this service can run more than one
  replica — and it should be answered when there is a real provider to answer it for.
- **The store is bounded at 4096 and refuses at the bound**, rather than evicting. Since nothing
  expires, an unbounded map only grows; and evicting the oldest would sign out a caller who did
  nothing wrong, who could not tell that from a bug. Reaching the bound means something is looping
  or nothing is closing, and saying so is the useful behaviour.
- **Token lookup is a hash lookup, not a constant-time comparison.** Deliberate: timing helps an
  attacker only if it narrows a search, and 256 bits from the OS leaves no prefix to walk and no
  shorter guess to confirm. This is the thing to revisit if tokens ever stop being random.
- **No `WWW-Authenticate` challenge** on the `401`, unchanged from X-02.
- **The 401/503 split is asserted end to end**, not just at the port:
  `a_rejected_credential_and_an_unreachable_provider_are_distinguishable` drives both through the
  assembled app and also checks the `503` does not leak the provider's address to the caller. That
  reason names this host's own dependencies, so it goes to the log.

## How the invariant is tested

The Acceptance asks for the tenant rule to be asserted three times, once per vector. Each test
authenticates as a principal armed into tenant `acme` while claiming `attacker` through one vector:

| Vector | Test | What it asserts |
| --- | --- | --- |
| Path segment | `a_tenant_in_a_path_segment_does_not_influence_the_tenant_used` | Against a route that genuinely declares `/{tenant}`, so the claim is delivered and readable — the handler echoes the segment it received *and* reports `acme`. Not "the segment was dropped" but "with the segment in hand, the principal still decides". |
| Body field | `a_tenant_in_a_body_field_does_not_influence_the_tenant_used` | `POST /api/session` with `{"tenant":"attacker"}` mints a session for `acme`, and `attacker` appears nowhere in the answer. |
| Header | `a_tenant_in_a_header_does_not_influence_the_tenant_used` | Three header spellings, because the rule is about the class of vector and not one name a future reverse proxy might introduce. |

Two structural tests back them, and these are the ones that cover stories not yet written:

- `no_published_route_takes_a_tenant_in_its_path` walks `published()` and fails if any route ever
  declares a tenant-ish path parameter. X-10's "no route accepts an address" inherits it.
- `the_surface_publishes_a_route_that_requires_a_principal` stops the enumeration test from going
  vacuous — a surface on which *every* route is anonymous satisfies "the anonymous set is what was
  declared" as happily as one where the guard works.
