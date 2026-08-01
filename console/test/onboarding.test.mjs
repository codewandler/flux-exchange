// What this service tells an agent, and what it is structurally unable to tell one.
//
// **The property this file exists for.** `docs/vision.md` calls the agent this platform's *primary*
// caller, and until this page existed nothing anywhere answered "what is this, and how do I connect
// to it?" for one. A page that answers it is easy. A page that is still *true* in two releases is
// the hard part, and it is the only part worth testing.
//
// The state being described changes weekly: an agent can be minted (X-36) and cannot yet
// authenticate (X-37), and can invoke nothing at all (X-12, blocked upstream). Prose describing that
// is false within a release. So the page does not describe it — it **derives** it from
// `src/surfaces.mts`, the same declaration the navigation reads, and the assertions below are that
// the derivation actually binds:
//
//   1. a step is available exactly when a *built* surface backs it, and a step no surface backs
//      cannot be available at all;
//   2. an unavailable step publishes no endpoint — an endpoint on the page is an invitation to call
//      it, and there is nothing to call;
//   3. an available step publishes one, so the day a surface flips to built this file goes red
//      until somebody states how;
//   4. and the derivation is live: given a surfaces array where `invoke` is built, the invoke step
//      becomes available with no edit here.
//
// (4) is what separates this from a page that merely happens to agree with the model today.
//
// The other half is disclosure. The page is reachable **signed out** — an agent that must
// authenticate to learn how to authenticate is a closed loop — so it must carry nothing belonging to
// a tenant. That is asserted structurally rather than by reading the copy: the element takes no
// props, its sources call no `fetch`, and every endpoint it names is parameterless.
//
// Node's built-in runner with a server-rendered render function, following `test/shell.test.mjs`
// and for the reason `src/CatalogueFailure.mts` sets out: a claim worth asserting deserves a real
// render rather than a grep over a template.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'

import { SURFACES } from '../src/surfaces.mts'
import { CONNECTORS_ENDPOINT } from '../src/service.mts'
import { parseRoute } from '../src/routing.ts'
import { ONBOARDING_PATH, STEPS, available, backing, withheld } from '../src/onboarding.mts'
import AgentOnboarding from '../src/AgentOnboarding.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** One app-layer source, read whole. */
const source = (file) => readFileSync(path.join(consoleRoot, 'src', file), 'utf-8')

/** The page as a browser would see it. It takes no props, which is half the point. */
const page = () => renderToString(createSSRApp(AgentOnboarding))

/** The step with this id, or a failure naming it — never `undefined` flowing into an assertion. */
function step(id) {
  const found = STEPS.find((candidate) => candidate.id === id)
  assert.ok(found, `the onboarding model declares no \`${id}\` step`)
  return found
}

/** The value of one attribute on the element that carries `data-step="<id>"`. */
function stepAttribute(html, id, attribute) {
  const element = new RegExp(`<[^>]*data-step="${id}"[^>]*>`).exec(html)
  assert.ok(element, `the page rendered no element for the \`${id}\` step`)
  const value = new RegExp(`${attribute}="([^"]*)"`).exec(element[0])
  return value ? value[1] : null
}

/**
 * `SURFACES` with one surface's fields overridden — a hypothetical build, for (4) above.
 *
 * Takes a patch rather than a `built` boolean since X-42: the field this page derives from is
 * `served`, and a helper that could only move `built` would have quietly stopped exercising the
 * derivation at all.
 */
const asIf = (id, patch) =>
  SURFACES.map((surface) => (surface.id === id ? { ...surface, ...patch } : surface))

// ---------------------------------------------------------------------------------------------
// Honest by construction: what the page claims is derived, not written.
// ---------------------------------------------------------------------------------------------

