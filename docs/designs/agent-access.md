---
epic: agent-access
status: accepted
---

# Design: the primary caller can authenticate

`docs/vision.md` opens with a claim this service does not yet honour:

> **Its primary caller is an agent, not a human.** People sign in to wire things up and to see what
> happened; agents are what call operations all day.

Today there are exactly two ways to become a principal: **OIDC sign-in**, which `oidc/mod.rs` says in
so many words is "a human at a browser", and the **development identity**, which is a roster of
handles carrying no secret and which forces a loopback bind. `PrincipalKind::Agent` exists as a type,
is constructible, prints correctly, and appears in the dev roster — and **nothing anywhere mints or
verifies an agent's token.**

So the platform's stated primary caller can authenticate only on loopback, in a mode that exists
because it is unsafe to expose. That is the gap this epic closes.

## Why now, and why this is not blocked

Almost everything else downstream of the vision waits on X-11 — `connector-pack` pins
`flux-runtime ^0.41` while flux is at 0.45, so `invoke`, and therefore grants-gating-invoke, and
therefore execution records, cannot link. **Agent access needs none of it.** It is principals, tokens
and tenancy: entirely inside this repository's own domain row —

> **flux-exchange** — *does it require holding a credential or knowing a tenant?*

— and it is the prerequisite for the half of the vision that is currently unreachable.

## What an agent token is, and what it is not

**It is a bearer credential this host mints, holds the verifier for, and can revoke.** It is not an
OIDC token, not a session, and not a vendor credential.

The vision already states the property that makes it safe, and it is the one to hold on to:

> **An agent's token grants access to an operation, never to a credential.** A stolen agent token
> yields a bounded operation set against one tenant's connections — never a vendor secret.

Three consequences that decide the design:

- **A token names a principal, and that principal names a tenant.** The tenant is read from the token
  the host issued, never from anything the caller sends — the same rule the whole surface already
  obeys. `routes::identity`'s vector tests exist to keep that true and must cover this path too.
- **The token is a credential, so it follows this repository's credential discipline**: drawn from
  the one entropy path, redacted in `Debug`, never in a log line, never echoed in a refusal. The
  precedent is `SessionToken` and `Binder`, and there is no reason to invent a third shape.
- **This host stores a verifier, not the token.** A session token can live in memory because a
  session is short and this host mints it for one browser. An agent token is long-lived and an
  operator will paste it into a config, so the store is the thing an attacker reads if they read
  anything — and it must not yield a usable token.

## The three lifetimes, applied

`vision.md` distinguishes session, channel and lease. **An agent token is none of them** — it is
closer to a credential than to a lifetime, and conflating it with a session is the mistake this
design most wants to avoid:

| | Scope | Dies when |
|---|---|---|
| Session | a conversation | closed, or its identity expires (X-16) |
| **Agent token** | **a principal** | **revoked, or its stated expiry passes** |

A session ends when the human's identity does. An agent token outlives every session and is killed by
an operator, not by a clock running out on someone's sign-in. They must not share a store or a type.

## Shape

Three stories, in order, because each needs the one before it:

1. **Mint** — an authenticated human creates an agent principal for their tenant and receives a token
   **once**. The host keeps a verifier. Shown once is the whole point: a token this host can display
   twice is a token this host is storing.
2. **Authenticate** — a request presenting an agent token resolves to that agent principal, through
   the existing `Identity` port, so every route that already requires a principal works unchanged.
   This is where the tenant-derivation vectors must be re-run for the new path.
3. **Revoke and list** — an operator can see which agents exist and kill one. Without this, minting is
   a one-way door, and a leaked token has no remedy.

## Who may mint, which is not a grant question (X-40)

X-36 built minting and reported the hole in it: nothing gated minting by `PrincipalKind`, so once
**Authenticate** lands and an agent's own token resolves, a leaked agent token mints successor
agents. The damage is not one extra agent — it is that **revocation stops being a remedy,
invisibly**. **Revoke** exists so a leaked token has an answer, and a token that mints successors
makes that answer incomplete in a way an operator cannot see: the descendants are ordinary agents
with no recorded relationship to the one that was revoked, so an operator who revokes the leaked
token, watches it stop resolving and closes the incident is wrong and has no way to find out.

**So `POST /api/agents` admits a `User` and nothing else**, declared on the route as
`Access::PrincipalOfKind` and enforced again at `AgentStore::mint`, because the store is the thing
that creates a principal.

**`Service` is refused too, and that is a decision rather than an omission.** A `Service` is another
backend acting on behalf of one of its own accounts and actors — the caller a programmatic
provisioning story would reach for — so refusing it costs something real. It is worth it: the
property this gate defends is that revoking a token ends the access it gave, and that holds only if
every minter is itself revocable by this host's operator. A `User` is, because sign-in is federated
and the account behind it is disabled at the provider. A `Service` is not: nothing in this repository
mints, verifies, lists or revokes a service credential. Admitting it would put the same defect one
level up and one level further out of sight, where there is not even a revoke route to be incomplete.
The story that wants a service to mint is the story that gives a service a revocation path.

**Why this does not wait for X-13.** Grants answer *what may this principal do* and need the grant
model, which is blocked upstream. This asks *what kind of principal is calling*, which this host
knows today from the credential it issued — no grant, no connector metadata, no policy. It is
authentication-shaped, and deferring it would ship a revocation mechanism that does not revoke.

**How far it reaches.** ⚠ *"Only here" was true when this was written and is not now — X-47 gated the
connection-settings write and X-54 gated credential supply and rotation, all to `User`. Four routes
are kind-gated; `routes::KIND_GATED` is the enforced list.* The argument below still stands and is
what this section is for. `DELETE /api/connections/{connector}` stays open to every
kind: it destroys tenant data inside the tenant the caller already belongs to, an operator can see it
and undo it by reconnecting, and nothing about it survives revocation of the token that did it.
Whether an agent should reach a destructive route at all is a real question — and it is the
grant-shaped one, so it belongs to X-13 rather than to a widened route table.

## What this epic deliberately does not do

- **It no longer leaves invocation ungated.** This bullet used to say *"it does not gate anything by
  grant — that is X-13, blocked upstream"*, and before that it said the only exception to what an
  agent may do was creating a principal. **X-13 has landed**, and both sentences are superseded. What
  is true now:

  An agent token authorises what any principal **of its tenant** may do, bounded twice over —
  by the **grants that tenant holds**, which `Invoker::invoke` consults before any credential is
  read; and by **four things this host decides by *kind* rather than by grant**: it may not create a
  principal (X-40), supply a connection setting (X-47), or supply or rotate a credential (X-54).
  Those four are each a caller deciding *what a tenant's operations run under* — the credential
  position, whether or not a value is ever seen — which is a different question from *which
  operations may run*, and is why they are kind-shaped rather than grant-shaped.

  ⚠ *Both superseded sentences are recorded rather than rewritten away. This bullet has now been
  wrong twice in the same direction — claiming a narrower set of exceptions than the code enforced —
  and a design that quietly grows exceptions is how the list stops being read.*

  **What is left is a gap of a different shape**: no surface edits a grant. See
  `docs/designs/invoke.md` §6.
- **It does not add a second identity port.** `Identity` already exists and both current providers
  bind it. A third binding is the shape; a parallel mechanism is not.
- **It does not mint tokens for humans.** Sign-in exists and works. An agent principal is a different
  kind for a different caller, which is why `PrincipalKind` has three variants and not one.
