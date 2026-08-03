// What this tenant may run, and the screen an operator changes it from.
//
// **The property this file exists for.** X-62's service half shipped `GET`/`PUT /api/grants` and
// `POST /api/grants/preview`, and the story says in its own words why the third of those is the
// point:
//
//   > A grant nobody can evaluate before saving is a grant somebody sets too wide.
//
// So the assertions below are not "the form submits". They are that **an operator sees what a grant
// would admit before it exists**, that the console cannot express a grant the service would refuse,
// and that the two refusals the service reserves reach a person as something they can act on rather
// than as a status code.
//
// **The console reimplements no rule.** `Selector::admits` lives in `exchange-host` and is projected
// through `OperationFacts::of`; the preview endpoint answers with what that projection admits, and
// this console renders the answer. There is deliberately nothing here that decides admission, and
// `the_console_decides_no_admission_of_its_own` scans for it — a second implementation would be a
// second answer, and the one an operator reads would be the one that is not deciding.
//
// Node's built-in runner, a stub `fetch`, and `test/mount.mjs` for the screen. Mounted rather than
// server-rendered because the claim is temporal — a preview arrives in response to a change, and
// saving is refused until it has — which is the same reason `agents.test.mjs` mounts.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineComponent, h, nextTick, reactive } from 'vue'

import { SURFACES, surfaceOfRoute } from '../src/surfaces.mts'
import { parseRoute } from '../src/routing.ts'
import { replacing, without } from '../src/granting.mts'
import Grants from '../src/Grants.mts'
import { find, mount, one, rendered } from './mount.mjs'
// A namespace import, deliberately: this file names functions that did not exist when it was
// written, and a missing *named* import is a module that fails to load — which reports as a broken
// test file rather than as the claim being false. A namespace lets the absence be an assertion.
import * as service from '../src/service.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** One app-layer source, read whole. */
const source = (file) => readFileSync(path.join(consoleRoot, 'src', file), 'utf-8')

/** Every app-layer source in this exchange-owned console. */
function appSources() {
  return readdirSync(path.join(consoleRoot, 'src'), { withFileTypes: true })
    .filter((entry) => entry.isFile() && /\.(vue|mts|ts)$/.test(entry.name))
    .map((entry) => path.join(consoleRoot, 'src', entry.name))
}

/**
 * A stub service that answers the three grant routes, and records every call.
 *
 * `asked` is half of what these tests are for: which endpoint, which method, and — the one that
 * matters most — **what was in the body**. A console that quietly put an operation id in a request
 * would pass every rendering assertion in this file.
 */
function granting({ held = [], editable = true, preview = null, save = null } = {}) {
  const asked = []

  const answer = (status, body) => ({
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
  })

  const fetch = async (url, init = {}) => {
    const method = init.method ?? 'GET'
    asked.push({ url, method, body: init.body ? JSON.parse(init.body) : null })

    if (url === service.GRANTS_PREVIEW_ENDPOINT) {
      return answer(preview?.status ?? 200, preview?.body ?? { connector: 'github', admits: [] })
    }
    if (url === service.GRANTS_ENDPOINT && method === 'PUT') {
      return answer(save?.status ?? 200, save?.body ?? { grants: held, editable })
    }
    if (url === service.GRANTS_ENDPOINT) {
      return answer(200, { grants: held, editable })
    }
    return answer(404, { error: `no stub for ${method} ${url}` })
  }

  return { fetch, asked }
}

/** One admitted operation, in the shape `OperationFacts` serialises to. */
const admits = (id, risk = 'low') => ({
  id,
  risk,
  idempotency: 'idempotent',
  effects: ['network'],
})

/**
 * One held grant, **as the console models one** — the shape `service.mts` reads a served grant into,
 * and therefore the shape the screen is handed.
 *
 * Deliberately not the wire shape: the axes are `maxRisk`/`effectsWithin` here and `max_risk`/
 * `effects_within` on the wire, and a fixture in the wrong one would make every screen assertion
 * below a test of the reader rather than of the screen. The wire shape appears only inside
 * [`granting`]'s stub bodies, where it belongs.
 */
const grant = (over = {}) => ({
  connector: 'github',
  vendor: 'GitHub',
  selector: { maxRisk: 'low', effectsWithin: null, idempotency: null },
  inbound: [],
  expressible: true,
  reason: '',
  exempt: null,
  declares: 12,
  admits: [admits('github-repo-get')],
  ...over,
})