test('the_page_cannot_claim_a_capability_this_service_does_not_serve', async () => {
  const html = await page()

  assert.ok(STEPS.length > 0, 'the onboarding model declares no steps; everything below is vacuous')
  assert.ok(
    SURFACES.some((surface) => !surface.served),
    'nothing is declared unserved, so this test could not catch the thing it exists for'
  )

  // Both halves are non-empty. A model where everything is withheld would pass the honesty
  // assertions while telling an agent author nothing, and one where everything is claimed would
  // pass them while claiming a platform that does not exist.
  const claimed = STEPS.filter((entry) => available(entry))
  const withheldSteps = STEPS.filter((entry) => !available(entry))
  assert.ok(claimed.length > 0, 'the page claims nothing works; there is at least one thing that does')
  assert.ok(withheldSteps.length > 0, 'the page withholds nothing, and this build cannot do everything')

  for (const entry of STEPS) {
    const surface = backing(entry)

    // 1. Availability is the model's, not the author's: a surface the *service* serves backs it, or
    //    it is withheld. `served` and not `built` — the second answers whether this console has a
    //    screen, and deriving an API claim from it is what made this page say `invoke` could not be
    //    called while the route was in the published surface.
    assert.equal(
      available(entry),
      surface !== null && surface.served,
      `the \`${entry.id}\` step is presented as ${available(entry) ? 'available' : 'unavailable'} and its backing surface says otherwise`
    )

    if (surface && !surface.served) {
      assert.equal(
        available(entry),
        false,
        `\`${entry.id}\` is presented as available on the strength of \`${surface.id}\`, which this build says the service does not serve`
      )
    }

    if (!surface) {
      assert.equal(
        available(entry),
        false,
        `\`${entry.id}\` is presented as available and no surface in \`surfaces.mts\` backs it — nothing may claim to work on this page unless the console already says that surface exists`
      )
    }

    // 2 & 3. The call. Withheld means nothing to call; available means say what to call, so a
    //        surface flipping to built cannot land as an unexplained "you can now".
    if (available(entry)) {
      assert.ok(
        entry.call,
        `\`${entry.id}\` is available and names no call, so the page says it works without saying how`
      )
    } else {
      assert.equal(
        entry.call,
        null,
        `\`${entry.id}\` cannot be done and names an endpoint anyway — an endpoint on this page is an invitation to call it`
      )
      assert.ok(
        withheld(entry).length > 0,
        `\`${entry.id}\` is withheld with no reason beside it, which reads as a broken page rather than as a statement about the service`
      )
    }

    // And the page renders the model rather than a second opinion of it.
    assert.equal(
      stepAttribute(html, entry.id, 'data-available'),
      String(available(entry)),
      `the \`${entry.id}\` step renders a different state from the one the model derives`
    )
  }
})

test('the_derivation_is_live_and_not_a_coincidence', () => {
  // `subscribe` and not `invoke`, since X-42: `invoke` is now served, so using it here would be
  // asserting a hypothetical against a fact and the test would stop moving anything.
  const subscribe = STEPS.find((entry) => entry.surface === 'subscribe')
  assert.ok(subscribe, 'no step is backed by the `subscribe` surface, so this test proves nothing')

  // As this build is.
  assert.equal(available(subscribe, SURFACES), false)
  assert.ok(withheld(subscribe, SURFACES).length > 0)

  // As a build where the service serves it would be — no edit to the copy, and none to this file.
  assert.equal(
    available(subscribe, asIf('subscribe', { served: true })),
    true,
    'marking `subscribe` served does not make the subscribe step available, so the page is not actually derived from `surfaces.mts`'
  )
  assert.equal(
    withheld(subscribe, asIf('subscribe', { served: true })),
    '',
    'the subscribe step still carries a reason it cannot be done in a build where it can'
  )

  // And the other direction, which is the one that protects a reader: a surface that regresses to
  // unserved takes its claim off the page with it.
  const minted = step('be-minted')
  assert.equal(available(minted, SURFACES), true)
  assert.equal(
    available(minted, asIf(minted.surface, { served: false })),
    false,
    'a surface regressing to not-served leaves its claim standing on the page'
  )

  // And the field that is *not* the one this page derives from cannot move a claim. `invoke` is
  // the surface where the two answers differ, so it is the one that proves the page reads the
  // right one: a console that grew a screen for it, or lost one, changes nothing here.
  const invoke = step('invoke')
  assert.equal(available(invoke, SURFACES), true, '`invoke` is served: the route is in the surface')
  for (const built of [true, false]) {
    assert.equal(
      available(invoke, asIf('invoke', { built })),
      true,
      `moving \`built\` to ${built} changed whether the page claims \`invoke\`, so this page is still deriving an API claim from whether the console has a screen`
    )
  }
})

