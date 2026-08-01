---
id: X-45
title: "An operator can mint an agent and see its token once"
status: ready
priority: 2
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
- [ ] **Failing-first test** — minting from the console yields a screen presenting the token exactly
      once, and fails before it exists.
- [ ] The token is **not persisted** by the console — not in `localStorage`, not in the URL, not in a
      route. A test asserts it appears nowhere but the DOM of that one view.
- [ ] Navigating away and returning **cannot** show it again, asserted rather than assumed.
- [ ] The screen states plainly that this host cannot show it again, and why — the store keeps a
      verifier, not the token.
- [ ] It says what the token can and cannot do **today**: it authenticates nothing yet (X-37), and it
      authorises nothing beyond any principal (X-13, blocked). Derive that from `surfaces.mts` the way
      X-41 does, so it cannot rot.
- [ ] Nothing under `console/src/components/` is modified.

## Notes
- **Wait for [X-40](X-40-who-may-mint-an-agent.md) if it has not landed**, or at minimum do not
  present minting as available to an agent principal. X-40 settles who may mint, and shipping a
  button before that decision would bake in the answer.
- Do not add a "copy to clipboard" that silently fails on a non-secure origin; if you offer copy,
  handle the failure visibly. An operator who thinks they copied a token they did not is worse off
  than one who selects it by hand.
