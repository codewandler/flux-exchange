---
id: X-69
title: "The public site shows how to run this and sign in, in five minutes"
status: ready
priority: 1
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web]
note: "X-57 made signing in without an identity provider actually work on loopback; nothing public says so. This is the page that turns a charter into something a stranger can run"
---

# The public site shows how to run this and sign in, in five minutes

## Goal
Somebody who has never seen this project can clone it, start it, sign in, and reach the console —
from the public site, without reading the source.

## Why now

X-57 shipped in v0.9.0: `FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme` arms a development identity,
`GET /api/signin` explains the mechanism, and `POST /api/session` exchanges a roster handle for a
session cookie. **It works** — X-57's review drove it end to end and got a resolved principal and an
`HttpOnly; Secure; SameSite=Strict` cookie.

Nothing public says any of that. The site has three pages: the charter, the boundary argument, and an
index of the surface. A visitor can learn what this refuses to do and cannot learn how to start it.

That is the wrong order for a platform trying to get used.

## ⚠ The thing this page must not do

**A roster handle is a credential with no secret in it.** That is what makes it zero-setup, and it is
exactly why `admit_bind` refuses every non-loopback address while the development identity is armed —
`bind.rs`'s own words: *a reachable bind whose authentication is a name anybody can guess is worse
than no authentication, because the surface in front of it believes every caller.*

So the page carries that **at the point of instruction, not in a footnote below it.** Somebody
skimming for a copyable command must meet the constraint inside the block they are copying. A page
that explains local sign-in and then mentions loopback three screens later is a page that gets
someone to put a secret-free roster on a public address.

State plainly: **this is how you run it on your own machine. It is not how you deploy it.** A
reachable deployment needs a real provider — OIDC today, local users with an actual verifier when
[[X-58]] lands.

## What the page has to cover

- Clone, `cargo run`, what binds and where.
- Arming the roster, and what the entry syntax means — `user:alice@acme` is a kind, an id and a
  **tenant fixed at startup**, which is the property that makes it safe rather than a convenience.
- Signing in: the handle as a bearer token, `POST /api/session`, then the console.
- **What you can actually do once in**, and what you cannot. ⚠ v0.9.0 is fail-closed on invocation:
  nothing runs until `FLUX_EXCHANGE_GRANTS` names a file and a grant is written. Since X-62 there are
  routes for that; the console screen is unbuilt at time of writing. **A getting-started page that
  ends at "you are signed in" sends somebody straight into a `403 not_granted` with no explanation.**
- The malformed-roster behaviour: the process refuses to start and names the entry, rather than
  silently dropping it.

## Acceptance
- [ ] A visitor can follow the page start to finish and reach the console. **Verify by doing it**, on
      a clean checkout, and paste the transcript into the story.
- [ ] The loopback constraint appears **inside** the instruction block, not after it.
- [ ] The page reaches the reader: linked from the site nav and from the landing page's hero actions,
      not only from the sidebar.
- [ ] It carries the invoke prerequisite, so nobody is sent into `403 not_granted` unexplained.
- [ ] **Failing-first test** — extend `web/test/site.test.mjs`: the built page names the loopback
      constraint, and no example contains anything credential-shaped. The existing token-shape and
      IP-address guards already fire on violations; a page full of `Bearer alice` and `127.0.0.1` will
      meet them, so decide deliberately how the example is written rather than widening the guard.
- [ ] No page claims a capability is or is not live — that is still [[X-64]]'s, and this page must not
      pre-empt it with prose.

## Notes
- `/api/signin` already answers this in prose on a development host, and
  `the_development_signin_page_explains_how_and_names_nobody` holds it. **Read that text and do not
  write a second, divergent one** — this epic exists because five renderings of one claim drifted.
  Either derive it or say plainly that the page restates it and which one is authoritative.
- The site's off-site link allow-list is `github.com` only. A link to `vitepress.dev` or `docs.rs`
  fails the suite; widening it is one line with a reason.
