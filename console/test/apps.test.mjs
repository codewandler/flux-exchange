// X-108: immutable App installation and Managed Agent chat remain tenant-derived.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import * as service from '../src/service.mts'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = (file) => readFileSync(path.join(root, 'src', file), 'utf8')
const answer = (status, body) => ({ ok: status >= 200 && status < 300, status, json: async () => body })

test('installation freezes operator choices and has nowhere to send a tenant', async () => {
  const asked = []
  const fetch = async (url, init = {}) => {
    asked.push({ url, method: init.method, body: JSON.parse(init.body) })
    return answer(url === '/api/model-profiles' ? 201 : 201, url === '/api/model-profiles'
      ? { id: 'demo', provider: 'static', model: 'static', revision: 1 }
      : { id: 'assistant', package: 'exchange-apps/slack-bot', version: '1.0.0', activation: 'active' })
  }

  await service.installApp({
    id: 'assistant', package: 'exchange-apps/slack-bot', version: '1.0.0', connection: 'team',
    access_layers: ['reply'], risk_ceiling: 'high', scopes: ['chat:reply'],
    profile: 'demo', static_reply: 'ready',
  }, { fetch })

  assert.equal(asked.length, 2)
  assert.deepEqual(asked[1], {
    url: '/api/apps', method: 'POST', body: {
      id: 'assistant', package: 'exchange-apps/slack-bot', version: '1.0.0',
      connections: { slack: 'team' }, model_profile: 'demo', access_layers: ['reply'],
      datasources: {}, risk_ceiling: 'high', scopes: ['chat:reply'], review: null,
    },
  })
  assert.equal(JSON.stringify(asked).includes('tenant'), false)
})

test('chat preserves the Flux conversation key and sends no authority fields', async () => {
  let request
  const outcome = await service.sendAppMessage('assistant', 'hello', 'thread-1', {
    fetch: async (url, init) => {
      request = { url, body: JSON.parse(init.body) }
      return answer(200, { reply: 'hi', session: 'thread-1', activation: 'active' })
    },
  })

  assert.deepEqual(request, { url: '/api/apps/assistant/chat', body: { message: 'hello', session: 'thread-1' } })
  assert.deepEqual(outcome, { status: 'answered', reply: 'hi', session: 'thread-1', activation: 'active' })
  assert.equal(JSON.stringify(request).includes('credential'), false)
})

test('the Apps view receives data and never fetches or names a credential value', () => {
  const view = source('Apps.vue')
  assert.doesNotMatch(view, /\bfetch\s*\(/)
  assert.doesNotMatch(view, /import(?!\s+type)[^\n]*service\.mts/)
  assert.match(view, /Slack Connection/)
  assert.match(view, /Optional access layers/)
  assert.match(view, /Risk ceiling/)
  assert.match(view, /Activation activity/)
  assert.doesNotMatch(view, /token|secret|password/i)
})

test('App styles name no literal colour and every custom property exists', () => {
  const styles = source('apps.css')
  const tokens = source('tokens.css')
  assert.doesNotMatch(styles, /#[0-9a-f]{3,8}\b|rgba?\(/i)
  const used = new Set([...styles.matchAll(/var\((--[a-z0-9-]+)/gi)].map((match) => match[1]))
  for (const token of used) assert.match(tokens, new RegExp(`${token.replaceAll('-', '\\-')}\\s*:`))
})
