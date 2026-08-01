// Where `vite dev` sends `/api`, and why that is a resolved value rather than a literal.
//
// **The property this file exists for.** The dev server is the one context in which the console and
// the service are not the same origin, so it proxies. X-69 walked the getting-started page, found
// the default port already taken, moved `FLUX_EXCHANGE_BIND` — and the proxy kept dialling the
// address written into the config file, which is a console that renders and reaches nothing. The
// first thing anybody does when a port is in use is change the port, so the target has to follow the
// setting the service reads rather than restate its default.
//
// No server is started here, and none needs to be: the target is a pure function of an environment,
// and the last test reads the config object the dev server would load. That last one is what keeps
// the rest honest — a resolver nothing called would satisfy every test above it and still leave the
// reader with a console that cannot reach the service.

import { test } from 'node:test'
import assert from 'node:assert/strict'

/** The setting the service reads its bind from. Spelled in `crates/exchange-server/src/bind.rs`. */
const BIND_ENV = 'FLUX_EXCHANGE_BIND'

/**
 * The resolver under test.
 *
 * Imported here rather than at the top of the file on purpose: a top-level import that cannot
 * resolve takes the whole file down with it, and the assertion worth reading when this feature is
 * missing is the last one — the address the config hands the dev server — not a module resolution
 * error standing in front of it.
 */
async function resolver() {
  return await import('../vite.proxy.mts')
}

test('with nothing set, the target is the address the service binds by default', async () => {
  const { apiProxyTarget, DEFAULT_BIND } = await resolver()

  assert.equal(apiProxyTarget({}), `http://${DEFAULT_BIND}`)
})

test('the target follows the bind the service was told to use', async () => {
  const { apiProxyTarget, BIND_ENV: exported } = await resolver()

  assert.equal(exported, BIND_ENV, 'the resolver reads a different setting than the service does')
  assert.equal(apiProxyTarget({ [BIND_ENV]: '127.0.0.1:9090' }), 'http://127.0.0.1:9090')
})

test('a blank setting is not an address, so the default still stands', async () => {
  // `FLUX_EXCHANGE_BIND=` is how a shell clears a variable, not how it names a host. The service
  // refuses to start on it and names the setting; a dev server that turned it into `http://` would
  // fail somewhere else entirely, and the reader would be debugging the wrong process.
  const { apiProxyTarget, DEFAULT_BIND } = await resolver()

  for (const blank of ['', '   ']) {
    assert.equal(apiProxyTarget({ [BIND_ENV]: blank }), `http://${DEFAULT_BIND}`)
  }
})

test('the address is dialled as it was written', async () => {
  // Including the shapes that are easy to be clever about. A bracketed IPv6 literal is already the
  // spelling a URL wants, and an unspecified address is what the operator said — rewriting either
  // would be this tree quietly repairing a configured value instead of using it.
  const { apiProxyTarget } = await resolver()

  assert.equal(apiProxyTarget({ [BIND_ENV]: '[::1]:8080' }), 'http://[::1]:8080')
  assert.equal(apiProxyTarget({ [BIND_ENV]: '0.0.0.0:8080' }), 'http://0.0.0.0:8080')
})

test('the dev server resolves its own /api target from the environment', async () => {
  // The wiring, asserted against the config object the dev server actually loads, and the reason
  // this file is not a test of a helper nobody calls. Set before the import, because the config
  // resolves its target while it is being evaluated.
  process.env[BIND_ENV] = '127.0.0.1:9091'

  const config = (await import('../vite.config.ts')).default

  assert.equal(
    config.server?.proxy?.['/api']?.target,
    'http://127.0.0.1:9091',
    `the dev server proxies /api somewhere other than the address ${BIND_ENV} names — a reader who moved the bind gets a console that renders and reaches nothing`,
  )
})
