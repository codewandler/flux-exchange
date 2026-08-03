---
id: X-129
title: "Bind local-release protocol identities to the delivered HTTP wire"
status: ready
priority: 0
epic: rich-connector-runtimes
areas: [exchange-host, exchange-server, protocol, tests]
depends_on: [X-113]
design: docs/designs/local-release-v1.md
note: "Milestone 1 — compatibility advertises exact tested HTTP wire identities, never placeholders or a package-version guess"
---

# Bind local-release protocol identities to the delivered HTTP wire

## Goal

Make the four delivered Exchange HTTP identities in X-126 true statements about actual routes and
serializable types. `compatibility --json` may advertise an id only when a provider-owned
bidirectional fixture proves that exact request/response wire still has that contract.

## Why this is a prerequisite

X-113 delivered authenticated effective discovery and one-shot invocation, but those routes predate
the local-release compatibility document and carry no shared version identity. Writing only a
generic version marker — or choosing a name only in release automation — would let
Flux accept a binary whose Rust wire changed underneath the same string.

The identity need not become a caller-controlled body field. It is compiled capability metadata,
bound by types and tests; the request continues to contain no tenant, authority or credential axis.

## Acceptance

- [ ] One typed provider module exports exactly these constants and no aliases:
      `exchange.api.v1`, `exchange.effective-catalogue-response.v1`,
      `exchange.invoke-request.v1` and `exchange.invoke-response.v1`. The local-release manifest,
      channel, compatibility and readiness serializers all use that source; package/workspace
      version parsing cannot produce a protocol id.
- [ ] `exchange.api.v1` is bound to Service Account bearer authentication and the exact two-route
      Milestone 1 surface: authenticated `GET /api/catalogue/effective` and
      `POST /api/operations/{operation}/invoke` with its optional `connection` query. A fixture
      proves anonymous access, an unknown query axis and a caller-supplied tenant/authority/credential
      axis all refuse as the delivered routes do.
- [ ] `exchange.effective-catalogue-response.v1` is bound directly to
      `routes::catalogue::view::EffectiveCatalogue`: top-level `generation` plus `operations`, and
      every operation/connection field and omission/null rule X-113 currently serializes. The
      positive fixture round-trips through the production type. Unknown/duplicate fields,
      type/null drift, a missing stable generation and a response containing tenant, credential,
      endpoint or runtime values are adversarial refusals.
- [ ] `exchange.invoke-request.v1` is the existing raw operation JSON body — no envelope — at the
      operation path, with only the optional tenant-local label in `?connection=`. Contract tests
      drive a real released read and approved write operation. Adding an endpoint, host, tenant,
      credential, runtime or UUID query/body envelope axis, accepting an unknown query key, or
      selecting a connection other than the resolved label fails the fixture.
- [ ] `exchange.invoke-response.v1` is bound to the exact success
      `exchange_host::Invocation {operation,content,view,is_error}` and the closed HTTP refusal
      bodies/statuses reachable on the same route, including sent/retryable distinctions. Fixtures
      round-trip production success/refusal types and fail on unknown/missing keys, status/body
      disagreement, an unbounded diagnostic, or any credential-shaped sentinel.
- [ ] Provider fixtures live under `tests/fixtures/exchange-http-v1/` with a checked filename/SHA-256
      inventory and machine-readable expected outcomes. A test derives every compatibility value
      from the typed constants and every fixture from production serializers/deserializers; deleting
      a field assertion, changing a type while retaining the id, or hard-coding the id only in the
      fixture fails first.
- [ ] A future compatible additive/change policy is written before implementation. Any wire change
      outside it requires a new protocol id and parallel compatibility support rather than silently
      changing a v1 fixture. X-126 cannot publish a manifest advertising these four ids until this
      story's provider gate is green on the tagged commit.

## Progress

- 2026-08-04: Filed by the local-release implementation audit. X-113's behavior is delivered; this
  story binds exact provider identities to those routes/types so X-126 stops advertising placeholders.

## Notes

- X-113 remains the behavior owner. This story adds protocol identity and conformance evidence, not
  another catalogue, invocation adapter or request construction path.
- X-125 separately owns `exchange.connection-plan.v1`; X-128 owns
  `exchange.supervisor-ready.v1`. All six meet in `docs/designs/local-release-v1.md`.
