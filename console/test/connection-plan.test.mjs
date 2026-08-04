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
  connectionAuthorityEndpoint,
  connectionPlanEndpoint,
  inspectConnectionAuthority,
  loadConnectionPlan,
  transitionConnectionAuthority,
} from '../src/service.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const fixturePath = path.resolve(consoleRoot, '../tests/fixtures/exchange-connection-plan-v1/connection-plan.v1.json')
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

function completeContract() {
  const complete = structuredClone(contract)
  complete.state = 'complete'
  for (const field of complete.fields) {
    if (!field.required) continue
    field.routable = true
    field.set = true
    if (field.target === null) field.target = { id: `fixture.${field.identity}` }
    delete field.reason
    if (field.authority !== undefined) {
      field.authority.state = 'approved'
      field.authority.actions = { revoke: field.authority.actions.revoke }
    }
  }
  return complete
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
  assert.deepEqual(
    state.plan.fields.map(({ identity, aliases }) => [identity, aliases]),
    contract.fields.map(({ identity, aliases }) => [identity, aliases]),
    'the browser dropped canonical CLI aliases from the shared contract',
  )

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

  const missingAliases = structuredClone(contract)
  delete missingAliases.fields[1].aliases
  const aliasesDropped = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, missingAliases).fetch,
  })
  assert.equal(aliasesDropped.status, 'failed')
  assert.match(aliasesDropped.failure.detail, /aliases/)

  for (const alias of ['site', '--Site', '--site_name', '---site', '--']) {
    const invalidAlias = structuredClone(contract)
    invalidAlias.fields[1].aliases = [alias]
    const invalid = await loadConnectionPlan(contract.connector, null, {
      fetch: answer(200, invalidAlias).fetch,
    })
    assert.equal(invalid.status, 'failed', `accepted malformed CLI alias ${alias}`)
    assert.match(invalid.failure.detail, /alias/i)
  }

  const repeatedWithinField = structuredClone(contract)
  repeatedWithinField.fields[1].aliases = ['--region', '--region']
  const repeated = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, repeatedWithinField).fetch,
  })
  assert.equal(repeated.status, 'failed')
  assert.match(repeated.failure.detail, /repeat.*alias/i)

  const duplicateAlias = structuredClone(contract)
  duplicateAlias.fields[2].aliases = [...duplicateAlias.fields[1].aliases]
  const duplicate = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, duplicateAlias).fetch,
  })
  assert.equal(duplicate.status, 'failed')
  assert.match(duplicate.failure.detail, /alias.*more than once/i)

  const secretAlias = structuredClone(contract)
  secretAlias.fields.find(({ secret }) => secret).aliases = ['--token']
  const secretOnArgv = await loadConnectionPlan(contract.connector, null, {
    fetch: answer(200, secretAlias).fetch,
  })
  assert.equal(secretOnArgv.status, 'failed')
  assert.match(secretOnArgv.failure.detail, /secret.*alias|alias.*secret/i)

  const unknownMembers = [
    (body) => { body.future = true },
    (body) => { body.fields[1].future = true },
    (body) => { body.fields[1].target.future = true },
    (body) => { body.fields[1].choices[0].future = true },
    (body) => { body.apply.future = true },
  ]
  for (const addUnknownMember of unknownMembers) {
    const body = structuredClone(contract)
    addUnknownMember(body)
    const unknown = await loadConnectionPlan(contract.connector, contract.selection, {
      fetch: answer(200, body).fetch,
    })
    assert.equal(unknown.status, 'failed', 'accepted an unknown v1 member')
    assert.match(unknown.failure.detail, /unknown|closed|member/i)
  }
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

