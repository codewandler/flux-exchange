// X-98: workflow authoring stays tenant-derived, upstream-shaped and value-free in the console.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import * as service from '../src/service.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = (file) => readFileSync(path.join(consoleRoot, 'src', file), 'utf8')
const answer = (status, body) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
})

const draft = {
  id: 'triage', title: 'Triage', revision: 4, published_version: 2,
  source: 'flow triage\n  return true\n', graph: null, diagnostics: [], input_schema: {},
}

test('workflow writes preserve exact source and have nowhere to send a tenant', async () => {
  const asked = []
  const fetch = async (url, init = {}) => {
    asked.push({ url, method: init.method, body: JSON.parse(init.body) })
    return answer(200, draft)
  }
  const exact = 'flow triage\n  # operator note\n  return true\n'

  await service.saveWorkflow(draft, exact, { fetch })

  assert.deepEqual(asked, [{
    url: '/api/workflows/triage',
    method: 'PUT',
    body: { revision: 4, title: 'Triage', edit: { mode: 'source', source: exact } },
  }])
  assert.equal(JSON.stringify(asked).includes('tenant'), false)
})

test('visual edits send the upstream graph back for server lowering', async () => {
  const asked = []
  const graph = { schema_version: 1, params: [], body: [{
    id: 'node-1', source_path: 'body[0]', kind: 'call', op: 'github.repo.get',
    args: [{ kind: 'lit', value: { owner: 'codewandler' } }],
  }] }
  const fetch = async (url, init = {}) => {
    asked.push({ url, method: init.method, body: JSON.parse(init.body) })
    return answer(200, { ...draft, graph })
  }

  await service.saveWorkflowGraph(draft, graph, { fetch })

  assert.deepEqual(asked[0].body.edit, { mode: 'graph', graph })
  assert.equal(JSON.stringify(asked).includes('tenant'), false)
})

test('activity parsing retains only the upstream value-free trace vocabulary', async () => {
  const run = {
    id: 'run-1', workflow_id: 'triage', version: 2, status: 'running', result: null,
    error: null, created_at_ms: 42,
    events: [{ sequence: 1, event: {
      node_id: 'send', source_path: 'flow.body[0]', occurrence: 1, phase: 'entered',
      branch: 'then', args: { token: 'must not survive' }, value: 'must not survive',
    } }],
  }
  const state = await service.loadActivity({ fetch: async () => answer(200, { runs: [run] }) })

  assert.equal(state.status, 'ready')
  assert.deepEqual(state.runs[0].events, [{ sequence: 1, event: {
    node_id: 'send', source_path: 'flow.body[0]', occurrence: 1, phase: 'entered', branch: 'then',
  } }])
})

test('workflow and activity views receive data and never fetch it', () => {
  for (const file of ['Workflows.vue', 'Activity.vue']) {
    const view = source(file)
    assert.doesNotMatch(view, /\bfetch\s*\(/)
    assert.doesNotMatch(view, /import(?!\s+type)[^\n]*service\.mts/)
  }
  assert.match(source('Workflows.vue'), /<VueFlow/)
  assert.match(source('Workflows.vue'), /'tree', 'freeform', 'source'/)
  assert.match(source('Workflows.vue'), /addOperation\(operation\)/)
  assert.match(source('Workflows.vue'), /moveSelected\(-1\)/)
  assert.match(source('Activity.vue'), /entry\.event\.node_id/)
})

test('workflow styles name no literal colour and every custom property exists', () => {
  const styles = source('workflows.css')
  const tokens = source('tokens.css')
  assert.doesNotMatch(styles, /#[0-9a-f]{3,8}\b|rgba?\(/i)
  const used = new Set([...styles.matchAll(/var\((--[a-z0-9-]+)/gi)].map((match) => match[1]))
  for (const token of used) {
    assert.match(tokens, new RegExp(`${token.replaceAll('-', '\\-')}\\s*:`), `${token} is not defined`)
  }
})
