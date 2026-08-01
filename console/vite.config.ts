import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

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
  // The service's own default bind, spelled once. Reading it from the environment would need
  // `@types/node`, and this file is type-checked by `vue-tsc` in the build gate — a dependency is
  // too much to pay for a value that changes when `FLUX_EXCHANGE_BIND` does, which is one edit here.
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:8080' },
    },
  },
})
