// What the console does with the catalogue the service serves — and, above everything else here,
// what it does when the service serves nothing at all.
//
// **The property this file exists for.** A console that renders an unreachable service as an empty
// catalogue is worse than one that crashes: it answers the reader's question ("what connectors are
// there?") with a confident, wrong "none". So `loadCatalogue` never degrades to an empty document.
// The two outcomes are different shapes — `failed` carries a failure and no catalogue at all, `ready`
// carries a catalogue and no failure — and the failure names the endpoint that did not answer, so a
// reader can tell a stopped service from a service with nothing in it without leaving the page.
//
// Node's built-in test runner and a stub `fetch`, deliberately: no server is started here. The
// service is being built in parallel against the same contract, and a test that needed it running
// would be a test of two things at once.
//
// The two views are asserted through Vue's server renderer, which ships inside `vue` itself — this
// is a real render of the real component, not a check that a string exists in a source file. Both
// are render-function components in `.mts` for exactly that reason; see their own headers.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'

import {
  CONNECTIONS_ENDPOINT,
  CONNECTORS_ENDPOINT,
  SESSION_ENDPOINT,
  failureMessage,
  loadCatalogue,
  operationsEndpoint,
} from '../src/service.mts'
import CatalogueFailure from '../src/CatalogueFailure.mts'
import CatalogueOperation from '../src/CatalogueOperation.mts'

/** One served operation, in the shape the catalogue routes publish. */
const operation = (over = {}) => ({
  id: 'zendesk-ticket-show',
  service: 'default',
  description: 'Read one ticket.',
  input_schema: { type: 'object', properties: {}, required: [] },
  risk: 'low',
  idempotency: 'idempotent',
  effects: ['network'],
  effects_derived: true,
  admitted: null,
  ...over,
})

/**
 * A stub service that answers the two catalogue routes from a fixed document.
 *
 * It records every URL it was asked for, because "which endpoint did the console actually call"
 * is half of what the failure message has to be able to say.
 */
function servedBy(document) {
  const asked = []
  const answer = (body, status = 200) =>
    new Response(JSON.stringify(body), {
      status,
      headers: { 'content-type': 'application/json' },
    })

  const fetchImpl = async (url) => {
    asked.push(url)
    if (url === CONNECTORS_ENDPOINT) {
      return answer({
        connectors: Object.keys(document).map((id) => ({
          id,
          vendor: id === 'zendesk' ? 'Zendesk' : id === 'slack' ? 'Slack' : id,
          description: `${id} connector`,
          operation_count: document[id].length,
        })),
      })
    }
    for (const id of Object.keys(document)) {
      if (url === operationsEndpoint(id)) return answer({ connector: id, operations: document[id] })
    }
    return answer({ error: `no connector named ${url.split('/').at(-2)}` }, 404)
  }

  return { fetchImpl, asked }
}

test('connector_human_facts_survive_the_service_adapter', async () => {
  const { fetchImpl } = servedBy({ zendesk: [operation()] })
  const state = await loadCatalogue({ fetch: fetchImpl })

  assert.equal(state.status, 'ready')
  assert.deepEqual(
    {
      id: state.catalog.connectors[0].id,
      vendor: state.catalog.connectors[0].vendor,
      description: state.catalog.connectors[0].description,
    },
    { id: 'zendesk', vendor: 'Zendesk', description: 'zendesk connector' }
  )
})

/** A service that is not there: `fetch` rejects, which is what an unreachable host looks like. */
const unreachable = async () => {
  throw new TypeError('fetch failed')
}

test('an_unreachable_service_names_the_endpoint_instead_of_an_empty_catalogue', async () => {
  const state = await loadCatalogue({ fetch: unreachable })

  assert.equal(state.status, 'failed', 'an unreachable service must not resolve to a catalogue')
  assert.equal(state.failure.kind, 'unreachable')
  assert.equal(state.failure.endpoint, CONNECTORS_ENDPOINT)

  // No catalogue, not an empty one. A caller that reached for `state.catalog` and got `{providers:
  // []}` would render "0 connectors" with a clear conscience, which is the whole failure being
  // guarded against.
  assert.equal(state.catalog, undefined, 'a failed load must carry no catalogue at all')

  const message = failureMessage(state.failure)
  assert.ok(
    message.includes(CONNECTORS_ENDPOINT),
    `the failure message must name the endpoint it could not reach; got: ${message}`
  )
})

test('zero_connectors_and_an_unreachable_service_do_not_look_the_same', async () => {
  const { fetchImpl } = servedBy({})
  const empty = await loadCatalogue({ fetch: fetchImpl })
  const stopped = await loadCatalogue({ fetch: unreachable })

  assert.equal(empty.status, 'ready')
  assert.deepEqual(empty.catalog.connectors, [], 'a service with nothing in it serves an empty catalogue')
  assert.equal(empty.failure, undefined, 'an empty catalogue is not a failure')

  assert.notEqual(
    empty.status,
    stopped.status,
    'a catalogue with no connectors and a service that did not answer must be distinguishable'
  )
})

