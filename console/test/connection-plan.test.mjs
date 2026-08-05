import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'

import Connect, { connectionPlanSubmission } from '../src/Connect.mts'
import { LatestConnectionRequest } from '../src/connection-plan-state.mts'
import {
  CONNECTION_PLAN_VERSION,
  LOCAL_MANAGEMENT_ENDPOINT,
  LOCAL_MANAGEMENT_PROTOCOL,
  applyConnectionPlan,
  connectionAuthorityEndpoint,
  connectionPlanEndpoint,
  inspectConnectionAuthority,
  loadConnectionPlan,
  transitionConnectionAuthority,
} from '../src/service.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const fixturePath = path.join(consoleRoot, 'test/fixtures/connection-plan.v2.json')
const contract = JSON.parse(readFileSync(fixturePath, 'utf8'))

const form = (props) => renderToString(createSSRApp(Connect, {
  connectors: ['example-helpdesk', 'jira-cloud', 'zendesk'],
  chosen: contract.connector,
  plan: { status: 'ready', plan: contract },
  outcome: null,
  busy: false,
  ...props,
}))

function answer(status, body) {
  const asked = []
  return {
    asked,
    fetch: async (url, init) => {
      asked.push({ url, init })
      return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
    },
  }
}

function unselectedContract() {
  const plan = structuredClone(contract)
  plan.credential_revision = null
  plan.selection = null
  plan.state = 'incomplete'
  for (const field of plan.fields) {
    if (!field.secret) field.set = false
    if (field.authority !== null) field.authority = { actions: [], revision: null, state: 'unset' }
  }
  // The fixture keeps one unroutable row to exercise display; it cannot enter a valid BEGIN.
  plan.fields.at(-1).required = false
  return plan
}

