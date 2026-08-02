// The fragment router, including X-86's route-local finder state and old provider-link migration.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { explorerPath, fragmentPath, migrateLegacySearch, parseRoute } from '../src/routing.ts'

test('a path with an anchor resolves to a URL with exactly one fragment', () => {
  const href = fragmentPath('/explorer#airtable')

  assert.equal(
    href.split('#').length - 1,
    1,
    `a URL has one fragment; a second "#" makes the first one swallow the rest: ${href}`,
  )
})

test('an_old_provider_anchor_becomes_the_connector_search_it_meant', () => {
  const href = fragmentPath('/explorer#airtable')
  const hash = href.slice(href.indexOf('#'))

  assert.deepEqual(parseRoute(hash), {
    name: 'explorer',
    view: { kind: 'connectors', query: 'airtable' },
  })
})

test('an operation path with an anchor keeps both halves', () => {
  const href = fragmentPath('/operations/airtable-record-create#request')
  const hash = href.slice(href.indexOf('#'))

  assert.deepEqual(parseRoute(hash), {
    name: 'operation',
    id: 'airtable-record-create',
    anchor: 'request',
  })
})

test('a path with no anchor is unchanged in meaning', () => {
  assert.deepEqual(parseRoute(fragmentPath('/explorer').slice(1)), {
    name: 'explorer',
    view: { kind: 'connectors', query: '' },
  })
  assert.deepEqual(parseRoute('#/operations/zendesk-test'), {
    name: 'operation',
    id: 'zendesk-test',
  })
})

test('finder_state_lives_inside_the_explorer_fragment', () => {
  const path = explorerPath({ kind: 'operations', query: 'google gmail' })
  assert.equal(path, '/explorer?kind=operations&q=google+gmail')
  assert.deepEqual(parseRoute(`#${path}`), {
    name: 'explorer',
    view: { kind: 'operations', query: 'google gmail' },
  })
})

test('a_legacy_document_query_moves_to_the_finder_without_overriding_new_state', () => {
  const empty = parseRoute('#/explorer')
  assert.deepEqual(migrateLegacySearch(empty, '?q=slack'), {
    name: 'explorer',
    view: { kind: 'connectors', query: 'slack' },
  })

  const current = parseRoute('#/explorer?kind=operations&q=github')
  assert.deepEqual(migrateLegacySearch(current, '?q=slack'), current)
})

// An unrecognised path still says so rather than quietly showing the explorer — the reason
// `parseRoute` has an `unknown` arm at all. An anchor must not smuggle a bad path past that.
test('an anchor does not turn an unknown path into a known one', () => {
  const hash = fragmentPath('/nowhere#airtable').slice(1)

  assert.equal(parseRoute(hash).name, 'unknown')
})
