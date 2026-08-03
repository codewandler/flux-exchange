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
  applyConnectionPlan,
  connectionPlanEndpoint,
  loadConnectionPlan,
} from '../src/service.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const fixturePath = path.resolve(consoleRoot, '../docs/fixtures/connection-plan.v1.json')
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

test('the_shared_v1_contract_is_read_whole_and_a_malformed_declared_row_fails_the_read', async () => {
  const served = answer(200, contract)
  const state = await loadConnectionPlan(contract.connector, contract.selection, { fetch: served.fetch })

  assert.deepEqual(served.asked.map(({ url, init }) => [url, init?.method ?? 'GET']), [
    [`${connectionPlanEndpoint(contract.connector)}?name=production`, 'GET'],
  ])
  assert.equal(state.status, 'ready')
  assert.equal(state.plan.version, CONNECTION_PLAN_VERSION)
  assert.deepEqual(state.plan, contract, 'the strict parser changed or dropped contract metadata')
  assert.deepEqual(state.plan.fields.map(({ identity }) => identity), contract.fields.map(({ identity }) => identity))

  const malformed = structuredClone(contract)
  delete malformed.fields[2].target
  const rejected = await loadConnectionPlan(contract.connector, null, { fetch: answer(200, malformed).fetch })
  assert.equal(rejected.status, 'failed')
  assert.equal(rejected.failure.kind, 'unreadable')
  assert.match(rejected.failure.detail, /target/)
  assert.equal(rejected.plan, undefined, 'an unreadable plan must never become a partial form')

  const wrong = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, { ...contract, connector: 'another-connector' }).fetch,
  })
  assert.equal(wrong.status, 'failed')
  assert.match(wrong.failure.detail, /another-connector.*example-helpdesk/)

  const emptyChoices = structuredClone(contract)
  emptyChoices.fields[1].choices = []
  const openClaimedClosed = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, emptyChoices).fetch,
  })
  assert.equal(openClaimedClosed.status, 'failed')
  assert.match(openClaimedClosed.failure.detail, /empty choices/)

  const missingProvenance = structuredClone(contract)
  delete missingProvenance.fields[3].provenance
  const provenanceDropped = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, missingProvenance).fetch,
  })
  assert.equal(provenanceDropped.status, 'failed')
  assert.match(provenanceDropped.failure.detail, /provenance/)
})

test('the_generic_consumer_renders_every_shared_contract_descriptor_without_vendor_logic', async () => {
  const html = await form({ plan: { status: 'ready', plan: contract } })
  assert.deepEqual(
    [...html.matchAll(/data-plan-field="([^"]+)"/g)].map((match) => match[1]),
    contract.fields.map(({ identity }) => identity),
    'the browser dropped or reordered a descriptor from the shared contract',
  )

  const source = readFileSync(path.join(consoleRoot, 'src/Connect.mts'), 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/\/\/.*$/gm, ' ')
  for (const vendorWord of ['jira', 'zendesk', 'site', 'subdomain', 'domain', 'api_token']) {
    assert.doesNotMatch(source, new RegExp(vendorWord, 'i'), `Connect.mts carries vendor/schema logic for ${vendorWord}`)
  }
})

test('the_form_renders_labels_rename_choices_secrets_optional_status_and_unroutable_rows', async () => {
  const optional = structuredClone(contract)
  optional.fields[2].required = false
  const html = await form({ plan: { status: 'ready', plan: optional } })

  assert.match(html, /data-connect="labels"/)
  for (const label of contract.labels) assert.match(html, new RegExp(`>${label}<`))
  assert.match(html, /data-plan-field="connection.name"[\s\S]*?name="name"/)
  assert.match(html, /value="production"/)
  assert.match(html, /data-plan-field="config.default.region"[\s\S]*?<select/)
  for (const choice of contract.fields[1].choices) {
    assert.match(html, new RegExp(`value="${choice.value}"[^>]*>${choice.label}<`))
  }
  assert.match(html, /data-plan-field="credential.example_helpdesk.api_token"[\s\S]*?data-provenance="provider.auth"/)
  assert.match(html, /data-plan-field="credential.example_helpdesk.api_token"[\s\S]*?type="password"/)
  assert.equal([...html.matchAll(/<input[^>]*\srequired(?:\s|>)/g)].length, 1, 'only the name blocks submission')
  assert.match(html, /data-required="false"/)
  assert.match(html, /Optional/)
  assert.match(html, /data-routable="false"/)
  assert.match(html, /not admitted by this build/)
  assert.match(html, /Incomplete/)
})

