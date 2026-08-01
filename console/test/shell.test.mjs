// The shell around the catalogue: what this console says it is, and what it refuses to pretend it
// can do.
//
// **The property this file exists for.** `docs/vision.md` gives the console two jobs — *wire things
// up* and *see what happened* — and the catalogue is neither; it is reference material about what
// this build could run. A console whose whole surface is the catalogue reads as a connector browser,
// so the shell states the platform's surfaces instead, and the reader can see the shape of the thing
// they are administering.
//
// Half of that shape is **not built**. `subscribe` and execution records do not exist, and `invoke`
// is a route with no screen to drive it from — two different gaps, which is why `surfaces.mts`
// answers `built` and `served` separately (X-42). `every_surface_is_marked_with_its_true_state`
// below asserts the first; the second is held against the server's own route table by the Rust gate.
// This repository's rule (`docs/vision.md` principle 7, and `AGENTS.md`) is that a page implying a
// working service costs more than an honest gap. A named, disabled entry is honest. A screen with a
// plausible empty state is a lie — and the difference between the two is exactly the kind of thing
// that decays silently, because a placeholder screen looks *better* than a disabled entry to anyone
// not asking whether it is real. So it is asserted, not intended:
//
//   1. a surface declared not-built carries no path;
//   2. no fragment the router can be handed resolves to one;
//   3. no app-layer source mounts a screen for one;
//   4. every href the shell renders resolves to a surface that *is* built.
//
// And, following `components.test.mjs`, the scanner behind (3) is run against a source it must
// reject, so it cannot decay into a vacuous pass by quietly failing to see anything.
//
// Node's built-in test runner with server-rendered render-function components, as
// `test/service.test.mjs` does and for the reason `src/CatalogueFailure.mts` sets out: a claim worth
// asserting deserves a real render rather than a grep over a template.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'

import { SURFACES, surfaceOfRoute } from '../src/surfaces.mts'
import { parseRoute } from '../src/routing.ts'
import { SESSION_ENDPOINT, SIGNIN_ENDPOINT, loadSession } from '../src/service.mts'
import ConsoleShell from '../src/ConsoleShell.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** The service this console administers, by the one name it is published under. */
const SERVICE_NAME = 'flux-exchange'

/** Render the shell in one session state, as a browser would see it. */
const shell = (session, active = null) =>
  renderToString(createSSRApp(ConsoleShell, { session, active }))

/** A resolved principal, in the shape `GET /api/session` publishes it. */
const principal = (over = {}) => ({ kind: 'user', id: 'alice', tenant: 'acme', ...over })

/** Every `href` in a fragment of HTML, in document order. */
const hrefs = (html) => [...html.matchAll(/href="([^"]*)"/g)].map((match) => match[1])

/** The value of one attribute on the element that carries `data-surface="<id>"`. */
function surfaceAttribute(html, id, attribute) {
  const element = new RegExp(`<[^>]*data-surface="${id}"[^>]*>`).exec(html)
  assert.ok(element, `the shell rendered no element for the \`${id}\` surface`)
  const value = new RegExp(`${attribute}="([^"]*)"`).exec(element[0])
  return value ? value[1] : null
}

// ---------------------------------------------------------------------------------------------
// What the shell is.
// ---------------------------------------------------------------------------------------------

test('the_shell_names_the_service_and_every_surface_it_has', async () => {
  const html = await shell({ status: 'ready', principal: null })

  assert.ok(
    html.includes(SERVICE_NAME),
    `the shell must name the service it administers; got: ${html}`
  )

  // A navigation region, not a scattering of links: a reader has to be able to see the platform's
  // shape in one place, which is the whole reason this story exists.
  assert.match(
    html,
    /<nav[^>]*data-shell="surfaces"/,
    'the shell must render a navigation region organised around the platform surfaces'
  )

  assert.ok(SURFACES.length > 0, 'no surfaces were declared; every assertion below would be vacuous')

  for (const surface of SURFACES) {
    assert.ok(
      html.includes(`data-surface="${surface.id}"`),
      `the shell renders no entry for the \`${surface.id}\` surface, so the platform hides part of its own shape`
    )
    assert.ok(
      html.includes(surface.label),
      `the \`${surface.id}\` surface is not named on the page; got: ${html}`
    )
  }
})

