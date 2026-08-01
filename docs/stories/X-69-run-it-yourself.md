---
id: X-69
title: "The public site shows how to run this and sign in, in five minutes"
status: done
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
- [x] A visitor can follow the page start to finish and reach the console. **Verify by doing it**, on
      a clean checkout, and paste the transcript into the story. → the transcript is under *Progress*
      below; a real browser, driven through the page's own devtools step, ends signed in.
- [x] The loopback constraint appears **inside** the instruction block, not after it. →
      `web/getting-started.md:25-30`, held by `web/test/site.test.mjs`'s *the loopback constraint is
      inside the block a reader would copy, not under it*.
- [x] The page reaches the reader: linked from the site nav and from the landing page's hero actions,
      not only from the sidebar. → `web/.vitepress/config.mts:45` (nav, first), `:62` (sidebar),
      `web/index.md:9-11` (the brand hero action).
- [x] It carries the invoke prerequisite, so nobody is sent into `403 not_granted` unexplained. →
      `web/getting-started.md`'s *Before anything will run*, and both stores are already in the start
      block because the grant surface itself needs both.
- [x] **Failing-first test** — extend `web/test/site.test.mjs`: the built page names the loopback
      constraint, and no example contains anything credential-shaped. → three page tests plus two
      scanner self-tests. **The guard needed a repair rather than a widening**, see *Progress*.
- [x] No page claims a capability is or is not live — that is still [[X-64]]'s, and this page must not
      pre-empt it with prose. → the page's `IMPORTANT` block sends that question to
      `GET /api/onboarding`; every refusal it describes is a mechanism, not an inventory.

## Progress

**Done (X-69).** `web/getting-started.md` is the fourth page, first in the nav and the brand hero
action. What it cost, and what doing it found:

### The examples could not be written the obvious way, and the guard was the reason

`web/test/site.test.mjs` forbids a value on the right-hand side of an `=`, so
`FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme` is refused publication — **correctly**, because a roster
handle is a working credential and that is the whole premise of this page. `development_page()`
reaches the same conclusion for the same reason and names no roster entry either. So the page prints
**no handle, no address and no port**: every example is a placeholder the reader substitutes, the
handle spelled `<handle>` exactly as `/api/signin` spells it, and the address taken from the startup
log into a shell variable.

Two changes to the guard, and only the second is a widening:

1. **It could not see inside a code block at all.** The highlighter wraps every token in its own
   element, and the scan replaces a tag with a space, so `export FOO=bar` reads as `export FOO = bar`
   and no rule fires. **This site had no fenced block on any page until this story**, so the rule
   about what an *example* may contain had never been asked about an example. `codeBlocksOf` reads
   each block as the clipboard would carry it, and both scanning tests now run over prose *and*
   blocks. This is strictly stronger than before.
2. **`FOO=<a placeholder>` is now exempt.** A page that may not name the variable it is telling
   somebody to set cannot instruct. A placeholder is not a value, and it cannot be pasted and run —
   paste it verbatim and the process refuses to start naming the entry, which is demonstrated below.
   `the environment-variable rule admits a placeholder and still catches a value` pins the width.

### Walked, on a clean clone, on 2026-08-01

Cloned to a scratch directory and followed the page. Everything below is verbatim.

```text
$ cargo run          # with the placeholders pasted exactly as the page prints them
ERROR flux_exchange: FLUX_EXCHANGE_DEV_IDENTITY entry "<kind:id@tenant>" names kind "<kind".
                     Accepted kinds: user, agent, service

$ cargo run          # roster: user:me@my-team
 WARN flux_exchange: DEVELOPMENT identity armed by FLUX_EXCHANGE_DEV_IDENTITY. Any caller presenting
                     one of these handles becomes that principal, with no secret required. This host
                     will refuse to serve on any address but loopback while it is armed
                     roster=me -> User:me@my-team
 WARN flux_exchange: no connection-settings store is bound (FLUX_EXCHANGE_SETTINGS is unset) …
 INFO flux_exchange: grants: …/state/grants (file store, mode 0600, what each tenant may run)
 INFO flux_exchange: credentials: …/state/credentials (file store, mode 0600, not encrypted)
 INFO flux_exchange: route module="grants" path="/api/grants" access=PrincipalOfKind([User])
 INFO flux_exchange: flux-exchange is listening local=<the loopback address it bound>

$ curl "$exchange/api/signin"
<h1>Sign in with this host's development identity</h1>
<p>… Present the handle of a rostered principal as a bearer token — `Authorization: Bearer
<handle>` — and `POST /api/session` exchanges it for a session cookie. … The development identity is
deliberately unavailable on any address but loopback, because a handle is a name rather than a
secret.</p>

$ curl -i -X POST "$exchange/api/session" -H 'Authorization: Bearer me' -c cookies
HTTP/1.1 200 OK
set-cookie: __Host-flux_exchange_session=…; Path=/; Secure; HttpOnly; SameSite=Strict
{"principal":{"id":"me","kind":"user","tenant":"my-team"}, …}

$ curl -b cookies "$exchange/api/session"
{"principal":{"id":"me","kind":"user","tenant":"my-team"}}
```