function canonical(value) {
  if (value === null || typeof value === 'boolean' || typeof value === 'string' || typeof value === 'number') {
    return JSON.stringify(value)
  }
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`
}

function wireFrame(direction, opcode, body) {
  const payload = typeof body === 'string' ? new TextEncoder().encode(body) : new TextEncoder().encode(canonical(body))
  const bytes = new Uint8Array(12 + payload.length)
  bytes.set([0x46, 0x58, 0x4c, 0x4d, 1, direction], 0)
  const view = new DataView(bytes.buffer)
  view.setUint16(6, opcode)
  view.setUint32(8, payload.length)
  bytes.set(payload, 12)
  return bytes
}

function decodeFrame(bytes) {
  const copy = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
  const view = new DataView(copy.buffer, copy.byteOffset, copy.byteLength)
  const payload = copy.subarray(12)
  return { direction: copy[5], opcode: view.getUint16(6), payload }
}

class FakeWebSocket {
  static instances = []

  listeners = new Map()
  sent = []
  binaryType = 'blob'
  protocol = LOCAL_MANAGEMENT_PROTOCOL

  constructor(url, protocols) {
    this.url = url
    this.protocols = protocols
    FakeWebSocket.instances.push(this)
    queueMicrotask(() => this.emit('open'))
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? []
    listeners.push(listener)
    this.listeners.set(type, listeners)
  }

  send(bytes) {
    this.sent.push(new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength).slice())
  }

  close() {
    this.emit('close')
  }

  emit(type, data) {
    for (const listener of this.listeners.get(type) ?? []) listener(type === 'message' ? { data } : {})
  }
}

const nextTurn = () => new Promise((resolve) => setImmediate(resolve))

test('the_console_reads_the_complete_closed_v2_plan_and_requests_that_version_explicitly', async () => {
  const served = answer(200, contract)
  const state = await loadConnectionPlan(contract.connector, contract.selection, { fetch: served.fetch })

  assert.deepEqual(served.asked.map(({ url, init }) => [url, init?.method ?? 'GET']), [[
    `${connectionPlanEndpoint(contract.connector)}?version=exchange.connection-plan.v2&name=production`,
    'GET',
  ]])
  assert.equal(CONNECTION_PLAN_VERSION, 'exchange.connection-plan.v2')
  assert.equal(state.status, 'ready')
  assert.deepEqual(state.plan, contract, 'the strict consumer changed or dropped v2 metadata')
  assert.equal(state.plan.fields.filter(({ secret }) => secret).every(({ set }) => set === null), true)
  assert.equal(state.plan.fields.filter(({ target }) => target !== null).every(({ target }) => /^[0-9a-f]{64}$/.test(target.revision)), true)

  for (const mutate of [
    (plan) => { plan.version = 'exchange.connection-plan.v1' },
    (plan) => { plan.apply = { method: 'POST', target: '/api/secret-json' } },
    (plan) => { delete plan.plan_revision },
    (plan) => { plan.credential_revision = null },
    (plan) => { plan.fields.find(({ secret }) => secret).set = false },
    (plan) => { plan.fields.find(({ secret }) => !secret).set = null },
    (plan) => { delete plan.fields[0].also_binds },
    (plan) => { plan.fields[0].target.revision = 'A'.repeat(64) },
    (plan) => { plan.fields[5].target.revision = '8'.repeat(64) },
    (plan) => { plan.fields[3].authority.actions = ['revoke', 'approve'] },
  ]) {
    const malformed = structuredClone(contract)
    mutate(malformed)
    const result = await loadConnectionPlan(contract.connector, contract.selection, { fetch: answer(200, malformed).fetch })
    assert.equal(result.status, 'failed', `accepted ${JSON.stringify(malformed)}`)
    assert.equal(result.plan, undefined)
  }
})

test('completion_ignores_secret_presence_but_requires_each_required_non_secret_fact', async () => {
  const complete = structuredClone(contract)
  complete.fields.at(-1).required = false
  complete.fields.find(({ authority }) => authority !== null).authority = {
    actions: ['revoke'], revision: '42', state: 'approved',
  }
  complete.fields.find(({ identity }) => identity === 'config.default.custom_origin').set = true
  complete.state = 'complete'
  assert.equal((await loadConnectionPlan(contract.connector, contract.selection, { fetch: answer(200, complete).fetch })).status, 'ready')

  const changedSecretFact = structuredClone(complete)
  changedSecretFact.fields.find(({ secret }) => secret).set = true
  const rejectedSecretFact = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, changedSecretFact).fetch,
  })
  assert.equal(rejectedSecretFact.status, 'failed')

  const missingSetting = structuredClone(complete)
  missingSetting.fields[1].set = false
  const rejectedState = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, missingSetting).fetch,
  })
  assert.equal(rejectedState.status, 'failed')
  assert.match(rejectedState.failure.detail, /complete|required|field/i)
})

test('the_form_renders_v2_metadata_but_only_an_unselected_plan_can_begin_connect', async () => {
  const selectedHtml = await form()
  assert.match(selectedHtml, /data-connect="labels"/)
  assert.match(selectedHtml, /data-plan-field="credential\.example_helpdesk\.api_token"[\s\S]*?type="password"/)
  assert.match(selectedHtml, /data-plan-field="credential\.example_helpdesk\.api_token"[\s\S]*?Not reported/)
  assert.match(selectedHtml, /data-authority-state="proposed"/)
  assert.match(selectedHtml, /data-connect="submit" disabled/)
  assert.doesNotMatch(selectedHtml, /Apply connection plan/)

  const unselected = unselectedContract()
  const createHtml = await form({ plan: { status: 'ready', plan: unselected } })
  assert.match(createHtml, /data-connect="submit"[^>]*>Connect</)
  assert.match(createHtml, /Secret controls cross only as raw local-management frames/)
  assert.doesNotMatch(createHtml, /expected_revisions|compensation|method=&quot;POST&quot;/)
})

test('the_submission_separates_value_free_begin_json_from_raw_secret_ordinals', () => {
  const plan = unselectedContract()
  const data = new FormData()
  data.set('name', 'new-production')
  data.set('setting.default.endpoint.region', 'europe')
  data.set('setting.default.username.example_helpdesk.api_token', 'owner@example.invalid')
  data.set('setting.default.endpoint.custom_origin', 'https://helpdesk.example.invalid')
  data.set('credential.example_helpdesk.service_token', 'sentinel-one')
  data.set('credential.example_helpdesk.api_token', 'sentinel-two')
  data.set('not-a-target', 'must-not-cross')

  const submission = connectionPlanSubmission(plan, data)
  assert.equal(submission.begin.connector, plan.connector)
  assert.equal(submission.begin.label, 'new-production')
  assert.equal(submission.begin.plan_revision, plan.plan_revision)
  assert.equal(submission.begin.targets[0].target, 'connection.name')
  assert.deepEqual(submission.begin.authorities, [{
    revision: null, target: 'setting.default.endpoint.custom_origin',
  }])
  assert.equal(JSON.stringify(submission.begin).includes('sentinel-'), false)
  assert.equal(JSON.stringify(submission.begin).includes('must-not-cross'), false)
  assert.deepEqual(submission.secrets.map(({ target, value }) => [target, new TextDecoder().decode(value)]), [
    ['credential.example_helpdesk.service_token', 'sentinel-one'],
    ['credential.example_helpdesk.api_token', 'sentinel-two'],
  ])
})

test('hosted_connect_uses_one_exact_subprotocol_and_raw_ordered_secret_frames', async () => {
  FakeWebSocket.instances.length = 0
  const plan = unselectedContract()
  const data = new FormData()
  data.set('name', 'new-production')
  data.set('setting.default.endpoint.region', 'europe')
  data.set('setting.default.username.example_helpdesk.api_token', 'owner@example.invalid')
  data.set('setting.default.endpoint.custom_origin', 'https://helpdesk.example.invalid')
  data.set('credential.example_helpdesk.service_token', 'raw-sentinel-one')
  data.set('credential.example_helpdesk.api_token', 'raw-sentinel-two')
  const submission = connectionPlanSubmission(plan, data)

  const pending = applyConnectionPlan(plan.connector, submission, {
    webSocket: FakeWebSocket,
    href: 'https://exchange.example.invalid/connections',
  })
  await nextTurn()
  const socket = FakeWebSocket.instances[0]
  assert.equal(socket.url, `wss://exchange.example.invalid${LOCAL_MANAGEMENT_ENDPOINT}`)
  assert.equal(socket.protocols, LOCAL_MANAGEMENT_PROTOCOL)
  assert.equal(socket.binaryType, 'arraybuffer')

  const begin = decodeFrame(socket.sent[0])
  assert.equal(begin.direction, 1)
  assert.equal(begin.opcode, 0x0001)
  assert.equal(new TextDecoder().decode(begin.payload), canonical(submission.begin))
  assert.doesNotMatch(new TextDecoder().decode(begin.payload), /raw-sentinel/)

  const transaction = '0000000000000001' + '2'.repeat(48)
  const digest = '3'.repeat(64)
  socket.emit('message', wireFrame(2, 0x0002, {
    proposal_digest: digest,
    secrets: submission.secrets.map(({ target }, at) => ({ ordinal: at + 1, target })),
    transaction_id: transaction,
  }))
  await nextTurn()

  const secretOne = decodeFrame(socket.sent[1])
  const secretTwo = decodeFrame(socket.sent[2])
  assert.equal(secretOne.opcode, 0x0003)
  assert.equal(new DataView(secretOne.payload.buffer, secretOne.payload.byteOffset).getUint16(0), 1)
  assert.equal(new TextDecoder().decode(secretOne.payload.subarray(2)), 'raw-sentinel-one')
  assert.equal(new DataView(secretTwo.payload.buffer, secretTwo.payload.byteOffset).getUint16(0), 2)
  assert.equal(new TextDecoder().decode(secretTwo.payload.subarray(2)), 'raw-sentinel-two')
  const commit = decodeFrame(socket.sent[3])
  assert.equal(commit.opcode, 0x0004)
  assert.deepEqual(JSON.parse(new TextDecoder().decode(commit.payload)), {
    proposal_digest: digest, transaction_id: transaction,
  })

  socket.emit('message', wireFrame(2, 0x0006, {
    commit: { audit: 'committed', resource: 'committed' },
    connector: plan.connector,
    label: 'new-production',
    operation: 'connect',
    receipt_id: '4'.repeat(64),
    replayed: false,
    schema: 'exchange.connect-receipt.v1',
  }))
  assert.deepEqual(await pending, {
    status: 'answered',
    result: {
      commit: { audit: 'committed', resource: 'committed' },
      connector: plan.connector,
      label: 'new-production',
      operation: 'connect',
      receipt_id: '4'.repeat(64),
      replayed: false,
      schema: 'exchange.connect-receipt.v1',
    },
  })
})

