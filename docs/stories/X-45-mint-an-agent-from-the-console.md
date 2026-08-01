---
id: X-45
title: "An operator can mint an agent and see its token once"
status: done
epic: agent-access
design: docs/designs/agent-access.md
areas: [console]
note: "X-36 shipped POST /api/agents and nothing in the UI reaches it. The agent-onboarding page will tell an agent author to mint a token; this is where they do it"
---

# An operator can mint an agent and see its token once

## Goal
A signed-in operator can create an agent principal for their tenant and copy its token.

## Why now

X-36 made minting possible and deliberately shipped no UI. X-41 is about to publish a page telling an
agent author *how* to get an identity — and the answer it can give today is "ask a human to `curl`".
This closes that loop.

## The property that shapes the whole screen

**The token is shown once.** X-36's store keeps a digest, so this host genuinely cannot show it
again — that is the design, not a limitation to work around. The screen must make that unmistakable
*before* the operator navigates away, and must not offer any affordance that implies it can be
retrieved later.

X-34 recorded the trade this inherits: a cookie-carried caller **does** receive a readable token
here, unlike at `/api/session`. Cross-site is closed by `SameSite=Strict`; same-origin XSS is not and
cannot be, because the token is on the page by construction. The remedy is revocation.

## Acceptance
- [x] **Failing-first test** — minting from the console yields a screen presenting the token exactly
      once, and fails before it exists.
- [x] The token is **not persisted** by the console — not in `localStorage`, not in the URL, not in a
      route. A test asserts it appears nowhere but the DOM of that one view.
- [x] Navigating away and returning **cannot** show it again, asserted rather than assumed.
- [x] The screen states plainly that this host cannot show it again, and why — the store keeps a
      verifier, not the token.
- [x] It says what the token can and cannot do **today**: it authenticates nothing yet (X-37), and it
      authorises nothing beyond any principal (X-13, blocked). Derive that from `surfaces.mts` the way
      X-41 does, so it cannot rot.
- [x] Nothing under `console/src/components/` is modified.

## Notes
- **Wait for [X-40](X-40-who-may-mint-an-agent.md) if it has not landed**, or at minimum do not
  present minting as available to an agent principal. X-40 settles who may mint, and shipping a
  button before that decision would bake in the answer.
- Do not add a "copy to clipboard" that silently fails on a non-secure origin; if you offer copy,
  handle the failure visibly. An operator who thinks they copied a token they did not is worse off
  than one who selects it by hand.

## Progress
- **Done 2026-08-01.** Console 51 -> 66 tests; Rust unchanged. Genuine merge-base failure — 51 pass,
  **15 fail**, exactly the assertions this story adds.
- **The screen mints for itself, against `App.vue`'s usual arrangement**, and the argument is
  *lifetime* rather than style: `App.vue` is the root and **outlives every screen**, so a token handed
  to it would still be in memory after the reader navigated away, one `v-if` from being rendered
  again. Holding it in the view's own `setup` closure is what makes "navigating away" mean the state
  **ceasing to exist** rather than a handler remembering to clear it.
- **New test infrastructure earned rather than avoided:** a ~140-line mount harness over Vue's own
  `createRenderer`, **no dependency**. Server-rendering can assert what a page *says*; *"shown once"*,
  *"gone when torn down"* and *"not there when you come back"* are **lifecycle** claims, and the
  Acceptance asked for them asserted rather than assumed.
- **The response body's `shown: "once"` is deliberately not read.** A page that stated the one-shot
  property only when the service remembered to say so would fall silent exactly when the service
  changed. The screen states it from what the store holds; the fixture pins the field so upstream
  drift surfaces here.
- **Verified against a live service**, not only fixtures: the real `403` matches the fixture byte for
  byte and the real `201` matches its shape.
- **Carried forward:** `minting.mts`'s `MAY_MINT` is a courtesy copy of `routes::agents::MAY_MINT`
  with no mechanical link. If the route ever widens, the console silently withholds the form from a
  principal that would now be admitted — the asymmetry is deliberate (refusals render whole) but
  worth knowing.
- **Carried forward:** `v-show`/`<KeepAlive>` creeping into `App.vue`'s routing is the single change
  that would silently turn the token into something living behind the next screen. There is a test on
  it, but it is a regex over `App.vue`.