test('a_surface_the_service_does_not_serve_has_no_screen_either', () => {
  // The invariant that keeps `absent` usable as the reason a capability is withheld. `built` and
  // `served` are independent in one direction only: a route with no screen is ordinary (`invoke`),
  // a screen over a capability the service does not serve is a screen over nothing.
  //
  // It matters here rather than only in the shell, because `withheld()` reads `absent` — the
  // surface's account of why it is missing — for any surface that is not served. If such a surface
  // could also be built, `absent` would be empty and a capability would be withheld with no reason
  // beside it.
  for (const surface of SURFACES) {
    if (surface.served) continue
    assert.equal(
      surface.built,
      false,
      `\`${surface.id}\` has a screen in this console and the service does not serve it, which is a screen over nothing`
    )
    assert.ok(
      surface.absent.length > 0,
      `\`${surface.id}\` is not served and says nothing about why, so every capability standing on it is withheld with no reason`
    )
  }
})

test('the_reason_a_step_is_withheld_is_the_surface_s_own_words', () => {
  for (const entry of STEPS) {
    const surface = backing(entry)
    if (!surface) continue

    // One reason, in one place. A step backed by a surface must not restate why it is unavailable:
    // two statements of one fact are two things to edit, and the second is the one that rots.
    assert.equal(
      entry.pending,
      '',
      `\`${entry.id}\` is backed by the \`${surface.id}\` surface and carries its own reason as well, which is the drift this page exists to make impossible`
    )

    if (!surface.served) {
      assert.equal(
        withheld(entry),
        surface.absent,
        `\`${entry.id}\` explains itself in its own words rather than the \`${surface.id}\` surface's, so the page and the navigation can drift apart`
      )
    }
  }
})

// ---------------------------------------------------------------------------------------------
// What this build can and cannot do today, stated once so the page is checked and not just the
// machinery that produces it.
// ---------------------------------------------------------------------------------------------

test('today_an_agent_can_be_minted_and_the_service_invokes_but_no_agent_can_authenticate', async () => {
  const html = await page()

  assert.equal(
    available(step('be-minted')),
    true,
    'X-36 mints an agent principal and shows its token once; the page must say so'
  )
  assert.equal(
    available(step('authenticate')),
    false,
    'nothing in this build resolves an agent token to a principal (X-37), and the page must not imply one does'
  )

  // **This assertion is inverted from what X-41 shipped, and the inversion is the point.** It read
  // `assert.equal(available(step('invoke')), false, '`invoke` is not built')` — which was true of
  // the console's screens and false of the service, because `POST /api/operations/{operation}/invoke`
  // shipped in v0.7.0 and sat in the published route table the whole time this page told agent
  // authors nothing could be called. The route table is what decides it now, measured in the server
  // by `routes::onboarding::tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`.
  assert.equal(
    available(step('invoke')),
    true,
    'the service runs operations and the page must say so'
  )
  assert.equal(step('invoke').call.endpoint, '/api/operations/{operation}/invoke')
  assert.ok(
    html.includes('/api/operations/{operation}/invoke'),
    `the page must name the route that runs an operation; got: ${html}`
  )

  // The concrete step that works today, on the page and not only in the model.
  const mint = step('be-minted')
  assert.equal(mint.call.method, 'POST')
  assert.equal(mint.call.endpoint, '/api/agents')
  assert.ok(html.includes('/api/agents'), `the page must name the route that mints an agent; got: ${html}`)
  assert.match(
    html,
    /shown\s+(?:<[^>]*>)?once/i,
    'the page must say the token is shown once — a caller who expects to fetch it again has lost it'
  )
  assert.ok(
    /signed[- ]in/i.test(html),
    'minting is done by a signed-in human and not by the agent; the page must say whose call it is'
  )
})