test('custom_origin_authority_is_strict_value_free_and_not_a_vendor_schema', async () => {
  const field = contract.fields.find(({ authority }) => authority !== undefined)
  assert.ok(field, 'the shared fixture has no authority-bearing field')

  const html = await form()
  assert.match(html, new RegExp(`data-plan-field="${field.identity}"[\\s\\S]*?data-authority-state="proposed"`))
  assert.match(html, /Review proposed origin/)
  assert.doesNotMatch(html, /Approve proposed authority/)
  assert.match(html, /Revoke proposal/)
  assert.match(html, /cannot use it until an operator approves this revision/i)
  assert.doesNotMatch(JSON.stringify(field), /https?:\/\//, 'the fixture disclosed the proposed origin')
  assert.equal(field.set, false, 'a proposal was reported as runtime-effective before approval')

  const malformed = [
    { ...field.authority, state: 'unset', revision: '1', actions: null },
    { ...field.authority, revision: null },
    { ...field.authority, revision: '0' },
    { ...field.authority, revision: '01' },
    { ...field.authority, revision: '18446744073709551616' },
    { ...field.authority, actions: { ...field.authority.actions, approve: { ...field.authority.actions.approve, method: 'POST' } } },
    { ...field.authority, actions: { ...field.authority.actions, revoke: { ...field.authority.actions.revoke, target: 'https://example.invalid/authority' } } },
    { ...field.authority, state: 'revoked' },
    { ...field.authority, state: 'unknown' },
    { ...field.authority, value: 'must-never-be-accepted' },
  ]
  for (const authority of malformed) {
    const body = structuredClone(contract)
    body.fields.find(({ identity }) => identity === field.identity).authority = authority
    const state = await loadConnectionPlan(contract.connector, contract.selection, { fetch: answer(200, body).fetch })
    assert.equal(state.status, 'failed', `accepted malformed authority ${JSON.stringify(authority)}`)
    assert.match(state.failure.detail, /authority/i)
  }

  const effectiveProposal = structuredClone(contract)
  effectiveProposal.fields.find(({ identity }) => identity === field.identity).set = true
  const inconsistent = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, effectiveProposal).fetch,
  })
  assert.equal(inconsistent.status, 'failed')
  assert.match(inconsistent.failure.detail, /runtime-effective/i)

  const approvedPlan = structuredClone(contract)
  const approvedField = approvedPlan.fields.find(({ identity }) => identity === field.identity)
  approvedField.authority.state = 'approved'
  approvedField.authority.actions = { revoke: approvedField.authority.actions.revoke }
  approvedField.set = true
  const approvedHtml = await form({ plan: { status: 'ready', plan: approvedPlan } })
  assert.match(approvedHtml, /data-authority-state="approved"/)
  assert.match(approvedHtml, /Revoke authority/)
  assert.doesNotMatch(approvedHtml, /Approve proposed authority/)

  const revokedPlan = structuredClone(contract)
  const revokedField = revokedPlan.fields.find(({ identity }) => identity === field.identity)
  revokedField.authority.state = 'revoked'
  revokedField.authority.actions = null
  const revokedHtml = await form({ plan: { status: 'ready', plan: revokedPlan } })
  assert.match(revokedHtml, /data-authority-state="revoked"/)
  assert.doesNotMatch(revokedHtml, /Approve proposed authority/)
  assert.doesNotMatch(revokedHtml, /Revoke proposal|Revoke authority/)

  const unsetPlan = structuredClone(contract)
  const unsetField = unsetPlan.fields.find(({ identity }) => identity === field.identity)
  unsetField.authority = { state: 'unset', revision: null, actions: null }
  const unsetHtml = await form({ plan: { status: 'ready', plan: unsetPlan } })
  assert.match(unsetHtml, /data-authority-state="unset"/)
  assert.match(unsetHtml, /No proposal/)

  const revokedAdvertisingApprove = structuredClone(revokedPlan)
  revokedAdvertisingApprove.fields.find(({ identity }) => identity === field.identity).authority.actions = {
    approve: field.authority.actions.approve,
  }
  const invalidRevoked = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, revokedAdvertisingApprove).fetch,
  })
  assert.equal(invalidRevoked.status, 'failed')
  assert.match(invalidRevoked.failure.detail, /revoked.*action/i)

  const approvedAdvertisingApprove = structuredClone(approvedPlan)
  approvedAdvertisingApprove.fields.find(({ identity }) => identity === field.identity).authority.actions.approve =
    field.authority.actions.approve
  const invalidApproved = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, approvedAdvertisingApprove).fetch,
  })
  assert.equal(invalidApproved.status, 'failed')
  assert.match(invalidApproved.failure.detail, /approved.*action/i)

  const unrelatedTarget = structuredClone(contract)
  const actions = unrelatedTarget.fields.find(({ identity }) => identity === field.identity).authority.actions
  actions.approve.target = '/api/grants'
  actions.revoke.target = '/api/grants'
  const unrelated = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, unrelatedTarget).fetch,
  })
  assert.equal(unrelated.status, 'failed')
  assert.match(unrelated.failure.detail, /declared setting/i)

  assert.equal(
    connectionAuthorityEndpoint('connector/name', 'production west', 'service/name', 'endpoint.custom/origin'),
    '/api/connections/connector%2Fname/instances/production%20west/settings/service%2Fname/endpoint.custom%2Forigin/authority',
  )
})

