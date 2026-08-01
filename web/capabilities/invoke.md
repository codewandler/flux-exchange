---
capability: invoke
---

# `invoke`

Name an operation and get a result, without naming a host, a credential or a tenant.

`invoke` is the outbound verb of a remote connector binding. [`subscribe`](/capabilities/subscribe)
is the inbound one — they are two directions of one thing rather than two features, and a model that
treats them as separate products gets the shape of this service wrong at the top.

## What a caller supplies, and what it cannot

The operation's id, as the [catalogue](/surface#invoke) publishes it, and that operation's own
declared parameters. That is the whole request. There is no envelope, and there is nowhere to put:

| Not accepted | Where it comes from instead |
|---|---|
| a tenant | derived from the principal the host resolved |
| a credential | resolved by address, from that tenant's connection |
| a host | the connector's own declaration |
| a connector | the operation names it |

Each of those is derived rather than accepted, and that is not an ergonomic choice. A field a caller
can set is a field a stolen token can set. See
[the credential boundary](/boundary) for the argument in full.

## The answer says what happened, not just whether it worked

An HTTP status answers "did this service succeed", which is a different question from "did the
vendor receive it". A `502` does not say whether the call went out. So the result carries whether it
was sent and whether it is safe to retry as fields of their own, and a vendor's own rejection comes
back intact rather than reshaped — *the vendor said no* and *we could not ask* are different events,
and a caller that cannot tell them apart cannot retry correctly.

## It is gated twice

Identity, and then grant. A resolved principal is not sufficient: an operation runs only if a grant
the caller's tenant holds admits it, decided from what the operation *declares* — its risk, its
effects, its idempotency — rather than from a list of names.

This is fail-closed, which means a tenant nobody has granted anything runs nothing at all. That is
the intended behaviour and it looks exactly like an outage; a caller meeting a refusal here should
ask whoever holds the tenant for a grant that admits what they need. A refusal names the operation
and never the rule that refused it, because a caller able to enumerate a tenant's policy can plan
around it.
