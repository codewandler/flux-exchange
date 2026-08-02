// X-86: the exchange-owned catalogue finder, tested at its pure seam.
//
// The old copied explorer spread filtering across component state and a documentation-shaped data
// contract. This test names the product behavior directly: one query, three real kinds, only visible
// facts searchable, and relevance without losing catalogue order as the stable tie-breaker.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import CatalogueFinder from '../src/CatalogueFinder.mts'
import { mount, nodes, text } from './mount.mjs'

import {
  decodeSearchView,
  deriveServices,
  emptySearchView,
  encodeSearchView,
  searchCounts,
  searchCatalogue,
} from '../src/catalog.mts'

const operation = (id, over = {}) => ({
  id,
  service: 'default',
  description: `Use ${id}`,
  inputSchema: { type: 'object', properties: {}, required: [] },
  risk: 'low',
  idempotency: 'idempotent',
  effects: ['network'],
  effectsDerived: true,
  admitted: null,
  ...over,
})

const catalog = {
  connectors: [
    {
      id: 'slack',
      vendor: 'Slack',
      description: 'Team chat and collaboration.',
      operationCount: 2,
      operations: [
        operation('slack-message-post', {
          service: 'chat',
          description: 'Post a message to a channel.',
          risk: 'medium',
          idempotency: 'conditional',
          effects: ['network', 'send_external'],
        }),
        operation('slack-user-show', { service: 'users', description: 'Read one user.' }),
      ],
    },
    {
      id: 'github',
      vendor: 'GitHub',
      description: 'Source hosting and pull requests.',
      operationCount: 2,
      operations: [
        operation('github-slack-import', {
          service: 'repos',
          description: 'Import a Slack archive.',
        }),
        operation('github-repository-delete', {
          service: 'repos',
          description: 'Delete one repository.',
          risk: 'destructive',
          idempotency: 'conditional',
          effects: ['network', 'delete'],
        }),
      ],
    },
  ],
}

test('services_are_derived_per_connector_without_a_second_catalogue', () => {
  assert.deepEqual(
    deriveServices(catalog).map(({ connector, service }) => [connector.id, service.name, service.operationCount]),
    [
      ['slack', 'chat', 1],
      ['slack', 'users', 1],
      ['github', 'repos', 2],
    ]
  )
})

test('one_query_searches_every_visible_fact_in_the_active_kind', () => {
  const connectors = searchCatalogue(catalog, { kind: 'connectors', query: 'team chat' })
  assert.deepEqual(connectors.map((result) => result.connector.id), ['slack'])

  const services = searchCatalogue(catalog, { kind: 'services', query: 'slack users' })
  assert.deepEqual(services.map((result) => [result.connector.id, result.service.name]), [['slack', 'users']])

  const operations = searchCatalogue(catalog, { kind: 'operations', query: 'destructive delete' })
  assert.deepEqual(operations.map((result) => result.operation.id), ['github-repository-delete'])

  assert.deepEqual(searchCounts(catalog, 'slack'), {
    connectors: 1,
    services: 2,
    operations: 3,
  })
})

test('exact_and_prefix_primary_names_rank_before_metadata_matches', () => {
  const exact = searchCatalogue(catalog, { kind: 'operations', query: 'slack-message-post' })
  assert.equal(exact[0].operation.id, 'slack-message-post')

  const slack = searchCatalogue(catalog, { kind: 'operations', query: 'slack' })
  assert.deepEqual(
    slack.map((result) => result.operation.id),
    ['slack-message-post', 'slack-user-show', 'github-slack-import'],
    'primary id matches precede a description-only match and source order breaks the tie'
  )
})

test('an_empty_query_browses_the_active_kind_in_catalogue_order', () => {
  const view = emptySearchView()
  assert.deepEqual(view, { kind: 'connectors', query: '' })
  assert.deepEqual(
    searchCatalogue(catalog, view).map((result) => result.connector.id),
    ['slack', 'github']
  )
})