test('the_failure_view_renders_the_endpoint_it_could_not_reach', async () => {
  const state = await loadCatalogue({ fetch: unreachable })
  const html = await renderToString(createSSRApp(CatalogueFailure, { failure: state.failure }))

  assert.ok(
    html.includes(CONNECTORS_ENDPOINT),
    `the failure view must name the endpoint on the page; got: ${html}`
  )
  assert.match(html, /could not be reached/i, 'the failure view must say the service was not reached')
  assert.match(html, /data-catalogue="failed"/, 'the failure view must mark the page as failed')
  assert.doesNotMatch(
    html,
    /\b0 connectors\b|no connectors/i,
    'the failure view must not describe the catalogue as empty — it read nothing'
  )
})

test('a_refused_response_names_the_endpoint_and_the_status', async () => {
  const refuse = async () => new Response('nope', { status: 503 })
  const state = await loadCatalogue({ fetch: refuse })

  assert.equal(state.status, 'failed')
  assert.equal(state.failure.kind, 'refused')
  assert.equal(state.failure.status, 503)
  const message = failureMessage(state.failure)
  assert.ok(message.includes(CONNECTORS_ENDPOINT), `got: ${message}`)
  assert.ok(message.includes('503'), `got: ${message}`)
})

test('an_unknown_connector_fails_the_load_and_names_its_endpoint', async () => {
  // The list names a connector whose operations route answers 404 — the service and the console
  // disagree about what exists. A partial catalogue rendered as a whole one is the same lie as an
  // empty one, so the load fails and says which endpoint refused.
  const fetchImpl = async (url) => {
    if (url === CONNECTORS_ENDPOINT) {
      return new Response(JSON.stringify({
        connectors: [{ id: 'zendesk', vendor: 'Zendesk', description: 'Support', operation_count: 12 }],
      }), {
        status: 200,
      })
    }
    return new Response(JSON.stringify({ error: 'no connector named zendesk' }), { status: 404 })
  }

  const state = await loadCatalogue({ fetch: fetchImpl })
  assert.equal(state.status, 'failed')
  assert.equal(state.failure.endpoint, operationsEndpoint('zendesk'))
  const message = failureMessage(state.failure)
  assert.ok(message.includes('zendesk'), `the refusal names the id the service refused; got: ${message}`)
})

test('a_body_the_console_cannot_read_is_a_failure_and_not_an_empty_catalogue', async () => {
  const garbage = async () => new Response('<html>a proxy answered</html>', { status: 200 })
  const state = await loadCatalogue({ fetch: garbage })

  assert.equal(state.status, 'failed')
  assert.equal(state.failure.kind, 'unreadable')
  assert.equal(state.catalog, undefined)
  assert.ok(failureMessage(state.failure).includes(CONNECTORS_ENDPOINT))
})

test('the_served_metadata_reaches_the_exchange_owned_catalogue', async () => {
  const { fetchImpl } = servedBy({
    zendesk: [
      operation(),
      operation({ id: 'zendesk-ticket-delete', service: 'tickets', risk: 'destructive', idempotency: 'conditional' }),
    ],
  })
  const state = await loadCatalogue({ fetch: fetchImpl })
  assert.equal(state.status, 'ready')

  const [connector] = state.catalog.connectors
  assert.equal(connector.id, 'zendesk')
  assert.equal(connector.vendor, 'Zendesk')
  assert.equal(connector.description, 'zendesk connector')
  assert.deepEqual(
    connector.operations.map((each) => [each.id, each.risk, each.idempotency, each.service]),
    [
      ['zendesk-ticket-show', 'low', 'idempotent', 'default'],
      ['zendesk-ticket-delete', 'destructive', 'conditional', 'tickets'],
    ]
  )

  const [first] = connector.operations
  assert.deepEqual(first.effects, ['network'])
  assert.equal(first.effectsDerived, true)
  assert.equal(first.admitted, null)
  assert.equal(first.method, undefined, 'an unpublished method must not become a blank visible fact')
  assert.equal(first.path, undefined, 'an unpublished path must not become a blank visible fact')
})