test('authority_actions_send_only_version_and_revision_and_accept_only_the_matching_transition', async () => {
  const field = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const revision = field.authority.revision
  const approved = {
    version: CONNECTION_PLAN_VERSION,
    connector: contract.connector,
    label: contract.selection,
    service: field.service,
    field: field.binds,
    action: 'approved',
    authority: { state: 'approved', revision },
  }
  const served = answer(200, approved)
  const outcome = await transitionConnectionAuthority({
    connector: contract.connector,
    label: contract.selection,
    service: field.service,
    field: field.binds,
    revision,
    action: field.authority.actions.approve,
  }, { fetch: served.fetch })

  assert.equal(outcome.status, 'answered')
  assert.deepEqual(outcome.result, approved)
  const [{ url, init }] = served.asked
  assert.equal(url, field.authority.actions.approve.target)
  assert.equal(init.method, 'PUT')
  assert.deepEqual(JSON.parse(init.body), { version: CONNECTION_PLAN_VERSION, revision })

  const wrongState = await transitionConnectionAuthority({
    connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
    revision, action: field.authority.actions.approve,
  }, { fetch: answer(200, { ...approved, authority: { state: 'revoked', revision } }).fetch })
  assert.equal(wrongState.status, 'failed')
  assert.match(wrongState.failure.detail, /approve|approved/i)

  const wrongVersion = await transitionConnectionAuthority({
    connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
    revision, action: field.authority.actions.approve,
  }, { fetch: answer(200, { ...approved, version: 'exchange.connection-plan.v2' }).fetch })
  assert.equal(wrongVersion.status, 'failed')
  assert.match(wrongVersion.failure.detail, /version/i)

  const disclosed = await transitionConnectionAuthority({
    connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
    revision, action: field.authority.actions.approve,
  }, { fetch: answer(200, { ...approved, authority: { ...approved.authority, value: 'not-a-response-field' } }).fetch })
  assert.equal(disclosed.status, 'failed')
  assert.match(disclosed.failure.detail, /unexpected|value-free|closed/i)

  const wrongStatus = await transitionConnectionAuthority({
    connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
    revision, action: field.authority.actions.approve,
  }, { fetch: answer(201, approved).fetch })
  assert.equal(wrongStatus.status, 'failed')
  assert.match(wrongStatus.failure.detail, /HTTP 201/i)
})

test('revocation_uses_delete_with_the_same_revision_and_returns_no_origin', async () => {
  const field = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const revision = field.authority.revision
  const body = {
    version: CONNECTION_PLAN_VERSION, connector: contract.connector, label: contract.selection,
    service: field.service, field: field.binds, action: 'revoked',
    authority: { state: 'revoked', revision },
  }
  const served = answer(200, body)
  const outcome = await transitionConnectionAuthority({
    connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
    revision, action: field.authority.actions.revoke,
  }, { fetch: served.fetch })
  assert.equal(outcome.status, 'answered')
  assert.equal(served.asked[0].init.method, 'DELETE')
  assert.deepEqual(JSON.parse(served.asked[0].init.body), { version: CONNECTION_PLAN_VERSION, revision })
  assert.doesNotMatch(JSON.stringify(outcome), /https?:\/\//)
})

test('unsupported_plan_versions_are_refused_before_render_or_submit', async () => {
  const state = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, { ...contract, version: 'exchange.connection-plan.v2' }).fetch,
  })
  assert.equal(state.status, 'failed')
  assert.match(state.failure.detail, /version/i)
  assert.equal(state.plan, undefined)
})

test('top_level_completion_must_match_every_required_routable_set_field', async () => {
  const lying = structuredClone(contract)
  const proposal = lying.fields.find(({ authority }) => authority?.state === 'proposed')
  assert.equal(proposal.required, true)
  assert.equal(proposal.routable, true)
  assert.equal(proposal.set, false)
  lying.state = 'complete'

  const state = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, lying).fetch,
  })
  assert.equal(state.status, 'failed')
  assert.match(state.failure.detail, /complete|required|field/i)
  assert.equal(state.plan, undefined)

  const understated = completeContract()
  understated.state = 'incomplete'
  const inverse = await loadConnectionPlan(contract.connector, contract.selection, {
    fetch: answer(200, understated).fetch,
  })
  assert.equal(inverse.status, 'failed')
  assert.match(inverse.failure.detail, /incomplete|required|field/i)
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
    expected_revisions: {},
  })
})