test('the_shell_covers_the_surfaces_the_vision_says_this_platform_has', () => {
  // Written out here rather than derived from `SURFACES`, so dropping a surface from the model
  // fails this test instead of quietly shrinking what the console claims to be.
  const expected = ['connections', 'grants', 'catalogue', 'identity', 'invoke', 'subscribe', 'activity']

  assert.deepEqual(
    SURFACES.map((surface) => surface.id).sort(),
    [...expected].sort(),
    'the set of surfaces the console presents changed; every entry is a claim about what this platform is'
  )
})

test('every_surface_is_marked_with_its_true_state', async () => {
  const html = await shell({ status: 'ready', principal: principal() })

  // The four that exist today, and the three that do not. This is the honest inventory the README
  // carries, restated where a reader of the console will actually meet it. `grants` joined the
  // first group in X-62: the service serves `GET`/`PUT /api/grants` and this console now has a
  // screen for it, which is the pair `built` and `served` exist to keep separable.
  const built = { connections: true, grants: true, catalogue: true, identity: true, invoke: false, subscribe: false, activity: false }

  for (const surface of SURFACES) {
    assert.equal(
      surface.built,
      built[surface.id],
      `the \`${surface.id}\` surface is declared ${surface.built ? 'built' : 'not built'} and it is not`
    )
    assert.equal(
      surfaceAttribute(html, surface.id, 'data-built'),
      String(built[surface.id]),
      `the \`${surface.id}\` entry does not render its true state, so the page and the model disagree`
    )
  }
})

// ---------------------------------------------------------------------------------------------
// The honesty invariant: a surface that is not built is unreachable.
// ---------------------------------------------------------------------------------------------

/**
 * Every route name an app-layer source decides a screen from.
 *
 * `parseRoute` is the only thing in this console that says which screen renders, and a screen is
 * mounted by comparing against the name it produced — so the names compared anywhere under `src/`
 * are the screens this app has. Scanning for the comparison rather than for the word means a
 * surface *named* in prose or in the model is not mistaken for a screen.
 */
