---
story: X-04
status: accepted
---

# OIDC sign-in: the authorization-code flow, and the half of it this workspace cannot carry

How a human proves who they are to this host. Extends
[`identity-and-session.md`](identity-and-session.md), which built the `Identity` port, the session a
caller carries, and the `Carrier` rule this story has to stay inside.

The sentence everything here answers to is `docs/vision.md`'s, and it cuts in an unusual direction
for a sign-in story:

> **The credential never crosses the boundary; the authority does.**

## Signing in is not connecting

**Decision: sign-in asks for `openid`, `email` and `profile`, and for nothing else, ever.**

This is the first decision because it is the one that would be cheapest to get wrong and most
expensive to undo. `SCOPES` is a single constant with the argument written beside it, and
`the_authorization_request_carries_pkce_and_asks_only_to_identify_the_human` asserts the exact
string that reaches the provider.

The temptation is real: the same provider that authenticates a human often *also* holds the vendor
API this host will eventually call, and one consent screen is cheaper than two. It is also the
moment this service would stop being what the vision says it is. A user who clicked "sign in with
Acme" agreed to be identified. Reading their mail is a different thing to agree to, and bundling it
into the sign-in scope means nobody ever agreed to it separately — the consent screen said "sign
in", and the authority granted was something else. Connecting a provider is its own flow, with its
own screen, and X-08's epic is where it belongs.

## What this story could not build, and why that is the shape it is

**Decision: the token exchange is a port. This binary binds none, and says so at startup.**

Completing an authorization-code flow needs two things this workspace does not have:

1. **An HTTP client**, to `POST` the code to the provider's token endpoint over TLS.
2. **A JOSE library**, to fetch the provider's JWKS and verify the id token's signature.

Adding either was outside this story's fence. The obvious workaround — write the JWT verification
here — is the one thing that must not happen, and it is worth being precise about why rather than
treating it as taste. A hand-written JWT verifier is the most reliably broken artefact in this
problem space: `alg: none`, HMAC-verified-against-an-RSA-public-key, key selection driven by an
attacker-supplied `kid`, unchecked `crit`. Every one of those produces a host that accepts a token
anybody can mint, **while looking exactly like a host that does not**. That failure has no symptom
until it is found from outside, which is the same property `identity-and-session.md` identified in a
decorative `HttpOnly` and rejected for the same reason.

So `TokenExchange` is a trait. It takes a code and a verifier and returns claims **whose signature
it has verified**; a composition that carries `reqwest` and `jsonwebtoken` binds one.

### The split is not arbitrary: binding checks stayed here

What crosses the seam is signature verification — the part that needs cryptography and the network.
What stayed on this side is every *binding* check: issuer, audience, expiry, nonce, subject. Those
need neither, and `Oidc::admit` is a pure function of `(claims, expected_nonce, now)` with a test
per check.

That matters more than it looks. If the binding checks lived behind the port, every composition
would re-implement them, and a composition that got `nonce` wrong would be a replay hole nobody
here could see. Keeping them on this side means they are written once, tested once, and a
composition can only get the *cryptography* wrong — which is the part it took a real library for.

### What this binary therefore does

`AppState::oidc_without_a_token_exchange()`. Configuration is read, validated and reported; the
routes are published; `/api/signin` serves an explanatory page. It does **not** redirect a browser
to a provider it could never return from usefully, because the Acceptance names that exact failure
— a login that looks fine and dies at the callback — as the thing not to ship.

Note what this composition deliberately does *not* claim. `oidc_without_a_token_exchange` reports
`IdentityBinding::Unbound`, not `Bound`. It would have been easy to let "OIDC is configured" satisfy
the bind rule, and it would have been wrong in exactly the way `IdentityBinding::Development` was
wrong: `admit_bind` asks whether anything *could* resolve a caller, and here nothing can. A
`0.0.0.0` bind in front of a host where no sign-in can complete is not a hole, but reporting `Bound`
would be a lie, and the next story to touch this would inherit it as a fact.

