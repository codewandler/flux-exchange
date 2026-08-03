import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import * as service from '../src/service.mts'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = (file) => readFileSync(path.join(root, 'src', file), 'utf8')
const answer = (status, body) => ({ ok: status >= 200 && status < 300, status, json: async () => body })

test('channel writes select and rebind by operator label only', async () => {
  const asked = []
  const held = { id: 'ch_1', connector: 'slack', connection: 'production', binding: 'socket', events: ['app_mention'], status: 'starting' }
  const fetch = async (url, init = {}) => {
    asked.push({ url, method: init.method, body: init.body ? JSON.parse(init.body) : undefined })
    return answer(201, held)
  }

  await service.createChannel('slack', 'production', 'socket', ['app_mention'], { fetch })
  await service.updateChannel(held, 'sandbox', ['message'], { fetch })

  assert.deepEqual(asked, [
    {
      url: '/api/channels', method: 'POST',
      body: { connector: 'slack', connection: 'production', binding: 'socket', events: ['app_mention'] },
    },
    {
      url: '/api/channels/ch_1', method: 'PUT',
      body: { connection: 'sandbox', events: ['message'] },
    },
  ])
  assert.equal(JSON.stringify(asked).includes('tenant'), false)
  assert.equal(JSON.stringify(asked).includes('instance'), false)
  assert.equal(JSON.stringify(asked).includes('authority'), false)
  assert.equal(JSON.stringify(asked).includes('credential'), false)
})

test('channel connection choices discard instance and credential metadata', async () => {
  const state = await service.loadConnections({
    fetch: async () => answer(200, {
      connections: [{
        connector: 'slack',
        label: 'production',
        instance: '7a6b796b-f6fd-4896-9122-3a1f546dd072',
        authority: 'com.slack.api',
        credentials: [{
          name: 'slack.bot_token',
          address: 'tenants/acme/com.slack.api/@instances/7a6b796b-f6fd-4896-9122-3a1f546dd072/bot_token',
          held: true,
        }],
      }],
    }),
  })

  assert.deepEqual(service.channelConnectionLabels(state), [
    { connector: 'slack', label: 'production' },
  ])
})

test('channel views receive completed states and never fetch', () => {
  const view = source('Channels.vue')
  assert.doesNotMatch(view, /\bfetch\s*\(/)
  assert.doesNotMatch(view, /import(?!\s+type)[^\n]*service\.mts/)
  assert.match(view, /Channel status/)
  assert.match(view, /Connection label/)
  assert.match(view, /Renaming a connection/)
  assert.match(view, /cannot be deleted/)
  assert.match(view, /Select declared events/)
  assert.match(view, /Delete channel/)
  assert.doesNotMatch(view, /\b(?:UUID|instance|authority|credential(?:s)?)\b/i)

  const app = source('App.vue')
  assert.match(app, /:connections="channelConnections"/)
  assert.match(app, /channelConnectionLabels\(connections\.value\)/)
})

test('channel styles name no literal colour and every custom property exists', () => {
  const styles = source('channels.css')
  const tokens = source('tokens.css')
  assert.doesNotMatch(styles, /#[0-9a-f]{3,8}\b|rgba?\(/i)
  const used = new Set([...styles.matchAll(/var\((--[a-z0-9-]+)/gi)].map((match) => match[1]))
  for (const token of used) assert.match(tokens, new RegExp(`${token.replaceAll('-', '\\-')}\\s*:`))
})
