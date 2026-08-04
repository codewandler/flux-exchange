# flux-exchange console

A Vue 3 + Vite single-page app: the admin surface of a flux-exchange deployment.

## What it is for

`docs/vision.md` gives this console exactly two jobs — people sign in **to wire things up** and **to
see what happened** — and it is deliberate that neither of them is the connector catalogue. The
catalogue is *reference material*: what this build could run. Since X-34 the shell says so, and a
reader lands on **Connections** rather than on the explorer.

## What is not built, and how this console says so

`subscribe` and execution records do not exist. They are **named in the navigation**,
because a platform that hides its own shape is not clearer for it, and they are **inert**: no href,
`aria-disabled`, struck through, tagged `not built`, with the reason on the entry and a sentence
under the rail.

There is deliberately **no screen behind either of them**. A page headed "Activity" reading "No
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

- **Connections is actionable, but still addresses and never values.** A searchable catalogue-backed
  picker reads the value-free `exchange.connection-plan.v2` projection and creates through one
  `exchange.local-management.v1` WebSocket ceremony. Secret controls become ordered raw `SECRET`
  frames and never enter JSON. Status cards show held addresses under progressive detail, and each
  held credential can still be rotated through the older atomic route until that separate consumer
  moves to the same ceremony.
- **Connect → Grant → Invoke is one visible journey.** Completion comes from the latest connection
  and grant responses. Grant presets compile to metadata selectors, preview groups the service's
  admitted answer by service and risk, and Invoke starts its JSON body from the runtime pack's exact
  projected input schema.
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
- **The catalogue finder renders only what flux-exchange publishes.** Connector vendor and
  description, derived services, and operation description, risk, idempotency and effects all come
  from the live API. It invents no method, path, parameter, credential, host or Flux source.
- **The service runs these operations, and the cards say so.** `POST
  /api/operations/{operation}/invoke` has been in the published surface since v0.7.0, so an operation
  of a connector this build carries is badged live rather than "not live yet" — the badge is a
  statement about *this deployment*, not about the reader. What it does not claim is that you may
  call it: that needs a principal, a connection and the credential the connector declares, and the
  catalogue-wide notice on the page is what draws that line. `src/service.mts` sets the flag and
  carries the argument for this reading and against the three tenant-specific ones (X-53). The
  signed-in Invoke screen now calls that API and preserves its `sent` and `retryable` distinctions.
- **`admitted` is three-valued and the catalogue route answers `null` for every operation.** That
  route is anonymous — it says what *exists*, not what you may call — so the console renders the
  third state and never a refusal, whether or not somebody is signed in.
- **Effects may be inferred.** When the service reports `effects_derived: true` it worked the effects
  out from the operation itself rather than reading a declaration, and the operation page says which
  of the two it is looking at.

Since X-86, flux-exchange owns its catalogue UI. One search field drives three tabs — Connectors,
Services and Operations — and the selected kind plus query live in the fragment route so the view is
shareable. An empty query browses the active kind, and connector or service results can narrow the
same finder to their operations. Channels will become a tab only when the service publishes real
channel metadata.

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
  App.vue              the root: routing and every three-state read
  surfaces.mts         what this platform is — the surfaces, including two future ones
  ConsoleShell.mts     the chrome: the service's name, the surface rail, the identity affordance
  Connections.mts      what this tenant has wired up, as addresses and never values
  Journey.mts          Connect → Grant → Invoke progress derived from server answers
  Invoke.mts           schema-backed invocation and result/refusal rendering
  service.mts          the network — catalogue, session and connections, each in three states
  CatalogueFailure.mts what the page shows when there is no catalogue, naming the endpoint
  CatalogueFinder.mts  one search field and the three result-kind tabs
  CatalogueOperation.mts operation detail from served facts only
  routing.ts           fragment routes, including shareable finder state
  theme.ts             light/dark, stored choice over OS preference
  tokens.css           the shared CSS custom-property vocabulary
  app.css              the document baseline (tables, code, links, headings)
  shell.css            the shell's own rules — scanned by a test for colours it must not name
  catalogue.css        responsive finder and operation-detail layout
  catalog.mts          served types, derived services, ranking and URL state
test/
  catalogue.test.mjs   search, ranking, tabs, drill-down and empty states
  components.test.mjs  catalogue data and design-token boundaries
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

`CatalogueFailure.mts`, `CatalogueFinder.mts` and `CatalogueOperation.mts` are render functions so
plain Node tests can render or mount their real output without adding a browser-only test harness.
The finder also exposes pure ranking and URL-state seams in `catalog.mts`.

## The invariant this repository has to keep

The catalogue UI belongs to flux-exchange and tracks the API it actually serves. It is not copied
from, packaged with, or synchronized against flux-connectors. The useful boundary remains local:
`service.mts` owns network access and three-state loading, `App.vue` passes completed data down, and
catalogue views render props without fetching. `test/components.test.mjs` holds that boundary.

## What this host supplies

Two things, deliberately kept explicit.

**The data.** `src/service.mts` fetches, validates and adapts the wire response; `src/App.vue` passes
the ready catalogue to the finder and operation detail. An unreachable request is decided before
either view renders.

**The routes.** `catalog.mts` encodes and decodes the active result kind and query. `routing.ts`
places that state after the hash because this console is one static document, and maps operation
links to the same fragment router.

The visual relationship is intentional but code-free: [`src/tokens.css`](src/tokens.css) retains
the VitePress-derived palette used by flux-connectors, while `catalogue.css` owns exchange-specific
layout. Light and dark both flow through `.dark` on the root element, and tests reject literal
colours or undefined custom properties in catalogue styles.

## Licence

MIT OR Apache-2.0, as the rest of the repository.