`AppState::with_oidc` — the constructor that does report `Bound` — takes the identity port and the
sign-in flow as **one argument**, because they are one object. The callback opens a session in it
and the guard resolves that session out of it; a composition able to set one without the other is
one where a completed sign-in resolves to nothing.

## The callback answers a caller with no credential. Why that is not X-03's hole reopened

This is the subtle one, and it was flagged before implementation started.

X-03 closed an escalation: `POST /api/session` mints a *readable* token only for a caller that
presented a readable credential, so same-origin script holding only the `HttpOnly` cookie cannot
trade it for one it can read and exfiltrate. The OIDC callback mints a session for a caller
presenting **no** credential at all — which is the same door approached from outside, and is
correct, because that caller authenticated at the IdP rather than to us.

Correct, but not by inheritance. Two structural properties make it safe, and neither is a branch
anyone has to remember:

1. **The callback reads no credential.** It never touches `Carrier`, never resolves a principal,
   never consults the session cookie. Holding one neither helps nor is required. Its authority comes
   from exactly one place: a `state` this host drew from `/dev/urandom`, remembered, and has not yet
   spent. There is no input a cookie-holding script has that a cookie-less one does not.
2. **It answers with a document, not a body.** The response is HTML plus `Set-Cookie`. No JSON, no
   field, no place a readable token could appear. A script that somehow drove a complete IdP
   round-trip comes away holding a cookie it cannot read — the same credential class it started
   with, and nothing escalated.

So X-03's rule is restated once wider, and this module is inside it:

> **No route reachable without a readable credential ever puts a session token in a body.**

`the_callback_issues_a_session_only_as_a_cookie` drives the **successful** path deliberately. The
refusals issue nothing at all and could never fail such an assertion; the case worth testing is the
one where a session really is created.

## `state`, and the test this story was written around

**Decision: `state` is 256 bits from the OS, bound server-side, single-use, and expires.**

The Acceptance asked for a failing-first test, and the honest way to produce one was to write the
flow *without* the state check and watch it break. The first commit on this branch is exactly that,
and the failure it produced is the point:

```
assertion `left == right` failed: a callback this host did not open must be refused:
  <h1>Signed in</h1>
  left: 200
 right: 400
```

A forged callback was answered **"Signed in"**. That is the attack: an attacker who cannot read the
victim's `state` walks the victim's browser into the callback carrying the attacker's own
authorization code, and the victim silently acquires a session belonging to the attacker's account.
Everything the victim then does, they do in the attacker's tenant, and it looks to them like their
own session.

The test is built so it cannot pass for the wrong reason. A refusal test on a route that 404s passes
vacuously, so this one first drives a **real sign-in through the same app** and asserts it succeeds;
the stub provider echoes the nonce this host actually bound, so every other check passes and the
state check is the only thing left between the forged callback and a session. It also asserts the
genuine `state` is still unspent afterwards — a forged callback that consumed it would be a denial
of service against the human who started the real sign-in.

Single-use is `take`, which removes. A replay of a callback that already succeeded gets the same
answer a forgery gets, because from outside they are the same event and neither should yield a
second session.

### What `state` does not solve

Worth recording, because it would be easy to read the test above as more than it is. `state` bound
*server-side* stops a callback this host never issued. It does not stop an attacker who legitimately
starts a sign-in **here**, authenticates at the IdP as themselves, and then walks a victim into the
callback with that genuinely-bound state — the classic OAuth login-CSRF. Closing that needs the
state tied to the victim's *browser* as well, which means a second cookie carried across the
redirect. That is a real gap, it is out of this story's Acceptance, and it should be its own story
rather than something quietly assumed handled.

### The pending store evicts where the session store refuses

**Decision: at `MAX_PENDING`, drop the oldest authorization request rather than turn the new one
away.** This is the opposite of `SessionStore`, and the divergence is deliberate.