test('hosted_connect_rejects_changed_secret_order_and_maps_only_closed_value_free_errors', async () => {
  FakeWebSocket.instances.length = 0
  const plan = unselectedContract()
  const data = new FormData()
  data.set('name', 'new-production')
  data.set('credential.example_helpdesk.service_token', 'one')
  data.set('credential.example_helpdesk.api_token', 'two')
  const submission = connectionPlanSubmission(plan, data)
  const pending = applyConnectionPlan(plan.connector, submission, {
    webSocket: FakeWebSocket, href: 'http://127.0.0.1:3000/',
  })
  await nextTurn()
  const socket = FakeWebSocket.instances[0]
  socket.emit('message', wireFrame(2, 0x0002, {
    proposal_digest: '3'.repeat(64),
    secrets: submission.secrets.map(({ target }, at) => ({ ordinal: at + 1, target })).reverse(),
    transaction_id: '0000000000000001' + '2'.repeat(48),
  }))
  const malformed = await pending
  assert.equal(malformed.status, 'failed')
  assert.match(malformed.failure.detail, /ordinal|target order/i)

  FakeWebSocket.instances.length = 0
  const refusedPending = applyConnectionPlan(plan.connector, submission, {
    webSocket: FakeWebSocket, href: 'http://127.0.0.1:3000/',
  })
  await nextTurn()
  FakeWebSocket.instances[0].emit('message', wireFrame(2, 0x7fff, {
    code: 'stale_plan', commit: 'none', retry: 'refresh',
    schema: 'exchange.local-management-error.v1', status: 409,
  }))
  assert.deepEqual(await refusedPending, {
    status: 'refused',
    refusal: { endpoint: LOCAL_MANAGEMENT_ENDPOINT, status: 409, error: 'stale_plan' },
  })
})

