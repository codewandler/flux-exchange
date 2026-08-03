---
capability: leases
---

# Leases

A **Lease** is one of [the three lifetimes](/surface#the-three-lifetimes): a pull-oriented runtime
resource scoped to a caller's grant. Its holder releases it, or its TTL passes. It is neither a
caller's resumable Session nor an operator-owned Channel.

That lifetime is useful for rich runtimes whose resources cannot be reduced to one request and one
response: a database transaction, a container exec stream or another connector-declared pull
resource. The Connector declares the runtime plan; the caller never chooses a host runtime or gains
a general-purpose socket.

## The implementation contract

[X-118, “Make leases own rich runtime resources”](https://github.com/codewandler/flux-exchange/blob/main/docs/stories/X-118-make-leases-own-runtime-resources.md)
owns acquisition, renewal, release, TTL expiry, grant revocation and the audit record for those
resources.

The status above comes from the build's onboarding descriptor. The intended boundary remains the
same in either status: a Lease carries bounded authority to a connector-declared resource, never a
credential value.
