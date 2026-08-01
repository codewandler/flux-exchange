---
id: X-27
title: "Configuration is read by name, not by position"
status: ready
priority: 2
epic: serve
areas: [exchange-server]
note: "raised by X-04's review and again by X-23's implementor, 2026-08-01: REQUIRED, the positional reads in OidcConfig::read, and TRANSPORT_CHECKED are three lists describing one set of variables, and the drift they permit has already shipped once"
---

# Configuration is read by name, not by position

## Goal
Adding an OIDC configuration variable cannot silently put a value in the wrong field.

## Why this keeps coming up

`OidcConfig::read` pulls values out of a `Vec` **positionally**, in an order that must match the
`REQUIRED` constant exactly. Three lists now describe one set of variables — `REQUIRED`, the
`next()` sequence in `read`, and `TRANSPORT_CHECKED`'s pairing — and a fourth would be one too many.

**The drift this permits has already shipped once.** X-04 added `TOKEN_ENDPOINT_ENV` and
`JWKS_URI_ENV` to `REQUIRED` and not to the test fixture, and five config tests failed; two reviewers
found it independently. The positional read itself was correct that time, but nothing checked it —
`every_configured_value_lands_in_its_own_field` was written afterwards, precisely because the class
of bug was real.

An off-by-one here does not fail loudly. It puts the client secret in the redirect URI field, or the
issuer in the token endpoint, and the host starts up and behaves wrongly.

## Acceptance
- [ ] **Failing-first test** — a deliberately mis-ordered read is caught. Since the point is to make
      the class impossible, this may be a compile-time argument rather than a runtime test: if so,
      demonstrate it by showing the old shape's failure mode and the new shape refusing to compile,
      and say plainly that is what you are showing.
- [ ] Every variable lands in its own field, still asserted by
      `every_configured_value_lands_in_its_own_field` — kept green and **unmodified** if the new
      shape allows it; if it must change, say exactly why.
- [ ] The refusal still names **every** unset variable in one message, in a stable order, so an
      operator fixes them in one pass. `the_refusal_names_every_unset_variable` stays green.
- [ ] `TRANSPORT_CHECKED` and the read cannot drift: adding a variable to one without the other is a
      compile error, or is caught by a test that names the omission.
- [ ] `ClientSecret`'s single-source claim survives — the environment stays the only way a secret
      enters, and `read` stays private to the module.

## Notes
- The shape suggested by two separate readers is a **struct with named fields** populated by name,
  so the compiler enforces the pairing that three parallel lists currently enforce by convention.
- Whatever the shape: the goal is that the *next* variable someone adds cannot be added wrongly.
  Judge the result by that, not by whether the diff is small.
- Do not widen this into a general configuration framework. One module reads one set of variables.
