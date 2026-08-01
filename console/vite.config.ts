import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

import { apiProxyTarget } from './vite.proxy.mts'

// Served from the site root today. The console's links go through the injected `PathResolver`
// (see `src/routing.ts`), so if this ever moves under a base path there is exactly one place to
// say so — `base` here, and the resolver reads it.
export default defineConfig({
  base: '/',
  plugins: [vue()],

  // The console fetches `/api/...` origin-relative, because in a deployment it is served by the
  // same host that answers those routes. `vite dev` is the one context where that is false: the
  // dev server owns this origin and the service is a separate process, so without this an
  // `/api/catalogue/connectors` fetch is answered by Vite's SPA fallback with `index.html`, and the
  // console reports "answered 200 with a body this console could not read".
  //
  // Deliberately a proxy rather than an absolute API base in the client: a base URL would have to be
  // configured in every deployment and would be wrong by default, whereas same-origin is correct
  // everywhere except here. `changeOrigin` is off — the service binds loopback and reads no Host.
  //
  // The target follows `FLUX_EXCHANGE_BIND` and falls back to the service's default (X-71). It used
  // to be the default, written out here, on the grounds that a reader who moves the bind can make
  // the one edit: X-69 walked its own page, met a port already in use, moved the bind, and got a
  // console that reached nothing — so the one edit is one nobody knows to make. `vite.proxy.mts`
  // resolves it, both so it can be asserted without a dev server and so no `@types/node` is needed
  // to read the setting.
  server: {
    proxy: {
      '/api': { target: apiProxyTarget() },
    },
  },
})
