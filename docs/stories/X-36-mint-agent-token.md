---
id: X-36
title: "An agent token is minted once, and this host keeps only a verifier"
status: done
epic: agent-access
design: docs/designs/agent-access.md
areas: [exchange-host]
note: "the first half of closing the vision's largest unblocked gap — nothing today can create a principal an agent could present"
---

# An agent token is minted once, and this host keeps only a verifier

## Goal
An authenticated human can create an agent principal for their tenant and receive a token exactly
once.

## Acceptance
- [x] **Failing-first test** — minting yields a token, and the value returned is **not** recoverable
      from anything this host stores. Assert against the store, not just the API shape: a token this
      host could display twice is a token this host is keeping.
- [x] The minted principal is `PrincipalKind::Agent` and carries **the minting principal's tenant**,
      read from the resolved principal and from nothing the caller sent. A body field named `tenant`
      must not influence it — follow `routes::identity`'s existing vector tests.
- [x] The token follows this repository's credential discipline: drawn from `entropy`, **redacted in
      `Debug`**, no `Display`, and absent from every refusal and every log line. `SessionToken` and
      `flow::Binder` are the precedents — do not invent a third shape.
- [x] The stored verifier is **not a usable token**. State plainly in the code what an attacker who
      reads the store obtains, and pin it with a test.
- [x] Minting requires an authenticated principal. An anonymous caller is refused, and the refusal
      names nothing about what exists.
- [x] A token carries a stated expiry, and a minted-expired or absurd expiry is **refused rather than
      clamped** — X-16 set that precedent for sessions and the argument is identical.

## Notes
- **This is not a session.** `docs/designs/agent-access.md` has the table: a session dies when the
  human's identity does; an agent token is killed by an operator. Different store, different type —
  sharing either is the mistake this design most wants to avoid.
- Where the store lives is a real decision. `SessionStore` is in-memory because sessions are short
  and per-process; agent tokens are long-lived and an operator will paste one into a config, so a
  process restart losing every agent's access is probably wrong. Decide, and write down why.
- **No dependency.** No password-hashing crate, no JWT for this. If you conclude one is needed, say
  so and stop — that is a dependency decision and the manifest is fenced.

## Progress
- **Done 2026-08-01.** Gate green: 44 + 213, clippy clean, fmt clean. Behavioural merge-base failure
  — a test written against the *base's own* API, so it compiled there and failed on behaviour.
- **The store keeps a digest, and that is proved rather than asserted.** The test presents **every
  value in the file, and the whole file**, back to `resolve`; every one returns `None` while the
  token itself resolves. A test asserting only that the token string is absent would pass for a store
  keeping it base64-encoded.
- **Seven weakenings, applied one at a time to the finished code, every one compiled and every one
  caught**: storing the token verbatim, clamping an over-long expiry, dropping durability, printing
  the token in `Debug`, warning instead of refusing on a writable store, reading the tenant from a
  body field, and treating an unreadable store as empty.
- **A distinction the implementor added because collapsing it either way is wrong:** *reading* this
  store is a roster disclosure; *writing* it is a **full authentication bypass** — plant a verifier,
  present the matching token as any agent in any tenant. So a group- or world-**writable** store is
  refused, and a merely readable one only warns, because refusing on readable would take `/health`,
  the catalogue and sign-in down over bytes nobody can spend.
- **The store is its own durable file**, not `SessionStore` (in memory — a restart takes out every
  agent at once) and not the credential store (`SecretStore` cannot enumerate, so X-38's "which
  agents exist" would be unanswerable, and an agent verifier is this host's own record rather than a
  tenant's vendor secret).
- **The finding that matters most, filed as [X-40](X-40-who-may-mint-an-agent.md) and ordered ahead
  of X-37:** nothing gates minting by principal kind, so once X-37 binds the `Identity` port a leaked
  agent token mints successor agents — and revoking the first would not kill the descendants.
  Revocation would stop being a remedy. The implementor said it should be decided *before* X-37
  merges, not after, and it was right.
- **Carried forward — wall-clock expiry.** `expires_at` is Unix seconds rather than an `Instant`,
  because a monotonic deadline means nothing after a restart. So this store inherits the weakness
  `session.rs` writes against: an NTP step backwards extends every agent token by the size of the
  step. If a token must die *now*, the answer is revocation, not the clock.
- **Carried forward — `MAX_LIVE_AGENTS` is host-wide, not per tenant.** One tenant minting in a loop
  exhausts the bound for everybody. The route answers `503` and keeps the count in the log, but the
  cross-tenant denial of service is real and a per-tenant bound is a design decision this story did
  not sanction.
- **A trade documented rather than copied:** a cookie-carried caller *does* receive a readable token
  here, unlike at `/api/session`. Cross-site is closed by `SameSite=Strict`; same-origin XSS is not
  and cannot be, because the token is on the page by construction. The remedy is revocation. The
  alternative is that the console can never mint an agent.
