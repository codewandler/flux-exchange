// Minting an agent from the console, and the one property the whole screen is shaped by.
//
// **The property.** X-36 mints an agent principal and answers with its token **once**. That is not
// a UI convention this console chose and could relax — `crate::agent` stores a *verifier*, so this
// host is genuinely unable to say the token a second time. A screen that implied otherwise would be
// promising something no code here can deliver, and the operator would find out at the moment it
// costs them most: after they navigated away.
//
// So the assertions below are temporal, and that is why this file mounts the component for real
// (`test/mount.mjs`) rather than rendering it to a string. Three of them cannot be made any other
// way:
//
//   1. **once** — a mint puts the token on the page exactly one time, in one place, in text and
//      never in an attribute;
//   2. **gone** — unmounting the view (navigating away) takes it with it, and mounting the view
//      again (coming back) does not bring it back, because nothing outside the view ever held it;
//   3. **nowhere else** — not `localStorage`, not `sessionStorage`, not a cookie, not the fragment,
//      not the request URL. Asserted against live spies, not by reading the source — though the
//      source is scanned too, because a spy only proves the path that ran.
//
// The rest is X-40 and X-41. X-40 settled that only a `User` may mint, so this screen must not
// offer minting to a principal this host would refuse — and when it is refused anyway, the
// service's own sentence is what the operator reads. X-41 settled how a page states what this build
// can and cannot do: **derived from `surfaces.mts`**, one-directionally, so a claim can only ever
// leave the page and never arrive on it uninvited.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { mount, rendered, text, attributes, nodes, one } from './mount.mjs'

// Static, because these modules exist today and a missing *named* export from them would fail this
// file at link time rather than at an assertion. The two modules this story adds are imported
// dynamically below, so that "there is no mint screen" reads as a sentence and not as a stack trace.
import * as service from '../src/service.mts'
import { parseRoute } from '../src/routing.ts'
import { SURFACES } from '../src/surfaces.mts'
import { STEPS, ONBOARDING_PATH, available, withheld } from '../src/onboarding.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** One app-layer source, read whole. */
const source = (file) => readFileSync(path.join(consoleRoot, 'src', file), 'utf-8')

/**
 * The mint screen, or the reason there is not one.
 *
 * Dynamic on purpose. Before this story `src/Agents.mts` does not exist, and the failure a reviewer
 * should see for that is a sentence about the console's missing capability — not a module loader
 * stack trace that says nothing about why anybody cared.
 */
async function screen() {
  try {
    return (await import('../src/Agents.mts')).default
  } catch (error) {
    return assert.fail(
      `the console has no screen an operator can mint an agent from: \`src/Agents.mts\` did not load (${error.message}). X-36 shipped POST /api/agents and no UI, so the best answer this console can give an agent author is still "ask a human to curl"`
    )
  }
}

/** The mint screen's model — the path, who may mint, the clipboard, and the derived standing. */
async function minting() {
  try {
    return await import('../src/minting.mts')
  } catch (error) {
    return assert.fail(`\`src/minting.mts\` did not load (${error.message})`)
  }
}

// ---------------------------------------------------------------------------------------------
// Fixtures, copied from what the service actually answers.
//
// `crates/exchange-server/src/routes/agents.rs` is where each of these bodies is composed. They are
// copied rather than invented for the reason `test/connect.test.mjs` gives: a fixture somebody made
// up lets the console and the service drift while this file stays green.
// ---------------------------------------------------------------------------------------------

/**
 * The token, in the shape this host mints one.
 *
 * 64 hex characters, which is the shape `routes::agents::carries_a_token` looks for rather than a
 * field called `token` — so a fixture of the right *shape* is what makes the assertions below about
 * where it may and may not appear mean anything.
 */
const TOKEN = 'b3f1a90c47d25e6188ab0f73c5d94e2076bb18ff4a3c05d9e71286435fa0cd97'

/** When the minted agent's token stops resolving, as the service echoes it: seconds since epoch. */
const EXPIRES_AT = 1793491200

/** `POST /api/agents` answering `201`. The one disclosure, and the whole point of the route. */
const MINTED = {
  principal: { kind: 'agent', id: 'ci-runner', tenant: 'acme' },
  expires_at: EXPIRES_AT,
  token: TOKEN,
  shown: 'once',
}

/**
 * The `403`, verbatim — `routes::refuse_kind(MAY_MINT)` composes it from the declared kinds.
 *
 * This is the sentence that makes the console's own guess about who may mint a courtesy rather than
 * a rule: the console withholds the form to avoid offering an operator something they will be
 * refused, and the service is what actually decides.
 */
