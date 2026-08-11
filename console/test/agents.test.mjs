// The browser manages Service Account metadata, while creation stays on the owner-local FXSA path.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { nextTick } from 'vue'

import { mount, rendered, text, nodes, one } from './mount.mjs'
import Agents from '../src/Agents.mts'
import { parseRoute } from '../src/routing.ts'
import { authorisation, mayMint, tokenStanding } from '../src/minting.mts'
import { SURFACES } from '../src/surfaces.mts'

const signedIn = (kind = 'user') => ({
  status: 'ready',
  principal: { kind, id: kind === 'user' ? 'alice' : 'runner', tenant: 'acme' },
})

async function settle() {
  await new Promise((resolve) => setImmediate(resolve))
  await nextTick()
}

function serving(accounts = []) {
  const asked = []
  const previous = globalThis.fetch
  globalThis.fetch = async (url, init) => {
    const method = init?.method ?? 'GET'
    asked.push({ url, method, body: init?.body })
    if (method === 'GET' && url === '/api/service-accounts') {
      return new Response(JSON.stringify({ service_accounts: accounts }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }
    if (method === 'DELETE') return new Response(null, { status: 204 })
    assert.fail(`the console attempted the secret-bearing ${method} ${url}`)
  }
  return { asked, restore: () => (globalThis.fetch = previous) }
}

test('creation_is_owner_local_and_the_browser_never_posts_for_a_token', async () => {
  const transport = serving()
  const view = mount(Agents, { session: signedIn() })
  try {
    await settle()
    const body = rendered(view.root)
    assert.match(body, /owner-local helper/)
    assert.match(body, /flux-exchange local service-account-mint/)
    assert.match(body, /never asks for it as HTTP JSON/)
    assert.equal(one(view.root, 'data-agents', 'token'), null)
    assert.equal(one(view.root, 'data-agents', 'mint-form'), null)
    assert.deepEqual(transport.asked, [
      { url: '/api/service-accounts', method: 'GET', body: undefined },
    ])
  } finally {
    view.unmount()
    transport.restore()
  }
})

test('a_human_lists_and_revokes_value_free_service_account_metadata', async () => {
  const transport = serving([
    { id: 'nightly/report', expires_at: 1793491200 },
    { id: 'deploy', expires_at: 1796083200 },
  ])
  const view = mount(Agents, { session: signedIn() })
  try {
    await settle()
    const before = rendered(view.root)
    assert.match(before, /nightly\/report/)
    assert.match(before, /deploy/)

    const revoke = nodes(view.root).find((node) =>
      node.props['data-agents'] === 'revoke' && node.props['data-account'] === 'nightly/report'
    )
    assert.ok(revoke)
    await view.fire(revoke, 'onClick')
    await settle()

    assert.deepEqual(transport.asked.at(-1), {
      url: '/api/service-accounts/nightly%2Freport', method: 'DELETE', body: undefined,
    })
    assert.doesNotMatch(rendered(view.root), /nightly\/report — stops resolving/)
    assert.match(rendered(view.root), /no longer authenticates/)
  } finally {
    view.unmount()
    transport.restore()
  }
})

test('only_a_user_gets_owner_local_creation_guidance_or_metadata_reads', async () => {
  for (const [session, expected] of [
    [{ status: 'loading' }, 'Reading your session'],
    [{ status: 'ready', principal: null }, 'Sign in to manage'],
    [signedIn('service_account'), 'Only a signed-in person'],
  ]) {
    const transport = serving()
    const view = mount(Agents, { session })
    try {
      await settle()
      assert.match(text(view.root), new RegExp(expected))
      assert.doesNotMatch(text(view.root), /service-account-mint/)
      assert.deepEqual(transport.asked, [])
    } finally {
      view.unmount()
      transport.restore()
    }
  }
})

test('service_account_policy_claims_remain_derived_from_the_surface_inventory', () => {
  assert.equal(mayMint(signedIn().principal), true)
  assert.equal(mayMint(signedIn('service_account').principal), false)
  const standing = tokenStanding(SURFACES)
  assert.ok(standing.length > 0)
  assert.ok(standing.every((entry) => typeof entry.can === 'boolean' && entry.step.id))
  assert.match(authorisation(SURFACES, true), /grants no authority by itself/)
})

test('the_service_account_screen_keeps_its_canonical_value_free_route', () => {
  assert.deepEqual(parseRoute('#/service-accounts'), { name: 'service-accounts' })
  assert.deepEqual(parseRoute('#/agents'), { name: 'unknown', path: '/agents' })
})
