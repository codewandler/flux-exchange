---
id: X-23
title: "A browser-facing endpoint is refused in cleartext too"
status: done
epic: serve
areas: [exchange-server]
note: "raised by X-17's implementor and again by X-19's, 2026-08-01: BACK_CHANNEL covers only the token endpoint and the key set, so FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT and _REDIRECT_URI take any scheme without a word"
---

# A browser-facing endpoint is refused in cleartext too

## Goal
Every OIDC endpoint an operator configures is refused in cleartext, not only the two that carry a
secret directly.

## Why this was deliberately left out, and why it should not stay out

X-17 refuses `http://` for `_TOKEN_ENDPOINT` and `_JWKS_URI`, and X-19 made that check agree with the
parser reqwest actually dials with. Both stories deliberately excluded
`FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT` and `FLUX_EXCHANGE_OIDC_REDIRECT_URI`, and the reasoning
in `BACK_CHANNEL`'s doc is sound as far as it goes: those are **browser-navigated** addresses, so the
browser enforces their transport, and the provider re-checks the redirect URI against a registration
this host does not own.

What that argument does not cover:

- **The authorization URL carries `state`, `nonce` and the PKCE challenge.** Over `http` they are
  readable and modifiable in flight by anything on the path, which is the position X-15 spent a whole
  story closing from a different direction. The browser does not refuse an `http` URL; it just uses it.
- **An operator who typed `http` did not decide anything** — they made a mistake this host is in a
  position to catch at startup, and instead says nothing at all.
- The refusal machinery already exists and is tested. What is missing is the two variables being in
  the list, and an argument for why loopback stays exempt here too.

## Acceptance
- [x] **Failing-first test** — an `http://` authorization endpoint is refused at startup, naming the
      variable. It is currently admitted, so the test fails before the fix.
- [x] An `http://` redirect URI is likewise refused, naming its own variable.
- [x] Loopback stays exempt, on the same argument X-17 recorded, and that exemption is asserted —
      a local test IdP is a real workflow and this must not break it.
- [x] `https` spellings of both still pass, in the same run.
- [x] The refusal is a `ConfigRefusal` and stays **non-fatal** in the sense `oidc/config.rs`'s module
      doc requires — `/health` and the catalogue keep serving.

## Notes
- The mechanism is `carries_a_secret_safely` and `host_in`, both already written and both already
  agreeing with `url` after X-19. This story is mostly about *which variables* go through them, plus
  the naming: `BACK_CHANNEL` is the wrong name once browser-facing endpoints are included, and a
  constant whose name contradicts its contents is worse than no constant.
- Say explicitly what this does **not** promise. X-19 recorded that `https` short-circuits before
  the host is examined, so this host vouches for the *channel* and never for *who is on the other
  end of it*. Extending the scheme check does not change that, and the doc should not imply it does.

## Progress
- **Done 2026-08-01.** Gate green: 39 + 166 tests, clippy clean, fmt clean. Genuine merge-base
  failure — the test asserts on the refusal *message* rather than the new variant, so it compiles
  and runs at the base and fails there.
- **UPGRADE RISK, and the reason this needs to be loud:** the one behaviour change is a *refusal*.
  Any deployment with an `http` authorization endpoint or redirect URI on a non-loopback address
  **loses `/api/signin` at startup** after upgrading, with a named message. That is the story's
  intent, but it is the first thing to check if sign-in "breaks" — look for `InsecureEndpoint` in
  the startup log.
- **Renames, since the story left them open.** `BACK_CHANNEL` -> `TRANSPORT_CHECKED`: what the four
  variables share is the check, not the direction of the request. `carries_a_secret_safely` ->
  `on_a_channel_this_host_will_use`, because the redirect URI carries no secret and the old name
  would be a lie for half the list. `InsecureBackChannel` -> `InsecureEndpoint`.
- **`ISSUER_ENV` stays unchecked, mechanically rather than in prose.** A test sets every required
  variable to a cleartext URL and asserts the refusal names exactly `TRANSPORT_CHECKED`. Nothing
  dials the issuer — this host does no discovery — so it is compared to the `iss` claim as a string.
  If that should change, the test fails loudly when someone changes it.
- **Non-fatality is asserted through the real router**, not argued: a test composes the state from
  the refusal and drives `/health` 200, the catalogue 200 and `/api/signin` 503. That needed one
  seam — `compose_oidc` now takes `Result<OidcConfig, ConfigRefusal>` rather than calling `from_env`
  itself, with the single production call site passing `from_env()`.
- **Carried forward:** `host_in`'s known conservatism (X-19: IPv4 shorthand, trailing dot, IDNA
  spellings of `localhost`) now covers two more variables, so its blast radius doubled. The remedy
  is unchanged — spell the address plainly — but it slightly raises the value of adopting the `url`
  crate, which X-19 left open as a dependency decision.
- **Known gap, not closed here:** `OidcConfig::for_test*` build the struct directly and bypass the
  check, so "every held `OidcConfig` is transport-checked" is not true of test builds. Arguably
  correct — those exist to construct configs the environment cannot — but it is not the stronger
  claim.