const MAY_NOT_MINT = 'this route admits only a principal of kind: user'

/** A resolved principal, in the shape `GET /api/session` publishes one. */
const principal = (over = {}) => ({ kind: 'user', id: 'alice', tenant: 'acme', ...over })

/** A session state carrying a resolved principal. */
const signedIn = (over = {}) => ({ status: 'ready', principal: principal(over) })

/**
 * A stub transport installed as the platform's, remembering everything it was asked.
 *
 * Installed globally rather than injected, deliberately: the screen mints by calling
 * `service.mintAgent` with no transport of its own, so this is the real path and not a seam opened
 * for the test.
 */
function serving(status, body) {
  const asked = []
  const previous = globalThis.fetch
  globalThis.fetch = async (url, init) => {
    asked.push({ url, init })
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'content-type': 'application/json' },
    })
  }
  return { asked, restore: () => (globalThis.fetch = previous) }
}

/** Fill the form and submit it, as an operator would. */
async function mintFrom(view, { id = 'ci-runner', days = '30' } = {}) {
  await view.fire(one(view.root, 'data-agents', 'id'), 'onInput', { target: { value: id } })
  await view.fire(one(view.root, 'data-agents', 'days'), 'onInput', { target: { value: days } })
  await view.fire(one(view.root, 'data-agents', 'mint-form'), 'onSubmit')
}

/** Every element on the page a reader could click. */
const clickable = (root) => nodes(root).filter((node) => typeof node.props.onClick === 'function')

/** How many times a string appears in another. */
function occurrences(haystack, needle) {
  let count = 0
  let at = haystack.indexOf(needle)
  while (at !== -1) {
    count += 1
    at = haystack.indexOf(needle, at + needle.length)
  }
  return count
}

/**
 * `SURFACES` with one surface's fields overridden — a hypothetical build, as `onboarding` does.
 *
 * A patch rather than a `built` boolean since X-42: what these screens derive from is `served`, and
 * a helper that could only move `built` would exercise nothing.
 */
const asIf = (id, patch) =>
  SURFACES.map((surface) => (surface.id === id ? { ...surface, ...patch } : surface))