test('replacement_submissions_carry_the_exact_plan_revision_and_initial_proposals_do_not', () => {
  const field = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const data = new FormData()
  data.set('name', contract.selection)
  data.set(field.target.id, 'https://replacement.example.invalid')

  assert.deepEqual(connectionPlanSubmission(contract, data).expected_revisions, {
    [field.target.id]: field.authority.revision,
  })

  const initial = structuredClone(contract)
  const initialField = initial.fields.find(({ identity }) => identity === field.identity)
  initialField.authority = { state: 'unset', revision: null, actions: null }
  const initialSubmission = connectionPlanSubmission(initial, data)
  assert.deepEqual(initialSubmission.expected_revisions, {})
  assert.equal(field.target.id in initialSubmission.values, true)
})

test('operator_inspection_reads_the_exact_normalized_proposal_without_putting_it_in_the_plan', async () => {
  const field = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const origin = 'https://normalized.example.invalid'
  const inspection = {
    version: CONNECTION_PLAN_VERSION,
    connector: contract.connector,
    label: contract.selection,
    service: field.service,
    field: field.binds,
    authority: { state: 'proposed', revision: field.authority.revision, origin },
  }
  const served = answer(200, inspection)
  const outcome = await inspectConnectionAuthority({
    connector: contract.connector,
    label: contract.selection,
    service: field.service,
    field: field.binds,
    state: field.authority.state,
    revision: field.authority.revision,
  }, { fetch: served.fetch })

  assert.equal(outcome.status, 'answered')
  assert.deepEqual(outcome.result, inspection)
  assert.deepEqual(served.asked.map(({ url, init }) => [url, init?.method ?? 'GET']), [
    [field.authority.actions.approve.target, 'GET'],
  ])
  assert.doesNotMatch(JSON.stringify(contract), new RegExp(origin))

  const beforeReview = await form()
  assert.match(beforeReview, /Review proposed origin/)
  assert.doesNotMatch(beforeReview, /Approve proposed authority/)
  const afterReview = await form({ authorityInspection: outcome })
  assert.match(afterReview, new RegExp(origin.replaceAll('.', '\\.')))
  assert.match(afterReview, new RegExp(`Revision ${field.authority.revision}`))
  assert.match(afterReview, /Approve proposed authority/)

  for (const malformed of [
    { ...inspection, version: 'exchange.connection-plan.v2' },
    { ...inspection, authority: { ...inspection.authority, revision: '43' } },
    { ...inspection, authority: { ...inspection.authority, origin: null } },
    { ...inspection, authority: { ...inspection.authority, authorization: 'must-not-render' } },
    { ...inspection, origin },
  ]) {
    const rejected = await inspectConnectionAuthority({
      connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
      state: field.authority.state, revision: field.authority.revision,
    }, { fetch: answer(200, malformed).fetch })
    assert.equal(rejected.status, 'failed')
    assert.match(rejected.failure.detail, /authority|closed|version|revision|origin/i)
  }
})