test('finder_urls_are_canonical_shareable_and_widen_unknown_state', () => {
  assert.equal(encodeSearchView(emptySearchView()), '')
  assert.equal(
    encodeSearchView({ kind: 'operations', query: ' google  gmail ' }),
    'kind=operations&q=google+gmail'
  )
  assert.deepEqual(decodeSearchView('?kind=services&q=google+gmail'), {
    kind: 'services',
    query: 'google gmail',
  })
  assert.deepEqual(decodeSearchView('?kind=channels&q=slack'), {
    kind: 'connectors',
    query: 'slack',
  })
  assert.deepEqual(decodeSearchView('?risk=destructive&sort=id'), emptySearchView())
})

test('the_finder_renders_one_search_bar_and_three_real_accessible_tabs', () => {
  const view = mount(CatalogueFinder, { catalog, initialView: emptySearchView() })
  const searches = nodes(view.root).filter((node) => node.tag === 'input' && node.props.type === 'search')
  const tabs = nodes(view.root).filter((node) => node.props.role === 'tab')

  assert.equal(searches.length, 1)
  assert.equal(tabs.length, 3)
  assert.deepEqual(tabs.map((tab) => tab.props['data-kind']), ['connectors', 'services', 'operations'])
  assert.equal(tabs[0].props['aria-selected'], 'true')
  assert.ok(!text(view.root).includes('Channels'), 'a tab with no real channel metadata was rendered')
  assert.match(text(view.root), /Slack/)
  assert.match(text(view.root), /GitHub/)
})

test('the_query_persists_across_tabs_and_connector_results_narrow_to_operations', async () => {
  const view = mount(CatalogueFinder, { catalog, initialView: emptySearchView() })
  const search = nodes(view.root).find((node) => node.tag === 'input' && node.props.type === 'search')
  await view.fire(search, 'onInput', { target: { value: 'slack' } })

  const services = nodes(view.root).find(
    (node) => node.props.role === 'tab' && node.props['data-kind'] === 'services'
  )
  await view.fire(services, 'onClick')
  assert.equal(
    nodes(view.root).find((node) => node.tag === 'input' && node.props.type === 'search').props.value,
    'slack'
  )
  assert.match(text(view.root), /chat/)
  assert.doesNotMatch(text(view.root), /repos/)

  const connectors = nodes(view.root).find(
    (node) => node.props.role === 'tab' && node.props['data-kind'] === 'connectors'
  )
  await view.fire(connectors, 'onClick')
  const action = nodes(view.root).find((node) => node.props.class === 'finder-card__action')
  await view.fire(action, 'onClick')

  assert.equal(
    nodes(view.root).find((node) => node.tag === 'input' && node.props.type === 'search').props.value,
    'slack'
  )
  assert.match(text(view.root), /slack-message-post/)
  assert.doesNotMatch(text(view.root), /github-repository-delete/)
})

test('arrow_keys_move_and_focus_the_active_tab', async () => {
  const view = mount(CatalogueFinder, { catalog, initialView: emptySearchView() })
  const connectors = nodes(view.root).find(
    (node) => node.props.role === 'tab' && node.props['data-kind'] === 'connectors'
  )
  let focused = false
  let prevented = false
  await view.fire(connectors, 'onKeydown', {
    key: 'ArrowRight',
    preventDefault: () => {
      prevented = true
    },
    currentTarget: {
      parentElement: {
        querySelector: (selector) => {
          assert.equal(selector, '[data-kind="services"]')
          return { focus: () => { focused = true } }
        },
      },
    },
  })

  const services = nodes(view.root).find(
    (node) => node.props.role === 'tab' && node.props['data-kind'] === 'services'
  )
  assert.equal(services.props['aria-selected'], 'true')
  assert.equal(prevented, true)
  assert.equal(focused, true)
})

test('an_empty_result_names_the_active_kind_and_can_clear_the_query', async () => {
  const view = mount(CatalogueFinder, {
    catalog,
    initialView: { kind: 'operations', query: 'nothing-here' },
  })
  assert.match(text(view.root), /No operations match “nothing-here”/)

  const clear = nodes(view.root).find((node) => node.props.class === 'finder__empty-action')
  await view.fire(clear, 'onClick')
  assert.match(text(view.root), /Browsing 4 operations/)
})
