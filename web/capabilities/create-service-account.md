---
capability: create-service-account
---

# Service Accounts

A **Service Account** is flux-exchange's durable non-human API principal. It can authenticate an
App, Agent or other automation, but it is not itself a Flux Agent: it has no model, loop,
datasources or capabilities merely because it exists.

## Lifecycle

A signed-in human creates a Service Account with `POST /api/service-accounts`. The response shows
its `fxsa_…` bearer token once; the host stores only a verifier. The same human can list ids and
expiries with `GET /api/service-accounts` and revoke one with
`DELETE /api/service-accounts/{id}`. No management response can serialize a token or verifier.

Present the token as `Authorization: Bearer …`. It resolves to `kind: service_account` in the
tenant of the human who created it, until its stated expiry or revocation.

## Authentication is not authority

Creating or authenticating a Service Account grants nothing. Tenant grants still select declared
operation and inbound-channel metadata; explicit deny wins. A Service Account cannot manage
connections, settings or grants, create a successor principal, or obtain a credential value.

## v0.16 compatibility

`POST /api/agents` remains a visibly deprecated create alias for v0.16 and always returns the
canonical `service_account` kind. It is removed in v0.17. Existing unprefixed tokens remain valid
through their original tenant and expiry without rewriting stored credential-shaped material.
