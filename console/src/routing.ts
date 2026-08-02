// The console's fragment router, including the catalogue finder's shareable view.

import { decodeSearchView, encodeSearchView, type SearchView } from './catalog.mts'
import { GRANTS_PATH } from './granting.mts'
import { AGENTS_PATH } from './minting.mts'
import { ONBOARDING_PATH } from './onboarding.mts'
import { nextTick, ref, type Ref } from 'vue'

export type PathResolver = (path: string) => string

/**
 * Where the console is served from, with exactly one trailing slash.
 *
 * `import.meta.env` is Vite's, and it does not exist under a plain `node --test`. Falling back to
 * `/` costs nothing in the bundle — Vite replaces the expression at build time — and it is what lets
 * this module be tested at all without standing up the bundler.
 */
const BASE = import.meta.env?.BASE_URL ?? '/'

/**
 * The catalogue's path as an href this document can follow.
 *
 * A fragment, because the console is one static document: a path router would need a server (or a
 * dev-server fallback plus a host rewrite) to hand every URL back to `index.html`, and there is no
 * server here to configure. The fragment is honest about that — the whole app is at `BASE`.
 *
 * A console path may carry an in-page anchor. On a path router that is an ordinary URL; here it
 * would be a second `#`, and a URL has one fragment. Encoding it keeps the delimiter unambiguous,
 * and [`parseRoute`] splits it back off.
 */
export const fragmentPath: PathResolver = (path) => `${BASE}#${path.replace(/#/g, '%23')}`

/** The route-local address of one finder view. */
export function explorerPath(view: SearchView): string {
  const search = encodeSearchView(view)
  return `/explorer${search ? `?${search}` : ''}`
}

/** Publish finder state without making every keystroke a Back-button entry. */
export function replaceExplorerView(view: SearchView): void {
  if (typeof window === 'undefined') return
  window.history.replaceState(window.history.state, '', fragmentPath(explorerPath(view)))
}

/**
 * One of the views this console renders, already resolved against the fragment.
 *
 * `anchor` is the in-page target the link asked for, absent when it asked for none. It is carried on
 * the route rather than read from `location` at scroll time because the fragment belongs to the
 * router here — by the time the browser sees it, it is spelled `%23` and is not an anchor it will
 * act on by itself.
 */
export type Route =
  | { name: 'connect'; anchor?: string }
  | { name: 'agents'; anchor?: string }
  | { name: 'connections'; anchor?: string }
  | { name: 'grants'; connector?: string; anchor?: string }
  | { name: 'invoke'; operation: string; anchor?: string }
  | { name: 'explorer'; view: SearchView; anchor?: string }
  | { name: 'operation'; id: string; returnView?: SearchView; anchor?: string }
  | { name: 'core'; kind: string; entry: string; anchor?: string }
  | { name: 'unknown'; path: string }

/**
 * The route a fragment names.
 *
 * Anything unrecognised becomes `unknown` carrying the path it could not place, rather than
 * silently redirecting to the explorer: a link that no longer resolves should say so, not quietly
 * show something else and let the reader believe they arrived.
 *
 * **This union is the complete list of screens this console has**, which is why the honesty
 * invariant in `test/shell.test.mjs` is stated over it: `invoke`, `subscribe` and `activity` are
 * named in `surfaces.mts` and appear in the navigation, and no fragment may resolve to any of them
 * while there is nothing behind them to render.
 */
