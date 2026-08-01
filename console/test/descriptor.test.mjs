// One truth, two renderings — and the assertion that they agree.
//
// **What this file is for.** X-41 gave an agent author a page. X-42 gives their agent a document it
// can fetch, served by the service at `/api/onboarding`. `docs/designs/agent-onboarding.md` names
// the failure mode of that pair before it happens:
//
//   > **Two renderings can still drift** if the descriptor and the page are wired to the source
//   > separately rather than sharing it. The test to write is that they agree, not that each is
//   > correct.
//
// So the central test here is not "the descriptor is right" and not "the page is right". It renders
// the page, reads the document the **service** serves, and compares them field by field. Either one
// changing without the other turns it red, and neither can be satisfied by a test that only ever
// looked at one side.
//
// **What "the document the service serves" means here.** The service serves
// `crates/exchange-server/src/routes/onboarding.json`, compiled in with `include_str!`. That file is
// generated from `src/descriptor.mts` by `scripts/agent-descriptor.mjs`, which makes it a committed
// copy of a derivation — the one thing in this arrangement that can rot. So it is read from disk
// here, exactly as the server reads it, rather than re-derived: a test that compared the derivation
// against the page would be green on the day somebody edited `surfaces.mts` and shipped a stale
// artifact. `the_descriptor_the_service_serves_is_the_one_this_console_derives` is what closes that,
// and it is the *only* test here that re-derives anything.
//
// The service adds exactly one field to that document — `sign_in_available`, which is a fact about a
// deployment rather than about the build, and which this console cannot know. It is absent from the
// artifact by design, and `routes::onboarding::tests` in the server crate is what holds the service
// to adding that and nothing else.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'

import { SURFACES } from '../src/surfaces.mts'
import { STEPS, available } from '../src/onboarding.mts'
import { descriptorJson } from '../src/descriptor.mts'
import AgentOnboarding from '../src/AgentOnboarding.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** Where the service keeps the document it serves, read the way the server reads it. */
const ARTIFACT = path.resolve(
  consoleRoot,
  '..',
  'crates',
  'exchange-server',
  'src',
  'routes',
  'onboarding.json'
)

/** How to make this file green again when it is red for the boring reason. */
const REGENERATE = 'run `node scripts/agent-descriptor.mjs` from `console/`'

/** The committed artifact's text, byte for byte. */
const served = () => readFileSync(ARTIFACT, 'utf-8')

/** The document the service serves, parsed. */
const descriptor = () => JSON.parse(served())

/** The page as a browser would see it. */
const page = () => renderToString(createSSRApp(AgentOnboarding))

/** `SURFACES` with one surface's `built` overridden — a hypothetical build. */
const asIf = (id, built) =>
  SURFACES.map((surface) => (surface.id === id ? { ...surface, built } : surface))

/**
 * The readable text of a fragment of rendered HTML.
 *
 * Tags dropped, the five entities Vue escapes put back, and whitespace collapsed — so an assertion
 * can compare a string from the document against what a reader actually sees, rather than against
 * the markup that happened to carry it.
 */