Then the console, in a real browser (headless Chrome over CDP), doing exactly what the page's
*Reach the console* section says — `npm install`, `npm run dev`, and the one `fetch` from the
devtools console:

```text
--- signed out, as the page says you arrive ---
flux-exchange / CONSOLE / Not signed in / Sign in / …
--- the one request the page has the browser make ---
{"id":"me","kind":"user","tenant":"my-team"}
--- after a reload ---
flux-exchange / CONSOLE / me / my-team / Sign out / …
```

And the refusals the last section exists to explain, in the order a reader meets them:

```text
$ curl -b cookies "$exchange/api/grants"                      # with only FLUX_EXCHANGE_GRANTS set
503 {"error":"this host holds no grants: it needs a grant store (`FLUX_EXCHANGE_GRANTS`) … and a
     credential store (`FLUX_EXCHANGE_CREDENTIALS`) …","setting":"FLUX_EXCHANGE_GRANTS"}

$ curl -b cookies -X POST "$exchange/api/operations/airtable-record-get/invoke" -d '{}'
403 {"message":"principal `User:me@my-team` holds no grant admitting operation
     `airtable-record-get`","refusal":"not_granted","sent":"no"}

$ curl -b cookies -X POST "$exchange/api/grants/preview" -d '{"connector":"airtable", …}'
200 {"admits":[{"id":"airtable-record-get","risk":"low","idempotency":"idempotent", …}],"declares":4}

$ curl -b cookies -X PUT "$exchange/api/grants" -d '{"grants":[{"connector":"airtable", …}]}'
200 {"editable":true,"grants":[{"admits":[{"id":"airtable-record-get", …}], …}]}

$ curl -b cookies -X POST "$exchange/api/operations/airtable-record-get/invoke" -d '{}'
422 {"message":"config error: `airtable-record-get` needs a credential and none is stored at
     `tenants/my-team/com.airtable.api/access_token` — the request was not sent",
     "supply_at":"/api/connections/airtable/settings"}
```

**The walkthrough changed the page twice.** Both corrections are things reading the source would not
have produced:

1. **`FLUX_EXCHANGE_CREDENTIALS` had to join the start block.** The grant surface is reached through
   the invoker, and the invoker is bound only when *both* stores are — so a reader who set only
   `FLUX_EXCHANGE_GRANTS` cannot even read `/api/grants`, let alone write one. A page that sets one
   store leaves them at a `503` at the exact step this story exists to prevent.
2. **The console step is a devtools `fetch`, not a click.** A browser cannot put an `Authorization`
   header on a navigation and there is no sign-in form yet ([[X-58]]), so the page says so plainly
   rather than implying an affordance that is not there.

### Left for somebody else

- **The default bind was occupied on this machine**, so most of the walkthrough ran on a moved
  loopback bind via `FLUX_EXCHANGE_BIND`, and only the console leg used the default — the console's
  dev-server proxy hard-codes the default in `console/vite.config.ts`, so a reader who moves the bind
  cannot reach the console at all. That is a real gap in the local path and it belongs to `console/`,
  which this story does not touch.
- The site's off-site allow-list was not widened; the page links only to the repository.

## Notes
- `/api/signin` already answers this in prose on a development host, and
  `the_development_signin_page_explains_how_and_names_nobody` holds it. **Read that text and do not
  write a second, divergent one** — this epic exists because five renderings of one claim drifted.
  Either derive it or say plainly that the page restates it and which one is authoritative.
- The site's off-site link allow-list is `github.com` only. A link to `vitepress.dev` or `docs.rs`
  fails the suite; widening it is one line with a reason.
