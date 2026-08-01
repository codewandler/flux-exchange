// The console's half of the components' one port.
//
// `src/catalog.mts` answers *which page* — `/operations/<id>`, `/core/<section>/<name>` — and that
// answer is the catalogue's, identical wherever the components are mounted. Turning it into an href
// a browser can follow is the **host's** answer, and this file is where this host gives it.
//
// The default the components fall back to is identity, which is honest but wrong here: this console
// is a single HTML document with no server to route `/operations/<id>` for it, so an identity href
// would 404. So the resolver is the fragment router below, and the router is real — every path the
// catalogue can produce resolves to a view this app actually renders.

import type { PathResolver } from './catalog.mts'
import { nextTick, ref, type Ref } from 'vue'

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
 * A catalogue path may already carry an in-page anchor — `CatalogSnapshot` and `OperationDetail`
 * both emit `/explorer#<provider>`, and `ProviderCard` renders the matching `id`. On a path router
 * that is an ordinary URL. Here it would be a *second* `#`, and a URL has one fragment: the first
 * one swallows the rest, so `#/explorer#airtable` names the path `/explorer#airtable`, which matches
 * no route. Encoding it keeps the delimiter unambiguous, and [`parseRoute`] splits it back off.
 *
 * The components are right to emit it and must not learn about this; `PathResolver` is exactly the
 * seam where a host says how its own URLs are spelled.
 */
export const fragmentPath: PathResolver = (path) => `${BASE}#${path.replace(/#/g, '%23')}`

/**
 * One of the views this console renders, already resolved against the fragment.
 *
 * `anchor` is the in-page target the link asked for, absent when it asked for none. It is carried on
 * the route rather than read from `location` at scroll time because the fragment belongs to the
 * router here — by the time the browser sees it, it is spelled `%23` and is not an anchor it will
 * act on by itself.
 */
export type Route =
  | { name: 'explorer'; anchor?: string }
  | { name: 'operation'; id: string; anchor?: string }
  | { name: 'core'; kind: string; entry: string; anchor?: string }
  | { name: 'unknown'; path: string }

/**
 * The route a fragment names.
 *
 * Anything unrecognised becomes `unknown` carrying the path it could not place, rather than
 * silently redirecting to the explorer: a link that no longer resolves should say so, not quietly
 * show something else and let the reader believe they arrived.
 */
export function parseRoute(hash: string): Route {
  const decoded = decodeURIComponent(hash.replace(/^#/, '')) || '/'

  // Split on the *first* `#` only: the anchor is an element id and cannot contain one, so anything
  // after a second `#` is part of the id and stays there.
  const split = decoded.indexOf('#')
  const path = split === -1 ? decoded : decoded.slice(0, split)
  const anchor = split === -1 ? undefined : decoded.slice(split + 1) || undefined

  // Spread rather than always setting the key: a route with no anchor should not carry
  // `anchor: undefined`, which reads as "asked for nothing" and compares unequal to a bare route.
  const at = anchor ? { anchor } : {}

  if (path === '/' || path === '/explorer') return { name: 'explorer', ...at }

  const operation = /^\/operations\/(.+)$/.exec(path)
  if (operation) return { name: 'operation', id: operation[1], ...at }

  const core = /^\/core\/([^/]+)\/(.+)$/.exec(path)
  if (core) return { name: 'core', kind: core[1], entry: core[2], ...at }

  // Deliberately the path without the anchor: an anchor cannot rescue a path that names no view, and
  // the reader needs to see which path failed, not where it would have scrolled.
  return { name: 'unknown', path }
}

/**
 * The current route, kept in step with the address bar.
 *
 * `hashchange` only — the fragment is written by following a link, never by this app, so there is
 * nothing here that could push a history entry per keystroke the way `OperationList` warns about.
 */
export function useRoute(): Ref<Route> {
  const route = ref<Route>(parseRoute(window.location.hash))
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
