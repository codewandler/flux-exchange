---
id: X-72
title: "Credential acquisition, and a labelled weak one (epic)"
status: in-progress
priority: 1
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
areas: [exchange-host, exchange-server]
note: "EPIC — owner-raised 2026-08-01: every credential here arrived by a human pasting it in, so the connector with 389 operations is the one nobody can connect. babelforce's password grant is the weakest way to fix that, which is why the hazard is declared metadata a deployment filters on"
---

# Credential acquisition, and a labelled weak one (epic)

## Goal
This host can **obtain** a vendor credential rather than only store one a human pasted in — and the
weakest way of obtaining one is declared, refused by default, and usable only where somebody opted in.

## Why this epic exists

`connector_catalog::Acquisition` ships one value, `Static`: *the stored secret, unchanged*. Every
connection this service has ever held started with an operator pasting a token. For babelforce that
is a dead end and the manifest says so in its own `[[auth]]` block — the token is *"minted outside
flux and supplied through the environment"*, and a babelforce user has an email address and a
password, not a token. **The connector with 389 catalogued operations is the one nobody can connect.**

Owner-raised 2026-08-01: use the OAuth2 password grant, and mark it as the weaker thing it is.

The marking is the interesting half. [`docs/vision.md`](../vision.md) principle 3 says grants select
by **declared metadata, not by name**. A deployment that forbids password-grant authentication should
say so once, about a property, and have every connector carrying that property refuse — rather than
keep a list of connector names that is wrong the moment the catalogue grows a 55th provider.

## The rule this lands under, which is not ours to relax

**An authentication endpoint is never a connector operation** — owner-stated 2026-08-01, recorded in
flux-connectors' `AGENTS.md` § Authentication contract. `providers/babelforce.toml` withholds
`/oauth/token`, `/oauth/authorize` and `/oauth/revoke` by that rule, and its accounting
`389 + 5 + 3 = 397` is checked in both directions by `babelforce_coverage.rs`. So this epic adds no
operation anywhere. The manifest **declares** the acquisition and its hazard; this host **performs**
it — which is what that same contract already says the host is for.

## Children
- **X-73** — the `AuthHazard` vocabulary: a hazard is a *kind*, `Risk` is a *level*, and folding one
  into the other makes `Selector::at_most` admit a password grant to anybody granted "at most high".
- **X-74** — the deployment filter, opt-in and fail-closed. **Ordered before X-75**, for the same
  reason X-40 was ordered before X-37: X-75 is what makes the hole reachable, and a gate that arrives
  after the thing it gates has already shipped ungated once.
- **X-75** — the host performs the grant: a port in `exchange-host`, its HTTP binding in
  `exchange-server` beside the `TokenExchange` that already does this for sign-in. Stores the token,
  discards the password.
- **X-76** — the quirk rule. babelforce's token endpoint *does* take `expires_in`, with different
  semantics per grant, and `account_id` switches the account on refresh. None of it is in the vendored
  specification, so none of it becomes a field on the vocabulary: **owner-decided 2026-08-02, a
  behaviour no document declares is a quirk of one endpoint.**

Upstream: **C-440** in flux-connectors declares the acquisition and the hazard on `[[auth]]`. This
epic reads what it publishes rather than keeping a local copy of the same fact.

## Acceptance
- [ ] The union of X-73, X-74 and X-75's acceptance.
- [ ] An operator connects babelforce with a username and a password on an opted-in deployment, and
      the stored credential is a token with an expiry.
- [ ] The password appears in no file the host writes and in no log line it emits — named test, not
      inspection.
- [ ] The same attempt on a deployment that did not opt in is refused **before any request leaves the
      process**, naming the hazard and the connector.

## Progress
- 2026-08-01 — filed with [`docs/designs/credential-acquisition.md`](../designs/credential-acquisition.md).
- 2026-08-03 — X-73/X-74 are complete and the X-75/X-76 implementation lane is in progress. The
  released connector catalogue still lacks C-440, so the server path is exercised through an
  explicit injected acquisition binding and the epic remains open pending live babelforce proof.

## Notes
- The design records why `produces_credential` is not the mechanism (C-432 measured it: `connector-flux`
  refuses to emit such an operation, because the emitted module binds the raw token to a model-visible
  symbol) and why a fifth `Risk` rung is not either.
- Precedent for the port/binding split: `crates/exchange-server/src/oidc/exchange.rs` holds the
  `TokenExchange` trait and `http_exchange.rs` its binding; `reqwest` appears in
  `crates/exchange-server/Cargo.toml` and in no other manifest in the workspace.