export function parseRoute(hash: string): Route {
  const decoded = decodeURIComponent(hash.replace(/^#/, '')) || '/'

  // Split on the *first* `#` only: the anchor is an element id and cannot contain one, so anything
  // after a second `#` is part of the id and stays there.
  const split = decoded.indexOf('#')
  const addressed = split === -1 ? decoded : decoded.slice(0, split)
  const anchor = split === -1 ? undefined : decoded.slice(split + 1) || undefined

  const queryAt = addressed.indexOf('?')
  const path = queryAt === -1 ? addressed : addressed.slice(0, queryAt)
  const search = queryAt === -1 ? '' : addressed.slice(queryAt + 1)

  // Spread rather than always setting the key: a route with no anchor should not carry
  // `anchor: undefined`, which reads as "asked for nothing" and compares unequal to a bare route.
  const at = anchor ? { anchor } : {}

  // `/` is **connections**, not the catalogue. The console's two jobs are wiring things up and
  // seeing what happened; the catalogue is reference material about what this build could run, and
  // landing a reader there is what made this console read as a connector browser. The finder keeps
  // its own path so its tab and query can be copied, restored and navigated independently.
  if (path === '/' || path === '/connections') return { name: 'connections', ...at }

  // What this tenant may run. A surface of the platform rather than a footer reference, so unlike
  // `/connect` and `/agents` it maps to one in `surfaceOfRoute` and lights the rail — an operator
  // editing a grant is working, and needs to see where they are. A bare path with no segment: the
  // tenant comes from the resolved principal, and a grant is addressed by the connector *inside*
  // the body, which is the same shape `/api/grants` has for the same reason.
  if (path === GRANTS_PATH) {
    const connector = new URLSearchParams(search).get('connector')?.trim()
    return { name: 'grants', ...(connector ? { connector } : {}), ...at }
  }

  if (path === '/invoke') {
    const operation = new URLSearchParams(search).get('operation')?.trim() ?? ''
    return { name: 'invoke', operation, ...at }
  }

  // How to connect an agent. Deliberately not a surface of the platform — it is a reference an agent
  // author reaches for once rather than a place an operator works, so it is reached from the footer
  // and `surfaceOfRoute` maps it to nothing, leaving the rail with no entry lit.
  if (path === ONBOARDING_PATH) return { name: 'connect', ...at }

  // Where an operator mints one. A bare path with nothing in it, and it must stay that way: this is
  // the one screen in this console that holds a credential value, and a route that could carry a
  // segment is a value in the address bar and in every history entry after it. Like `/connect` it
  // is not a surface of the platform — `surfaceOfRoute` maps it to nothing — because it is
  // something an operator does with the identity they already have rather than a seventh place to
  // go. See `minting.mts` for why the name is `/agents`.
  if (path === AGENTS_PATH) return { name: 'agents', ...at }

  if (path === '/explorer') {
    // Before X-86 provider links were anchors into the four-column card grid. The grid is gone, so
    // treating that anchor as the connector query it always meant preserves the destination rather
    // than leaving a link that scrolls nowhere.
    const view = anchor
      ? { kind: 'connectors' as const, query: anchor }
      : decodeSearchView(search)
    return { name: 'explorer', view }
  }

  const operation = /^\/operations\/(.+)$/.exec(path)
  if (operation) {
    const params = new URLSearchParams(search)
    const returnView = params.has('return_kind') || params.has('return_q')
      ? decodeSearchView(`kind=${encodeURIComponent(params.get('return_kind') ?? '')}&q=${encodeURIComponent(params.get('return_q') ?? '')}`)
      : undefined
    return { name: 'operation', id: operation[1], ...(returnView ? { returnView } : {}), ...at }
  }

  const core = /^\/core\/([^/]+)\/(.+)$/.exec(path)
  if (core) return { name: 'core', kind: core[1], entry: core[2], ...at }

  // Deliberately the path without the anchor: an anchor cannot rescue a path that names no view, and
  // the reader needs to see which path failed, not where it would have scrolled.
  return { name: 'unknown', path }
}

/** Carry the old document-level `?q=` into X-86's route-local finder address. */
export function migrateLegacySearch(route: Route, search: string): Route {
  if (route.name !== 'explorer' || route.view.query) return route
  const query = new URLSearchParams(search.replace(/^\?/, '')).get('q')?.trim() ?? ''
  return query ? { name: 'explorer', view: { ...route.view, query } } : route
}

/**
 * The current route, kept in step with the address bar.
 *
 * Finder edits use `replaceState`, while following a link emits `hashchange`. This keeps typing out
 * of the Back stack and still makes browser navigation authoritative.
 */
export function useRoute(): Ref<Route> {
  const initial = migrateLegacySearch(parseRoute(window.location.hash), window.location.search)
  if (initial.name === 'explorer' && window.location.search) replaceExplorerView(initial.view)
  const route = ref<Route>(initial)
  window.addEventListener('hashchange', () => {
    route.value = parseRoute(window.location.hash)
    scrollToRoute(route.value)
  })
  return route
}

/**
 * Put the reader where the link pointed: at the anchor if it named one, at the top otherwise.
 *
 * The browser will not do this for us. The anchor reaches us percent-encoded — that is the whole
 * point of the encoding — so `location.hash` is not something it recognises as an element to jump
 * to, and the element does not exist yet anyway until the new view has rendered. Hence `nextTick`.
 *
 * A missing element scrolls to the top rather than staying put: a link into a provider that is no
 * longer served should leave the reader somewhere they can see that, not silently mid-page at
 * whatever the previous view's scroll offset happened to be.
 */
function scrollToRoute(route: Route): void {
  void nextTick(() => {
    const anchor = 'anchor' in route ? route.anchor : undefined
    const target = anchor ? document.getElementById(anchor) : null

    if (target) target.scrollIntoView()
    else window.scrollTo({ top: 0 })
  })
}
