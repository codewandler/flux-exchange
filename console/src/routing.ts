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
import { ref, type Ref } from 'vue'

/** Where the console is served from, with exactly one trailing slash. */
const BASE = import.meta.env.BASE_URL

/**
 * The catalogue's path as an href this document can follow.
 *
 * A fragment, because the console is one static document: a path router would need a server (or a
 * dev-server fallback plus a host rewrite) to hand every URL back to `index.html`, and there is no
 * server here to configure. The fragment is honest about that — the whole app is at `BASE`.
 */
export const fragmentPath: PathResolver = (path) => `${BASE}#${path}`

/** One of the views this console renders, already resolved against the fragment. */
export type Route =
  | { name: 'explorer' }
  | { name: 'operation'; id: string }
  | { name: 'core'; kind: string; entry: string }
  | { name: 'unknown'; path: string }

/**
 * The route a fragment names.
 *
 * Anything unrecognised becomes `unknown` carrying the path it could not place, rather than
 * silently redirecting to the explorer: a link that no longer resolves should say so, not quietly
 * show something else and let the reader believe they arrived.
 */
export function parseRoute(hash: string): Route {
  const path = decodeURIComponent(hash.replace(/^#/, '')) || '/'
  if (path === '/' || path === '/explorer') return { name: 'explorer' }

  const operation = /^\/operations\/(.+)$/.exec(path)
  if (operation) return { name: 'operation', id: operation[1] }

  const core = /^\/core\/([^/]+)\/(.+)$/.exec(path)
  if (core) return { name: 'core', kind: core[1], entry: core[2] }

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
    window.scrollTo({ top: 0 })
  })
  return route
}
