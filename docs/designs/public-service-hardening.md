# Design: the boundary around a public process

**Status:** accepted · **Epic:** `remote-deployment` · **Story:** X-87

## Decision

The identity policy does not change: the configured OIDC tenant is the organization boundary, and
every account in it may sign in. The first public-deployment review found operational controls around
that policy that existed only at the browser or platform edge. This story puts them in the process
that holds the credentials.

### Logout closes the session that authenticated it

`DELETE /api/session` already derives the presented token from the guarded request, so it can only
close the caller's own session. It currently delegates that close only to `DevIdentity`; an OIDC
cookie is merely deleted from one browser while the server continues to accept a copied value until
expiry. `AppState` will expose one provider-aware close operation, exhaustively matching the bound
identity. The OIDC implementation will close the same `SessionStore` it resolves against. A generic
test identity has no session-management contract, so that test-only composition remains a no-op.

### Bound the work that crosses expensive boundaries

The limits are process-wide because this single-machine deployment has one pending-flow store, one
invoker and one set of vendor quotas. They do not derive an identity from a caller-controlled IP or
forwarding header.

- At most 30 OIDC authorization starts are admitted per rolling minute. A start is what allocates a
  pending flow; bounding it below the store's 1,024-entry capacity prevents a short anonymous flood
  from evicting every legitimate flow. A callback capable of reaching the token exchange must carry
  a binder from one of those starts, so the same admission bounds expensive completions.
- At most 120 invocations are admitted per rolling minute, and at most 16 execute concurrently.
  Saturation refuses immediately with `429` and `Retry-After`; it does not occupy an unbounded queue.
  The permit is held only around invocation, leaving health, sign-in and administration responsive.

These are availability limits, not grants. A request passes identity and grant checks exactly as it
did before, then receives a finite share of this process. The rate counter uses a bounded fixed-size
window representation; traffic cannot turn the limiter itself into an allocation attack.

### Audit actions, never material

Successful mutations and executions emit `tracing` events with `audit = true`, a stable `action`, the
resolved acting principal, and only the non-secret address needed to investigate the event. An agent
token, credential value, setting value, OIDC code, session token and request body are never fields.
Refusals retain their existing operational logs; this addition answers the different question “what
authority was successfully exercised?”

### Headers belong to the same router as the policy

One response middleware adds CSP, frame, MIME-sniffing, referrer, permissions and transport headers
to every response, including errors and static-console fallbacks. The CSP permits only same-origin
scripts, styles and connections plus `data:` images, matching the built console. API and sign-in paths
also receive `Cache-Control: no-store`; immutable console assets remain cacheable.

HSTS omits `preload`: enrolling every current and future subdomain is a separate operator decision.
The application can safely send HSTS behind TLS termination; browsers ignore it on plain HTTP.

### Dependency audit is a gate with explicit exceptions

CI runs `cargo audit` at a pinned tool version and `npm audit --audit-level=high` in each independent
Node tree. The Rust graph currently contains RUSTSEC-2023-0071 through `jsonwebtoken`/`rsa`. The
advisory concerns RSA key generation; this service parses provider public keys and verifies signed
tokens, and generates no RSA key. CI narrowly ignores that advisory with this rationale while still
failing for every other vulnerability. The ignore is removed when the dependency line resolves it.

## Rejected alternatives

- **Rely only on Fly's edge limits.** They are useful defence in depth, but they are deployment state
  rather than the security contract of the binary and do not protect another composition.
- **Rate-limit by `X-Forwarded-For`.** The app has no authenticated trust relationship with that
  header; accepting it would let a caller choose its own bucket.
- **Revoke every session for a principal on logout.** The presented session is the authority proven
  by this request. Global sign-out needs an explicit session inventory/revocation operation, not an
  accidental widening of a browser logout.
- **Apply `no-store` to console assets.** It does not protect credential-bearing responses more and
  needlessly defeats hashed-asset caching.