test('a_response_loss_retry_accepts_the_direct_same_proposal_receipt_without_resending_secrets', async () => {
  FakeWebSocket.instances.length = 0
  const plan = unselectedContract()
  const data = new FormData()
  data.set('name', 'replayed-production')
  data.set('credential.example_helpdesk.service_token', 'must-not-be-resent')
  data.set('credential.example_helpdesk.api_token', 'must-not-be-resent-either')
  const submission = connectionPlanSubmission(plan, data)
  const pending = applyConnectionPlan(plan.connector, submission, {
    webSocket: FakeWebSocket, href: 'https://exchange.example.invalid/',
  })
  await nextTurn()
  const socket = FakeWebSocket.instances[0]
  socket.emit('message', wireFrame(2, 0x0006, {
    commit: { audit: 'committed', resource: 'committed' },
    connector: plan.connector,
    label: 'replayed-production',
    operation: 'connect',
    receipt_id: '5'.repeat(64),
    replayed: true,
    schema: 'exchange.connect-receipt.v1',
  }))
  const outcome = await pending
  assert.equal(outcome.status, 'answered')
  assert.equal(outcome.result.replayed, true)
  assert.deepEqual(socket.sent.map((bytes) => decodeFrame(bytes).opcode), [0x0001])
  assert.equal(socket.sent.some((bytes) => new TextDecoder().decode(bytes).includes('must-not-be-resent')), false)
})