test('derived_effects_are_rendered_as_inferred_and_never_as_declared', async () => {
  const derivedState = await loadCatalogue({
    fetch: servedBy({ zendesk: [operation({ effects_derived: true })] }).fetchImpl,
  })
  const derived = await renderToString(createSSRApp(CatalogueOperation, {
    catalog: derivedState.catalog,
    id: 'zendesk-ticket-show',
  }))
  assert.match(derived, /network/)
  assert.match(derived, /inferred/i, 'a derived effect must be presented as an inference')
  assert.doesNotMatch(derived, /declared by the connector/i)

  const declaredState = await loadCatalogue({
    fetch: servedBy({ zendesk: [operation({ effects_derived: false })] }).fetchImpl,
  })
  const declared = await renderToString(createSSRApp(CatalogueOperation, {
    catalog: declaredState.catalog,
    id: 'zendesk-ticket-show',
  }))
  assert.match(declared, /declared by the connector/i)
})

test('admitted_null_is_a_third_state_and_never_reads_as_denied', async () => {
  const rendered = async (admitted) => {
    const state = await loadCatalogue({
      fetch: servedBy({ zendesk: [operation({ admitted })] }).fetchImpl,
    })
    return renderToString(createSSRApp(CatalogueOperation, {
      catalog: state.catalog,
      id: 'zendesk-ticket-show',
    }))
  }

  const unresolved = await rendered(null)
  assert.match(unresolved, /not evaluated/i, 'null must say there is no answer')
  assert.doesNotMatch(
    unresolved,
    /\bdenied\b|\brefused\b|\bnot admitted\b/i,
    'a null admission is not a refusal — there is no principal to refuse'
  )

  const refused = await rendered(false)
  assert.match(refused, /refused/i, 'false is a refusal and must read as one')

  const admitted = await rendered(true)
  assert.match(admitted, /admitted/i)
})

/** The two operations of one connector, as the catalogue routes publish them. */
const zendesk = () => [
  operation(),
  operation({
    id: 'zendesk-ticket-delete',
    service: 'tickets',
    risk: 'destructive',
    idempotency: 'conditional',
  }),
]


/**
 * The same host, serving the same catalogue, holding one tenant's state.
 *
 * The tenant-scoped routes answer differently per tenant and are recorded on `asked`, so a console
 * that started deriving the explorer from what a tenant holds would be caught twice: by the document
 * moving, and by the request it had to make to move it.
 */
function servedToTenant(document, tenant) {
  const { fetchImpl: catalogue, asked } = servedBy(document)
  const json = (body) =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })

  const fetchImpl = async (url, init) => {
    if (url === SESSION_ENDPOINT) {
      asked.push(url)
      return json({ principal: { kind: 'user', id: tenant.who, tenant: tenant.id } })
    }
    if (url === CONNECTIONS_ENDPOINT) {
      asked.push(url)
      return json({ connections: tenant.connections })
    }
    return catalogue(url, init)
  }

  return { fetchImpl, asked }
}

test('the_explorer_is_the_same_document_for_two_tenants', async () => {
  // The explorer is reachable anonymously, so whatever decides a badge on it must not be per-tenant.
  // Driven the way `routes::onboarding::tests::the_document_is_identical_with_two_tenants_connected`
  // drives the descriptor: two tenants that really do differ, and a comparison on the whole document
  // rather than on a field somebody thought to check — a leak worth catching would arrive as a count
  // or a flag, not as a `tenant` key.
  const document = { zendesk: zendesk(), slack: [operation({ id: 'slack-message-post', service: 'chat' })] }

  const hosts = [
    servedToTenant(document, {
      id: 'acme',
      who: 'alice',
      connections: [{ connector: 'zendesk', credentials: [{ name: 'zendesk.api_token', stored: true }] }],
    }),
    servedToTenant(document, { id: 'globex', who: 'bob', connections: [] }),
  ]

  // The two hosts really are holding different state, or this test asserts nothing.
  const held = []
  for (const host of hosts) held.push(await (await host.fetchImpl(CONNECTIONS_ENDPOINT)).text())
  assert.notEqual(held[0], held[1], 'both tenants hold the same connections; the comparison below is empty')

  // Those two probes are requests *this test* made. Clear them, so what remains on `asked` is only
  // what the console asked for.
  for (const host of hosts) host.asked.length = 0

  const documents = []
  for (const host of hosts) {
    const state = await loadCatalogue({ fetch: host.fetchImpl })
    assert.equal(state.status, 'ready')
    documents.push(JSON.stringify(state))
  }

  assert.equal(
    documents[0],
    documents[1],
    'the catalogue moved with the tenant, so something the explorer renders is read from what a ' +
      'host holds rather than from what it serves anonymously'
  )

  // And it never asked. The components take everything they render as props (`components.test.mjs`),
  // so a page that asked nothing tenant-scoped can render nothing tenant-specific.
  for (const host of hosts) {
    assert.ok(host.asked.length > 0, 'the console asked for nothing at all; this assertion would be vacuous')
    for (const url of host.asked) {
      assert.ok(
        url.startsWith(CONNECTORS_ENDPOINT),
        `rendering the catalogue asked for \`${url}\`, which is not one of the anonymous catalogue routes`
      )
    }
  }
})
