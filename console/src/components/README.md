# The explorer's components, in three tiers

> **These components arrived here from [flux-connectors][], where they were the explorer of the
> documentation site.** They were copied, not rewritten, and this file came with them. It has been
> edited only where the layout changed — the rules it states are unchanged, because the rules are
> precisely what made the copy possible.
>
> [flux-connectors]: https://github.com/codewandler/flux-connectors

These fifteen Vue components are the explorer. Since C-142 **none of them imports its host**, so the
set can be mounted somewhere other than the site it was written for — a product's own admin surface,
a Storybook, a test harness — without a rewrite and without extracting a package. This console is the
proof: it is that second host, and not one component needed a change beyond the path to
`catalog.mts`.

That is not an aspiration; `test/components.test.mjs` asserts it, in
`no_component_imports_the_site_framework` (the port of the assertion of the same name in
flux-connectors' `web/test/explorer.test.mjs`). A component may import **Vue**, a **sibling
component**, and **`../catalog.mts`**. Nothing else. In particular it may not import `node:*`, a
build-time data loader, or this console's service client (`../service.mts`): a component that
reaches for its own data cannot be attached anywhere, so everything a component renders arrives as a
prop or as injected context.

## The one thing a host has to supply

`catalog.mts` answers *which page* — `/operations/<id>`, `/core/<section>/<name>` — and that answer is
the catalogue's, identical wherever the components are mounted. Turning that path into an href a
browser can follow is the **host's** answer, and it differs: the docs site is served under a base path
and so has `withBase`; another host has its own router, or none at all.

So it is a port, not an import:

```ts
import { inject } from 'vue'
import { PATH_RESOLVER, identityPath, type PathResolver } from '../catalog.mts'

const resolvePath = inject<PathResolver>(PATH_RESOLVER, identityPath)
```

The default is **identity** — a host that says nothing leaves the path exactly as the catalogue gave
it, which is the honest behaviour and not a fallback that quietly breaks links. This console supplies
a fragment resolver from [`../routing.ts`](../routing.ts), provided in
[`../App.vue`](../App.vue); those two files are the only ones on this side of the boundary that know
how this app is routed.

## The other thing a host has to supply

These components name **no colour**. Every rule in them goes through a CSS custom property —
`--vp-c-text-2`, `--vp-c-divider`, `--vp-c-warning-soft` — which is the visual half of the same
property: a component that names no colour can be re-skinned by its host. This console defines that
vocabulary in [`../tokens.css`](../tokens.css) and changes no component to do it.

The cost of the indirection is that a property the host forgets to define resolves to *nothing*,
silently, and the element renders unstyled on a page that otherwise looks fine. So that is asserted
too, in `every_variable_the_components_use_is_defined`.

## The tiers

### Presentational — props only, no catalogue knowledge

Renders what it is given. Its props are strings, numbers and plain objects; it could be lifted into
any Vue application that has never heard of this catalogue.

| Component | Takes |
|---|---|
| `FluxSource.vue` | `source: string` |
| `SchemaBlock.vue` | `schema: unknown` |
| `SpecChip.vue` | `value: string` |

`FluxSource` deliberately does **not** highlight. Shiki has no Flux grammar, and colouring Flux by
another language's rules would be worse than plain text — so it is plain text, the bytes the emitter
produced. Do not "fix" this; `SchemaBlock` highlights because JSON and YAML are real grammars.

### Catalogue-aware — typed against `../catalog.mts`

Knows the *shape* of the catalogue and none of its contents. It still takes everything it renders as
a prop; the coupling is to the type, not to a source of data.

| Component | Takes |
|---|---|
| `IssueNotice.vue` | `issues: Issue[]` |
| `ParameterTable.vue` | `parameters: Parameter[]` |
| `StatusBadge.vue` | `operation: Operation` |
| `ProviderCard.vue` | `provider: Provider` |
| `InboundSurface.vue` | `provider: Provider` |
| `OperationRow.vue` | `operation: Operation` (+ `resolvePath`) |
| `CoreExplorer.vue` | `core: CoreCatalog` (+ `resolvePath`) |
| `CatalogSnapshot.vue` | `catalog: Catalog` (+ `resolvePath`) |

`CoreExplorer` holds local filter state. That is ephemeral view state, not routing — it is not in the
URL and nothing outside the component can observe it — so it stays in this tier.

### Page — owns routing and state

Mounted by a page, addressed by a URL, and the tier where a route parameter or the query string is
allowed to matter. In this console it is [`../App.vue`](../App.vue) that mounts them, against the
routes [`../routing.ts`](../routing.ts) parses; on the docs site it was a markdown page each.

| Component | Mounted by | Owns |
|---|---|---|
| `CatalogExplorer.vue` | route `/` | the explorer's composition and its headline counts |
| `OperationDetail.vue` | route `/operations/<id>` | resolving the `id` route parameter against the catalogue |
| `CoreDetail.vue` | route `/core/<kind>/<name>` | resolving the `kind`/`name` route parameters |
| `OperationList.vue` | `CatalogExplorer.vue` | the shareable view: the query string, read on mount and **replaced**, never pushed |

Two rules this tier exists to hold:

- **Read the URL on mount, not during setup.** There is no `location` while the page is being
  rendered, and reading one during setup would make the server's markup disagree with the client's
  first render. `OperationList` guards with a local `typeof window !== 'undefined'` rather than
  importing the framework's `inBrowser`.
- **Replace, never push.** A pushed history entry per filter change means the back button walks back
  through every keystroke of a search instead of leaving the explorer.

## And the rule that outranks all three

**No hand-written catalogue data, in any tier.** No component names a provider, a vendor, a service,
a host, a credential, an operation id or an issue code. A "reusable" component that hardcoded one of
those would trade the discipline the whole catalogue depends on for a convenience, which is the exact
failure the original repository exists to correct.

In flux-connectors this is enforced mechanically, by tests that check every component source against
the values in the generated `public/catalog.json`. **That check has not been ported, and it cannot be
yet: this console fetches its catalogue at runtime, so there is no committed artifact holding a set
of real values to check the sources against.** What holds the line here in the meantime is the
import rule above — a component cannot reach `../service.mts`, so it cannot launder the served
document into itself by importing it. Hardcoding a string is still possible and still forbidden.
When flux-exchange emits a catalogue artifact, port that test too.