test('v2_authority_actions_are_names_and_requests_derive_the_same_origin_endpoint', async () => {
  const field = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const transition = {
    connector: contract.connector,
    label: contract.selection,
    service: field.service,
    field: field.binds,
    revision: field.authority.revision,
    action: 'approve',
  }
  const response = {
    version: CONNECTION_PLAN_VERSION,
    connector: transition.connector,
    label: transition.label,
    service: transition.service,
    field: transition.field,
    action: 'approved',
    authority: { state: 'approved', revision: transition.revision },
  }
  const served = answer(200, response)
  assert.deepEqual(await transitionConnectionAuthority(transition, { fetch: served.fetch }), {
    status: 'answered', result: response,
  })
  assert.equal(served.asked[0].url, connectionAuthorityEndpoint(
    transition.connector, transition.label, transition.service, transition.field,
  ))
  assert.equal(served.asked[0].init.method, 'PUT')
  assert.deepEqual(JSON.parse(served.asked[0].init.body), {
    version: CONNECTION_PLAN_VERSION, revision: transition.revision,
  })

  const inspected = {
    version: CONNECTION_PLAN_VERSION,
    connector: transition.connector,
    label: transition.label,
    service: transition.service,
    field: transition.field,
    authority: { state: 'proposed', revision: transition.revision, origin: 'https://normalized.example.invalid' },
  }
  const inspection = await inspectConnectionAuthority({
    connector: transition.connector,
    label: transition.label,
    service: transition.service,
    field: transition.field,
    state: 'proposed',
    revision: transition.revision,
  }, { fetch: answer(200, inspected).fetch })
  assert.deepEqual(inspection, { status: 'answered', result: inspected })
  assert.doesNotMatch(JSON.stringify(contract), /normalized\.example\.invalid/)
})

test('loading_refusal_failure_and_request_generation_states_remain_distinct', async () => {
  assert.match(await form({ plan: { status: 'loading' } }), /Reading example-helpdesk&#39;s connection plan/)
  assert.match(await form({
    plan: { status: 'refused', refusal: { endpoint: '/plan', status: 403, error: 'operator required' } },
  }), /data-connect="refused"/)
  assert.match(await form({
    plan: { status: 'failed', failure: { kind: 'unreachable', endpoint: '/plan', status: null, detail: 'fetch failed' } },
  }), /data-connect="plan-failed"/)

  const requests = new LatestConnectionRequest()
  const oldConnector = requests.begin('first', null)
  const oldLabel = requests.begin('second', 'sandbox')
  const current = requests.begin('second', 'production')
  assert.equal(requests.admits(oldConnector, 'second', 'production'), false)
  assert.equal(requests.admits(oldLabel, 'second', 'production'), false)
  assert.equal(requests.admits(current, 'second', 'production'), true)
  requests.invalidate()
  assert.equal(requests.admits(current, 'second', 'production'), false)
})

test('secret_values_have_no_reactive_persistent_or_json_mirror', () => {
  const connect = readFileSync(path.join(consoleRoot, 'src/Connect.mts'), 'utf8')
  const app = readFileSync(path.join(consoleRoot, 'src/App.vue'), 'utf8')
  const service = readFileSync(path.join(consoleRoot, 'src/service.mts'), 'utf8')
  for (const source of [connect, app]) {
    for (const store of ['localStorage', 'sessionStorage', 'document.cookie', 'history.pushState', 'history.replaceState']) {
      assert.doesNotMatch(source, new RegExp(store.replace('.', '\\.')))
    }
  }
  assert.match(connect, /new FormData\(event\.currentTarget/)
  assert.match(connect, /outcome\?\.status === 'answered'/)
  assert.match(service, /secretFrame\(at \+ 1, secret\.value\)/)
  assert.doesNotMatch(service, /JSON\.stringify\(submission\)/)
})
