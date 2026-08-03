---
capability: subscribe
---

# `subscribe`

Have this host terminate a vendor's generated WebSocket channel, route what arrives against the
connector's closed declaration, and hand an authenticated subscriber a typed event.

`subscribe` is the inbound verb of a remote connector binding, and [`invoke`](/capabilities/invoke)
is the outbound one. One binding, two directions.

## A channel outlives its subscribers

Of the [three lifetimes](/surface#the-three-lifetimes), a persistent vendor connection is a
**Channel**: it is scoped to the deployment, it pushes, and it ends when an operator removes it. It
is not a Session and it is not a Lease.

That distinction is not vocabulary for its own sake. Conflating a Channel with a Session produces a
specific, real bug — a vendor socket that closes when one subscriber disconnects — and its owner
discovers it as silently dropped vendor events. One word per thing is how that stops being possible
to write.

## The inbound confused-deputy problem

The outbound argument is that a caller names an operation rather than a credential. The inbound one
is its mirror: **a subscriber cannot name a binding it has not been granted.** Delivering a vendor's
events to whoever asks for them by name would make this service a deputy that leaks one tenant's
traffic to another, which is the same mistake as handing out a credential, arriving from the other
direction.

So a subscription is scoped to an opaque tenant-owned channel id and a closed connector/binding/event
set the tenant's inbound grant admits. The vendor connection is authenticated from host-held
credentials, and only discriminator values declared by the connector become event labels. Delivery
is live and at-most-once: there is no replay or cursor, and a subscriber that overruns its bounded
queue is disconnected without stopping the vendor channel. Webhook signature verification and a
durable delivery inbox remain separate, unbuilt slices.
