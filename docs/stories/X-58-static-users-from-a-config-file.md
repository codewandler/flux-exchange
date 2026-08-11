---
id: X-58
title: "An operator can define users in a config file and sign in on their own network"
status: done
priority: 1
epic: local-identity
design: docs/designs/local-identity.md
areas: [exchange-server]
note: "the axis that actually unblocks the console: a local identity with a real secret, so it may bind a reachable address — unlike DevIdentity, whose roster handle is a name anybody can guess"
---

# An operator can define users in a config file and sign in on their own network

## Goal
A deployment can authenticate humans without an identity provider, on an address other than loopback.

## Why this is not "DevIdentity with a config file"

`DevIdentity` has **no secret**: the roster handle *is* the credential. That is what makes it
zero-setup, and it is exactly why `bind.rs` refuses it on any reachable address — *a reachable bind
whose authentication is a name anybody can guess is worse than no authentication, because the surface
in front of it believes every caller.*

So this is a different thing, and the difference is the secret. **It stays a different thing in the
code, too**: `DevIdentity` is not extended, not refactored into a backend of this, and not given a
file loader. Collapsing them would put a secret-free mode one config edit away from a reachable bind.

## Constraints that are not negotiable

- **A file, not an environment variable.** A roster in an env var is visible in `ps`, in a container
  inspect, and in a crash dump. A file has a mode — and this host already refuses a credential store
  whose mode is too wide, so there is a precedent to follow rather than a rule to invent.
- **A verifier, never a password.** The agent token store is the shape: this host keeps a verifier and
  **cannot show the token again**. A users file holding plaintext would be the only place in this
  repository where a secret is stored to be compared rather than presented.
- **No default user.** A default credential is a published credential.
- **Malformed means refuse to start**, naming the entry — `DevIdentity`'s rule, for its reason: a
  roster that silently lost a principal is a roster whose operator is debugging the wrong thing.
- **The tenant comes from the file**, fixed at startup. No request field may reach it.

## The open question to settle first

**Does this take passwords, or pre-computed verifiers?**

Taking passwords means owning a KDF, a work factor, and a rehash-on-login story — a real discipline,
and one this platform may not want. The alternative is a file of verifiers an operator generates with
a shipped subcommand: less friendly, far less to get wrong.

Decide it in the design before writing code, and write down which and why.

## Acceptance
- [x] **Failing-first test** — a user defined in the file can sign in and is resolved to the principal
      and tenant the file names.
- [x] **Failing-first test** — a wrong secret is refused, and the refusal does not distinguish "no
      such user" from "wrong secret".
- [x] This binding may listen on a reachable address, and `bind.rs` says so **as its own state** —
      neither `Development` nor OIDC-`Bound`. A test pins that `Development` is still refused there.
- [x] A file with a mode wider than the host accepts is refused, the way the credential store's is.
- [x] No plaintext secret is stored, logged, or returned. Assert it adversarially, the way
      `NewConnection`'s missing `Debug` does — make the wrong thing fail to compile where you can.
- [x] Malformed file → the process refuses to start and names the entry.
- [x] The console can sign a user in through it end to end. [[X-57]] must land first.

## Notes
- Depends on [[X-57]], **but not for the reason this story originally gave.** It said "without it the
  console will not show a sign-in affordance"; X-57 established that the console shows the affordance
  **unconditionally** and always has — nothing under `console/src` reads `sign_in_available`. What
  X-57 actually did was fix where that affordance *leads*, and give `available()` an honest meaning
  for a non-OIDC provider to inherit.
- **So this story inherits an unbuilt piece X-57 named**: gating the affordance on the field, which is
  what X-43 published it for and what nothing has ever done. A local-users deployment that is
  misconfigured should not render a link into a refusal, and the form this story builds is the first
  thing that makes the distinction matter.
- **This is the first thing in this repository that looks like an operator surface**, and [[X-54]] is
  already blocked on the absence of one. The two should be designed knowing about each other — if a
  users file can say who may configure a connection, X-54's question gets easier.
