// Wiring a connector up from the console — the first of the console's two jobs, and the one it
// could not do.
//
// **The property this file exists for.** `docs/vision.md` gives the console two jobs, *wire things
// up* and *see what happened*. X-34 built the surface that shows what is wired and said plainly
// that it built no way to wire anything, so until now an operator read their connections in a
// browser and created them with `curl`. This is the form, and the interesting part of it is not the
// layout — it is that four properties survive the form existing:
//
//   1. **The inputs are the catalogue's declaration.** Not a list kept here. A connector that gains
//      a credential gains an input with nobody editing this console, and the assertions below drive
//      that with a credential name this repository has never heard of.
//   2. **No credential value is ever rendered back.** The form writes; it does not read values. The
//      value reaches the request body and nothing else — not the URL, not the state the console
//      keeps, not the page that follows — and after a successful connect the reader sees addresses
//      and `held`, exactly what `Connections.mts` showed before this story.
//   3. **A refusal is the service's own sentence, whole.** X-18, X-20 and X-29 argued those
//      sentences at length; a console that summarises them throws that away. The `409` sentence is
//      206 characters long, so "whole" is a real assertion and not a formality.
//   4. **The `409` points at rotation.** X-39 shipped `PUT …/credentials/{credential}` precisely so
//      that replacing a credential does not mean destroying the connection first. A console that
//      offered delete-then-create behind a button would reintroduce the window X-39 removed, so the
//      page must not name `DELETE` at all on that path.
//
// Node's built-in runner with server-rendered render-function components, following
// `test/shell.test.mjs` and `test/onboarding.test.mjs`, and for the reason `src/CatalogueFailure.mts`
// sets out: a claim worth asserting deserves a real render rather than a grep over a template.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'

import {
  connect,
  connectionEndpoint,
  credentialEndpoint,
  loadDeclaration,
} from '../src/service.mts'
import Connect from '../src/Connect.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

// ---------------------------------------------------------------------------------------------
// Fixtures, copied from what the service actually answers.
//
// Every body below was taken from a running `flux-exchange` rather than invented, because the whole
// point of the assertions is that this console reads what the service said. A fixture somebody made
// up would let the console and the service drift while this file stayed green.
// ---------------------------------------------------------------------------------------------

/** `POST /api/connections/slack` with no values — the refusal that publishes the declaration. */
const SLACK_DECLARATION = {
  connector: 'slack',
  declared: ['slack.bot_token', 'slack.signing_secret'],
  error:
    'a connection to `slack` carries at least one credential value; it declares `slack.bot_token`, `slack.signing_secret`',
}

/** `POST /api/connections/zendesk` answering `201`: addresses and `held`, never a value. */
const ZENDESK_CONNECTION = {
  connector: 'zendesk',
  vendor: 'Zendesk',
  authority: 'com.zendesk.api',
  credentials: [
    { name: 'zendesk.api_token', address: 'tenants/acme/com.zendesk.api/api_token', held: true },
  ],
}

/**
 * The `409`, verbatim.
 *
 * 206 characters, which is why this file asserts the sentence appears *whole*: the console's own
 * refusal summariser truncates at 200, and a refusal cut off two characters before its end is a
 * refusal this console re-worded.
 */
const ALREADY_CONNECTED =
  'this tenant already has a connection to connector `zendesk`, and the credential address has no ' +
  'instance dimension to tell two of them apart — a second one would overwrite the first rather ' +
  'than sit beside it'

/** The connectors the catalogue lists, as ids — the only name this source publishes for one. */
const CONNECTORS = ['anthropic', 'slack', 'zendesk']

/** The form as a browser would see it, in one state. */
const form = (props) =>
  renderToString(
    createSSRApp(Connect, {
      connectors: CONNECTORS,
      chosen: null,
      declaration: null,
      outcome: null,
      busy: false,
      ...props,
    })
  )

/** A ready declaration state, as `loadDeclaration` produces one. */
const declared = (connector, credentials) => ({
  status: 'ready',
  declaration: { connector, credentials },
})