function screensMountedBy(source) {
  const found = new Set()
  for (const match of source.matchAll(/name\s*(?:===|!==|:)\s*['"]([A-Za-z][A-Za-z0-9_-]*)['"]/g)) {
    found.add(match[1])
  }
  // `case 'operation':` inside a `switch (route.name)` is the same decision spelled differently.
  for (const match of source.matchAll(/\bcase\s+['"]([A-Za-z][A-Za-z0-9_-]*)['"]\s*:/g)) {
    found.add(match[1])
  }
  return found
}

/** Every app-layer source — the console's own, never the fifteen carried components. */
function appSources() {
  return readdirSync(path.join(consoleRoot, 'src'), { withFileTypes: true })
    .filter((entry) => entry.isFile() && /\.(vue|mts|ts)$/.test(entry.name))
    .map((entry) => path.join(consoleRoot, 'src', entry.name))
}

test('a_surface_that_is_not_built_is_unreachable', async () => {
  const absent = SURFACES.filter((surface) => !surface.built)
  assert.ok(absent.length > 0, 'nothing is declared unbuilt; this test would pass vacuously')

  // 1. The model itself. A path is somewhere to go, and there is nowhere to go.
  for (const surface of absent) {
    assert.equal(
      surface.path,
      null,
      `\`${surface.id}\` is declared not built and carries a path, which is the two halves of the model disagreeing`
    )
    assert.ok(
      surface.absent.length > 0,
      `\`${surface.id}\` is not built and says nothing about why; a disabled entry with no reason is a dead end`
    )
  }

  // 2. The router. Nothing a reader can type, guess or follow resolves to one of them.
  for (const surface of absent) {
    for (const guess of [`/${surface.id}`, `/${surface.id}/`, `/${surface.id}/anything`, `/${surface.id}s`]) {
      const route = parseRoute(`#${guess}`)
      assert.equal(
        route.name,
        'unknown',
        `\`${guess}\` resolves to \`${route.name}\`, so a surface that is not built has a route`
      )
    }
    assert.equal(
      surfaceOfRoute(surface.id),
      null,
      `a route named \`${surface.id}\` maps to a surface, so something intends to render one`
    )
  }

  // 3. The screens. No app-layer source decides anything from a route named after an unbuilt
  //    surface — which is what a placeholder screen would look like in the diff.
  const mounted = new Set(appSources().flatMap((file) => [...screensMountedBy(readFileSync(file, 'utf-8'))]))
  assert.ok(mounted.size > 0, 'no route comparison was found in any source; the scan is not working')

  for (const surface of absent) {
    assert.ok(
      !mounted.has(surface.id),
      `an app-layer source mounts a screen for \`${surface.id}\`, which is not built — an empty screen for a thing that does not exist is the lie this test exists to prevent`
    )
  }

  // 4. The page. Every link the shell renders goes somewhere real: a built surface, or the
  //    service's own sign-in route, which is not a fragment at all.
  const html = await shell({ status: 'ready', principal: principal() })
  const reachable = new Set(SURFACES.filter((surface) => surface.path).map((surface) => surface.path))

  for (const href of hrefs(html)) {
    if (href === SIGNIN_ENDPOINT) continue
    assert.ok(
      href.includes('#'),
      `the shell renders \`${href}\`, which is not a fragment — this console is one static document and a bare path 404s`
    )
    const target = decodeURIComponent(href.slice(href.indexOf('#') + 1))
    assert.ok(
      reachable.has(target),
      `the shell links to \`${target}\`, which is not a surface this build has`
    )
  }

  // 5. And the entries for them are inert: no href, and said to be inert for a screen reader too.
  for (const surface of absent) {
    const element = new RegExp(`<[^>]*data-surface="${surface.id}"[^>]*>`).exec(html)[0]
    assert.ok(!element.includes('href='), `the \`${surface.id}\` entry is a link to something that does not exist`)
    assert.match(
      element,
      /aria-disabled="true"/,
      `the \`${surface.id}\` entry looks disabled and does not say so, so a screen reader is told it is ordinary`
    )
  }
})

// The guard on the guard, in the shape `components.test.mjs` uses: a scanner that had silently
// stopped seeing route comparisons would leave the assertion above green and worth nothing.
test('the_screen_scanner_sees_the_screens_it_exists_to_catch', () => {
  const caught = [
    ["v-else-if=\"route.name === 'invoke'\"", 'invoke'],
    ['if (route.value.name === "activity") return null', 'activity'],
    ["const at = { name: 'subscribe' }", 'subscribe'],
    ["switch (route.name) { case 'activity': break }", 'activity'],
    ["route.name !== 'invoke'", 'invoke'],
  ]

  for (const [source, screen] of caught) {
    assert.ok(
      screensMountedBy(source).has(screen),
      `the scanner did not see the \`${screen}\` screen in: ${source}`
    )
  }

  // And it does not read a surface merely *named* as a screen — the model names all six, and the
  // shell renders all six, so a scanner that could not tell a name from a route would make the
  // honesty invariant unsatisfiable.
  const model = "{ id: 'invoke', label: 'Invoke', path: null }"
  assert.ok(!screensMountedBy(model).has('invoke'), 'the scanner read a declared surface as a screen')
})

// ---------------------------------------------------------------------------------------------
// Identity: who is signed in, read from the service and never invented.
// ---------------------------------------------------------------------------------------------

test('signed_out_offers_a_link_to_sign_in_and_never_a_fetch', async () => {
  const html = await shell({ status: 'ready', principal: null })

  assert.equal(surfaceAttribute(html, 'identity', 'data-session'), 'anonymous')
  assert.match(
    html,
    new RegExp(`<a[^>]*href="${SIGNIN_ENDPOINT}"`),
    `signing in must be an anchor to ${SIGNIN_ENDPOINT}: it answers 303 to the provider, and a fetch would follow the redirect in the page instead of navigating`
  )
})

test('signed_in_names_who_and_offers_sign_out', async () => {
  const html = await shell({ status: 'ready', principal: principal({ id: 'alice', tenant: 'acme' }) })

  assert.equal(surfaceAttribute(html, 'identity', 'data-session'), 'user')
  assert.ok(html.includes('alice'), `a signed-in shell must name who; got: ${html}`)
  // The tenant, because every credential address this console can show is derived from it — a
  // console that names the person but not the tenant cannot explain what it is showing.
  assert.ok(html.includes('acme'), `a signed-in shell must name the tenant; got: ${html}`)
  assert.match(html, /data-shell="signout"/, 'a signed-in shell must offer sign-out')

  const signin = new RegExp(`href="${SIGNIN_ENDPOINT}"`)
  assert.doesNotMatch(html, signin, 'a signed-in shell must not also offer sign-in')
})

test('a_session_that_could_not_be_read_is_never_rendered_as_signed_out', async () => {
  const failed = await shell({
    status: 'failed',
    failure: { kind: 'unreachable', endpoint: SESSION_ENDPOINT, status: null, detail: 'fetch failed' },
  })

  assert.equal(surfaceAttribute(failed, 'identity', 'data-session'), 'failed')
  assert.ok(
    failed.includes(SESSION_ENDPOINT),
    `a session that could not be read must name the endpoint that did not answer; got: ${failed}`
  )

  const anonymous = await shell({ status: 'ready', principal: null })
  assert.notEqual(
    surfaceAttribute(failed, 'identity', 'data-session'),
    surfaceAttribute(anonymous, 'identity', 'data-session'),
    'a service that did not answer and a caller who is signed out must never render alike'
  )

  const loading = await shell({ status: 'loading' })
  assert.equal(surfaceAttribute(loading, 'identity', 'data-session'), 'loading')
})

// ---------------------------------------------------------------------------------------------
// Reading the session. `service.mts` is the only module that knows a network exists.
// ---------------------------------------------------------------------------------------------

/** A stub service answering one status and body at `/api/session`. */
const answering = (status, body) => async () =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

test('a_401_is_nobody_signed_in_and_not_a_failed_read', async () => {
  const state = await loadSession({
    fetch: answering(401, { error: 'this route requires a principal and none was presented' }),
  })

  assert.equal(state.status, 'ready', 'a refusal to name a principal is an answer, not an outage')
  assert.equal(state.principal, null)
})

test('an_unreachable_service_is_not_nobody_signed_in', async () => {
  const state = await loadSession({
    fetch: async () => {
      throw new TypeError('fetch failed')
    },
  })

  assert.equal(state.status, 'failed')
  assert.equal(state.failure.endpoint, SESSION_ENDPOINT)
  assert.equal(state.principal, undefined, 'a failed read must carry no principal field at all')
})

test('a_resolved_principal_is_read_as_the_service_publishes_it', async () => {
  const state = await loadSession({
    fetch: answering(200, { principal: { kind: 'user', id: 'alice', tenant: 'acme' } }),
  })

  assert.equal(state.status, 'ready')
  assert.deepEqual(state.principal, { kind: 'user', id: 'alice', tenant: 'acme' })
})

// ---------------------------------------------------------------------------------------------
// The look, which has to survive both themes.
// ---------------------------------------------------------------------------------------------

test('the_shell_names_no_colour_of_its_own', () => {
  const css = readFileSync(path.join(consoleRoot, 'src', 'shell.css'), 'utf-8')
  const rules = css.replace(/\/\*[\s\S]*?\*\//g, '')

  // The carried components name no colour and neither does this: `tokens.css` is the one colour
  // vocabulary, and a second one would be a light-mode value hardcoded into a dark page.
  const literals = [...rules.matchAll(/#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g)].map((m) => m[0])
  assert.deepEqual(
    literals,
    [],
    'the shell stylesheet names a colour directly; every colour goes through a token in `tokens.css` so light and dark both work'
  )

  // And every token it reads is defined, because an undefined custom property resolves to nothing
  // and the element renders unstyled on a page that otherwise looks fine.
  const defined = new Set()
  for (const sheet of ['tokens.css', 'app.css']) {
    const source = readFileSync(path.join(consoleRoot, 'src', sheet), 'utf-8')
    for (const match of source.matchAll(/(--[A-Za-z0-9-]+)\s*:/g)) defined.add(match[1])
  }

  const read = [...rules.matchAll(/var\(\s*(--[A-Za-z0-9-]+)/g)].map((match) => match[1])
  assert.ok(read.length > 0, 'the shell stylesheet reads no token; this test would pass vacuously')
  assert.deepEqual(
    [...new Set(read)].filter((name) => !defined.has(name)),
    [],
    'the shell reads a custom property this console defines nowhere, so those rules paint nothing'
  )
})
