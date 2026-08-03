# Service Accounts: the non-human principal

**Status:** accepted · **Story:** X-107 · **Removal checkpoint:** v0.17.0

## Vocabulary and authority

A **Service Account** is a durable non-human identity that authenticates with a bearer token. It is
not an Agent: an Agent is the model + execution loop + bounded capabilities hosted by an installed
App. A Service Account may call granted operations and subscribe to granted channels, but it owns no
credential and receives no authority merely by existing. Grants continue to select declarations by
risk, effects and idempotency rather than by principal or operation name.

Only a signed-in `User` may create, list or revoke Service Accounts. A Service Account cannot edit
connections, settings, grants, or create a successor. Those checks remain declared at the route and
repeated at the store/mutation boundary where applicable.

## Resource and token lifecycle

The canonical API is:

```
POST   /api/service-accounts
GET    /api/service-accounts
DELETE /api/service-accounts/{id}
```

Creation returns the token once. Listing returns the stable id and expiry only; neither token nor
verifier has a serializable route representation. Revocation removes every verifier for the tenant
and id and is idempotent only when the target existed for that tenant; another tenant's id remains
indistinguishable from an absent one.

New bearer tokens are `fxsa_` followed by 64 lowercase hexadecimal characters. Existing unprefixed
tokens remain resolvable from the unchanged verifier-keyed file format, so migration rewrites no
credential-shaped bytes and changes no tenant or expiry.

## Authentication composition

Bearer authentication asks the configured human identity provider and the Service Account store.
Exactly one resolved principal is admitted. No answer is anonymous; two answers are an ambiguous
credential and are refused rather than ordered. This makes service-account tokens useful at the
existing invoke and channel admission boundaries without teaching those routes a second identity
mechanism.

## Compatibility window

`/api/agents` is a compatibility alias for the canonical create route in v0.16 only. Its response is
still `service_account`; it cannot mint a legacy kind. Every alias response carries `Deprecation`, a
`Link` to `/api/service-accounts`, and a warning naming removal in v0.17. X-121 owns deletion at that
release.

`FLUX_EXCHANGE_SERVICE_ACCOUNTS` is canonical. `FLUX_EXCHANGE_AGENTS` is accepted through v0.16; if
both are set they must resolve to the same trimmed path or startup refuses. Documentation and startup
diagnostics lead with the canonical name.

`PrincipalKind` serializes `service_account`. Deserialization accepts `agent` only for committed
legacy data/descriptor compatibility through v0.16. The anonymous descriptor moves to vocabulary
version 2 and names the canonical resource and live bearer-auth capability. The console route is
`#/service-accounts`; `#/agents` redirects without preserving the retired noun.