/** A stub transport answering one status and body, remembering what it was asked. */
function answering(status, body) {
  const asked = []
  const fetch = async (url, init) => {
    asked.push({ url, init })
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'content-type': 'application/json' },
    })
  }
  return { fetch, asked }
}

/** Every element in a fragment of HTML that carries the given attribute. */
const elementsWith = (html, attribute) => [
  ...html.matchAll(new RegExp(`<[^>]*\\b${attribute}="([^"]*)"[^>]*>`, 'g')),
]

/**
 * Every app-layer source, with its comments removed.
 *
 * The comments have to go before anything is scanned for a hardcoded credential name: this
 * repository documents by example, and `service.mts` has said "e.g. `zendesk.api_token`" in a doc
 * comment since X-34. A scan that could not tell a doc comment from a list would make the property
 * below unsatisfiable rather than false.
 */
function appSourcesWithoutComments() {
  return readdirSync(path.join(consoleRoot, 'src'), { withFileTypes: true })
    .filter((entry) => entry.isFile() && /\.(vue|mts|ts)$/.test(entry.name))
    .map((entry) => {
      const source = readFileSync(path.join(consoleRoot, 'src', entry.name), 'utf-8')
      return {
        name: entry.name,
        code: source
          .replace(/\/\*[\s\S]*?\*\//g, ' ')
          .replace(/<!--[\s\S]*?-->/g, ' ')
          .replace(/\/\/.*$/gm, ' '),
      }
    })
}

// ---------------------------------------------------------------------------------------------
// 1. The inputs are the catalogue's declaration.
// ---------------------------------------------------------------------------------------------

test('a_connect_form_offers_one_input_per_credential_the_connector_declares', async () => {
  const html = await form({
    chosen: 'slack',
    declaration: declared('slack', SLACK_DECLARATION.declared),
  })

  assert.match(
    html,
    /<form[^>]*data-connect="form"/,
    `the console renders no connect form, so it still cannot do the first of its two jobs; got: ${html}`
  )

  const inputs = elementsWith(html, 'data-credential').map((match) => match[1])
  assert.deepEqual(
    inputs,
    SLACK_DECLARATION.declared,
    'the form must offer exactly the credentials the connector declares, in the order the service declared them'
  )

  // Named on the page as well as in an attribute: the operator has to know which secret goes in
  // which box, and `slack.bot_token` and `slack.signing_secret` are not interchangeable.
  for (const name of SLACK_DECLARATION.declared) {
    assert.ok(html.includes(name), `the form does not name the \`${name}\` credential; got: ${html}`)
  }
})

test('a_credential_this_console_never_heard_of_still_gets_an_input', async () => {
  // The point of the story: a connector that gains a credential must gain an input with nobody
  // editing this console. Nothing in this repository declares this name.
  const invented = 'acme.newly_declared_token'
  const html = await form({
    chosen: 'acme',
    declaration: declared('acme', [invented]),
  })

  assert.equal(
    elementsWith(html, 'data-credential').map((match) => match[1])[0],
    invented,
    'a credential the console has never seen produced no input, so the inputs are not the declaration'
  )

  // And the other half of the same property, asserted structurally: no source here carries a
  // credential name, so there is no list that could go stale.
  const known = [
    'zendesk.api_token',
    'slack.bot_token',
    'slack.signing_secret',
    'anthropic.api_key',
  ]
  for (const { name, code } of appSourcesWithoutComments()) {
    for (const credential of known) {
      assert.ok(
        !code.includes(credential),
        `\`src/${name}\` names the credential \`${credential}\`, so this console keeps its own list of what a connector declares — the thing this story exists to avoid`
      )
    }
  }
})

test('the_declaration_is_read_from_the_service_and_the_read_sends_no_value', async () => {
  const service = answering(422, SLACK_DECLARATION)
  const state = await loadDeclaration('slack', { fetch: service.fetch })

  assert.equal(state.status, 'ready', `the declaration was not read; got: ${JSON.stringify(state)}`)
  assert.deepEqual(
    state.declaration.credentials,
    SLACK_DECLARATION.declared,
    'the declaration must be exactly what the service published, unfiltered and unreordered'
  )

  assert.equal(service.asked.length, 1, 'the declaration must cost exactly one request')
  const [{ url, init }] = service.asked
  assert.equal(url, connectionEndpoint('slack'))
  assert.deepEqual(
    JSON.parse(init.body),
    { credentials: {} },
    'asking what a connector declares must carry no credential value at all'
  )
})

// ---------------------------------------------------------------------------------------------
// 2. No credential value is ever rendered back. The north star, at the UI layer.
// ---------------------------------------------------------------------------------------------

test('no_credential_value_reaches_a_url_the_console_keeps_or_the_page_that_follows', async () => {
  const sentinel = 'sentinel-value-no-page-may-ever-show-42'
  const service = answering(201, ZENDESK_CONNECTION)

  const outcome = await connect(
    'zendesk',
    { 'zendesk.api_token': sentinel },
    { fetch: service.fetch }
  )

  const [{ url, init }] = service.asked
  assert.equal(init.method, 'POST')
  assert.ok(
    init.body.includes(sentinel),
    'the value must actually be sent, or everything below is vacuous'
  )
  assert.ok(
    !url.includes(sentinel),
    `the value reached the URL \`${url}\` — a value in a query string is a value in an access log`
  )

  // What the console goes on holding. `GET /api/connections` answers with addresses and `held`, and
  // a create answers with the same view, so there is nothing here a later render could leak.
  assert.equal(outcome.status, 'connected')
  assert.ok(
    !JSON.stringify(outcome).includes(sentinel),
    `the console kept the value it sent: ${JSON.stringify(outcome)}`
  )

  const html = await form({
    chosen: 'zendesk',
    declaration: declared('zendesk', ['zendesk.api_token']),
    outcome,
  })

  assert.ok(!html.includes(sentinel), `a credential value was rendered back; got: ${html}`)

  // Addresses and `held`, exactly as the read-only view shows them.
  assert.ok(
    html.includes(ZENDESK_CONNECTION.credentials[0].address),
    `a successful connect must show the address the credential now lives at; got: ${html}`
  )
  assert.match(
    html,
    /data-held="true"/,
    `a successful connect must show that something is now held; got: ${html}`
  )
})

test('the_inputs_are_secrets_and_hold_nothing_that_could_be_rendered_back', async () => {
  const html = await form({
    chosen: 'slack',
    declaration: declared('slack', SLACK_DECLARATION.declared),
  })

  const inputs = elementsWith(html, 'data-credential').map((match) => match[0])
  assert.equal(inputs.length, SLACK_DECLARATION.declared.length)

  for (const input of inputs) {
    assert.match(input, /type="password"/, `a credential input is not a password field: ${input}`)
    assert.doesNotMatch(
      input,
      /\bvalue="/,
      `a credential input carries a value attribute, which is a value rendered into the document: ${input}`
    )
    // A password manager offering last time's secret is the same leak by another route.
    assert.match(
      input,
      /autocomplete="new-password"/,
      `a credential input invites the browser to fill it from what it stored: ${input}`
    )
  }

  // And nothing on the path a credential value travels writes it somewhere it would outlive the
  // request. Scoped to those four files rather than to `src/`: `theme.ts` keeps the reader's light
  // or dark choice in `localStorage`, which is correct and has nothing to do with this, and a scan
  // that could not tell the two apart would be one somebody eventually deletes.
  const path = ['Connect.mts', 'Connections.mts', 'service.mts', 'App.vue']
  const onPath = appSourcesWithoutComments().filter((source) => path.includes(source.name))
  assert.equal(onPath.length, path.length, 'a source on the credential path was renamed or removed')

  for (const { name, code } of onPath) {
    for (const store of ['localStorage', 'sessionStorage', 'document.cookie']) {
      assert.ok(
        !code.includes(store),
        `\`src/${name}\` reaches for \`${store}\`; a credential value must not be persisted anywhere by this console`
      )
    }
  }
})

// ---------------------------------------------------------------------------------------------
// 3. A refusal is the service's own sentence, whole.
// ---------------------------------------------------------------------------------------------

test('a_refusal_is_shown_in_the_services_own_words_and_is_not_cut_short', async () => {
  const body = {
    connector: 'zendesk',
    error: ALREADY_CONNECTED,
    addresses: ['tenants/acme/com.zendesk.api/api_token'],
  }
  const service = answering(409, body)

  const outcome = await connect('zendesk', { 'zendesk.api_token': 'x' }, { fetch: service.fetch })

  assert.equal(outcome.status, 'refused', `a 409 is a refusal; got: ${JSON.stringify(outcome)}`)
  assert.equal(outcome.refusal.status, 409)
  assert.equal(
    outcome.refusal.error,
    ALREADY_CONNECTED,
    'the refusal must be the service’s sentence, byte for byte'
  )

  assert.ok(
    ALREADY_CONNECTED.length > 200,
    'the sentence under test is short enough to survive a summariser, so this assertion proves nothing'
  )

  const html = await form({
    chosen: 'zendesk',
    declaration: declared('zendesk', ['zendesk.api_token']),
    outcome,
  })
  assert.ok(
    html.includes(ALREADY_CONNECTED),
    `the refusal was re-worded or cut short on the way to the page; got: ${html}`
  )
})

test('an_existing_connection_points_at_rotation_and_never_at_delete', async () => {
  const service = answering(409, { connector: 'zendesk', error: ALREADY_CONNECTED })
  const outcome = await connect('zendesk', { 'zendesk.api_token': 'x' }, { fetch: service.fetch })

  const html = await form({
    chosen: 'zendesk',
    declaration: declared('zendesk', ['zendesk.api_token']),
    outcome,
  })

  // X-39's route, named with the credential it would replace.
  assert.ok(
    html.includes(credentialEndpoint('zendesk', 'zendesk.api_token')),
    `a 409 must point at rotation and name the route that does it; got: ${html}`
  )
  assert.ok(html.includes('PUT'), `the rotation route must be named with its method; got: ${html}`)

  // The whole point. Delete-then-create is the window X-39 exists to remove, and a console that
  // offers it behind a button puts the window back.
  assert.ok(
    !html.includes('DELETE'),
    `the page names DELETE on the already-connected path, which is the delete-then-create window X-39 removed; got: ${html}`
  )

  const source = readFileSync(path.join(consoleRoot, 'src', 'Connect.mts'), 'utf-8')
  assert.ok(
    !/method:\s*['"]DELETE['"]/.test(source),
    'the connect form issues a DELETE, which is delete-then-create stitched together behind a button'
  )
})

// ---------------------------------------------------------------------------------------------
// 4. The three states, kept apart — `service.mts`'s discipline, on one more read.
// ---------------------------------------------------------------------------------------------

test('a_declaration_that_could_not_be_read_is_not_a_connector_with_no_credentials', async () => {
  const state = await loadDeclaration('slack', {
    fetch: async () => {
      throw new TypeError('fetch failed')
    },
  })

  assert.equal(state.status, 'failed')
  assert.equal(state.failure.endpoint, connectionEndpoint('slack'))
  assert.equal(
    state.declaration,
    undefined,
    'a failed read must carry no declaration field at all, or a caller that forgets to check gets an empty one'
  )

  const html = await form({ chosen: 'slack', declaration: state })
  assert.ok(
    html.includes(connectionEndpoint('slack')),
    `a failed read must name the endpoint that did not answer; got: ${html}`
  )
  assert.ok(
    !/<input/.test(html),
    `a read that failed rendered a form with no inputs, which reads as a connector that declares none; got: ${html}`
  )
})
