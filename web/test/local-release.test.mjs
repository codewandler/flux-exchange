import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'

import { pages, repoRoot, webRoot } from './rendered.mjs'

const RELEASE_PAGE = 'local-releases.md'
const RELEASE_LINK = '/flux-exchange/local-releases'
const TARGETS = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'aarch64-unknown-linux-gnu',
  'x86_64-unknown-linux-gnu',
  'x86_64-pc-windows-msvc',
]
const PROTOCOLS = [
  'exchange.api.v1',
  'exchange.effective-catalogue-response.v1',
  'exchange.invoke-request.v1',
  'exchange.invoke-response.v1',
  'exchange.connection-plan.v1',
  'exchange.supervisor-ready.v1',
]

function releaseSource() {
  return readFileSync(path.join(webRoot, RELEASE_PAGE), 'utf8').replace(/\s+/g, ' ')
}

test('the public release page states the complete portable release contract', () => {
  const source = releaseSource()

  for (const target of TARGETS) {
    assert.ok(source.includes(target), `${RELEASE_PAGE} does not name supported target ${target}`)
  }
  for (const protocol of PROTOCOLS) {
    assert.ok(source.includes(protocol), `${RELEASE_PAGE} does not name protocol ${protocol}`)
  }

  for (const required of [
    'flux-exchange compatibility --json',
    'https://github.com/codewandler/flux-exchange',
    'flux-exchange-release-trust.json',
    'flux-exchange-release-channel.json',
    'offline root',
    'delegated',
    'rollback',
    'expires',
    'offline import',
    'crates.io',
    'Flux release artifact',
    'official integration plugin',
    'connector runtime',
    'exchange-stable-v1-generation-',
    'exchange-trust-v1-version-',
    'v0.18.0',
    'X-134',
    'implementation evidence',
    'credential-handoff',
  ]) {
    assert.ok(source.includes(required), `${RELEASE_PAGE} does not explain ${required}`)
  }
})

test('the public release page is reachable from every themed page', () => {
  const built = pages()
  assert.ok(
    built.some(({ name }) => name === 'local-releases.html'),
    'the public site does not publish local-releases.html'
  )

  for (const { name, html } of built.filter(({ name }) => name !== '404.html')) {
    assert.ok(
      html.includes(`href="${RELEASE_LINK}"`),
      `${name} does not link to ${RELEASE_LINK}; release verification is hidden from operators`
    )
  }
})

test('the operator runbook names delegated secret inputs without claiming they exist', () => {
  const runbook = readFileSync(path.join(repoRoot, 'docs', 'local-binary-releases.md'), 'utf8').replace(
    /\s+/g,
    ' '
  )

  for (const required of [
    'FLUX_EXCHANGE_CHANNEL_SIGNING_KEY_B64',
    'FLUX_EXCHANGE_RELEASE_SIGNING_KEY_B64',
    'canonical RFC 4648 base64',
    'complete minisign secret-key file bytes',
    '.github/release-root-policy.json',
    'intentionally absent',
    'X-126 creates or configures no production signing secret',
    'exchange-stable-v1-generation-',
    'exchange-trust-v1-version-',
    'v0.18.0',
    '`main` is not protected',
    'no `local-release` environment',
    'X-134',
    'implementation evidence',
    'credential-handoff',
  ]) {
    assert.ok(runbook.includes(required), `operator runbook does not state ${required}`)
  }
})
