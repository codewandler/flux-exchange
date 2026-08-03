---
capability: grants
---

# Grants

A grant is the policy a tenant places between identity and action. Authenticating proves who a
caller is; it grants no Connector authority by itself.

## Select declared properties, not names

Grants select from facts a Connector publishes about an operation: connector, risk, effects and
idempotency. They do not enumerate operation ids. This means a newly added operation meets the same
policy on the day it appears instead of slipping through a stale allow-list or waiting for somebody
to notice a new name.

An explicit deny beats an explicit allow. If no selector admits an operation or inbound event set,
the request refuses before a credential is read and before vendor traffic is delivered.

## Policy belongs to the operator

`GET /api/grants` is the collection entry point for an authorized operator. Replacing policy and
previewing a proposed selector use the same operator boundary. An Agent or Service Account cannot
read policy it could use to plan around a refusal and cannot widen its own authority.

This is the difference between access to an operation and access to a credential: a stolen API
principal can exercise only the declared capabilities an operator admitted. It never receives a
general-purpose remote client or a vendor secret. [The boundary page](/boundary) follows that
argument through both outbound calls and inbound events.
