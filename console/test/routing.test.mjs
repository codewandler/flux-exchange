// How the catalogue's paths become hrefs this document can follow — and, above everything else
// here, what happens when a path already carries an in-page anchor.
//
// **The property this file exists for.** The carried components produce `/explorer#airtable`: a page
// plus an anchor, and `ProviderCard` really does render `id="airtable"` for it to land on. That is a
// perfectly ordinary URL wherever the components are mounted on a *path* router. This console mounts
// them on a *fragment* router, and a URL has exactly one fragment — so the naive
// `` `${BASE}#${path}` `` produces `#/explorer#airtable`, whose fragment is the whole string
// `/explorer#airtable`, which matches no route. The reader gets "unknown path" for a link the
// component was right to emit.
//
// So the anchor has to survive the trip: encoded on the way out, split off on the way back in, and
// carried on the route so the app can scroll to it. This is the host's job and not the components' —
// `PathResolver` is precisely the seam where a host says how its own URLs are spelled, and a
// component that had to know it was on a fragment router would be a component that cannot be
// mounted anywhere else.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { fragmentPath, parseRoute } from '../src/routing.ts'

test('a path with an anchor resolves to a URL with exactly one fragment', () => {
  const href = fragmentPath('/explorer#airtable')

  assert.equal(
    href.split('#').length - 1,
    1,
    `a URL has one fragment; a second "#" makes the first one swallow the rest: ${href}`,
  )
})

test('a path with an anchor round-trips back to the explorer, not to unknown', () => {
  const href = fragmentPath('/explorer#airtable')
  const hash = href.slice(href.indexOf('#'))

  assert.deepEqual(parseRoute(hash), { name: 'explorer', anchor: 'airtable' })
})

test('an operation path with an anchor keeps both halves', () => {
  const href = fragmentPath('/operations/airtable-record-create#request')
  const hash = href.slice(href.indexOf('#'))

  assert.deepEqual(parseRoute(hash), {
    name: 'operation',
    id: 'airtable-record-create',
    anchor: 'request',
  })
})

test('a path with no anchor is unchanged in meaning', () => {
  assert.deepEqual(parseRoute(fragmentPath('/explorer').slice(1)), { name: 'explorer' })
  assert.deepEqual(parseRoute('#/operations/zendesk-test'), {
    name: 'operation',
    id: 'zendesk-test',
  })
})

// An unrecognised path still says so rather than quietly showing the explorer — the reason
// `parseRoute` has an `unknown` arm at all. An anchor must not smuggle a bad path past that.
test('an anchor does not turn an unknown path into a known one', () => {
  const hash = fragmentPath('/nowhere#airtable').slice(1)

  assert.equal(parseRoute(hash).name, 'unknown')
})