// ---------------------------------------------------------------------------------------------
// The surface: this console has a screen for it, and it is reachable.
// ---------------------------------------------------------------------------------------------

/**
 * **X-62's failing-first test, first half.** The console declares a grants surface, it is built and
 * served, a fragment resolves to it, and an app-layer source mounts a screen for it.
 *
 * `test/shell.test.mjs` states the honesty invariant in one direction — *a surface that is not built
 * is unreachable* — over the model, the router, the sources and the page. This is the converse for
 * the one surface this story adds, and it is worth stating rather than assuming: a `built: true` with
 * no route behind it is the same two halves of the model disagreeing, pointed the other way, and it
 * renders as a rail entry that navigates to "Not found".
 */
test('the_console_has_a_screen_for_editing_a_grant', () => {
  const grants = SURFACES.find((surface) => surface.id === 'grants')

  assert.ok(
    grants,
    'the console declares no `grants` surface, so a tenant that has been granted nothing has nowhere to be granted anything — which is the whole of what X-62 is for'
  )
  assert.equal(grants.built, true, 'the grants surface is declared unbuilt')
  assert.equal(
    grants.served,
    true,
    'the service serves GET/PUT /api/grants; a surface that says otherwise reports the console’s gap as the platform’s'
  )
  assert.equal(grants.absent, '', 'a surface that exists carries a reason it does not')

  // The router. A rail entry with a path that resolves to nothing is a link to "Not found".
  assert.ok(grants.path, 'the grants surface is built and has nowhere to go')
  assert.equal(
    parseRoute(`#${grants.path}`).name,
    'grants',
    `\`${grants.path}\` resolves to no grants route, so the entry in the rail navigates nowhere`
  )
  assert.equal(
    surfaceOfRoute('grants'),
    'grants',
    'the grants route lights up no surface, so a reader who follows the link is told they are nowhere'
  )

  // The screen. Scanned the way `shell.test.mjs` scans for one, so a surface claiming a screen it
  // does not have fails here rather than at a reader.
  const mounts = appSources().some((file) =>
    /name\s*(?:===|:)\s*['"]grants['"]/.test(readFileSync(file, 'utf-8'))
  )
  assert.ok(
    mounts,
    'no app-layer source decides a screen from the `grants` route, so the surface is declared built and nothing renders it'
  )
})

// ---------------------------------------------------------------------------------------------
// The preview: what a grant would admit, before it is one.
// ---------------------------------------------------------------------------------------------

/**
 * **X-62's failing-first test, second half.** The console can ask what a proposed grant would admit
 * without saving it.
 *
 * The endpoint exists because the derivation must not live here — `POST /api/grants/preview` answers
 * from `OperationFacts::of` through `ConnectorSurface::admitted`, which is the projection the gate
 * itself decides on. What this asserts is the console's half: that asking is a read of the preview
 * route and **nothing else**, so evaluating a policy is not one typo away from applying one.
 */
test('the_console_can_ask_what_a_grant_would_admit_without_saving_it', async () => {
  assert.equal(
    typeof service.previewGrant,
    'function',
    '`service.mts` cannot ask what a grant would admit, so a screen could only show an operator what they had already saved'
  )

  const stub = granting({
    preview: {
      status: 200,
      body: {
        connector: 'github',
        vendor: 'GitHub',
        selector: { max_risk: 'low', effects_within: null, idempotency: null },
        expressible: true,
        declares: 12,
        admits: [admits('github-repo-get'), admits('github-issue-list')],
      },
    },
  })

  const state = await service.previewGrant(
    { connector: 'github', selector: { maxRisk: 'low', effectsWithin: null, idempotency: null } },
    { fetch: stub.fetch }
  )

  assert.equal(state.status, 'ready', `the preview did not answer: ${JSON.stringify(state)}`)
  assert.deepEqual(
    state.grant.admits.map((operation) => operation.id),
    ['github-repo-get', 'github-issue-list'],
    'the preview dropped what the service said the grant admits, which is the one thing it is for'
  )
  assert.equal(state.grant.declares, 12, 'the preview lost the count the admitted list is read against')

  // Exactly one call, and it wrote nothing.
  assert.deepEqual(
    stub.asked.map((call) => `${call.method} ${call.url}`),
    [`POST ${service.GRANTS_PREVIEW_ENDPOINT}`],
    `asking what a grant would admit must read the preview route and nothing else; it asked: ${JSON.stringify(stub.asked)}`
  )
})

test('the_console_refuses_to_read_malformed_inbound_authority', async () => {
  const state = await service.loadGrants({
    fetch: granting({
      held: [{
        connector: 'slack',
        vendor: 'Slack',
        selector: {},
        inbound: [{ binding: 'socket', events: ['app_mention', { invented: true }] }],
        expressible: true,
        declares: 0,
        admits: [],
      }],
    }).fetch,
  })

  assert.equal(state.status, 'failed', 'a malformed inbound event was silently discarded')
  assert.equal(state.failure.kind, 'unreadable')
  assert.match(state.failure.detail, /non-string event/)
})

// ---------------------------------------------------------------------------------------------
// The rule this surface must not break.
// ---------------------------------------------------------------------------------------------

/**
 * **X-62's failing-first test, third half.** Nothing this console sends can name an operation.
 *
 * X-13's Goal is that a grant is decided from an operation's declared metadata *and not from a list
 * of names*, and X-62's route refuses `allow_ids`, `deny_ids` and four more spellings with a `422`.
 * The console's obligation is stronger than not tripping that refusal: there must be **nowhere in a
 * request it composes for an id to go**, which is why this walks the bodies of every call rather
 * than checking a status.
 *
 * Driven over a save carrying a grant whose *preview* is full of operation ids — which is exactly
 * the shape a console that echoed back what it rendered would send.
 */
test('the_console_never_sends_an_operation_id', async () => {
  assert.equal(
    typeof service.replaceGrants,
    'function',
    '`service.mts` cannot write a grant, so nothing in this console can grant anything'
  )

  const stub = granting()

  await service.replaceGrants(
    [
      { connector: 'github', selector: { maxRisk: 'low', effectsWithin: ['network'], idempotency: null } },
      { connector: 'slack', selector: { maxRisk: null, effectsWithin: null, idempotency: 'idempotent' } },
    ],
    { fetch: stub.fetch }
  )

  assert.equal(stub.asked.length, 1, 'a save is one write')
  const [write] = stub.asked
  assert.equal(write.method, 'PUT')
  assert.equal(write.url, service.GRANTS_ENDPOINT)

  /** Every key anywhere in a request body — nesting cannot hide one. */
  const keys = (value) => {
    if (Array.isArray(value)) return value.flatMap(keys)
    if (value && typeof value === 'object') {
      return Object.keys(value).concat(Object.values(value).flatMap(keys))
    }
    return []
  }

  // The six spellings `routes::grants::NAMES_AN_OPERATION` refuses, restated here because the
  // console's claim is the stronger one: it does not send them, rather than being told not to.
  for (const named of ['allow_ids', 'deny_ids', 'allow', 'deny', 'operation', 'operations']) {
    assert.ok(
      !keys(write.body).includes(named),
      `the console sent \`${named}\`, which names operations — a grant written as a list of names silently stops covering a connector the moment it gains an operation: ${JSON.stringify(write.body)}`
    )
  }

  // And what it did send is the three axes, in the service's own spelling.
  assert.deepEqual(
    write.body,
    {
      grants: [
        { connector: 'github', selector: { max_risk: 'low', effects_within: ['network'] } },
        { connector: 'slack', selector: { idempotency: 'idempotent' } },
      ],
    },
    'the body is not the selector the service takes; an omitted axis admits every value of it, and a null is not the same statement'
  )
})

/**
 * The console decides no admission of its own.
 *
 * The other half of the rule above, and the one that would rot silently: a screen that computed
 * *which operations this selector admits* in TypeScript would render an answer that agrees with the
 * gate until the day it does not — and the one an operator reads would be the one that is not
 * deciding. So the app layer may hold the vocabulary and may render an answer, and there must be
 * nothing anywhere in it that compares a risk against a bound.
 *
 * Scanned rather than reasoned about, in the shape `components.test.mjs` uses. The patterns are the
 * shapes a hand-rolled `admits` takes: an ordered index lookup on a risk list, or a comparison
 * against a `maxRisk`/`max_risk` value.
 *
 * **`catalog.mts` is excluded, and the exclusion is narrow rather than convenient.** It is one of
 * the modules carried from flux-connectors — `AGENTS.md` forbids editing it here — and it ranks risk
 * in order to *sort* an operation list, which is a presentation order and not an admission. Sorting
 * by risk and deciding what a risk bound admits are different acts; the scanner cannot tell them
 * apart, so the one file that legitimately does the first is named here with the reason.
 */
test('the_console_decides_no_admission_of_its_own', () => {
  const suspicious = [
    /indexOf\s*\(\s*[A-Za-z_.]*[Rr]isk/,
    /RISK[A-Z_]*\.indexOf/,
    /[Mm]ax[_R]?[Rr]isk\s*[<>]=?/,
    /[<>]=?\s*[A-Za-z_.]*[Mm]ax[_R]?[Rr]isk/,
  ]

  const scanned = appSources().filter((file) => path.basename(file) !== 'catalog.mts')
  assert.ok(scanned.length > 0, 'no source was scanned, so this proves nothing')

  for (const file of scanned) {
    const text = readFileSync(file, 'utf-8').replace(/\/\*[\s\S]*?\*\/|\/\/[^\n]*/g, '')
    for (const pattern of suspicious) {
      assert.ok(
        !pattern.test(text),
        `${path.basename(file)} compares a risk against a bound (${pattern}), which is a second implementation of \`Selector::admits\` — the preview route answers this from the projection the gate decides on`
      )
    }
  }

  // And the guard on the guard: the scanner sees what it exists to catch.
  const hand_rolled = 'const admits = (op) => RISKS.indexOf(op.risk) <= RISKS.indexOf(maxRisk)'
  assert.ok(
    suspicious.some((pattern) => pattern.test(hand_rolled)),
    'the scanner did not see a hand-rolled admission rule, so the assertion above proves nothing'
  )
})

// ---------------------------------------------------------------------------------------------
// The screen.
// ---------------------------------------------------------------------------------------------

/** A signed-in principal of a kind this host admits at `/api/grants`. */
const user = (kind = 'user') => ({ kind, id: 'alice', tenant: 'acme' })

/**
 * The screen, mounted behind a reactive wrapper so props can change the way `App.vue` changes them.
 *
 * `mount` takes props once, and every claim worth making here is temporal — a preview arrives
 * *after* the question, and what the button does before it arrives is the whole point. So the
 * fixtures live in a `reactive` the test writes to, which is exactly what the root does.
 */
function screen(over = {}) {
  const state = reactive({
    session: { status: 'ready', principal: user() },
    grants: { status: 'ready', grants: [], editable: true },
    connectors: ['github', 'slack'],
    catalogueRisks: ['low', 'high'],
    catalogueEffects: ['network'],
    channelDeclarations: { status: 'ready', declarations: [] },
    preview: null,
    outcome: null,
    busy: false,
    ...over,
  })

  const emitted = { preview: [], save: [] }

  const Wrapper = defineComponent({
    name: 'GrantsHarness',
    setup: () => () =>
      h(Grants, {
        session: state.session,
        grants: state.grants,
        connectors: state.connectors,
        catalogueRisks: state.catalogueRisks,
        catalogueEffects: state.catalogueEffects,
        channelDeclarations: state.channelDeclarations,
        preview: state.preview,
        outcome: state.outcome,
        busy: state.busy,
        onPreview: (proposed) => emitted.preview.push(proposed),
        onSave: (next) => emitted.save.push(next),
      }),
  })

  return { ...mount(Wrapper), state, emitted }
}

/** The one element carrying `data-grants="<name>"`, or `null`. */
const at = (root, name) => one(root, 'data-grants', name)

/** Choose a connector, the way an operator does. */
const choose = (screen, connector) =>
  screen.fire(at(screen.root, 'connector'), 'onChange', { target: { value: connector } })

/**
 * **The claim this screen exists for.** Saving is refused until the service has said what the grant
 * would admit.
 *
 * The story's own words: *a grant nobody can evaluate before saving is a grant somebody sets too
 * wide*. A form that saved and then reported what it had done would satisfy every other assertion
 * in this file and miss the point entirely — so this drives the order: choose, and the button is
 * still refused; the answer arrives, and only then is it offered.
 *
 * The middle assertion is the one with teeth. Without it, a screen that enabled the button on
 * *choosing a connector* would pass the first and last.
 */
test('the_save_is_refused_until_the_preview_has_answered', async () => {
  const view = screen()

  assert.equal(
    at(view.root, 'save').props.disabled,
    true,
    'saving is offered before a connector has even been chosen'
  )

  await choose(view, 'github')

  assert.deepEqual(
    view.emitted.preview,
    [{ connector: 'github', selector: { maxRisk: 'low', effectsWithin: null, idempotency: null } }],
    'choosing a connector did not ask the service what the grant would admit'
  )
  assert.equal(
    at(view.root, 'save').props.disabled,
    true,
    'the grant can be saved before anything has said what it admits, which is the whole failure this screen is arranged against'
  )
  assert.match(
    rendered(view.root),
    /evaluated is a grant set too wide/,
    'the disabled button does not say why it is disabled'
  )

  // The service answers.
  view.state.preview = {
    status: 'ready',
    grant: grant({ admits: [admits('github-repo-get'), admits('github-issue-list')] }),
  }
  await nextTick()

  assert.equal(at(view.root, 'save').props.disabled, false, 'the preview answered and saving is still refused')
  assert.equal(
    at(view.root, 'preview').props['data-grants'],
    'preview',
    'the answer is not on the page'
  )

  const shown = rendered(view.root)
  assert.match(shown, /github-repo-get/, 'the preview does not name what the grant would admit')
  assert.match(shown, /Admits 2 of the 12/, 'the admitted count is not read against what the connector declares')

  // And saving sends the whole set, composed from what was read.
  await view.fire(at(view.root, 'form'), 'onSubmit')
  assert.deepEqual(
    view.emitted.save,
    [[{ connector: 'github', selector: { maxRisk: 'low', effectsWithin: null, idempotency: null } }]],
    'saving did not send the whole set the service replaces'
  )
})

test('inbound channel grants come from declarations and survive preview and whole-set writes', async () => {
  const stub = granting({
    preview: {
      body: {
        connector: 'slack',
        vendor: 'Slack',
        selector: { max_risk: 'low' },
        inbound: [{ binding: 'socket', events: ['app_mention'] }],
        expressible: true,
        declares: 1,
        admits: [admits('slack-message-send')],
      },
    },
  })
  const proposal = {
    connector: 'slack',
    selector: { maxRisk: 'low', effectsWithin: null, idempotency: null },
    inbound: [{ binding: 'socket', events: ['app_mention'] }],
  }
  const preview = await service.previewGrant(proposal, { fetch: stub.fetch })
  assert.equal(preview.status, 'ready')
  assert.deepEqual(preview.grant.inbound, proposal.inbound)
  assert.deepEqual(stub.asked[0].body.inbound, proposal.inbound)

  const view = screen({
    channelDeclarations: {
      status: 'ready',
      declarations: [{
        connector: 'slack',
        name: 'socket',
        service: 'default',
        description: 'Events API socket',
        transport: 'socket',
        events: [{ name: 'app_mention', description: 'An app mention', group: 'messages', default: false }],
      }],
    },
  })
  await choose(view, 'slack')
  const inboundChoice = find(view.root, 'type', 'checkbox').at(-1)
  assert.ok(inboundChoice, 'the declared inbound event has no checkbox')
  await view.fire(inboundChoice, 'onChange', { target: { checked: true } })
  assert.deepEqual(view.emitted.preview.at(-1), proposal)

  view.state.preview = { status: 'ready', grant: grant({ ...proposal, vendor: 'Slack' }) }
  await nextTick()
  assert.match(rendered(view.root), /Inbound channel events[\s\S]*app_mention/)
  await view.fire(at(view.root, 'form'), 'onSubmit')
  assert.deepEqual(view.emitted.save, [[proposal]])
})

test('an unavailable declaration read preserves held inbound authority', async () => {
  const held = grant({
    connector: 'slack',
    vendor: 'Slack',
    inbound: [{ binding: 'socket', events: ['app_mention'] }],
  })
  const view = screen({
    grants: { status: 'ready', editable: true, grants: [held] },
    channelDeclarations: {
      status: 'failed',
      failure: { kind: 'unreachable', endpoint: '/api/catalogue/connectors/slack/channels', status: null, detail: 'offline' },
    },
  })
  await choose(view, 'slack')
  assert.deepEqual(
    view.emitted.preview.at(-1).inbound,
    [{ binding: 'socket', events: ['app_mention'] }],
    'an unavailable declaration read silently dropped existing inbound authority from the draft'
  )
  assert.match(rendered(view.root), /cannot be edited safely/)
})

/**
 * A preview is asked for again whenever the answer would differ, and the selector is what changes.
 *
 * Without this, "the preview answered" could be satisfied by a screen that asked once and then let
 * an operator widen the bound underneath a stale answer — which is the same too-wide grant with an
 * agreeing-looking page above it.
 */
test('changing_a_bound_asks_again_before_it_can_be_saved', async () => {
  const view = screen()
  await choose(view, 'github')

  view.state.preview = { status: 'ready', grant: grant() }
  await nextTick()
  assert.equal(at(view.root, 'save').props.disabled, false)

  await view.fire(at(view.root, 'max-risk'), 'onChange', { target: { value: 'destructive' } })

  assert.deepEqual(
    view.emitted.preview.at(-1),
    { connector: 'github', selector: { maxRisk: 'destructive', effectsWithin: null, idempotency: null } },
    'widening the risk bound did not ask the service what it would now admit'
  )
})

/**
 * A preview about another connector is not an answer about this draft.
 *
 * `App.vue` discards answers that arrive after a newer question; this is the second half of the same
 * guard, and the one a reader would notice — a page showing `slack`'s operations under a `github`
 * heading is worse than one showing none.
 */
test('a_preview_for_another_connector_is_not_this_drafts_answer', async () => {
  const view = screen()
  await choose(view, 'github')

  view.state.preview = { status: 'ready', grant: grant({ connector: 'slack' }) }
  await nextTick()

  assert.equal(
    at(view.root, 'save').props.disabled,
    true,
    'a preview about a different connector enabled the save'
  )
  assert.ok(at(view.root, 'preview-stale'), 'the stale answer was rendered as this draft’s')
})

/**
 * **A tenant granted nothing is told that nothing runs.**
 *
 * The state every deployment starts in since X-13 closed the gate fail-closed, and the one an
 * operator meets first. An empty listing rendered as a blank page would send them looking for an
 * outage; what they need is that this is the *safe* state and what to do about it.
 */
test('a_tenant_granted_nothing_is_told_that_nothing_runs', () => {
  const view = screen()
  const shown = rendered(view.root)

  assert.ok(at(view.root, 'empty'), 'a tenant holding no grants renders no statement that it holds none')
  assert.match(shown, /runs nothing/, 'the empty state does not say what it means')
  assert.match(shown, /403/, 'the empty state does not say what happens to an invocation today')
})

// ---------------------------------------------------------------------------------------------
// The two refusals, and the one this screen pre-empts.
// ---------------------------------------------------------------------------------------------

/**
 * **A grant that names operations is shown, and saving says what would be lost.**
 *
 * The `409` blocks the primary flow for exactly today's population — anyone who already hand-wrote a
 * grants file with a `deny`. The service is right to refuse (the only evidence of a silent drop
 * would be an operation running that used to be refused), but a status code is not an answer to
 * anybody. So this is where it becomes one, and it is **pre-empted**: the operator is told before
 * they fill anything in, with the grant on screen and the operations it names.
 */
test('a_grant_that_names_operations_is_shown_and_saving_says_what_would_be_lost', async () => {
  const view = screen({
    grants: {
      status: 'ready',
      editable: false,
      grants: [
        grant({
          expressible: false,
          reason: 'this grant names operations explicitly, which this surface does not express',
          exempt: { always: [], never: ['github-repo-get'] },
        }),
      ],
    },
  })

  const shown = rendered(view.root)

  // Shown as stored — the read is honest about a grant this screen could not have written.
  assert.ok(at(view.root, 'inexpressible'), 'the grant that blocks the write is not marked on the page')
  assert.match(shown, /github-repo-get/, 'what the grant names is not shown, so an operator cannot see what is at stake')
  assert.match(shown, /names operations explicitly/, 'the service’s own reason is not rendered')

  // And the write is refused before it is attempted, with the connector named.
  assert.ok(at(view.root, 'blocked'), 'nothing says why saving is not offered')
  assert.match(rendered(view.root), /github/, 'the blocking grant is not named')

  await choose(view, 'slack')
  view.state.preview = { status: 'ready', grant: grant({ connector: 'slack' }) }
  await nextTick()

  assert.equal(
    at(view.root, 'save').props.disabled,
    true,
    'a set holding a grant this screen cannot express was offered for replacement anyway'
  )

  // Belt and brace: even driven directly, the composition refuses rather than dropping it.
  assert.equal(
    replacing(view.state.grants.grants, { connector: 'slack', selector: {} }),
    null,
    '`replacing` composed a set that silently drops an exception it could not express'
  )
  assert.equal(without(view.state.grants.grants, 'github'), null, '`without` did the same')
})

/**
 * Both refusals reach a person in the service's own words.
 *
 * X-62's route composes these sentences and each carries its argument; a console that paraphrased
 * would be inventing a worse one. The `409` gets one thing added rather than substituted — what is
 * in the way, which the service names by connector and the listing above can show in full.
 */
test('the_two_refusals_reach_a_person_in_the_services_own_words', () => {
  const conflict =
    'this tenant’s grant for `github` names operations explicitly, and this surface does not express that'
  const unprocessable =
    '`allow_ids` names operations, and a grant written here selects them by what they declare'

  const blocked = screen({
    grants: {
      status: 'ready',
      editable: false,
      grants: [grant({ expressible: false, exempt: { always: [], never: ['github-repo-get'] } })],
    },
    outcome: { status: 'refused', refusal: { endpoint: '/api/grants', status: 409, error: conflict } },
  })

  const shownConflict = rendered(blocked.root)
  assert.match(shownConflict, new RegExp(conflict.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), 'the 409 is not quoted whole')
  assert.equal(at(blocked.root, 'refused').props['data-status'], '409')
  assert.ok(
    at(blocked.root, 'blocked-detail'),
    'the 409 arrives with nothing to act on: the console knows which grant is in the way and does not say'
  )
  assert.match(shownConflict, /Nothing was changed/, 'the refusal does not say whether anything happened')

  const refused = screen({
    outcome: { status: 'refused', refusal: { endpoint: '/api/grants', status: 422, error: unprocessable } },
  })
  assert.match(
    rendered(refused.root),
    new RegExp(unprocessable.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    'the 422 is not quoted whole'
  )
})

/**
 * The console cannot send two grants for one connector — the other `422` this route reserves.
 *
 * Structural rather than validated: the whole set is composed by *replacing* the entry for the
 * connector being edited, so there is no path through this console that produces the body the
 * service refuses. Driven through the screen and asserted on what was emitted.
 */
test('the_console_cannot_send_two_grants_for_one_connector', async () => {
  const view = screen({
    grants: { status: 'ready', editable: true, grants: [grant(), grant({ connector: 'slack' })] },
  })

  await choose(view, 'github')
  view.state.preview = { status: 'ready', grant: grant() }
  await nextTick()
  await view.fire(at(view.root, 'form'), 'onSubmit')

  const [sent] = view.emitted.save
  assert.ok(sent, 'nothing was sent')
  assert.deepEqual(
    sent.map((entry) => entry.connector).sort(),
    ['github', 'slack'],
    'the set sent is not one grant per connector'
  )
  assert.equal(
    new Set(sent.map((entry) => entry.connector)).size,
    sent.length,
    'the console composed a set naming a connector twice, which the route refuses with 422'
  )
})

/**
 * Revoking sends the set without that connector, and nothing else moves.
 *
 * There is no `DELETE /api/grants/{connector}` and there should not be: `Grants::set` takes the
 * whole set so that what an operator states is the end state, rather than a sequence nobody can see
 * the end of. This is that shape at the screen.
 */
test('revoking_sends_the_set_without_that_connector', async () => {
  const view = screen({
    grants: { status: 'ready', editable: true, grants: [grant(), grant({ connector: 'slack' })] },
  })

  const buttons = find(view.root, 'data-grants', 'revoke')
  assert.equal(buttons.length, 2, 'a held grant offers no way to revoke it')

  await view.fire(buttons[0], 'onClick')

  assert.deepEqual(
    view.emitted.save,
    [[{ connector: 'slack', selector: { maxRisk: 'low', effectsWithin: null, idempotency: null } }]],
    'revoking one grant did not send the rest of the set unchanged'
  )
})

// ---------------------------------------------------------------------------------------------
// Who may be here, and what this console cannot offer.
// ---------------------------------------------------------------------------------------------

/**
 * A Service Account is told why there is no listing, rather than being shown one that failed.
 *
 * `routes::grants::MAY_GRANT` admits a `User` on the **read** as well as the write, which is the
 * half that is easy to get wrong — `admit_grant` withholds a tenant's policy from a refused caller
 * so that a token cannot enumerate it one call at a time, and a read open to every kind would hand
 * the whole of it over at once. So the screen must not ask, and must say why.
 */
test('a_service_account_is_told_why_there_is_no_listing', () => {
  const view = screen({ session: { status: 'ready', principal: user('service_account') } })
  const shown = rendered(view.root)

  assert.equal(at(view.root, 'gate').props['data-state'], 'may-not-grant')
  assert.equal(at(view.root, 'form'), null, 'a Service Account is offered a form the service would refuse')
  assert.equal(at(view.root, 'held'), null, 'a Service Account is shown a tenant’s policy')
  assert.equal(at(view.root, 'empty'), null, 'a Service Account is told this tenant holds nothing, which is a fact about the tenant')
  assert.match(shown, /enumerate/, 'the refusal does not say why the read is closed, so it reads as arbitrary')
})

/**
 * A reader with no session is offered the way in, and never a listing.
 *
 * The sign-in anchor is rendered unconditionally and is deliberately not gated on anything this
 * console read — X-57 settled that `/api/signin` explains itself for a host that cannot complete
 * one, which is a better answer than a console hiding the link and leaving a reader with nothing.
 */
test('a_reader_with_no_session_is_offered_the_way_in', () => {
  const anonymous = screen({ session: { status: 'ready', principal: null } })
  assert.equal(at(anonymous.root, 'gate').props['data-state'], 'anonymous')
  assert.equal(at(anonymous.root, 'held'), null)
  assert.match(rendered(anonymous.root), /\/api\/signin/, 'there is no way in')

  // And a session that could not be read is never rendered as signed out.
  const unknown = screen({
    session: { status: 'failed', failure: { kind: 'unreachable', endpoint: '/api/session', status: null, detail: '' } },
  })
  assert.equal(at(unknown.root, 'gate').props['data-state'], 'unknown')
  assert.match(
    rendered(unknown.root),
    /does not know/,
    'a failed session read is rendered as a statement about the reader rather than about the console'
  )
})

/**
 * A risk level the catalogue publishes and this console cannot offer is **stated**.
 *
 * `RISK_LEVELS` is a list this console maintains, which is the shape this whole story is against.
 * It has to be — `max_risk` means *at or below* and an order cannot be recovered from a set of
 * strings — so the cost is paid by saying so rather than by hoping. Without this, a level added
 * upstream would simply be missing from the chooser: an operator would set the widest bound offered,
 * read a preview that agreed with it, and never learn that operations above it exist.
 */
test('a_risk_level_this_console_cannot_offer_is_stated_rather_than_dropped', () => {
  const ordinary = screen()
  assert.equal(at(ordinary.root, 'unknown-risks'), null, 'a build whose levels are all offered still warns')

  const widened = screen({ catalogueRisks: ['low', 'high', 'catastrophic'] })
  assert.ok(
    at(widened.root, 'unknown-risks'),
    'the catalogue publishes a risk level this console does not offer and the page says nothing'
  )
  assert.match(rendered(widened.root), /catastrophic/, 'the unknown level is not named')
})

/**
 * The grants stylesheet names no colour of its own.
 *
 * The same scan `agents.css` and `onboarding.css` carry, for the same reason: every colour goes
 * through a token in `tokens.css`, so light and dark both work without this file knowing about
 * either.
 */
test('the_grants_screen_names_no_colour_of_its_own', () => {
  const rules = source('grants.css').replace(/\/\*[\s\S]*?\*\//g, '')

  const literals = [...rules.matchAll(/#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g)].map((m) => m[0])
  assert.deepEqual(literals, [], 'the grants stylesheet names a colour directly')

  const defined = new Set()
  for (const sheet of ['tokens.css', 'app.css']) {
    for (const match of source(sheet).matchAll(/(--[A-Za-z0-9-]+)\s*:/g)) defined.add(match[1])
  }

  const read = [...rules.matchAll(/var\(\s*(--[A-Za-z0-9-]+)/g)].map((match) => match[1])
  assert.ok(read.length > 0, 'the grants stylesheet reads no token; this test would pass vacuously')
  assert.deepEqual(
    [...new Set(read)].filter((name) => !defined.has(name)),
    [],
    'the screen reads a custom property this console defines nowhere, so those rules paint nothing'
  )
})