`identity-and-session.md` argued for refusal, and the argument was good *there*: a session is minted
behind a principal, so filling that store takes an authenticated caller looping, refusing says
exactly that, and evicting would sign out somebody who did nothing wrong and could not tell it from
a bug. None of that transfers. This store sits behind `GET /api/signin`, which is **anonymous**. At
1024 entries with a ten-minute TTL, refusing at the bound means any unauthenticated caller can lock
every real user out of signing in for ten minutes, for the cost of 1024 requests.

The original code justified the bound by noting that filling it needs requests "far faster than a
human could". That describes an attacker; it is not a reason one will not turn up.

Eviction costs the evicted sign-in one click, and it fails *loudly*, at the callback, with an
actionable message. A pending authorization is not a credential anybody holds and carries no
invariant worth preserving — at most ten minutes of intent. Expired entries are swept first, so a
live request is only ever discarded when the store is genuinely full of live ones.

This is not "repair" in place of "refusal". Nothing weaker is substituted, nothing fails silently,
and memory is still bounded. What changed is who pays when the bound is reached, and it should not
be the honest user. What it does **not** do is make `/api/signin` cheap to abuse in general —
per-IP accounting or a cost on the route is a real question, and a separate one.

## `nonce` is checked, not merely requested

A missing `nonce` refuses. `admit` compares `claims.nonce.as_deref() != Some(expected)`, so `None`
fails — and `every_binding_check_refuses_on_its_own` has "no nonce at all" as its own case, because
the easy bug here is `if let Some(n) = claims.nonce { check }`, which requests a nonce, appears to
check one, and accepts a token that omits it.

## PKCE, and the one piece of cryptography in this repository

**Decision: `S256`, with SHA-256 written out, pinned to RFC 7636's worked example.**

PKCE was not optional. The authorization code comes back through the browser, in a URL — visible to
history, proxies, referrers, and anything else registered for that redirect. PKCE makes the code
useless alone. RFC 9700 asks for it even from a confidential client, because the client secret and
the verifier answer different questions: the secret keeps a stranger off the token endpoint, PKCE
keeps *this code* from being replayed by whatever else saw the redirect.

That left the hash. Three options, and only one honest:

1. Use `plain`, where the challenge *is* the verifier — so anything that read the authorization
   request has the secret. RFC 7636 §4.2 forbids it for a client that can do better, and shipping it
   would be shipping a control that only appears to exist.
2. Skip PKCE. Same objection.
3. Write SHA-256, and pin it.

This is (3), in `oidc/sha256.rs`, and it is deliberately the only cryptography in the repository.
It is worth being precise about why this is a different judgment from the JWT one above, since both
are "don't write crypto":

- **SHA-256 is a deterministic function with published vectors.** "Is this correct" is a closed
  question, and the tests close it: FIPS 180-4's examples, both padding edges, the million-`a`
  vector, and — the one that matters most — RFC 7636 Appendix A, which pins the entire
  verifier → challenge chain exactly as a provider recomputes it. Every expectation was produced by
  an independent implementation, not written from memory.
- **It fails closed.** A wrong digest produces a challenge the provider rejects: a loudly broken
  sign-in, not a quietly weakened one. A wrong signature verifier produces a host that accepts
  forged tokens.
- **There is no key material and no attacker-chosen input.** It hashes a 43-character verifier this
  host drew itself.

**Replace it with `sha2` the moment a dependency can be added.** `digest` is the whole surface.

## One entropy source

`state`, `nonce`, the PKCE verifier and the session token are four names for "a value an attacker
cannot predict", and they now share `entropy.rs` — one read of `/dev/urandom`, refusing rather than
falling back. Previously `session.rs` owned that read privately; a second copy in the OIDC module is
how one of the four quietly becomes the weak one.

## The tenant comes from the configuration

`AGENTS.md` § Invariants, first entry: *the tenant comes from the resolved principal and from
nothing a caller controls.* Federation adds a second thing to worry about — the provider's claims —
so this story answers both.

**Decision: `FLUX_EXCHANGE_OIDC_TENANT`, fixed by the operator at startup.** The same shape as the
development roster, and for the same reason: there is no code path from a request *or* a claim to a
tenant, because the tenant is parsed by `Tenant::new` when the process starts and thereafter only
read off a resolved `Principal`.

