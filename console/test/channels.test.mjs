import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import * as service from '../src/service.mts'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = (file) => readFileSync(path.join(root, 'src', file), 'utf8')
const answer = (status, body) => ({ ok: status >= 200 && status < 300, status, json: async () => body })

test('channel writes select declarations and have nowhere to send tenant or connection authority', async () => {
  const asked = []
  const held = { id: 'ch_1', connector: 'slack', connection: 'slack', binding: 'socket', events: ['app_mention'], status: 'starting' }
  const fetch = async (url, init = {}) => {
    asked.push({ url, method: init.method, body: init.body ? JSON.parse(init.body) : undefined })
    return answer(201, held)
  }

  await service.createChannel('slack', 'socket', ['app_mention'], { fetch })

  assert.deepEqual(asked, [{
    url: '/api/channels', method: 'POST',
    body: { connector: 'slack', binding: 'socket', events: ['app_mention'] },
  }])
  assert.equal(JSON.stringify(asked).includes('tenant'), false)
  assert.equal(JSON.stringify(asked).includes('connection'), false)
})

test('channel views receive completed states and never fetch', () => {
  const view = source('Channels.vue')
  assert.doesNotMatch(view, /\bfetch\s*\(/)
  assert.doesNotMatch(view, /import(?!\s+type)[^\n]*service\.mts/)
  assert.match(view, /Channel status/)
  assert.match(view, /Select declared events/)
  assert.match(view, /Delete channel/)
})

test('channel styles name no literal colour and every custom property exists', () => {
  const styles = source('channels.css')
  const tokens = source('tokens.css')
  assert.doesNotMatch(styles, /#[0-9a-f]{3,8}\b|rgba?\(/i)
  const used = new Set([...styles.matchAll(/var\((--[a-z0-9-]+)/gi)].map((match) => match[1]))
  for (const token of used) assert.match(tokens, new RegExp(`${token.replaceAll('-', '\\-')}\\s*:`))
})
