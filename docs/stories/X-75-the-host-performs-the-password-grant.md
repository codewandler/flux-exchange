---
id: X-75
title: "The host trades a username and a password for a token, and keeps only the token"
status: in-progress
priority: 3
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
areas: [exchange-host, exchange-server]
note: "the mechanism: a port in exchange-host, its HTTP binding in exchange-server beside the TokenExchange sign-in already uses. RFC 6749 §4.3 makes discarding the password a MUST for the client, and here the client is us"
---

# The host trades a username and a password for a token, and keeps only the token

## Goal
An operator connects babelforce by supplying a babelforce username and password; this host obtains an
access token, stores the token at the connection's ordinary credential address, and the password
never reaches disk, a log, or an error body.

## Why the host does this, rather than an operation doing it

flux-connectors' `AGENTS.md` § Authentication contract:

> Generated Flux names a credential and nothing more. It must not add prefixes, base64-encode pairs,
> refresh tokens, or perform session login. **The host resolves the credential, performs effectful
> acquisition such as OAuth2**, applies the placement scheme, and registers values with its redactor.

This repository is that host. There is no `babelforce-token` operation to add and adding one is
refused upstream twice over — see [[X-72]] and the design.

## Where the transport lives, which is already decided

This host constructs no request of its own (principle 6), and
`crates/exchange-host/Cargo.toml`'s `[dependencies]` is an allow-list read by
`tests/no_second_request_path.rs`. **An OAuth token request is a request somebody constructs.** The
answer is in the tree: `crates/exchange-server/src/oidc/exchange.rs` declares the `TokenExchange`
trait, `http_exchange.rs` binds it, and `reqwest` appears in `crates/exchange-server/Cargo.toml` and
in no other manifest in the workspace.

**A port in `exchange-host`, its HTTP binding in `exchange-server`.** The published crate gains a
trait and no transport.

## What it does, in order

1. Take the username and password **borrowed, not owned** — `Redemption<'_>`'s shape, redacting on
   `Debug` for the same reason.
2. Register both with the redactor **before the request is built**, not after. The first `?` that
   propagates a transport error is the one that carries a password into a log.
3. `POST /oauth/token`, `grant_type=password`.
4. Store the **access token** at the connection's ordinary credential address. Placement code does
   not change: by the time anything puts a value on a request, a minted credential is a value in the
   store like a pasted one.
5. **Discard the password.** RFC 6749 §4.3 makes this a MUST for the client.
6. Record expiry from the response's `expires_in`, and the `refresh_token` if one was issued.
   babelforce **rotates refresh tokens on every use** — the stored one is replaced on each refresh or
   the next refresh fails `invalid_grant`.

## Acceptance
- [ ] A trait in `exchange-host` for obtaining a credential, with its HTTP binding in
      `exchange-server`. `tests/no_second_request_path.rs` passes **unchanged** — no new entry in the
      `ALLOWED` list, because nothing new is a transport in the host.
- [ ] Gated by [[X-74]]'s posture: with the posture unset the attempt is refused **before any request
      leaves the process**.
- [ ] **Failing-first test** — the password is absent from everything the host persists and everything
      it logs, on both the success and the failure path. Write the failure-path half first: assert on
      the rendered error of a rejected grant, watch it fail carrying the password, then close it.
- [ ] The stored credential carries its expiry, and a rotated refresh token replaces the stored one.
- [ ] A vendor refusal that is **MFA-shaped is distinguishable from a wrong password.** RFC 9700 §2.4
      states the grant cannot carry two-factor authentication, so a tenant with MFA enabled cannot use
      this path at all — and a refusal that reads as "bad credentials" sends that operator to re-type
      a password that was correct.
- [ ] X-60's question is answerable for a credential that arrived this way: whatever records *who
      supplied a credential* must not record a credential nobody supplied as though a human pasted it.

## Progress
- 2026-08-03 — implementation started from the already-landed X-74 posture. The local seam is an
  explicit server-owned acquisition binding injected into `AppState`; no released connector is
  treated as declaring the path while upstream C-440 remains unreleased.
- 2026-08-03 — the in-tree seam now runs end to end through the real atomic credential store. The
  fail-closed posture is checked before the performer, password input is immediately secret-shaped
  and never persisted or echoed, access/refresh/expiry are one batch, refresh rotates the pair, and
  MFA has a distinct value-free refusal. Sole and labelled connections move and remove the reserved
  companion records with the ordinary access-token address. Audit evidence records `acquired` and
  `initiated_by`, never a human supplier. Focused host boundary/no-second-request-path tests and all
  375 server tests outside a concurrent onboarding-descriptor failure pass. The story remains open
  because released C-440 metadata and live babelforce proof do not yet exist.

## Notes
- Blocked on nothing in this repository. It reads better after **C-440** upstream declares the
  acquisition, but the grant can be driven from a connection's own configuration first and the
  declaration wired in when it exists.
- The requested token TTL is [[X-76]] and is **not** part of this story. babelforce does accept
  `expires_in` on the `password` grant, but it is undeclared in every vendored document and its
  meaning differs per grant, so it lands there as a **quirk of that endpoint** rather than as a field
  on anything this story adds. This story requests no lifetime and records what the vendor returns.
- Related: [[X-39]] (rotation without a window where the connection is gone) — a refresh is a rotation,
  and the two should not grow two mechanisms.
