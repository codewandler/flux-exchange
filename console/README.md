# flux-exchange console

A Vue 3 + Vite single-page app that browses a Flux connector catalogue.

## Status

**This renders fixture data. There is no flux-exchange service.**

Read that plainly, because the console is designed to look finished and it is not backed by anything:

- There is **no API, no server and no generated catalogue.** Nothing on any page was fetched. The
  console makes no network request at all after loading its own bundle.
- Everything you see — the two connectors, their six operations, the credentials, the webhook
  binding, the three Flux core entries — is hand-written in
  [`src/fixtures/catalog.ts`](src/fixtures/catalog.ts). Every name in it is invented and every host is
  under `example.invalid`, the reserved TLD that resolves nowhere.
- There is **no API client**, not even a stubbed one. A module named `api.ts` that returned fixtures
  would be the same lie with better ergonomics, and would be the first thing someone mistook for
  working code.
- A banner says all of this at the top of every page. It is not dismissible, and it should not be
  made dismissible until it is no longer true.

What the console *is* for today: the fifteen explorer components carried over from
[flux-connectors](https://github.com/codewandler/flux-connectors) render, in this app, unmodified.
That is the thing being demonstrated. When a real catalogue exists, it replaces the fixture import in
`src/App.vue` and nothing else has to change.

## Running it

```sh
npm install
npm run dev      # vite dev server
npm run build    # vue-tsc --noEmit && vite build
npm test         # node --test test/*.test.mjs
npm run preview  # serve the built output
```

## Layout

```
index.html
vite.config.ts
tsconfig.json
src/
  main.ts              mounts the app; sets the theme before first paint
  App.vue              the shell: banner, header, routing, and the one `provide()`
  routing.ts           the fragment router and the PathResolver this host supplies
  theme.ts             light/dark, stored choice over OS preference
  tokens.css           the CSS custom properties the components speak
  app.css              the document baseline (tables, code, links, headings)
  catalog.mts          the catalogue's typed contract — types and pure selectors, no data
  fixtures/catalog.ts  the fixture catalogue, and the only data this console has
  components/          the fifteen carried components; see components/README.md
test/
  components.test.mjs  the boundary that keeps the components carryable
```

## The invariant this repository has to keep

The components under `src/components/` were copied, not rewritten. That was possible because of one
asserted property in their original home: **a component may import Vue, a sibling component, and the
catalogue's typed contract, and nothing else.** No framework, no `node:*`, no data loader — everything
a component renders arrives as a prop or as injected context.

[`test/components.test.mjs`](test/components.test.mjs) is the port of that assertion, and it is the
most valuable file here. The failure it prevents is invisible in the output: a component that imports
this console's fixtures, or a router, renders exactly the same page and silently stops being
mountable anywhere else. The test also checks itself — it runs its scanner against sources that must
be rejected, so it cannot decay into a vacuous pass.

If you add a component here, it obeys the same rule. If you need a component to know something,
pass it.

## What this host supplies

Two things, and deliberately only two.

**A `PathResolver`.** `catalog.mts` answers *which page* (`/operations/<id>`,
`/core/<section>/<name>`); turning that into a followable href is the host's answer, injected under
`PATH_RESOLVER`. The components' default is identity, which would break every link here — this
console is one static document with no server to route those paths — so `src/App.vue` provides the
fragment resolver from `src/routing.ts`, and the fragment router actually renders every path the
catalogue can produce.

**The design tokens.** The components name no colour; they are written entirely against VitePress's
CSS variables. [`src/tokens.css`](src/tokens.css) defines that vocabulary with the values read out of
the built flux-connectors site, so the console inherits the docs site's identity without a single edit
to a component. Light and dark both, via `.dark` on the root element, which is where the components'
own dark rules expect it. `every_variable_the_components_use_is_defined` in the test suite fails if a
component reads a property the token layer forgot — an undefined custom property renders as nothing,
and that is the kind of breakage nobody notices.

## Licence

MIT OR Apache-2.0, as the rest of the repository.