/** Every app-layer source, with its comments removed — this repository documents by example. */
function appSourcesWithoutComments() {
  return readdirSync(path.join(consoleRoot, 'src'), { withFileTypes: true })
    .filter((entry) => entry.isFile() && /\.(vue|mts|ts)$/.test(entry.name))
    .map((entry) => ({
      name: entry.name,
      code: readFileSync(path.join(consoleRoot, 'src', entry.name), 'utf-8')
        .replace(/\/\*[\s\S]*?\*\//g, ' ')
        .replace(/<!--[\s\S]*?-->/g, ' ')
        .replace(/\/\/.*$/gm, ' '),
    }))
}

// ---------------------------------------------------------------------------------------------
// 1. The console can mint at all, and the token is on the page exactly once.
// ---------------------------------------------------------------------------------------------

test('an_operator_mints_from_the_console_and_the_token_is_shown_exactly_once', async () => {
  const Agents = await screen()
  const stub = serving(201, MINTED)

  try {
    const view = mount(Agents, { session: signedIn() })

    // Before the mint there is no token on the page and no request has been made. A screen that
    // fetched something on mount would be a screen that could show a token nobody asked for.
    assert.ok(
      !rendered(view.root).includes(TOKEN),
      'the mint screen renders a token before anybody minted one'
    )
    assert.equal(stub.asked.length, 0, 'the mint screen wrote to the service merely by being opened')

    await mintFrom(view, { id: 'ci-runner', days: '30' })

    // The write went where X-36 put it, as a POST, with the agent's name and an expiry the operator
    // stated. `expires_at` is never defaulted by this host — a body without one is refused — so a
    // console that omitted it would send a request that cannot succeed.
    assert.equal(stub.asked.length, 1, 'minting must cost exactly one request')
    const [{ url, init }] = stub.asked
    assert.equal(init.method, 'POST')
    assert.equal(url, '/api/agents')
    const sent = JSON.parse(init.body)
    assert.equal(sent.id, 'ci-runner', 'the agent is named by the operator, not by this console')
    assert.equal(typeof sent.expires_at, 'number', 'an agent token always carries an expiry')
    assert.ok(
      sent.expires_at > Math.floor(Date.now() / 1000),
      `the expiry sent (${sent.expires_at}) is already past, so this host would refuse it`
    )
    assert.equal(
      sent.tenant,
      undefined,
      'the console sent a tenant; the tenant is read from the resolved principal and from nothing a caller controls'
    )

    // The disclosure. Once, in text, and in exactly one place.
    const body = text(view.root)
    assert.ok(
      body.includes(TOKEN),
      `the token the service minted never reached the page, so an operator still has no way to get one; got: ${body}`
    )
    assert.equal(
      occurrences(body, TOKEN),
      1,
      'the token is rendered in more than one place, so "shown once" is already not true of this page'
    )
    assert.ok(
      !attributes(view.root).includes(TOKEN),
      'the token was rendered into an attribute — a value in markup is a value in a copied outerHTML, a screenshot of devtools, and anything that serialises the page'
    )

    // And the page says the thing that makes this the only chance to store it, with the reason.
    assert.match(
      body,
      /shown\s+once|only\s+time|cannot\s+show\s+it\s+again/i,
      `the screen must say plainly that this is the only time the token is shown; got: ${body}`
    )
    assert.match(
      body,
      /verifier/i,
      `the screen must say why it cannot be shown again — this host keeps a verifier, not the token; got: ${body}`
    )

    // The service's own claim about that, echoed by the fixture so a change of mind upstream shows
    // up here rather than as a page quietly saying something the service no longer does.
    assert.equal(MINTED.shown, 'once', 'the service no longer says this token is shown once')

    // Who was minted, in the tenant the caller was resolved to — the operator has to be able to
    // tell which agent this token belongs to before they leave the page.
    assert.ok(body.includes(MINTED.principal.id), `the screen must name the agent it minted; got: ${body}`)
    assert.ok(body.includes(MINTED.principal.tenant), `the screen must name the tenant; got: ${body}`)

    view.unmount()
  } finally {
    stub.restore()
  }
})

// ---------------------------------------------------------------------------------------------
// 2. Gone when the reader leaves, and it does not come back.
// ---------------------------------------------------------------------------------------------

test('navigating_away_and_returning_cannot_show_the_token_again', async () => {
  const Agents = await screen()
  const stub = serving(201, MINTED)

  try {
    const first = mount(Agents, { session: signedIn() })
    await mintFrom(first)
    assert.ok(rendered(first.root).includes(TOKEN), 'nothing was minted; the rest proves nothing')

    // Navigating away. The view is unmounted, and the token goes with it because the only thing
    // that ever held it was this view's own state.
    first.unmount()
    assert.ok(
      !rendered(first.root).includes(TOKEN),
      'the token survived the view being torn down, so it is held by something that outlives the screen'
    )

    // Coming back. A fresh view of the same screen, and no second mint.
    const asked = stub.asked.length
    const again = mount(Agents, { session: signedIn() })
    assert.ok(
      !rendered(again.root).includes(TOKEN),
      'returning to the mint screen showed the token again, which this host cannot do — it stores a verifier, so anything the console can show twice is something the console kept'
    )
    assert.equal(
      stub.asked.length,
      asked,
      'returning to the screen asked the service for something; there is no route that hands a minted token back, so whatever this was, it was not that'
    )
    again.unmount()

    // And there is nothing to ask. The service module exposes one way to obtain a token and it is
    // the mint itself: no reader, no cache, no "last minted".
    const readers = Object.keys(service).filter(
      (name) => /token/i.test(name) && !/^mintAgent$/.test(name)
    )
    assert.deepEqual(
      readers,
      [],
      `\`service.mts\` exports ${readers.join(', ')}; a second way to reach a token is a second place it can be shown twice`
    )
  } finally {
    stub.restore()
  }
})

test('the_screen_offers_no_affordance_that_implies_the_token_can_be_retrieved', async () => {
  const Agents = await screen()
  const stub = serving(201, MINTED)

  try {
    const view = mount(Agents, { session: signedIn() })
    await mintFrom(view)
    assert.ok(rendered(view.root).includes(TOKEN), 'nothing was minted; the rest proves nothing')

    // Dismissing it is one-way. This is the control an operator uses when they have stored the
    // token and do not want it on a screen behind them, and it must not be a toggle.
    const discard = one(view.root, 'data-agents', 'discard')
    assert.ok(discard, 'the screen offers no way to take the token off the page once it is stored')
    await view.fire(discard, 'onClick')

    assert.ok(
      !rendered(view.root).includes(TOKEN),
      'dismissing the token left it on the page'
    )
    assert.equal(
      one(view.root, 'data-agents', 'token'),
      null,
      'the element that held the token is still in the tree after it was dismissed'
    )

    // Nothing brings it back. Every clickable control still on the page is fired, and none of them
    // may produce a token — a "show again" is the affordance this whole screen exists not to have.
    for (const control of clickable(view.root)) {
      await view.fire(control, 'onClick')
    }
    assert.ok(
      !rendered(view.root).includes(TOKEN),
      'a control on the screen brought the token back after it was dismissed'
    )

    view.unmount()

    // And nothing in this console reaches for an agent by name. `/api/agents` is a collection with
    // no parameter and this build serves nothing under it — no listing, no revoke (X-38) — so a
    // console that spelled a path there would be inviting a `404` on the one screen an operator
    // reaches when a token has leaked.
    for (const { name, code } of appSourcesWithoutComments()) {
      assert.ok(
        !/\/api\/agents\//.test(code),
        `\`src/${name}\` names a route under /api/agents/, and nothing in this build serves one`
      )
    }
  } finally {
    stub.restore()
  }
})

// ---------------------------------------------------------------------------------------------
// 3. Nowhere but the DOM of that one view.
// ---------------------------------------------------------------------------------------------

test('the_token_is_persisted_nowhere_by_this_console', async () => {
  const Agents = await screen()
  const { AGENTS_PATH } = await minting()
  const stub = serving(201, MINTED)

  /** A store that remembers being written to. Node defines neither, so these are the only ones. */
  const spy = () => {
    const written = []
    return {
      written,
      store: {
        setItem: (key, value) => written.push([key, value]),
        getItem: () => null,
        removeItem: () => {},
        clear: () => {},
        key: () => null,
        length: 0,
      },
    }
  }

  const local = spy()
  const session = spy()
  Object.defineProperty(globalThis, 'localStorage', { value: local.store, configurable: true })
  Object.defineProperty(globalThis, 'sessionStorage', { value: session.store, configurable: true })

  try {
    const view = mount(Agents, { session: signedIn() })
    await mintFrom(view)
    assert.ok(rendered(view.root).includes(TOKEN), 'nothing was minted; the rest proves nothing')

    // 1. Not in a store the browser keeps.
    assert.deepEqual(local.written, [], 'the console wrote to localStorage while holding a token')
    assert.deepEqual(session.written, [], 'the console wrote to sessionStorage while holding a token')

    // 2. Not in the URL. Not the request's — a value in a query string is a value in an access log
    //    — and not the fragment this console routes on, which is the browser's history.
    for (const { url } of stub.asked) {
      assert.ok(!url.includes(TOKEN), `the token reached the request URL \`${url}\``)
    }
    assert.deepEqual(
      parseRoute(`#${AGENTS_PATH}`),
      { name: 'agents' },
      'the mint screen’s route carries fields; a route that can hold a token is a token in the address bar and in every history entry after it'
    )

    view.unmount()

    // 3. And it could not have been. A spy only proves the path that ran; this is every path.
    const onPath = ['Agents.mts', 'minting.mts', 'service.mts', 'App.vue']
    const sources = appSourcesWithoutComments().filter((entry) => onPath.includes(entry.name))
    assert.equal(sources.length, onPath.length, 'a source on the mint path was renamed or removed')

    for (const { name, code } of sources) {
      for (const store of [
        'localStorage',
        'sessionStorage',
        'document.cookie',
        'history.pushState',
        'history.replaceState',
        'location.hash',
      ]) {
        assert.ok(
          !code.includes(store),
          `\`src/${name}\` reaches for \`${store}\`; the token this host shows once must not be written anywhere it would outlive the view`
        )
      }
    }
  } finally {
    stub.restore()
    delete globalThis.localStorage
    delete globalThis.sessionStorage
  }
})

test('nothing_above_the_view_is_given_the_token_to_hold', async () => {
  const Agents = await screen()

  // The screen takes a session and nothing else. In particular it takes no minted agent: a token
  // arriving as a prop would be a token held by whatever passed it, which outlives the view.
  assert.deepEqual(
    Object.keys(Agents.props ?? {}),
    ['session'],
    'the mint screen takes a prop other than the session; the token must be produced inside the view and held nowhere above it'
  )

  // And the app wires it with exactly that. `App.vue` holds the catalogue, the session, the
  // connections and the connect outcome — the token is the one thing it must never see, which is
  // why this screen mints for itself rather than being handed a result.
  const app = source('App.vue')
  const mounted = /<Agents([^>]*)\/>/.exec(app)
  assert.ok(mounted, 'App.vue does not mount the mint screen, so nothing routes to it')
  assert.match(
    mounted[1],
    /^[\s]*:session="session"[\s]*$/,
    `App.vue passes the mint screen ${mounted[1].trim()}; it must pass the session and nothing else`
  )

  const { code } = appSourcesWithoutComments().find((entry) => entry.name === 'App.vue')
  assert.ok(
    !/\btoken\b/i.test(code),
    'App.vue names a token; the one component that outlives every screen must not be able to hold one'
  )

  // And the branch that mounts it must *destroy* it when the reader leaves. This is the load-bearing
  // half of "navigating away takes the token with it": a `v-if` tears the instance down and its
  // state with it, while a `v-show` leaves it mounted and holding the token behind whatever screen
  // the reader went to, and a `<KeepAlive>` around it does the same thing more deliberately.
  const branch = /<template\s+v-(?:if|else-if)="route\.name === 'agents'">([\s\S]*?)<\/template>/.exec(app)
  assert.ok(
    branch,
    'the mint screen is not mounted behind a route branch that destroys it; a `v-show` or a wrapper that keeps it alive would leave the token in memory on the page the reader navigated to'
  )
  assert.match(branch[1], /<Agents\b/, 'the agents branch mounts something other than the mint screen')
  assert.ok(
    !/keep-?alive/i.test(app),
    'App.vue keeps a screen alive across navigation, which is exactly what the mint screen must not survive'
  )
})

// ---------------------------------------------------------------------------------------------
// 4. X-40: only a `User` may mint, and this screen must not offer what would be refused.
// ---------------------------------------------------------------------------------------------

test('minting_is_offered_only_to_a_principal_this_host_would_admit', async () => {
  const Agents = await screen()
  const { MAY_MINT, mayMint } = await minting()

  assert.deepEqual(
    [...MAY_MINT],
    ['user'],
    'the console’s idea of who may mint no longer matches `routes::agents::MAY_MINT`, which admits a `User` and nothing else'
  )

  const form = async (session) => {
    const view = mount(Agents, { session })
    const state = {
      form: one(view.root, 'data-agents', 'mint-form') !== null,
      body: text(view.root),
      gate: one(view.root, 'data-agents', 'gate')?.props['data-state'] ?? null,
    }
    view.unmount()
    return state
  }

  // A signed-in human: the form.
  const user = await form(signedIn({ kind: 'user' }))
  assert.ok(user.form, 'a signed-in user is offered no way to mint, which is the whole story')
  assert.equal(user.gate, 'may-mint')

  // An agent and a service: refused by this host, so never offered here. X-40's argument is that
  // revocation must stay a remedy, and a console that offered the button would teach an operator
  // that it is available and let them discover the `403` themselves.
  for (const kind of ['agent', 'service']) {
    const other = await form(signedIn({ kind }))
    assert.ok(
      !other.form,
      `a \`${kind}\` principal is offered a mint form, and this host refuses one — the console must not offer what it knows will be refused`
    )
    assert.equal(other.gate, 'may-not-mint')
    assert.ok(
      other.body.length > 0 && /revok/i.test(other.body),
      `a \`${kind}\` is refused with no reason beside it; the reason is that every minter must itself be revocable, and a refusal with no reason reads as a broken console`
    )
    assert.equal(mayMint({ kind, id: 'x', tenant: 'acme' }), false)
  }

  // Signed out: the gate, and the way through it. Not an empty form.
  const anonymous = await form({ status: 'ready', principal: null })
  assert.ok(!anonymous.form, 'a signed-out reader is offered a mint form')
  assert.equal(anonymous.gate, 'anonymous')

  // A session that could not be read is never rendered as signed out — the same distinction the
  // shell holds one endpoint over.
  const unknown = await form({
    status: 'failed',
    failure: { kind: 'unreachable', endpoint: service.SESSION_ENDPOINT, status: null, detail: 'fetch failed' },
  })
  assert.ok(!unknown.form)
  assert.equal(unknown.gate, 'unknown')
  assert.ok(
    unknown.body.includes(service.SESSION_ENDPOINT),
    'a session that could not be read must name the endpoint that did not answer'
  )

  const loading = await form({ status: 'loading' })
  assert.equal(loading.gate, 'loading')
})

test('a_refusal_is_the_services_own_sentence_and_no_token_is_invented', async () => {
  const Agents = await screen()
  const stub = serving(403, { error: MAY_NOT_MINT })

  try {
    const view = mount(Agents, { session: signedIn() })
    await mintFrom(view)

    const body = text(view.root)
    assert.ok(
      body.includes(MAY_NOT_MINT),
      `a refusal must reach the operator in the service's own words; got: ${body}`
    )
    assert.equal(
      one(view.root, 'data-agents', 'token'),
      null,
      'a refused mint rendered a token element; there is no token'
    )
    view.unmount()
  } finally {
    stub.restore()
  }
})

test('a_mint_that_never_reached_the_service_is_not_a_refusal', async () => {
  const Agents = await screen()
  const previous = globalThis.fetch
  globalThis.fetch = async () => {
    throw new TypeError('fetch failed')
  }

  try {
    const view = mount(Agents, { session: signedIn() })
    await mintFrom(view)

    const body = text(view.root)
    assert.ok(
      body.includes('/api/agents'),
      `a failed write must name the endpoint that did not answer; got: ${body}`
    )
    assert.equal(one(view.root, 'data-agents', 'token'), null)
    // An outage and a refusal are different events, and an operator responds to them differently:
    // one is retried, the other is not.
    assert.equal(
      one(view.root, 'data-agents', 'refused'),
      null,
      'an unreachable service was rendered as the service refusing, which tells an operator this host said no when it said nothing at all'
    )
    view.unmount()
  } finally {
    globalThis.fetch = previous
  }
})

// ---------------------------------------------------------------------------------------------
// 5. X-41's rule: what the token can and cannot do is derived, never written.
// ---------------------------------------------------------------------------------------------

test('what_the_token_can_and_cannot_do_today_is_derived_from_surfaces', async () => {
  const Agents = await screen()
  const { tokenStanding, authorisation, MINT_STEP } = await minting()

  // The step this screen *is*. Everything `onboarding.mts` lists after it is something done while
  // *holding* the token — that ordering is the model's own, documented as "the order it happens to
  // them", and it is what the standing below is sliced from rather than a second list kept here.
  // Named so a rename in `onboarding.mts` fails loudly instead of silently emptying the slice.
  const at = STEPS.findIndex((step) => step.id === MINT_STEP)
  assert.notEqual(
    at,
    -1,
    `\`onboarding.mts\` declares no \`${MINT_STEP}\` step, so the mint screen is slicing what a token can do out of nothing`
  )

  const standing = tokenStanding(SURFACES)
  assert.deepEqual(
    standing.map((entry) => entry.step.id),
    STEPS.slice(at + 1).map((step) => step.id),
    'the screen states something other than the steps a token holder takes; reading the catalogue needs no token, and minting is what the operator is doing rather than what the token can do'
  )
  assert.ok(standing.length > 0, 'the screen states nothing about what the token can do')

  // Every entry agrees with the model, which agrees with `surfaces.mts`. This is the same
  // one-directional rule `onboarding.mts` states: a claim can only leave the page.
  for (const entry of standing) {
    assert.equal(
      entry.can,
      available(entry.step, SURFACES),
      `the mint screen presents \`${entry.step.id}\` as ${entry.can ? 'available' : 'unavailable'} and \`surfaces.mts\` says otherwise`
    )
    assert.equal(entry.reason, withheld(entry.step, SURFACES))
    if (!entry.can) {
      assert.ok(
        entry.reason.length > 0,
        `\`${entry.step.id}\` is withheld with no reason beside it`
      )
    }
  }

  // The two the acceptance names, on the page and not only in the model.
  const authenticate = STEPS.find((step) => step.id === 'authenticate')
  const invoke = STEPS.find((step) => step.id === 'invoke')
  assert.ok(authenticate && invoke)
  assert.equal(available(authenticate, SURFACES), false, 'X-37 has landed; this page needs rewriting')

  // **Inverted by X-42, and this is the correction rather than a relaxation.** This read
  // `assert.equal(available(invoke, SURFACES), false)`, which was true of the console's screens and
  // false of the service: `POST /api/operations/{operation}/invoke` shipped in v0.7.0 and has been
  // in the published route table ever since, while this screen told an operator a token could not
  // reach it. The route table decides it now — see
  // `routes::onboarding::tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`.
  assert.equal(
    available(invoke, SURFACES),
    true,
    'the service runs operations; this screen must not tell an operator otherwise'
  )

  const stub = serving(201, MINTED)
  try {
    const view = mount(Agents, { session: signedIn() })
    await mintFrom(view)
    const body = text(view.root)

    // It authenticates nothing yet (X-37), in `surfaces.mts`'s own words rather than in a second
    // sentence that could drift from it.
    assert.ok(
      body.includes(withheld(authenticate, SURFACES)),
      `the screen must say the token authenticates nothing yet, in the words the model already carries; got: ${body}`
    )
    // And it authorises nothing beyond any principal (X-13).
    assert.ok(
      authorisation(SURFACES).length > 0,
      'the screen states nothing about what the token authorises'
    )
    assert.ok(
      body.includes(authorisation(SURFACES)),
      `the screen must say what the token authorises today; got: ${body}`
    )
    view.unmount()
  } finally {
    stub.restore()
  }
})

test('the_derivation_is_live_and_takes_claims_off_the_page_rather_than_putting_them_on', async () => {
  const { tokenStanding, authorisation } = await minting()

  const stateOf = (surfaces, id) => tokenStanding(surfaces).find((entry) => entry.step.id === id)

  // `subscribe` rather than `invoke` since X-42: `invoke` is served, so driving the hypothetical
  // through it would be asserting a fact against itself and this test would move nothing.

  // As this build is.
  assert.equal(stateOf(SURFACES, 'subscribe').can, false)
  assert.ok(stateOf(SURFACES, 'subscribe').reason.length > 0)

  // As a build where the service serves it would be — no edit to this screen's copy, and none here.
  assert.equal(
    stateOf(asIf('subscribe', { served: true }), 'subscribe').can,
    true,
    'marking `subscribe` served does not change what this screen says a token can do, so the screen is not actually derived from `surfaces.mts`'
  )
  assert.equal(
    stateOf(asIf('subscribe', { served: true }), 'subscribe').reason,
    '',
    'the subscribe entry still carries a reason it cannot be done in a build where it can'
  )

  // The direction that protects a reader: a surface regressing to unserved withdraws the claim
  // standing on it, in the same expression and with nobody remembering to.
  assert.equal(stateOf(asIf('subscribe', { served: false }), 'subscribe').can, false)

  // And it reads the right field. `invoke` is the surface where "does the console have a screen"
  // and "does the service serve it" give different answers, so it is the one that proves this
  // screen is not deriving an API claim from the console's navigation.
  for (const built of [true, false]) {
    assert.equal(
      stateOf(asIf('invoke', { built }), 'invoke').can,
      true,
      `moving \`built\` to ${built} changed what this screen says a token can do, so it is still reading whether the console has a screen`
    )
  }

  // The authorisation sentence is one-directional too, and X-42 moved which event withdraws it.
  // It used to go the moment anything a token holder does became available; that fired on `invoke`
  // being corrected, which would have blanked the paragraph exactly when it stopped being vacuous.
  // It now goes when a token can actually be **presented**, because that is when "what may this
  // principal do" becomes the grant question (X-13) this screen must not answer from a surface list.
  assert.ok(
    authorisation(SURFACES).length > 0,
    'nothing is said about what a token authorises, which is the fact an operator minting one is owed'
  )
  // X-13 rewrote the sentence this used to pin. It said "no grant model", and asserting that string
  // was right while it was true; the claim now is the narrower one the build actually makes, and
  // this assertion moved with it rather than being dropped — a paragraph nobody pins is a paragraph
  // that outlives what it describes.
  assert.match(
    authorisation(SURFACES),
    /grant this tenant holds/i,
    'the sentence must say what now bounds a token — a grant — rather than describing a build that gated invocation by identity alone'
  )
  assert.doesNotMatch(
    authorisation(SURFACES),
    /no grant model/i,
    'the screen still tells an operator there is no grant model, which stopped being true in X-13'
  )
  assert.equal(
    authorisation(SURFACES, /* a build where a token can be presented */ true),
    '',
    'the screen goes on answering what a token authorises in a build where one can actually be presented — that is a grant question (X-13) and this page must stop answering it rather than answer it wrongly'
  )
})

// ---------------------------------------------------------------------------------------------
// 6. Copy, and the failure that is invisible if nobody makes it visible.
// ---------------------------------------------------------------------------------------------

test('a_copy_that_did_not_happen_says_so_on_the_page', async () => {
  const Agents = await screen()
  const stub = serving(201, MINTED)

  try {
    const view = mount(Agents, { session: signedIn() })
    await mintFrom(view)

    const copy = one(view.root, 'data-agents', 'copy')
    assert.ok(copy, 'the screen offers no way to copy the token')

    // This process has no clipboard, which is exactly the state a browser is in on a non-secure
    // origin: `navigator.clipboard` is undefined and `writeText` silently never happens. An
    // operator who believes they copied a token they did not is worse off than one who selected it
    // by hand, so the failure has to be on the page.
    await view.fire(copy, 'onClick')

    const body = text(view.root)
    assert.ok(
      one(view.root, 'data-agents', 'copy-failed'),
      `a copy that did not happen left nothing on the page saying so; got: ${body}`
    )
    assert.doesNotMatch(
      body,
      /\bcopied\b/i,
      `the page claims the token was copied when nothing was; got: ${body}`
    )
    assert.match(
      body,
      /secure origin|https|by hand|select/i,
      `a failed copy must tell the operator what to do instead; got: ${body}`
    )

    // And the token is still there to select. A failed copy that also hid the token would be the
    // worst of both.
    assert.ok(rendered(view.root).includes(TOKEN), 'a failed copy took the token off the page')
    view.unmount()
  } finally {
    stub.restore()
  }
})

test('the_clipboard_write_reports_what_actually_happened', async () => {
  const { writeClipboard } = await minting()

  // A clipboard that works.
  const written = []
  const ok = await writeClipboard(TOKEN, { writeText: async (value) => written.push(value) })
  assert.deepEqual(ok, { ok: true })
  assert.deepEqual(written, [TOKEN], 'the value was reported copied and never handed to the clipboard')

  // A clipboard that refuses — the permissions case, and the one a `.catch(() => {})` would eat.
  const refused = await writeClipboard(TOKEN, {
    writeText: async () => {
      throw new Error('Write permission denied.')
    },
  })
  assert.equal(refused.ok, false)
  assert.ok(
    refused.reason.includes('Write permission denied.'),
    `a refused copy must carry the browser's own words; got: ${JSON.stringify(refused)}`
  )

  // No clipboard at all — a non-secure origin. Not an exception, and not a success.
  const absent = await writeClipboard(TOKEN, undefined)
  assert.equal(absent.ok, false)
  assert.ok(absent.reason.length > 0, 'a copy with no clipboard reported no reason')
})

// ---------------------------------------------------------------------------------------------
// 7. Where it hangs, and what it is not.
// ---------------------------------------------------------------------------------------------

test('the_mint_screen_is_reachable_and_is_not_declared_a_platform_surface', async () => {
  const { AGENTS_PATH } = await minting()

  assert.equal(
    parseRoute(`#${AGENTS_PATH}`).name,
    'agents',
    'no fragment resolves to the mint screen, so nothing can reach it'
  )
  assert.notEqual(AGENTS_PATH, ONBOARDING_PATH, 'the mint screen and the onboarding page are one page')

  // Reachable from the footer, beside the page that sends an agent author here. Not from the rail:
  // `surfaces.mts` states what this platform *is*, and minting is something an operator does on the
  // identity it already has rather than a seventh surface.
  const app = source('App.vue')
  const footer = /<footer[^>]*>([\s\S]*?)<\/footer>/.exec(app)
  assert.ok(footer, 'App.vue renders no footer')
  assert.match(
    footer[1],
    /AGENTS_PATH/,
    'the footer does not link to the mint screen, so an operator has no way to find it'
  )
  assert.ok(
    !SURFACES.some((surface) => surface.path === AGENTS_PATH),
    'the mint screen is declared as a platform surface, which puts it in the main rail and claims this platform has a seventh surface'
  )

  // The onboarding page is where an agent author is told to get an identity; it must still be the
  // first branch of `<main>`, because it is the one screen that depends on nothing.
  const main = /<main>([\s\S]*?)<\/main>/.exec(app)
  assert.ok(main, 'App.vue renders no `<main>`')
  assert.equal(
    /v-(?:if|else-if)="([^"]*)"/.exec(main[1])[1],
    "route.name === 'connect'",
    'something is now evaluated before the onboarding page'
  )
})

