# flux-exchange console

A Vue 3 + Vite single-page app: the admin surface of a flux-exchange deployment.

## What it is for

`docs/vision.md` gives this console exactly two jobs — people sign in **to wire things up** and **to
see what happened** — and it is deliberate that neither of them is the connector catalogue. The
catalogue is *reference material*: what this build could run. Since X-34 the shell says so, and a
reader lands on **Connections** rather than on the explorer.

## What is not built, and how this console says so

`invoke`, `subscribe` and execution records do not exist. They are **named in the navigation**,
because a platform that hides its own shape is not clearer for it, and they are **inert**: no href,
`aria-disabled`, struck through, tagged `not built`, with the reason on the entry and a sentence
under the rail.

There is deliberately **no screen behind any of them**. A page headed "Activity" reading "No
executions yet" would claim that nothing has happened when the truth is that nothing can run, and
this repository has spent several stories removing exactly that class of thing. It is not a
convention anybody has to remember — `test/shell.test.mjs` asserts four separate things about it:
the model carries no path for an unbuilt surface, no fragment resolves to one, no source under
`src/` mounts a screen from a route named after one, and every link the shell renders resolves to a
surface that does exist. The scanner behind the third is run against sources it must reject, the way
`components.test.mjs` guards its own.

`src/surfaces.mts` is the one place any of that is stated. Adding a surface, or promoting one from
unbuilt to built, is an edit there and nowhere else.

## Status

**This reads the live catalogue, session and connections from the flux-exchange service.**

What that does and does not mean:

- **Connections is read-only.** It lists what this tenant has wired up, as **addresses and never
  values** — `GET /api/connections` publishes where each declared credential lives and whether
  something is stored there, and there is nowhere in this console for a value to appear. Connecting
  a connector is `POST /api/connections/{connector}`; there is no form for it yet.
- **Identity is read from `/api/session`, never invented.** Signed out offers a link to
  `/api/signin` — a link, because that route answers `303` to the identity provider and a `fetch`
  would chase the redirect inside the page. Signed in names the principal *and the tenant*, because
  every credential address on the page is derived from the tenant. A session that could not be read
  renders as **unknown**, never as signed out: an outage and a sign-out are different events.

- The console fetches `GET /api/catalogue/connectors` and then
  `GET /api/catalogue/connectors/{id}/operations` for each one. **Served from a different origin than
  the API, it shows a failure and no catalogue** — the endpoints are origin-relative.
- **If the service is not there, the console says so and names the endpoint.** It does not render an
  empty catalogue. "Zero connectors" and "the service did not answer" are different facts and the
  page shows different things for them; `test/service.test.mjs` is what holds that.
- **The served catalogue is thinner than the one these components were written for.** flux-exchange
  publishes what an operation *is* and what it costs — connector, service, description, risk,
  idempotency, effects — and nothing else. No request method or path, no parameters, no credentials,
  no hosts, no Flux source, and no Flux core catalogue. Those fields are shown empty rather than
  filled in with something plausible, and a catalogue-wide notice on every page says that an empty
  field there means *unpublished by this source*, not absent from the connector.
- **Nothing can be called.** There is no invoke route yet, so every operation reads "not live yet",
  which is true.
- **`admitted` is three-valued and the catalogue route answers `null` for every operation.** That
  route is anonymous — it says what *exists*, not what you may call — so the console renders the
  third state and never a refusal, whether or not somebody is signed in.
- **Effects may be inferred.** When the service reports `effects_derived: true` it worked the effects
  out from the operation itself rather than reading a declaration, and the operation page says which
  of the two it is looking at.

