---
id: X-34
title: "The console presents an execution platform, not a catalogue browser"
status: done
epic: catalogue
areas: [console]
note: "owner-raised 2026-08-01: 'flux-exchange is the execution platform - not just a catalog explorer'. The console renders one reference view and no chrome, while the service behind it holds credentials for many tenants and runs operations for them"
---

# The console presents an execution platform, not a catalogue browser

## Goal
Someone opening the console sees an execution platform they administer — what is connected, what ran,
what could run — rather than a document about connectors.

## The framing, which is the substance of this story

`docs/vision.md` is unambiguous about what this service is and who the console is for:

> a service that **holds credentials, terminates channels, runs operations for many callers, and
> records what happened.**

> Its primary caller is an **agent**, not a human. People sign in **to wire things up and to see what
> happened**; agents are what call operations all day. … **the API is the product and the console is
> the admin surface**, not the other way round.

That gives the console exactly two jobs — *wire things up* and *see what happened* — and the
catalogue is **neither**. It is reference material: what this platform *could* run. Today it is the
entire console, which is why the console reads as a connector browser rather than as the admin
surface of something that executes.

**Reorganise around the two jobs.** Connections are how you wire things up and they are built. Records
are how you see what happened and they are not. The catalogue is supporting reference and should stop
being the front door.

## The honesty constraint, which is where this is easy to get wrong

`AGENTS.md` and the README are strict, and an admin surface for a platform that cannot yet execute is
the easiest place in the repository to violate them:

> keeping [the inventory] accurate is part of the job — a page that implies a working service costs
> more than an honest gap.

`invoke`, `subscribe` and execution records are **not built**. They must appear — a platform that
hides its own shape is no clearer — and they must be visibly unbuilt. A named, disabled entry is
honest. A screen with a plausible empty state is a lie, and this repository has spent several stories
removing exactly that class of thing.

## Acceptance
- [x] **Failing-first test** — a test asserts the shell renders the service's name and a navigation
      region organised around the platform's surfaces, and fails before the shell exists.
      `console/test/` uses `node --test` with server-rendered components; `CatalogueFailure.mts` is
      the precedent for a render function assertable without a browser.
- [x] Navigation covers **every surface, each marked with its true state**: Connections (built),
      Catalogue (built), Sign-in/identity (built), Invoke (**not built**), Subscribe (**not built**),
      Activity/records (**not built**).
- [x] **A test asserts every surface marked not-built is unreachable** — no route resolves to it and
      no placeholder screen exists. This is the honesty invariant, enforced rather than intended.
- [x] An identity affordance in the header: signed out offers **Sign in** (a link to `/api/signin` —
      it answers `303`, so it is a link and not a fetch); signed in names who, and offers sign-out.
      Read `/api/session`; do not invent a session.
- [x] The catalogue explorer keeps working, with its existing tests green and **unmodified**. This
      story reframes what surrounds it; it does not change it.
- [x] **No file under `console/src/components/` is modified.** Those 15 are shared with
      flux-connectors and `components.test.mjs` enforces it — anything the shell needs is a new file.
- [x] Light and dark both work through the existing `tokens.css`; no second colour vocabulary.

## Notes
- The console is **one static document** with a fragment router (`routing.ts`) because there is no
  server to route paths for it. Navigation goes through that router — `href="/connections"` would
  404. Read `routing.ts`'s own doc first; it argues this at length.
- `service.mts` is the single place that knows a network exists. Anything fetched belongs there
  beside `loadCatalogue`, in the same three-state shape (`loading` / `failed` / `ready`) and for the
  same reason: a service that is not running and a caller who is signed out must never render alike.
- **Connections has no view yet and this story does not have to build one** — the nav entry and the
  route are enough, and a follow-up can fill it. Say clearly which you did.
- This is the first UI work in a repository whose discipline is otherwise backend. Taste is yours;
  the constraints are the honesty invariant, the carried components, and the fragment router.

## Progress
- **Done 2026-08-01.** Console 21 -> 33 tests; Rust unchanged at 43 + 182; build clean.
- **The honesty invariant was negative-controlled, not asserted.** Three violations were introduced
  in turn — a not-built surface given a path, then a route, then a mounted screen — and **each prong
  was confirmed to fire on its own** before being restored. A green invariant test proves nothing by
  itself, and this is the first test in the repository written that way.
- **Deviation, argued and accepted: Connections got a view.** The story said a nav entry and a route
  were enough. A route with no view is a blank screen — worse than either option offered — and
  Connections *is* built at the API, so a disabled entry would have been a lie in the other
  direction. Read-only: it renders addresses and `held` flags, **never values**. No connect form;
  `POST` needs per-connector credential inputs and belongs in its own story.
- **Deviation, argued and accepted: `/` resolves to Connections rather than the explorer.** The Goal
  says a reader should see what is connected, and leaving the catalogue as the landing page would
  have left it the front door. `/explorer` keeps its own path, so every `/explorer#<provider>` link
  the carried components emit is unaffected and `routing.test.mjs` passes unmodified.
- **None of the 15 carried components is touched**, verified by path.
- **Carried forward — session-shape coupling.** `loadSession` reads `{principal:{id,tenant,kind}}`
  and `Principal`'s serde is derived, so a field rename in `exchange-host` breaks the header
  *silently* into `failed` rather than loudly. First place to look if the header goes blank.
- **Carried forward:** `signOut()` reloads, and if `DELETE /api/session` succeeds while the cookie
  survives (a proxy stripping `Set-Cookie`) the reload lands signed in again with no error shown —
  `App.vue` currently discards the failure `signOut` returns.
- **Carried forward:** the screen scanner is a regex over source text, so a screen mounted through an
  indirection would slip past that prong. The other three are structural, so a surface would still
  be unreachable.