test('an_authority_partial_answer_is_explicitly_may_have_happened_and_value_free', async () => {
  const field = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const revision = field.authority.revision
  const partial = {
    version: CONNECTION_PLAN_VERSION,
    connector: contract.connector,
    label: contract.selection,
    service: field.service,
    field: field.binds,
    action: 'approved',
    authority: { state: 'approved', revision },
    outcome: 'partial',
    may_have_happened: true,
  }
  const outcome = await transitionConnectionAuthority({
    connector: contract.connector, label: contract.selection, service: field.service, field: field.binds,
    revision, action: field.authority.actions.approve,
  }, { fetch: answer(207, partial).fetch })

  assert.equal(outcome.status, 'answered')
  assert.equal(outcome.result.outcome, 'partial')
  assert.equal(outcome.result.may_have_happened, true)
  assert.doesNotMatch(JSON.stringify(outcome), /https?:\/\//)
  const html = await form({ authorityOutcome: outcome })
  assert.match(html, /may have happened/i)
  assert.match(html, new RegExp(`Revision ${revision}`))
})

test('secrets_exist_only_in_the_post_body_and_never_in_url_or_returned_outcome', async () => {
  const sentinel = 'sentinel-secret-never-retained'
  const completed = { outcome: 'complete', steps: [], plan: completeContract() }
  const served = answer(200, completed)
  const outcome = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION,
    name: 'production',
    values: { 'credential.example_helpdesk.api_token': sentinel },
    expected_revisions: {},
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
    const body = { outcome: value, steps: [], plan: value === 'complete' ? completeContract() : contract }
    const status = value === 'partial' ? 207 : value === 'refused' ? 422 : 200
    const outcome = await applyConnectionPlan(contract.connector, {
      version: CONNECTION_PLAN_VERSION,
      name: 'production',
      values: {},
      expected_revisions: {},
    }, { fetch: answer(status, body).fetch })

    assert.equal(outcome.status, 'answered', `${value} was collapsed into a transport state`)
    assert.equal(outcome.result.outcome, value)
    const html = await form({ outcome })
    assert.match(html, new RegExp(`data-outcome="${value}"`))
  }

  const proposal = contract.fields.find(({ authority }) => authority?.state === 'proposed')
  const mayHaveHappened = {
    outcome: 'partial',
    steps: [{
      target: proposal.target.id,
      outcome: 'applied',
      action: 'proposed',
      revision: proposal.authority.revision,
      may_have_happened: true,
      reason: 'audit finalization did not confirm the durable proposal',
    }],
    plan: contract,
  }
  const proposedPartial = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION, name: 'production', values: {}, expected_revisions: {},
  }, { fetch: answer(207, mayHaveHappened).fetch })
  assert.equal(proposedPartial.status, 'answered')
  const proposedHtml = await form({ outcome: proposedPartial })
  assert.match(proposedHtml, /proposal revision 42 may have happened/i)
  assert.match(proposedHtml, /audit finalization did not confirm the durable proposal/)
  assert.match(proposedHtml, /re-read/i)
  assert.doesNotMatch(proposedHtml, /https?:\/\//)

  for (const mutate of [
    (body) => { body.outcome = 'complete' },
    (body) => { delete body.steps[0].may_have_happened },
    (body) => { body.steps[0].action = 'approved' },
    (body) => { body.steps[0].revision = '41' },
    (body) => { body.steps[0].target = 'setting.default.endpoint.region' },
    (body) => { body.steps[0].reason = 7 },
  ]) {
    const malformed = structuredClone(mayHaveHappened)
    mutate(malformed)
    const rejected = await applyConnectionPlan(contract.connector, {
      version: CONNECTION_PLAN_VERSION, name: 'production', values: {}, expected_revisions: {},
    }, { fetch: answer(malformed.outcome === 'partial' ? 207 : 200, malformed).fetch })
    assert.equal(rejected.status, 'failed')
    assert.match(rejected.failure.detail, /proposal|partial|step|revision|authority|unknown/i)
  }

  const partialAt200 = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION, name: 'production', values: {}, expected_revisions: {},
  }, { fetch: answer(200, { outcome: 'partial', steps: [], plan: contract }).fetch })
  assert.equal(partialAt200.status, 'failed')
  assert.equal(partialAt200.failure.status, 200)
  assert.match(partialAt200.failure.detail, /cannot carry outcome `partial`/)

  const completeAt500 = await applyConnectionPlan(contract.connector, {
    version: CONNECTION_PLAN_VERSION, name: 'production', values: {}, expected_revisions: {},
  }, { fetch: answer(500, { outcome: 'complete', steps: [], plan: completeContract() }).fetch })
  assert.equal(completeAt500.status, 'failed')
  assert.equal(completeAt500.failure.status, 500)

  for (const body of [
    { outcome: 'complete', steps: [], plan: completeContract(), future: true },
    { outcome: 'complete', steps: [{ target: 'connection.name', outcome: 'applied', future: true }], plan: completeContract() },
  ]) {
    const unknown = await applyConnectionPlan(contract.connector, {
      version: CONNECTION_PLAN_VERSION, name: 'production', values: {}, expected_revisions: {},
    }, { fetch: answer(200, body).fetch })
    assert.equal(unknown.status, 'failed', 'accepted an unknown apply-response member')
    assert.match(unknown.failure.detail, /closed|unknown/i)
  }
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
  assert.match(appSource, /authorityRequests\.admits/)
  assert.match(
    appSource,
    /watch\(\s*\(\) => route\.value\.name,\s*\(next, previous\)[\s\S]*?previous === 'connections'[\s\S]*?invalidateAuthority\(\)/,
    'leaving the connection screen does not clear an inspected normalized origin from root state',
  )
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