The fifteen explorer components carried over from
[flux-connectors](https://github.com/codewandler/flux-connectors) still render here unmodified. The
adapter that puts the served document into the shape they read is `src/service.mts`, and it is the
only module in this app that knows a network exists.

## Running it

```sh
npm install
npm run dev      # vite dev server
npm run build    # vue-tsc --noEmit && vite build
npm test         # node --test 'test/**/*.test.mjs'
npm run preview  # serve the built output
```

## Layout

```
index.html
vite.config.ts
tsconfig.json
src/
  main.ts              mounts the app; sets the theme before first paint
  App.vue              the root: routing, every three-state read, and the one `provide()`
  surfaces.mts         what this platform is — the six surfaces, and which three are not built
  ConsoleShell.mts     the chrome: the service's name, the surface rail, the identity affordance
  Connections.mts      what this tenant has wired up, as addresses and never values
  service.mts          the network — catalogue, session and connections, each in three states
  CatalogueFailure.mts what the page shows when there is no catalogue, naming the endpoint
  OperationFacts.mts   effects and admission — what the carried contract has no field for
  routing.ts           the fragment router and the PathResolver this host supplies
  theme.ts             light/dark, stored choice over OS preference
  tokens.css           the CSS custom properties the components speak
  app.css              the document baseline (tables, code, links, headings)
  shell.css            the shell's own rules — scanned by a test for colours it must not name
  catalog.mts          the catalogue's typed contract — types and pure selectors, no data
  components/          the fifteen carried components; see components/README.md
test/
  components.test.mjs  the boundary that keeps the components carryable
  shell.test.mjs       the surfaces, the identity affordance, and the honesty invariant
  routing.test.mjs     the fragment router, anchors included
  service.test.mjs     an unreachable service is never an empty catalogue
  discovery.test.mjs   the suite finds tests at every depth — see below
  discovery/
    subdirectory.test.mjs  the nested test that proves it
```

`shell.css` is a separate stylesheet rather than more of `app.css` for one reason: every colour in
it has to go through a token from `tokens.css`, and `the_shell_names_no_colour_of_its_own` scans
that file to hold it. One colour vocabulary, so light and dark both work without a second set of
values.

The quoting in `npm test` is load-bearing. `test/*.test.mjs` matches one directory level, so a test
filed under `test/<anything>/` never runs and the suite reports success without it — which is what
`test/discovery/subdirectory.test.mjs` exists to catch by running. The `**` must reach Node
unexpanded, because `npm` runs scripts through `sh`, and POSIX `sh` has no globstar: unquoted,
`test/**/*.test.mjs` is expanded by the shell as `test/*/*.test.mjs` and only the second level
survives. `test/discovery.test.mjs` asserts both properties, and measures the second against a
throwaway fixture rather than trusting a Node version's documented behaviour — on Node 22.23.1,
pointing `--test` at a directory does not enumerate it, it fails with `MODULE_NOT_FOUND`.

`CatalogueFailure.mts` and `OperationFacts.mts` are render functions rather than single-file
components, and that is deliberate: both carry a claim worth asserting — *the failure names the
endpoint*, *a null admission is not a refusal* — and a render function can be server-rendered by
`vue/server-renderer` under plain `node --test`, with no bundler and no new dependency. Written as
SFCs they could only have been checked by grepping a template, which is not evidence that anything
renders.

## The invariant this repository has to keep

The components under `src/components/` were copied, not rewritten. That was possible because of one
asserted property in their original home: **a component may import Vue, a sibling component, and the
catalogue's typed contract, and nothing else.** No framework, no `node:*`, no data loader — everything
a component renders arrives as a prop or as injected context.

[`test/components.test.mjs`](test/components.test.mjs) is the port of that assertion, and it is the
most valuable file here. The failure it prevents is invisible in the output: a component that imports
this console's service client, or a router, renders exactly the same page and silently stops being
mountable anywhere else. The test also checks itself — it runs its scanner against sources that must
be rejected, so it cannot decay into a vacuous pass.

This is why the fetch is at the app layer and not in a component. If one of the fifteen needs
something it does not have, that is a change to make **upstream in flux-connectors**, where they are
shared — not here.

If you add a component here, it obeys the same rule. If you need a component to know something,
pass it.

## What this host supplies

Three things, and deliberately only three.

**The data.** `src/service.mts` fetches, and `src/App.vue` passes it down as props. Nothing under
`src/components/` fetches, and nothing under `src/components/` can tell where the document came from
— which is the same property that let it come from a fixture before this. The shell and the
connections view are **not** under `src/components/` and must never be: they are this console's own,
and one of them imports the service client.

**A `PathResolver`.** `catalog.mts` answers *which page* (`/operations/<id>`,
`/core/<section>/<name>`); turning that into a followable href is the host's answer, injected under
`PATH_RESOLVER`. The components' default is identity, which would break every link here — this
console is one static document with no server to route those paths — so `src/App.vue` provides the
fragment resolver from `src/routing.ts`, and the fragment router resolves every path the catalogue
can produce. A `/core/…` path resolves to a statement that this source publishes no Flux core
entries, rather than to a blank page.

**The design tokens.** The components name no colour; they are written entirely against VitePress's
CSS variables. [`src/tokens.css`](src/tokens.css) defines that vocabulary with the values read out of
the built flux-connectors site, so the console inherits the docs site's identity without a single edit
to a component. Light and dark both, via `.dark` on the root element, which is where the components'
own dark rules expect it. `every_variable_the_components_use_is_defined` in the test suite fails if a
component reads a property the token layer forgot — an undefined custom property renders as nothing,
and that is the kind of breakage nobody notices.

## Licence

MIT OR Apache-2.0, as the rest of the repository.
