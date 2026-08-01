# The surface

An index of the things flux-exchange deals in, and what each word means here. One word per thing,
because most of the confusion in this problem space is two names for one concept or one name for two.

> [!IMPORTANT]
> **Every entry below is a definition, and none of them is a claim that the capability is available.**
> This page says what a channel *is*; it does not say whether the build you are talking to serves
> one. That answer is `GET /api/onboarding` — anonymous, machine-readable, and held to the service's
> route table by a test in both directions.
>
> Per-entry status here, derived from that same descriptor rather than written by an author, is a
> tracked change (story X-64), and the pages that describe each entry in depth come after it (X-65).
> Until then this page is deliberately silent about availability rather than usefully wrong about it.

## The three lifetimes

Routinely conflated, and conflating them produces real bugs — a webhook endpoint that dies when an
agent's conversation ends, or a lease that outlives the grant that opened it. So they are three
words, and each has one owner:

| | Scoped to | Direction | Ends when |
|---|---|---|---|
| **Session** | a caller's conversation | — | it is closed or expires; resumable |
| **Channel** | a deployment | pushes | the operator removes it |
| **Lease** | a caller's grant | pulls | the holder releases it, or its TTL passes |

`lease` is deliberately not called "session": in flux, `session` already means an event-sourced
conversation, and the two have opposite lifetimes and opposite owners. One name each, so a sentence
about one can never be misread as a sentence about the other.

## The index

### Principals and tenants

Who is asking, and on whose behalf. A **principal** is resolved by the host's identity port — a
signed-in human, or an agent presenting a token of its own. A **tenant** is derived from that
principal and from nothing a caller controls, which is the invariant the whole
[credential boundary](/boundary) rests on.

### Connections and credentials

A **connection** is one tenant's configured relationship with one connector: the credential it needs,
plus any non-secret per-connection values a templated connector requires. The two are stored
separately on purpose — a subdomain is not a credential, and one store holding both would make
"held" mean two different things.

A credential is addressed, never handed out. Nothing that reads a connection returns a secret value.

### `invoke`

Run one catalogue operation for the caller's tenant. The caller names the operation; the request is
built from that operation's own compiled Flux, and the credential is resolved by address from the
caller's tenant. It is the outbound verb of a connector binding.

### `subscribe` and channels

The inbound verb of the same binding. A vendor's signed payload is verified at the boundary and the
subscriber receives a typed, declared event, scoped to bindings that tenant already has.

**A webhook is a Channel** — not a trigger, and not a session artifact. It belongs to the deployment
and it outlives every conversation that reads from it, which is exactly the distinction the three
lifetimes above exist to keep.

### Grants

The authority a tenant hands an agent, expressed as a selection over declared operation metadata
rather than a list of names, with an explicit deny beating an explicit allow. See
[authority is granted by property, not by name](/boundary#authority-is-granted-by-property-not-by-name).

### Workflows

What an operator calls a workflow — triggers, conditions, schedules, a flow of operations — is **a
stored, versioned, per-tenant flux-app Program**. It is not a second execution model living here, and
describing triggers as a flux-exchange feature would be the largest untruth this site could tell.

The reason is not purity, it is what falls out for free: determinism, replay, fork and diff, approval
gates, typing and risk derivation. And it makes a composed operation **indistinguishable from a
vendor one** — same catalogue entry, same gating, same address — so an agent cannot tell whether an
operation came from an OpenAPI document or from somebody dragging boxes on a canvas. A visual editor
emits IR; the IR lowers to Flux. The simplified schema an editor wants is a *projection*, never a
second model.

### Execution records

Who asked, which grant admitted it, what was called, and what came back. The measure is that every
execution is explainable after the fact without reconstructing it from logs.

## Reading this site safely

The [repository](https://github.com/codewandler/flux-exchange) carries the itemized inventory of what
is not built, and it is expected to stay accurate — a page or a type that implies a working service
costs more than an honest gap. This site is a *third* rendering of overlapping facts, so treat the
descriptor as the source and this page as the vocabulary.