test('the_catalogue_endpoint_the_page_names_is_the_one_the_console_reads', () => {
  // Two statements of one fact, deliberately not coupled in the source — `onboarding.mts` imports
  // nothing that knows a network exists. So they are held together here instead, which is the
  // assertion the design asks for: that they agree, not that each is correct.
  assert.equal(
    step('read-the-catalogue').call.endpoint,
    CONNECTORS_ENDPOINT,
    'the page tells an agent to read a catalogue endpoint this console does not itself read'
  )
})

// ---------------------------------------------------------------------------------------------
// Reachable signed out, and carrying nothing that belongs to a tenant.
// ---------------------------------------------------------------------------------------------

test('the_page_is_reachable_without_a_session', async () => {
  // The router reaches it. A fragment, because this console is one static document.
  assert.equal(parseRoute(`#${ONBOARDING_PATH}`).name, 'connect')

  // It renders with no session, no principal and no props — there is nothing to withhold from it.
  const html = await page()
  assert.ok(html.length > 0, 'the onboarding page rendered nothing')

  // And it is the head of the app's branch chain, so nothing can be evaluated before it: a `v-if`
  // at the head of a chain has no predecessor to gate it. This is the structural form of "an agent
  // that must authenticate to learn how to authenticate is a closed loop".
  const app = source('App.vue')
  const main = /<main>([\s\S]*?)<\/main>/.exec(app)
  assert.ok(main, 'App.vue renders no `<main>`')
  const first = /v-(?:if|else-if)="([^"]*)"/.exec(main[1])
  assert.ok(first, 'no branch was found in App.vue’s `<main>`; this scan is not working')
  assert.equal(
    first[1],
    "route.name === 'connect'",
    'the onboarding screen is not the first branch of `<main>`, so something is evaluated before it — and the only thing this page must depend on is the route'
  )
})

