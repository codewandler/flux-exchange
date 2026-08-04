import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { CONNECTION_PLAN_VERSION } from '../src/service.mts'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

test('the_console_produces_only_the_v2_plan_and_hosted_fxlm_contract', () => {
  assert.equal(CONNECTION_PLAN_VERSION, 'exchange.connection-plan.v2')

  const service = readFileSync(path.join(consoleRoot, 'src/service.mts'), 'utf8')
  const connect = readFileSync(path.join(consoleRoot, 'src/Connect.mts'), 'utf8')
  const app = readFileSync(path.join(consoleRoot, 'src/App.vue'), 'utf8')
  const production = `${service}\n${connect}\n${app}`

  assert.doesNotMatch(production, /exchange\.connection-plan\.v1/)
  assert.doesNotMatch(production, /ConnectionPlanApply|expected_revisions/)
  assert.doesNotMatch(production, /method:\s*'POST'[\s\S]{0,200}JSON\.stringify\(submission\)/)
  assert.match(service, /exchange\.local-management\.v1/)
  assert.match(service, /new WebSocket|WebSocketConstructor/)
})