Mapping the tenant from a claim was the alternative. It is better than caller-controlled, since the
provider signs it — but at a provider where users edit their own profile, some claims are
caller-controlled after all, and telling which is which is per-provider knowledge this host does not
have. `neither_a_request_nor_a_claim_influences_the_tenant` drives a callback carrying a hostile
query parameter *and* a hostile `email` domain, and asserts the resolved principal is in the
configured tenant.

The cost is honest: one configured provider federates one tenant. Serving several from one provider
decides how a claim is mapped and who is trusted to assert it, and deserves its own story rather
than a default chosen here.

The principal's id is **`sub`**, not `email`. An address can be changed, released and re-registered
to somebody else; `sub` is the provider's stable identifier. A principal id that can be reassigned
is one that eventually names the wrong human.

## Missing configuration does not stop the process

**Decision: a startup message naming every unset variable, and an explanatory page. Not a panic.**

This is the one refusal in the binary that is *not* a `StartupRefusal`, and the distinction is the
point. A reachable bind with no identity provider is a hole, so the process must not start.
Unconfigured OIDC is an **absent feature**: `/health` still answers and the catalogue still serves,
and exiting would take those down to punish an operator who has not set up federation yet.

Three details:

- **Every unset variable, in one message.** An operator fixing six variables one restart at a time
  is an operator we made do six restarts.
- **Set-but-empty is unset.** Naming `FLUX_EXCHANGE_OIDC_CLIENT_SECRET` and leaving it blank is a
  mistake with a silent success mode — the same reasoning `DevIdentityRefusal::EmptyRoster` records.
- **"Nothing configured" and "half configured" read differently**, because one is a deployment that
  has not enabled sign-in and the other is a mistake, and an operator does different things about
  them.

The **page** does not enumerate the variables. That is the startup log's job, and the log is the
operator's channel; this page answers anonymous callers, and a list of the settings a host expects
is a small map of it — the same line `routes::catalogue` and X-03's `503` already hold. The
constant `WITHHELD_FROM_THE_PAGE` keeps that decision referenced, so renaming a variable is a
compile error here and the choice gets re-read rather than rotting.

## The client secret

`ClientSecret` has no constructor outside `config.rs`: the environment, through `from_env`, and
nowhere else. Not a request, not a query parameter, not a file, not a field in another config. Its
`Debug` redacts, there is no `Display`, and the value leaves only through `expose`, which is
greppable so a reviewer can enumerate every disclosure.

The redaction test asserts through the **whole config**, not the secret alone, because that is how a
secret actually reaches a log: somebody adds the configuration to a `tracing` call and the derived
`Debug` walks into the field.

## Why the callback answers a page rather than a redirect

The session cookie is `SameSite=Strict`. `identity-and-session.md` flagged the OIDC redirect as the
one thing that might have to move, and it did not have to.

A `303` from the callback would be the tail of a redirect chain that began at the provider's origin,
and a `Strict` cookie is withheld on a request whose chain started cross-site: the browser would
store the session and then not send it, and the operator would see a sign-in that silently did
nothing. A small page with a meta-refresh makes the next navigation one *this document* initiated —
same-site — so the cookie travels. The alternative was `SameSite=Lax`, which would have widened a
control doing real work in order to save a page.

## What this deliberately does not do

- **No token exchange, no JWKS, no discovery.** The three things that need dependencies. Discovery
  is why the authorization endpoint is configured rather than read from
  `/.well-known/openid-configuration`.
- **Sessions still do not expire.** X-03 left this to X-04 on the grounds that an id token has an
  `exp` to bind to. It is not done: with no token exchange there is no id token in this build to
  bind to, and building the binding against claims no composition can yet produce would be untested
  machinery. It belongs with the story that binds a real exchange.
- **No `state` bound to the browser**, so the login-CSRF above is open. Its own story.
- **No refresh, no sign-out at the provider, no back-channel logout.**
- **One tenant per provider.**
