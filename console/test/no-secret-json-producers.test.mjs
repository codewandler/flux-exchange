import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import * as service from '../src/service.mts'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'src')
const source = (name) => readFileSync(path.join(root, name), 'utf8')

test('the console has no secret-bearing credential or service-account JSON producer', () => {
  assert.equal(
    'rotateCredential' in service,
    false,
    'the browser still exports the credential rotation JSON producer',
  )
  assert.equal(
    'mintServiceAccount' in service,
    false,
    'the browser still exports the service-account token JSON consumer',
  )

  const client = source('service.mts')
  assert.doesNotMatch(client, /JSON\.stringify\(\{\s*value\s*\}/)
  assert.doesNotMatch(client, /body\.token/)

  const connections = source('Connections.mts')
  assert.doesNotMatch(connections, /type:\s*'password'/)
  assert.doesNotMatch(connections, /emit\('rotate'/)

  const accounts = source('Agents.mts')
  assert.doesNotMatch(accounts, /data-agents':\s*'token'/)
  assert.doesNotMatch(accounts, /mintServiceAccount/)
  assert.match(accounts, /owner-local/)

  const app = source('App.vue')
  assert.doesNotMatch(app, /@rotate=/)
  assert.doesNotMatch(app, /rotateCredential/)
})