test('nothing_tenant_specific_can_reach_this_page', async () => {
  // 1. It takes no input. A component with no props cannot render a tenant, a principal, an address
  //    or a count, because none was passed to it.
  assert.ok(
    !AgentOnboarding.props || Object.keys(AgentOnboarding.props).length === 0,
    'the onboarding element declares props, which is a way for tenant data to reach a page that describes the shape of the service and never its contents'
  )

  // 2. It reads nothing. Rendered against a transport that refuses every call, which is what a
  //    tenant-scoped read would have to go through.
  const transport = globalThis.fetch
  globalThis.fetch = () => {
    throw new Error('the onboarding page attempted a read')
  }
  try {
    await page()
  } finally {
    globalThis.fetch = transport
  }

  // 3. And it could not have: neither source calls `fetch`, and neither imports the one module in
  //    this console that knows a network exists.
  for (const file of ['onboarding.mts', 'AgentOnboarding.mts']) {
    const text = source(file)
    assert.doesNotMatch(text, /\bfetch\s*\(/, `${file} calls fetch; this page reads nothing`)
    assert.doesNotMatch(
      text,
      /from\s+['"][^'"]*service\.mts['"]/,
      `${file} imports \`service.mts\`, which is the only module here that knows a network exists`
    )
  }

  // 4. No endpoint it names has a place a *value* could go. The rule was "parameterless", which
  //    was the right instinct written one notch too tight: it would have meant either omitting the
  //    one route that runs an operation or describing it in prose to get past this assertion, and
  //    a page that hides a route from a test is worse than the rule it is dodging. What must never
  //    appear is a tenant, a principal, a credential or an address, so the parameters that may
  //    appear are written out — `/api/connections/{connector}` on this page would still be a
  //    tenant's contents, because `connector` is not on this list.
  const CATALOGUE_KEYS = ['operation']
  for (const entry of STEPS) {
    if (!entry.call) continue
    assert.match(
      entry.call.endpoint,
      /^\/api\/[a-z0-9/{}-]+$/,
      `\`${entry.id}\` names \`${entry.call.endpoint}\`, which is not a route path`
    )
    for (const parameter of [...entry.call.endpoint.matchAll(/\{([^}]*)\}/g)].map((m) => m[1])) {
      assert.ok(
        CATALOGUE_KEYS.includes(parameter),
        `\`${entry.id}\` names \`${entry.call.endpoint}\`, and \`{${parameter}}\` is not a catalogue key — a parameter that is not one is a tenant, a principal, a credential or an address, and this page describes the shape of the service and never its contents`
      )
    }
  }

  // 5. Nothing that looks like a credential address, which is the one tenant-shaped string this
  //    console renders anywhere.
  const html = await page()
  assert.doesNotMatch(html, /tenants\//, `the page renders a credential address; got: ${html}`)
})

// ---------------------------------------------------------------------------------------------
// Where it hangs: the footer, not the rail.
// ---------------------------------------------------------------------------------------------

test('it_is_reached_from_the_footer_and_not_from_the_surface_rail', async () => {
  const app = source('App.vue')
  const footer = /<footer[^>]*>([\s\S]*?)<\/footer>/.exec(app)
  assert.ok(footer, 'App.vue renders no footer')
  assert.match(
    footer[1],
    /ONBOARDING_PATH/,
    'the footer does not link to the onboarding page, so an agent author has no way to find it'
  )

  // It is not a surface of the platform and must not be listed as one: the rail is where an
  // operator works, and this is a reference reached for once.
  assert.ok(
    !SURFACES.some((surface) => surface.path === ONBOARDING_PATH),
    'the onboarding page is declared as a platform surface, which puts it in the main rail'
  )
})

// ---------------------------------------------------------------------------------------------
// The look, which has to survive both themes. The same scan `shell.test.mjs` runs, over this
// page's own stylesheet.
// ---------------------------------------------------------------------------------------------

test('the_onboarding_page_names_no_colour_of_its_own', () => {
  const rules = source('onboarding.css').replace(/\/\*[\s\S]*?\*\//g, '')

  const literals = [...rules.matchAll(/#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g)].map((m) => m[0])
  assert.deepEqual(
    literals,
    [],
    'the onboarding stylesheet names a colour directly; every colour goes through a token in `tokens.css` so light and dark both work'
  )

  const defined = new Set()
  for (const sheet of ['tokens.css', 'app.css']) {
    for (const match of source(sheet).matchAll(/(--[A-Za-z0-9-]+)\s*:/g)) defined.add(match[1])
  }

  const read = [...rules.matchAll(/var\(\s*(--[A-Za-z0-9-]+)/g)].map((match) => match[1])
  assert.ok(read.length > 0, 'the onboarding stylesheet reads no token; this test would pass vacuously')
  assert.deepEqual(
    [...new Set(read)].filter((name) => !defined.has(name)),
    [],
    'the page reads a custom property this console defines nowhere, so those rules paint nothing'
  )
})

// ---------------------------------------------------------------------------------------------
// X-62: what the page says about grants, once a route edits one.
// ---------------------------------------------------------------------------------------------

/**
 * **X-62's failing-first test on this side.** The `invoke` warning stops saying that nothing edits
 * a grant, and names where one is edited instead.
 *
 * The sentence it replaces is this story's Acceptance written as prose: *"Grants are held per
 * tenant and there is no route that edits one, so a tenant nobody has granted anything runs nothing
 * at all: ask whoever operates this deployment."* That was worth saying while it was true — X-13
 * shipped the gate fail-closed with no surface behind it — and it is a claim about **this build**,
 * so it has to leave the page in the same change that makes it false. A document that keeps telling
 * an agent author there is no way for their tenant to be granted anything is the same dishonesty as
 * one claiming a capability it does not have, pointed the other way.
 *
 * Asserted on the model and on the rendered page, because the model is what generates the
 * descriptor the **service** serves and the page is what a human reads. Two renderings, one fact —
 * which is what `test/descriptor.test.mjs` exists to keep true.
 */
test('the_invoke_warning_names_the_route_a_grant_is_edited_at', async () => {
  const warn = step('invoke').call.warn

  assert.doesNotMatch(
    warn,
    /no route that edits/,
    'a route edits a grant now, and a page still saying none does sends an operator off to ask somebody rather than to the console'
  )
  assert.match(
    warn,
    /\/api\/grants/,
    'the warning must name where a grant is edited — "ask whoever operates this deployment" was the whole answer only while there was nowhere to point'
  )

  const html = await page()
  assert.ok(
    html.includes('/api/grants'),
    `the page must name the route a grant is edited at; got: ${html}`
  )
})
