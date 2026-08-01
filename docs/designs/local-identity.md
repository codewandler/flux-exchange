# Design: signing in without an identity provider

**Status:** accepted · **Epic:** `local-identity` · **Stories:** X-57, X-58, X-59

## Why

**You cannot use this console without standing up an OIDC provider.** That is the whole problem, and
it is worth stating as a product fact rather than a gap in a list: everything shipped through v0.8.0
— connections, credentials, the catalogue, minting an agent, invoking an operation — is reachable
only by a signed-in principal, and the only thing that makes a principal is an authorization-code
flow against a provider somebody configured.

That is a lot of setup to look at a page, and it is standing between this platform and its own
operators.

### Where the wall actually is

Not where it looks. A development identity **already exists** and works: `DevIdentity`
(`crates/exchange-server/src/dev_identity.rs`) mints principals from a roster an operator writes at
startup, `FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme,agent:triage-bot@acme`. It is armed explicitly,
refuses to start on a malformed roster rather than silently dropping an entry, and fixes each
principal's tenant at startup so no request field can reach it. It is good work and it is not the
problem.

The problem is one function:

```rust
// crates/exchange-server/src/state.rs:103
pub fn available(&self) -> bool {
    match self {
        SignIn::Oidc(_) => true,
        SignIn::Unconfigured | SignIn::NoTokenExchange => false,
    }
}
```

**`available()` does not mean "can somebody sign in here". It means "is OIDC configured".** So a
deployment with a perfectly good development identity armed reports sign-in unavailable, the console
hides its sign-in affordance, and the only way in is to present a roster handle as a bearer token by
hand. The console is locked out of a host that would happily let it in.

That conflation is the shared prerequisite for everything below, and it is [[X-57]].

### The second constraint, which is not a bug

`DevIdentity` has **no secret at all** — a roster handle *is* the credential. That is deliberate and
it is what makes it usable without a provider. It is also why arming it forces a **loopback bind**
(`IdentityBinding::Development`, refused on any reachable address).

So the existing mechanism cannot be the answer for anything but "on my own machine". A homelab, a
VM, a container behind a reverse proxy — all reachable, all refused. Extending the loopback rule to
let a secret-free identity listen on a network would be the exact hole `bind.rs` exists to refuse: *a
reachable bind whose authentication is a name anybody can guess is worse than no authentication,
because the surface in front of it believes every caller.*

## The two modes are orthogonal, not alternatives

The request framed these as *either* static users from a config file *or* a local no-multi-tenancy
mode. They are **two different axes**, and saying so is most of the design:

| | **one tenant** | **many tenants** |
|---|---|---|
| **local users** | a laptop, a homelab | a small team, self-hosted |
| **OIDC** | one company's deployment | the multi-tenant product |

- **Authentication** — *how does a request become a principal?* This is what blocks console use.
- **Tenancy** — *how many tenants does this deployment hold?* This is a simplification, not a blocker.

All four cells are legitimate. Building them as one mode would mean a deployment that wanted local
users on a shared host had to also give up tenant separation, which is a security downgrade nobody
asked for.

So: **[[X-58]] is the authentication axis and it is the one that unblocks the console.** [[X-59]] is
the tenancy axis and it is a convenience.

## Approach

### 1. `available()` answers the question it is named for (X-57)

`SignIn` gains a variant for a locally-configured provider, and `available()` becomes *can this
deployment turn a caller into a principal*. The OIDC branch is unchanged.

**This is a change to a published anonymous surface.** `sign_in_available` is a field of
`GET /api/onboarding` (X-42) and of `GET /api/signin/availability` (X-43), and X-42's tests assert
that a host with no identity provider says sign-in is unavailable *rather than pretending*. Those
tests must keep meaning what they mean — "no provider" must still answer `false` — while "a local
provider" starts answering `true`. That is a three-state question being asked as a boolean, and the
right move is to check whether the boolean still suffices rather than to widen it by reflex.

### 2. Static users from a config file (X-58)

A file, not an environment variable, and the difference matters: a roster in an env var is visible in
`ps`, in a container inspect, and in a crash dump. A file has a mode.

**It carries a verifier, never a password.** The credential store's own shape is the precedent — this
host keeps a verifier for an agent token and cannot show the token again. A users file holding
plaintext passwords would be the one place in this repository where a secret is stored to be compared
rather than to be presented, and it would be readable by anything that can read a config file.

**Because it has a real secret, it may bind a reachable address.** That is the whole difference from
`DevIdentity`, and it is why this is a new thing rather than a config-file front-end to the existing
one. `IdentityBinding` gains a state that is neither `Development` nor OIDC-`Bound`.

`DevIdentity` **stays exactly as it is** and is not folded into this. It is the zero-setup path, it
is loopback-only, and collapsing the two would put a secret-free mode one config edit away from a
reachable bind.

### 3. Single-tenant deployment (X-59)

`Deployment::SingleTenant` already exists — `admit_runtime` takes it and `runtime.rs` distinguishes
it from `MultiTenant`. So the concept has a foothold and this story extends it rather than inventing
it.

What it must **not** become is "no tenant". Every credential address is
`tenants/<tenant>/<authority>/<credential>`, and a mode that omitted the segment would write
credentials where nothing else looks for them — the same stranding upstream's instance elision exists
to avoid. Single-tenant means **one tenant, named once, at startup**, and every principal is of it.

The honest gain is that nobody has to invent a tenant id to try the thing, and that a listing does not
show a tenant column that always says the same word.

## Alternatives considered

- **Let `DevIdentity` bind a reachable address.** Rejected, and this is the one to keep rejecting: a
  roster handle is a name anybody can guess, and the surface in front of it believes every caller.
- **Ship a default admin user.** Rejected. A default credential is a published credential; every
  product that has done this has had the CVE.
- **Reuse the agent token store for humans.** Attractive — it already keeps verifiers rather than
  tokens — and rejected for now: minting an agent requires a signed-in human, so bootstrapping is
  circular. Worth revisiting once a local provider can create the first human.
- **Make the console work anonymously in a local mode.** Rejected: the console's surfaces are gated on
  a resolved principal for good reasons, and an anonymous mode would fork every one of those checks.
  The fix is to make signing in cheap, not to make it optional.
- **Do the tenancy mode first.** Rejected: it does not unblock anything. You would still not be able
  to sign in.

## Risks & open questions

- **This widens what can turn a caller into a principal**, on a host that holds credentials. Every
  story here is a security review, not a convenience feature.
- **`sign_in_available` is published anonymously in two places.** Whatever X-57 does must keep the
  X-42 property that a host with no provider says so plainly, and must not let a stranger learn
  *which kind* of provider a deployment runs — `SignIn::available` already collapses three states for
  exactly that reason.
- **A config file is an operator surface and this repository has none.** X-54 is already blocked on
  the absence of an operator-scoped notion; a users file is the first thing that looks like one, and
  the two should be designed knowing about each other.
- **Password handling is a whole discipline.** If X-58 takes passwords it takes a KDF, a work factor,
  and a rehash-on-login story. If that is more than this platform wants to own, the alternative is a
  file of pre-hashed verifiers the operator generates with a shipped subcommand — less friendly, far
  less to get wrong.

## Acceptance / done

An operator can clone this repository, write one config file, start the server on their own network,
open the console in a browser, sign in, wire up a connection and invoke an operation — without an
identity provider, and without any mode in which authentication is a name anybody can guess.
