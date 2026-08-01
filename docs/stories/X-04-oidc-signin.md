---
id: X-04
title: "OIDC sign-in"
status: done
epic: serve
design: docs/designs/oidc-signin.md
note: "signing in proves who the operator is; it mints no token for any vendor operation. Connecting a provider is a different flow with a different consent screen"
---

# OIDC sign-in

## Goal
A human can sign in with an OIDC provider, and the resulting principal names a tenant.

## Acceptance
- [x] Authorization-code flow with PKCE, state and nonce validated.
- [x] **Failing-first test** — a callback whose `state` does not match the one bound at `/signin` is
      refused with no session issued.
- [x] The client secret comes from the environment and from nowhere else, and its `Debug` prints a
      redaction rather than the value.
- [x] Missing configuration produces a **startup message naming the unset variables** and a service
      that serves an explanatory page — not a panic, and not a broken login that fails at the callback.

## Progress
- **The dependency decision was taken by the owner on 2026-08-01: `reqwest` + `jsonwebtoken` +
  `sha2`.** This is the record two reviewers looked for in the tree and could not find, so it is
  written here rather than left in a session log. It lifts the dependency fence **for this story
  only**; the manifests and `Cargo.lock` are therefore a legitimate part of X-04's diff and not a
  fence violation. `sha2` also retires the hand-written `oidc/sha256.rs`.
- **Correction, from review:** the manifest comment claiming this "does not acquire a C toolchain
  dependency" was **wrong**. reqwest's `rustls` feature resolves to `__rustls-aws-lc-rs`, so
  `aws-lc-rs` / `aws-lc-sys` (C and assembly) are in the build, and a container without a C
  toolchain that built this repository before will now fail. OpenSSL and a second TLS stack are
  genuinely absent, which was the other half of the claim and does hold.
- **Closed 2026-08-01.** The token exchange is bound and sign-in completes end to end. Gate green:
  142 tests, clippy clean under `-D warnings`, fmt clean.
- **Two independent reviews attacked the diff before it merged.** The crypto envelope held: a token
  MAC'd with the provider's public key as an HMAC secret is refused (forged with *both* the PEM and
  the JWK modulus spelling, because a vulnerable verifier passes whichever bytes it holds),
  `alg: none` is refused, an unpublished `kid` is refused with no try-until-one-verifies path, and
  the claim-check split was verified claim by claim against `Oidc::admit` rather than taken on trust.
- **Both reviews caught the first commit shipping a red gate**: `REQUIRED` gained two variables and
  the `complete()` test fixture did not, failing five config tests.
  `every_configured_value_lands_in_its_own_field` now pins the positional read that allowed it — the
  drift it was specified to catch had already happened once.
- Follow-on work filed rather than smuggled in: [X-16](X-16-session-expiry.md) (session expiry, whose
  stated blocker this story removes) and [X-17](X-17-exchange-failure-modes.md) (an operator's own
  misconfiguration is currently reported as a refused credential).
- **Historic — merged and reviewed PASS, but PARTIAL, blocked on a dependency decision.**
  Everything on this side of the network seam is built and tested: the authorization URL, PKCE
  `S256`, `state` and `nonce` generation and validation, `iss`/`aud`/`exp`/`sub`/`nonce` admission,
  the config refusal and explanatory page. `TokenExchange` is a **port with no binding**, so
  `/api/signin` serves an explanation rather than redirecting to a provider it could never return
  from.
- **What is missing and why:** redeeming the code at the token endpoint needs an **HTTP client**, and
  verifying the id token's signature needs a **JOSE/JWT library**. Neither exists in this workspace
  and the implementor was fenced from adding one. It did not hand-roll signature verification, which
  was the right call — sibling flux-connectors already uses `reqwest` and `jsonwebtoken` for exactly
  this, so the family precedent exists and the decision is which crates this repository takes on.
- **The one crypto exception, and how far it was checked.** `oidc/sha256.rs` is a hand-written
  SHA-256 for the PKCE challenge, because `sha2` is unavailable. A review copied it into a standalone
  crate and diffed it against Python `hashlib` over **every message length 0..=600** (both padding
  edges) plus a 512 MiB message at exactly 2^32 bits: all identical. `base64url` identical over 201
  lengths; RFC 7636 Appendix A's chain reproduced independently. `plain` is unreachable — the method
  is a `const`. **Replace with `sha2` when dependencies open.**
- **The failing-first proof is better than the literal one.** The named test would pass vacuously
  against a 404, so the implementor committed the whole flow *minus* the state binding first: the
  forged callback answered `<h1>Signed in</h1>` — a victim signed in as the attacker — and the next
  commit closed it.
- Composition follows X-03's precedent: OIDC-configured-without-an-exchange reports **`Unbound`**,
  not `Bound`, because `admit_bind` asks whether anything *could* resolve a caller and here nothing
  can. No reachable bind is legal in this build; a live sweep confirmed it.
- **Found here and filed separately:** server-side `state` does not close login-CSRF — see
  [X-15](X-15-login-csrf.md).
- Tenant is fixed at startup from `FLUX_EXCHANGE_OIDC_TENANT` rather than mapped from a claim, since
  some providers let users edit their own profile claims. Stronger than claim-mapping, at the cost of
  one provider federating one tenant.

## Notes
- **Signing in is not connecting.** Sign-in asks for `openid`/`email`/`profile` and mints nothing for
  a vendor. Provider connection is its own flow; do not conflate the consent screens.
- Design first: this story is non-trivial and has no design doc yet. Write one under
  `docs/designs/` (`/track:design`) before implementing. The cross-repo reasoning it builds on is
  flux's [`docs/designs/ecosystem.md`](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md).