test('the_mint_screen_does_not_reach_into_catalogue_views', async () => {
  const code = source('Agents.mts')
  assert.doesNotMatch(
    code,
    /from\s+['"]\.\/components\//,
    '`Agents.mts` imports a catalogue view; minting identity and browsing catalogue entries are separate surfaces'
  )
})

// ---------------------------------------------------------------------------------------------
// 8. The look, which has to survive both themes. The same scan the shell and onboarding run.
// ---------------------------------------------------------------------------------------------

test('the_mint_screen_names_no_colour_of_its_own', () => {
  const rules = source('agents.css').replace(/\/\*[\s\S]*?\*\//g, '')

  const literals = [...rules.matchAll(/#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g)].map((m) => m[0])
  assert.deepEqual(
    literals,
    [],
    'the mint screen’s stylesheet names a colour directly; every colour goes through a token in `tokens.css` so light and dark both work'
  )

  const defined = new Set()
  for (const sheet of ['tokens.css', 'app.css']) {
    for (const match of source(sheet).matchAll(/(--[A-Za-z0-9-]+)\s*:/g)) defined.add(match[1])
  }

  const read = [...rules.matchAll(/var\(\s*(--[A-Za-z0-9-]+)/g)].map((match) => match[1])
  assert.ok(read.length > 0, 'the mint screen’s stylesheet reads no token; this test would pass vacuously')
  assert.deepEqual(
    [...new Set(read)].filter((name) => !defined.has(name)),
    [],
    'the mint screen reads a custom property this console defines nowhere, so those rules paint nothing'
  )

  assert.match(
    source('app.css'),
    /@import\s+'\.\/agents\.css'/,
    'the mint screen’s stylesheet is never imported, so the screen renders unstyled'
  )
})
