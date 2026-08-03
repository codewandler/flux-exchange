// The complete public surface and the vocabulary contracts X-65 adds.
//
// This suite reads the rendered site through `pages()` — the same total enumerator as every other
// public-content rule. A page hidden below another directory is still public and still scanned.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { pages } from './rendered.mjs'

const DEPLOYED_BASE = '/flux-exchange/'

/** Human-readable text, without bundled scripts and styles. */
function textOf(html) {
  return html
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, ' ')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim()
}

/** The authored page body, excluding shared navigation whose links name every capability. */
function proseOf(html) {
  const main = /<main\b[^>]*>([\s\S]*?)<\/main>/.exec(html)
  assert.ok(main, 'the rendered page has no authored <main> body')
  return textOf(main[1])
}

function capability(id) {
  const name = `capabilities/${id}.html`
  const found = pages().find((page) => page.name === name)
  assert.ok(found, `the intended surface has no standalone ${name} page (X-65)`)
  return found
}

test('the intended surface has one derived-status page per public capability', () => {
  for (const id of ['connections', 'invoke', 'subscribe', 'leases', 'grants', 'agents', 'workflows']) {
    const { name, html } = capability(id)
    assert.match(
      html,
      new RegExp(`data-capability="${id}"[^>]*data-live="(?:true|false)"`),
      `${name} does not carry the descriptor-derived status for its own capability`
    )
  }
})

test('the three lifetimes are stated once and every capability use points to that table', () => {
  const lifetimeTables = pages().filter(({ html }) =>
    /<table\b[^>]*>[\s\S]*?<strong>Session<\/strong>[\s\S]*?<strong>Channel<\/strong>[\s\S]*?<strong>Lease<\/strong>[\s\S]*?<\/table>/.test(
      html
    )
  )
  assert.deepEqual(
    lifetimeTables.map(({ name }) => name),
    ['surface.html'],
    'Session, Channel and Lease must be stated together exactly once, in the canonical table'
  )

  for (const { name, html } of pages().filter(({ name }) => name.startsWith('capabilities/'))) {
    if (!/\b(?:Session|Channel|Lease)s?\b/.test(proseOf(html))) continue
    assert.match(
      html,
      new RegExp(`href="${DEPLOYED_BASE}surface#the-three-lifetimes"`),
      `${name} names a lifetime without linking to the one canonical table`
    )
  }
})

test('planned capability pages name the story that would build them', () => {
  assert.match(textOf(capability('agents').html), /\bX-108\b/)
  assert.match(textOf(capability('leases').html), /\bX-118\b/)
})

/** An affirmative claim that Exchange, this host or this service runs a trigger or schedule. */
function claimsExchangeRunsTriggerOrSchedule(text) {
  return (text.match(/[^.!?]+[.!?]?/g) ?? []).some(
    (sentence) =>
      !/\b(?:does|do|will|can)\s+not\b/i.test(sentence) &&
      /\b(?:flux-exchange|this (?:host|service))\b.{0,80}\b(?:runs?|executes?|schedules?)\b.{0,50}\b(?:triggers?|schedules?)\b/i.test(
        sentence
      )
  )
}

test('no page describes a trigger or schedule as something this service runs', () => {
  assert.equal(
    claimsExchangeRunsTriggerOrSchedule('flux-exchange runs scheduled triggers for a tenant'),
    true,
    'the trigger-ownership scanner cannot catch the failure it claims to prevent'
  )

  const workflows = capability('workflows')
  assert.match(
    textOf(workflows.html),
    /flux-exchange does not run triggers or schedules/i,
    `${workflows.name} does not say where the trigger and schedule boundary is`
  )

  for (const { name, html } of pages()) {
    assert.equal(
      claimsExchangeRunsTriggerOrSchedule(textOf(html)),
      false,
      `${name} describes a trigger or schedule as something flux-exchange runs`
    )
  }
})