test('one_control_and_one_submission_key_represent_rows_that_share_a_target', async () => {
  const html = await form()
  assert.equal(
    [...html.matchAll(/name="credential\.example_helpdesk\.service_token"/g)].length,
    1,
    'two descriptors sharing one target must ask for the value only once',
  )

  const data = new FormData()
  data.set('name', 'renamed-support')
  data.set('credential.example_helpdesk.service_token', 'one-token')
  data.set('setting.default.endpoint.region', 'europe')
  data.set('not-a-published-target', 'must-not-be-sent')

  const submission = connectionPlanSubmission(contract, data)
  assert.deepEqual(submission, {
    version: CONNECTION_PLAN_VERSION,
    name: 'renamed-support',
    current_name: 'production',
    values: {
      'credential.example_helpdesk.service_token': 'one-token',
      'setting.default.endpoint.region': 'europe',
    },
  })
})

test('secrets_exist_only_in_the_post_body_and_never_in_url_or_returned_outcome', async () => {
  const sentinel = 'sentinel-secret-never-retained'
  const completed = { outcome: 'complete', steps: [], plan: { ...contract, state: 'complete' } }
  const served = answer(200, completed)
  const outcome = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION,
    name: 'production',
    values: { 'credential.example_helpdesk.api_token': sentinel },
  }, { fetch: served.fetch })

  const [{ url, init }] = served.asked
  assert.equal(url, connectionPlanEndpoint(contract.connector))
  assert.equal(init.method, 'POST')
  assert.match(init.body, new RegExp(sentinel))
  assert.doesNotMatch(url, new RegExp(sentinel))
  assert.doesNotMatch(JSON.stringify(outcome), new RegExp(sentinel))

  const html = await form({ plan: { status: 'ready', plan: completed.plan }, outcome })
  assert.doesNotMatch(html, new RegExp(sentinel))
})

test('complete_incomplete_refused_and_partial_apply_outcomes_are_distinct', async () => {
  for (const value of ['complete', 'incomplete', 'refused', 'partial']) {
    const body = { outcome: value, steps: [], plan: { ...contract, state: value === 'complete' ? 'complete' : 'incomplete' } }
    const status = value === 'partial' ? 207 : value === 'refused' ? 422 : 200
    const outcome = await applyConnectionPlan(contract.connector, {
      version: CONNECTION_PLAN_VERSION,
      name: 'production',
      values: {},
    }, { fetch: answer(status, body).fetch })

    assert.equal(outcome.status, 'answered', `${value} was collapsed into a transport state`)
    assert.equal(outcome.result.outcome, value)
    const html = await form({ outcome })
    assert.match(html, new RegExp(`data-outcome="${value}"`))
  }

  const partialAt200 = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION, name: 'production', values: {},
  }, { fetch: answer(200, { outcome: 'partial', steps: [], plan: contract }).fetch })
  assert.equal(partialAt200.status, 'failed')
  assert.equal(partialAt200.failure.status, 200)
  assert.match(partialAt200.failure.detail, /cannot carry outcome `partial`/)

  const completeAt500 = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION, name: 'production', values: {},
  }, { fetch: answer(500, { outcome: 'complete', steps: [], plan: { ...contract, state: 'complete' } }).fetch })
  assert.equal(completeAt500.status, 'failed')
  assert.equal(completeAt500.failure.status, 500)
})

test('loading_refusal_failure_and_picker_states_remain_distinct', async () => {
  const loading = await form({ plan: { status: 'loading' } })
  assert.match(loading, /Reading example-helpdesk&#39;s connection plan/)
  assert.match(loading, /data-connect="submit" disabled/)
  assert.match(loading, /role="combobox"/)

  const sentence = 'this operator may not configure this connector'
  const refused = await form({ plan: { status: 'refused', refusal: { endpoint: '/plan', status: 403, error: sentence } } })
  assert.match(refused, new RegExp(sentence))
  assert.match(refused, /data-connect="refused"/)

  const failed = await form({
    plan: { status: 'failed', failure: { kind: 'unreachable', endpoint: '/api/connections/example-helpdesk/plan', status: null, detail: 'fetch failed' } },
  })
  assert.match(failed, /could not be reached/)
  assert.match(failed, /data-connect="plan-failed"/)
})

test('secret_values_have_no_reactive_or_persistent_mirror', () => {
  const connectSource = readFileSync(path.join(consoleRoot, 'src/Connect.mts'), 'utf8')
  const appSource = readFileSync(path.join(consoleRoot, 'src/App.vue'), 'utf8')
  for (const source of [connectSource, appSource]) {
    for (const store of ['localStorage', 'sessionStorage', 'document.cookie', 'history.pushState', 'history.replaceState']) {
      assert.doesNotMatch(source, new RegExp(store.replace('.', '\\.')))
    }
  }
  assert.match(connectSource, /new FormData\(event\.currentTarget/)
  assert.match(appSource, /connectionPlanRequests\.admits/)
  assert.match(appSource, /connectionApplyRequests\.admits/)
})

test('a_late_plan_for_an_old_connector_or_label_cannot_replace_the_current_one', () => {
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
