// The tripwire for test discovery.
//
// This file has no subject of its own. It exists at a *depth* — one directory below `test/` — and
// the only thing it asserts is that it ran at all. Its value is entirely in where it sits.
//
// Before X-32 the suite was `node --test test/*.test.mjs`, and that glob matches exactly one
// directory level: this file would have been skipped, `npm test` would have reported 18 passing
// tests, and CI would have been green. That is the failure this repository keeps finding and
// removing — a check that looks like it covers something and does not — except here the invisible
// thing is the check itself.
//
// So if someone narrows the discovery pattern again, this file stops running and the count drops.
// A dropped count is quiet, which is why `../discovery.test.mjs` sits at the top level and fails
// loudly for the same regression. The two are a pair: this one is the canary, that one is the alarm.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

test('a_test_in_a_subdirectory_of_test_is_discovered', () => {
  // Asserted rather than left as an empty body, so the test cannot be mistaken for a stub and
  // deleted: it is a statement about this file's own location, and it is the whole point of it.
  const here = path.dirname(fileURLToPath(import.meta.url))
  const testRoot = path.resolve(here, '..')

  assert.equal(
    path.basename(testRoot),
    'test',
    'this file must sit one level below `console/test/` or it is not testing discovery at all'
  )
  assert.notEqual(
    path.resolve(here),
    testRoot,
    'this file has been moved up into `console/test/` itself, where a single-level glob would find it — it no longer proves that nested tests run'
  )
})
