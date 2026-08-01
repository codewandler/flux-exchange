---
id: X-27
title: "Configuration is read by name, not by position"
status: done
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
- [x] **Failing-first test** — a deliberately mis-ordered read is caught. Since the point is to make
      the class impossible, this may be a compile-time argument rather than a runtime test: if so,
      demonstrate it by showing the old shape's failure mode and the new shape refusing to compile,
      and say plainly that is what you are showing.
- [x] Every variable lands in its own field, still asserted by
      `every_configured_value_lands_in_its_own_field` — kept green and **unmodified** if the new
      shape allows it; if it must change, say exactly why.
- [x] The refusal still names **every** unset variable in one message, in a stable order, so an
      operator fixes them in one pass. `the_refusal_names_every_unset_variable` stays green.
- [x] `TRANSPORT_CHECKED` and the read cannot drift: adding a variable to one without the other is a
      compile error, or is caught by a test that names the omission.
- [x] `ClientSecret`'s single-source claim survives — the environment stays the only way a secret
      enters, and `read` stays private to the module.

## Notes
- The shape suggested by two separate readers is a **struct with named fields** populated by name,
  so the compiler enforces the pairing that three parallel lists currently enforce by convention.
- Whatever the shape: the goal is that the *next* variable someone adds cannot be added wrongly.
  Judge the result by that, not by whether the diff is small.
- Do not widen this into a general configuration framework. One module reads one set of variables.

## Progress
- **Done 2026-08-01.** Gate green: 43 + 178 tests, clippy clean, fmt clean.
- **The proof is the strongest available for this shape of change, and it is worth reading.** At the
  base the implementor applied the X-04-shaped mistake — a new variable read and given a field, not
  added to `REQUIRED` — and ran the whole gate. **Build, 214 tests and clippy `-D warnings` all
  passed while the host ran with an empty value**, because `next()` walked off the end of an
  eight-element vec and `unwrap_or_default()` returned `""`. Nothing said a word. After the fix the
  same edit is three distinct compile errors (`E0560`, `E0063`, `E0027`).
- **`REQUIRED` and `TRANSPORT_CHECKED` are deleted, not kept and cross-checked.** Keeping them would
  have left the drift possible and merely *detected*; deriving both from the read means the second
  list does not exist. `transport_checked()` reports which variables the read took through the
  channel check, so there is nothing left to disagree with.
- **Behaviour is unchanged** — same refusals, same order, same messages.
  `the_refusal_names_every_unset_variable` is untouched and
  `every_configured_value_lands_in_its_own_field` keeps all eight accessor assertions; one line
  changed because the constant it counted no longer exists.
- The two new tests were **mutation-checked** rather than assumed: reading a bare literal instead of
  a documented constant, and fetching one value around the reader, each make them fail.
- **`ClientSecret`'s single-source claim survives** and is slightly stronger — the secret is wrapped
  on the line it is read, so no bare `String` outlives that expression, and `Supplied` derives no
  `Debug`.
- **Carried forward:** the one pairing a person can still get wrong is writing the wrong constant on
  a line (`issuer: reader.value(CLIENT_SECRET_ENV)`). It is local, glaring, and caught by
  `every_configured_value_lands_in_its_own_field` — but it is not a compile error, and making it one
  would need a dependency.
- Also carried: the quoted order rests on Rust evaluating struct fields in written order.
  **Correction, from X-27's review:** the note here originally named
  `an_empty_environment_names_every_variable_the_read_consumes` as the guard. It is not — it asserts
  `unset: required()`, and `required()` derives from the same read, so both sides move together. A
  reorder leaves the module green. What actually catches one is
  `the_refusal_names_every_unset_variable`, which pins two variables by name. Pre-existing gap, not
  introduced by the refactor: the same reorder passes at the base too.
- **Reviewed PASS.** The "behaviour unchanged" claim was verified *mechanically* rather than by eye:
  18 refusal scenarios rendered on both revisions, `sha256` identical (`c5a41e70…5142b`, 12027
  bytes, `diff` exit 0). The silent-failure base proof was reproduced independently in a fresh
  clone. `ClientSecret`'s single-source claim was proved rather than read — adding a `Debug` format
  to `Supplied` fails to compile.
