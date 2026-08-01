---
id: X-68
title: "The sign-in callback is the oracle it says it is"
status: ready
priority: 2
epic: local-identity
design: docs/designs/local-identity.md
areas: [exchange-server]
note: "found by X-57's review, 2026-08-01: the callback's comment says it 'stays the oracle for nothing', but ?error=access_denied answers 401 on a federated host and 400 on a development one — and the withheld-variable guard does not list the dev roster's own variable"
---

# The sign-in callback is the oracle it says it is

## Two findings from X-57's review, both small and both the same class

### 1. A comment claims more than the route does

`crates/exchange-server/src/routes/signin.rs:203-208` says the callback *"stays the oracle for
nothing"*. Measured:

- federated host, `GET /api/signin/callback?error=access_denied` → **401**, *"the identity provider's
  answer was not accepted"*
- development host, same request → **400**, *"this sign-in could not be matched to one that started
  here"*

So it distinguishes two kinds of provider that now both report `sign_in_available: true`. The
**scoped** half of the sentence is true — a forged `state` gets the same `400` on both — but the
general claim is not.

**Not a regression** (at base a development host answered `503` there, equally distinguishable), and
reachable only from loopback while the development identity is armed. It is a false sentence in the
file whose whole job this release was to make honest, and **no test covers the `error=` arm at all**.

### 2. The guard does not cover the variable most worth guarding

`signin.rs:440-449`'s `WITHHELD_FROM_THE_PAGE` lists the eight OIDC variables, and
`the_development_signin_page_explains_how_and_names_nobody` reuses it. The page does not name
`DEV_IDENTITY_ENV` today — verified — but **the one variable most worth withholding from *that
particular page* is the one the guard would not catch.** A list that omits its own subject is a guard
whose reach is narrower than its name, which this repository has now corrected five times.

## Acceptance
- [ ] **Failing-first test** — the `error=` arm is driven on both a federated and a development host,
      and whichever property is chosen is asserted. It is untested today in either shape.
- [ ] The comment either becomes true, or is narrowed to the claim that is true and names what it does
      not cover. **Do not widen the code to match the comment without deciding that is right** — making
      both hosts answer identically here may be the better answer, or may lose a distinction an
      operator needs.
- [ ] `WITHHELD_FROM_THE_PAGE` covers `DEV_IDENTITY_ENV`, and the test proves the guard fires on it —
      add it to the page temporarily and watch the test go red.

## Notes
- Both are loopback-bounded and neither is exploitable. This is about the gap between what the code
  says and what it does, on the route where that gap is least affordable.
