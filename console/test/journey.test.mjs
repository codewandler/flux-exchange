// X-88: the operator journey, at pure and mounted seams before the screens compose it.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import ConnectorPicker from '../src/ConnectorPicker.mts'
import { grantPreset, groupAdmitted, previewChange, setupJourney } from '../src/journey-model.mts'
import { bodyFromSchema, validateBody } from '../src/invoking.mts'
import { invokeOperation } from '../src/service.mts'
import { mount, nodes, text } from './mount.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = (name) => readFileSync(path.join(root, 'src', name), 'utf8')

const connectors = [
  {
    id: 'slack', vendor: 'Slack', description: 'Team chat', operationCount: 1,
    operations: [{ id: 'slack-post', service: 'chat', risk: 'medium', inputSchema: {} }],
  },
  {
    id: 'github', vendor: 'GitHub', description: 'Source hosting', operationCount: 2,
    operations: [
      { id: 'github-get', service: 'repos', risk: 'low', inputSchema: {} },
      { id: 'github-delete', service: 'repos', risk: 'destructive', inputSchema: {} },
    ],
  },
]

test('setup_is_one_connect_grant_invoke_journey_derived_from_server_answers', () => {
  const steps = setupJourney({
    connections: [{ connector: 'github', credentials: [{ held: true }] }],
    grants: [{ connector: 'github', admits: [{ id: 'github-get' }] }],
    active: 'grant',
  })
  assert.deepEqual(steps.map(({ id, state }) => [id, state]), [
    ['connect', 'complete'], ['grant', 'current'], ['invoke', 'ready'],
  ])
})

test('connector_picker_searches_human_facts_and_reports_connection_state', async () => {
  const picked = []
  const view = mount(ConnectorPicker, {
    connectors,
    connected: ['github'],
    value: '',
    label: 'Connector',
    onChoose: (id) => picked.push(id),
  })
  const input = nodes(view.root).find((node) => node.props.role === 'combobox')
  assert.ok(input)
  await view.fire(input, 'onInput', { target: { value: 'source' } })
  assert.match(text(view.root), /GitHub/)
  assert.doesNotMatch(text(view.root), /Slack/)
  assert.match(text(view.root), /connected/i)
  const option = nodes(view.root).find((node) => node.props.role === 'option')
  await view.fire(option, 'onClick')
  assert.deepEqual(picked, ['github'])
})

test('grant_presets_are_conservative_metadata_selectors', () => {
  assert.deepEqual(grantPreset('read-only', ['network', 'delete']), {
    maxRisk: 'low', effectsWithin: null, idempotency: null,
  })
  assert.deepEqual(grantPreset('no-destructive', ['network', 'delete', 'money', 'send_external']), {
    maxRisk: null, effectsWithin: ['network', 'send_external'], idempotency: null,
  })
  assert.equal(Object.hasOwn(grantPreset('read-only', []), 'operations'), false)
})

test('preview_says_whether_authority_widens_and_groups_by_service_then_risk', () => {
  assert.equal(previewChange(['github-get'], ['github-get', 'github-delete']), 'wider')
  assert.equal(previewChange(['github-get'], ['github-get']), 'unchanged')
  assert.equal(previewChange(['github-get', 'github-delete'], ['github-get']), 'narrower')
  assert.deepEqual(groupAdmitted({ connectors }, ['github-get', 'github-delete']), [
    { connector: 'github', service: 'repos', risks: [
      { risk: 'low', operations: ['github-get'] },
      { risk: 'destructive', operations: ['github-delete'] },
    ] },
  ])
})

test('invoke_body_starts_from_and_validates_the_published_schema', () => {
  const schema = {
    type: 'object',
    properties: { owner: { type: 'string' }, issue: { type: 'integer' }, labels: { type: 'array' } },
    required: ['owner', 'issue'],
  }
  assert.deepEqual(bodyFromSchema(schema), { owner: '', issue: 0 })
  assert.deepEqual(validateBody(schema, { owner: 'acme', issue: 42 }), [])
  assert.deepEqual(validateBody(schema, { owner: 7 }), [
    '`owner` must be a string', '`issue` is required',
  ])
})

test('invocation_sends_the_parameter_object_verbatim_without_a_credential_write', async () => {
  const asked = []
  const fetch = async (url, init) => {
    asked.push({ url, method: init.method, body: JSON.parse(init.body) })
    return { ok: true, status: 200, json: async () => ({ operation: 'github-get', content: '{"ok":true}', view: null, is_error: false }) }
  }
  const invoked = await invokeOperation('github-get', { owner: 'acme', issue: 42 }, { fetch })
  assert.equal(invoked.status, 'invoked')
  assert.deepEqual(asked.map(({ method, body }) => ({ method, body })), [
    { method: 'POST', body: { owner: 'acme', issue: 42 } },
  ])
})

test('the_ten_findings_have_live_ui_seams', () => {
  assert.match(source('Connections.mts'), /owner-local management/)
  assert.match(source('Grants.mts'), /preset/)
  assert.match(source('Invoke.mts'), /elapsed/)
  assert.match(source('CatalogueFinder.mts'), /mark/)
  assert.match(source('CatalogueFinder.mts'), /keydown/)
  assert.match(source('ConsoleShell.mts'), /rail__future/)
  assert.doesNotMatch(source('service.mts'), /rotateCredential/)
  assert.match(source('service.mts'), /invokeOperation/)
  assert.match(source('shell.css'), /max-width:\s*640px/)
  assert.match(source('app.css'), /prefers-reduced-motion/)
})
