---
id: X-154
title: "Read the complete OAuth2 declaration from the catalog artifact"
status: backlog
priority: 1
epic: catalog-artifact
areas: [exchange-host, exchange-server]
note: "X-147's three unticked criteria need a complete OAuth2 surface; the 0.21 catalog::OAuth2 struct ships empty endpoint and client_id for babelforce, and the artifact is where that stops being true"
---

# Read the complete OAuth2 declaration from the catalog artifact

## Goal

Give [[X-147]]'s remaining acceptance criteria a declaration complete enough to satisfy them, by
reading the catalog artifact's full credential surface rather than the partial one the 0.21 generated
catalogue carries.

## Why this is a separate story from X-147

X-147 delivered the authorization-code leg against an **injected** `DelegatedGrant`, and left three
criteria unticked on purpose:

- composing the authorize URL from the connector's own declaration,
- production composing a non-empty `AcquisitionBindings`,
- refusing a connector that declares a grant this host cannot perform.

[[X-146]] moved the pins to connector 0.21, so `catalog::Acquisition::OAuth2` now exists. **It is not
yet enough.** Measured against the released 0.21.0 catalogue on 2026-08-12:

```
gitlab      endpoint: "login"  authorize_path: "/oauth/authorize"  client_id: ""   grants: [AuthorizationCode, RefreshToken]
babelforce  endpoint: ""       authorize_path: ""                  client_id: ""   grants: [Password, RefreshToken]
```

GitLab is usable; babelforce carries empty strings where the endpoint and authorize path belong. A
host cannot compose an authorize URL from that, and the difference is not a vendor fact — it is what
the current op grammar can express.

`docs/designs/catalog-artifact.md` is explicit that the artifact closes this:

> *every credential with scheme, acquisition, placement, subject, hazard, user-half binding, and the
> **complete** `OAuth2Spec` (grants, paths, redirect, scopes) plus token-endpoint quirks — ending the
> `oauth2: bool` collapse.*

## Acceptance

- [ ] The authorize URL is composed from the artifact's `OAuth2Spec` — endpoint resolved against that
      service's base URL, plus declared `authorize_path` and `scopes`. A caller names no host, path
      or scope; a scope absent from the declaration is one this host does not request. **This ticks
      X-147's second criterion.**
- [ ] Production composes a **non-empty** `AcquisitionBindings` derived from the artifact, and the
      C-440 comment in `crates/exchange-server/src/credential_acquisition.rs` is replaced by what is
      then true. **This ticks X-147's seventh.**
- [ ] A connector declaring a grant this host cannot perform is refused **at composition**, naming the
      grant — never attempted, never silently downgraded to another grant in its list. babelforce
      declares `Password` and `RefreshToken`; GitLab declares `AuthorizationCode` and `RefreshToken`;
      both must compose or refuse deliberately. **This ticks X-147's eighth.**
- [ ] A declaration too incomplete to compose from — an empty endpoint or authorize path where the
      grant needs one — is refused at composition **naming the connector and the missing field**,
      rather than producing a malformed URL a vendor rejects opaquely.
- [ ] The hazard is read from the declaration rather than from the injected binding. Upstream C-440
      has landed: babelforce declares `hazard: Some(ResourceOwnerSecretShared)`, the first released
      connector to do so, and X-74's gate should now be driven by released metadata rather than only
      by fixtures.
- [ ] Failing-first tests for each refusal, and one end-to-end test that composes GitLab's authorize
      URL from the artifact and matches it against the URL X-147's route produces from an injected
      grant — the two must agree, which is what proves the injected seam was a faithful stand-in.
- [ ] No token, code, verifier or client secret appears in an error, a log or a `Debug`. The client id
      is public by specification and may appear; nothing else may.

## Progress

- 2026-08-12: Filed. Blocked on the artifact carrying the complete `OAuth2Spec` (upstream C-536) and
  on Exchange reading it ([[X-153]]).

## Notes

- Read [[X-151]] for the epic's scope.
- This is the story that finally closes [[X-72]], the credential-acquisition epic, since X-147's
  remaining criteria are its last open ones and [[X-148]] follows from a credential existing.
- `client_id` is empty for **both** shipped OAuth2 connectors. Whether it is connector data at all,
  or deployment configuration like the redirect URI already is, is a real question this story has to
  answer rather than assume — an OAuth2 client id is per-registration, and two deployments of Exchange
  against the same GitLab are two registrations.
