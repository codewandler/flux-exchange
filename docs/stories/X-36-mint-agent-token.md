---
id: X-36
title: "An agent token is minted once, and this host keeps only a verifier"
status: ready
priority: 1
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
- [ ] **Failing-first test** — minting yields a token, and the value returned is **not** recoverable
      from anything this host stores. Assert against the store, not just the API shape: a token this
      host could display twice is a token this host is keeping.
- [ ] The minted principal is `PrincipalKind::Agent` and carries **the minting principal's tenant**,
      read from the resolved principal and from nothing the caller sent. A body field named `tenant`
      must not influence it — follow `routes::identity`'s existing vector tests.
- [ ] The token follows this repository's credential discipline: drawn from `entropy`, **redacted in
      `Debug`**, no `Display`, and absent from every refusal and every log line. `SessionToken` and
      `flow::Binder` are the precedents — do not invent a third shape.
- [ ] The stored verifier is **not a usable token**. State plainly in the code what an attacker who
      reads the store obtains, and pin it with a test.
- [ ] Minting requires an authenticated principal. An anonymous caller is refused, and the refusal
      names nothing about what exists.
- [ ] A token carries a stated expiry, and a minted-expired or absurd expiry is **refused rather than
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