function text(html) {
  return html
    .replace(/<[^>]*>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim()
}

/** A string from the document, normalised the same way, so the two are comparable. */
const said = (value) => value.replace(/\s+/g, ' ').trim()

/** The `<li>` the page rendered for one capability, or a failure naming it. */
function block(html, id) {
  const found = new RegExp(`<li[^>]*data-step="${id}"[^>]*>([\\s\\S]*?)</li>`).exec(html)
  assert.ok(found, `the page renders no step for the \`${id}\` capability the descriptor publishes`)
  return found[0]
}

/** One attribute off an element. */
function attribute(element, name) {
  const value = new RegExp(`${name}="([^"]*)"`).exec(element)
  return value ? value[1] : null
}

// ---------------------------------------------------------------------------------------------
// The artifact the service serves is a copy of a derivation, and copies rot.
// ---------------------------------------------------------------------------------------------

test('the_descriptor_the_service_serves_is_the_one_this_console_derives', () => {
  assert.equal(
    served(),
    descriptorJson(),
    `the document the service compiles in is not what this console derives today — ${REGENERATE}. ` +
      'Until then the service is publishing a description of a build that no longer exists.'
  )
})

// ---------------------------------------------------------------------------------------------
// The Acceptance's fourth item: the page and the descriptor agree, compared against each other.
// ---------------------------------------------------------------------------------------------

test('the_page_and_the_descriptor_agree', async () => {
  const document = descriptor()
  const html = await page()
  const whole = text(html)

  // 1. What this service is. One sentence, in both renderings, and neither owns it.
  assert.ok(
    whole.includes(said(document.service.name)),
    `the descriptor calls this service \`${document.service.name}\` and the page does not say so`
  )
  assert.ok(
    whole.includes(said(document.service.summary)),
    `the descriptor and the page describe this service differently.\n  descriptor: ${document.service.summary}\n  page: ${whole}`
  )

  // 2. Where the document lives. The page names it so an agent author knows their agent does not
  //    have to read the page; if the route moves, both must move.
  assert.ok(
    whole.includes(`GET ${document.endpoint}`),
    `the descriptor is served at \`${document.endpoint}\` and the page does not point an agent at it`
  )

  // 3. The auth scheme, and — the part that matters — the two renderings agreeing about whether
  //    presenting a token identifies anybody. The document says so in a boolean; the page says so
  //    by deferring to a capability, and that capability's rendered state is what is compared.
  assert.ok(
    whole.includes(`${document.authentication.header}: ${document.authentication.scheme}`),
    `the descriptor publishes the \`${document.authentication.scheme}\` scheme and the page names no scheme`
  )
  assert.equal(
    attribute(block(html, document.authentication.capability), 'data-available'),
    String(document.authentication.live),
    'the descriptor and the page disagree about whether a token presented to this host identifies its holder'
  )

  // 4. Every capability, both ways. The document's entries must be on the page, and the page must
  //    claim nothing the document does not — a step rendered for a capability the descriptor omits
  //    is the same drift seen from the other end.
  assert.ok(document.capabilities.length > 0, 'the descriptor publishes no capabilities; everything below is vacuous')

  const claimed = document.capabilities.filter((capability) => capability.live)
  const withheld = document.capabilities.filter((capability) => !capability.live)
  assert.ok(claimed.length > 0, 'the descriptor says nothing works; there is at least one thing that does')
  assert.ok(withheld.length > 0, 'the descriptor withholds nothing, and this build cannot do everything')

  for (const capability of document.capabilities) {
    const rendered = block(html, capability.id)
    const shown = text(rendered)

    assert.equal(
      attribute(rendered, 'data-available'),
      String(capability.live),
      `\`${capability.id}\`: the descriptor says live=${capability.live} and the page renders the other one`
    )
    assert.ok(
      shown.includes(said(capability.title)),
      `\`${capability.id}\`: the descriptor and the page give it different titles.\n  descriptor: ${capability.title}\n  page: ${shown}`
    )
    assert.ok(
      shown.includes(said(capability.summary)),
      `\`${capability.id}\`: the descriptor and the page summarise it differently.\n  descriptor: ${capability.summary}\n  page: ${shown}`
    )

    if (capability.live) {
      const { method, endpoint, caller, note, warn } = capability.call
      assert.ok(
        shown.includes(`${method} ${endpoint}`),
        `\`${capability.id}\`: the descriptor tells an agent to call \`${method} ${endpoint}\` and the page does not.\n  page: ${shown}`
      )
      for (const [field, value] of [
        ['caller', caller],
        ['note', note],
        ['warn', warn],
      ]) {
        if (value === '') continue
        assert.ok(
          shown.includes(said(value)),
          `\`${capability.id}\`: the descriptor's \`${field}\` is not what the page says.\n  descriptor: ${value}\n  page: ${shown}`
        )
      }
    } else {
      assert.equal(capability.call, null, `\`${capability.id}\` is not live and names a call anyway`)
      assert.ok(
        shown.includes(said(capability.withheld)),
        `\`${capability.id}\`: the descriptor and the page give different reasons it cannot be done.\n  descriptor: ${capability.withheld}\n  page: ${shown}`
      )
    }
  }

  // The other direction. Every step the page renders is a capability the document publishes, so an
  // agent author who fetches instead of reading is not missing one.
  const onThePage = [...html.matchAll(/data-step="([^"]*)"/g)].map((match) => match[1])
  assert.deepEqual(
    onThePage,
    document.capabilities.map((capability) => capability.id),
    'the page and the descriptor list different capabilities, in a different order, or both'
  )
})

// ---------------------------------------------------------------------------------------------
// The agreement has to be a derivation, not a coincidence.
// ---------------------------------------------------------------------------------------------

test('the_descriptor_is_derived_and_not_a_coincidence', () => {
  const today = JSON.parse(descriptorJson(SURFACES))
  const live = (document, id) => document.capabilities.find((entry) => entry.id === id)

  // As this build is.
  assert.equal(live(today, 'invoke').live, false)
  assert.ok(live(today, 'invoke').withheld.length > 0)

  // As a build where `invoke` exists would be — with no edit to `descriptor.mts`, none to
  // `onboarding.mts`, and none here.
  const hypothetical = JSON.parse(descriptorJson(asIf('invoke', true)))
  assert.equal(
    live(hypothetical, 'invoke').live,
    true,
    'marking `invoke` built does not make the invoke capability live, so the descriptor is not actually derived from `surfaces.mts`'
  )
  assert.equal(
    live(hypothetical, 'invoke').withheld,
    '',
    'the invoke capability still carries a reason it cannot be done in a build where it can'
  )

  // And the direction that protects a reader: a surface regressing to unbuilt withdraws the
  // capability standing on it from the served document, not only from the page.
  const minted = STEPS.find((step) => step.id === 'be-minted')
  assert.equal(available(minted, SURFACES), true)
  const regressed = JSON.parse(descriptorJson(asIf(minted.surface, false)))
  assert.equal(
    live(regressed, 'be-minted').live,
    false,
    'a surface regressing to not-built leaves its capability standing in the document the service serves'
  )
  assert.equal(live(regressed, 'be-minted').call, null)
})

// ---------------------------------------------------------------------------------------------
// It is published to strangers, so it holds nothing that belongs to anyone.
// ---------------------------------------------------------------------------------------------

test('the_descriptor_names_nothing_a_tenant_could_occupy', () => {
  const document = descriptor()

  // Every endpoint is parameterless — `/api/connections/{connector}` here would be a tenant's
  // contents wearing the shape of a route. The same rule `nothing_tenant_specific_can_reach_this_page`
  // holds over the page, applied to the copy that is actually served.
  const endpoints = [
    document.endpoint,
    ...document.capabilities.filter((entry) => entry.call).map((entry) => entry.call.endpoint),
  ]
  assert.ok(endpoints.length > 1, 'no endpoints were checked, so this proves nothing')
  for (const endpoint of endpoints) {
    assert.match(
      endpoint,
      /^\/api\/[a-z0-9/-]+$/,
      `\`${endpoint}\` carries a parameter; this document describes the shape of the service, never its contents`
    )
  }

  // And nothing that looks like a credential address, which is the one tenant-shaped string this
  // console renders anywhere.
  assert.doesNotMatch(served(), /tenants\//, 'the descriptor renders a credential address')

  // The derivation reads nothing, and could not have: it does not import the one module in this
  // console that knows a network exists. A document assembled from a live read would be a document
  // that could describe one deployment's contents.
  const source = readFileSync(path.join(consoleRoot, 'src', 'descriptor.mts'), 'utf-8')
  assert.doesNotMatch(source, /\bfetch\s*\(/, 'descriptor.mts calls fetch')
  assert.doesNotMatch(
    source,
    /from\s+['"][^'"]*service\.mts['"]/,
    'descriptor.mts imports `service.mts`, which is the only module here that knows a network exists'
  )
})
