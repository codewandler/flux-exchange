---
capability: subscribe
---

# `subscribe`

Have this host terminate a vendor's channel, verify what arrives against the connector's own
declaration, and hand a subscriber a typed event.

`subscribe` is the inbound verb of a remote connector binding, and [`invoke`](/capabilities/invoke)
is the outbound one. One binding, two directions.

## A webhook is a Channel

Of the [three lifetimes](/surface#the-three-lifetimes), a vendor's inbound endpoint is a **Channel**:
it is scoped to the deployment, it pushes, and it ends when an operator removes it. It is not a
Session and it is not a Lease.

That distinction is not vocabulary for its own sake. Conflating a Channel with a Session produces a
specific, real bug — a webhook endpoint that stops existing when some agent's conversation ends —
and the endpoint's owner discovers it as silently dropped vendor events. One word per thing is how
that stops being possible to write.

## The inbound confused-deputy problem

The outbound argument is that a caller names an operation rather than a credential. The inbound one
is its mirror: **a subscriber cannot name a binding it has not been granted.** Delivering a vendor's
events to whoever asks for them by name would make this service a deputy that leaks one tenant's
traffic to another, which is the same mistake as handing out a credential, arriving from the other
direction.

So a subscription is scoped to bindings the tenant already holds, and the vendor's signed payload is
verified at the boundary before anything downstream sees it. A payload that does not verify is
refused rather than passed along annotated — a consumer that receives an event marked *unverified*
will eventually act on one.
