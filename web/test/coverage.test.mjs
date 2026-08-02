// The suite scans every page this site publishes, and this is what says so.
//
// **The failure this exists to prevent, which already happened once.** Every content rule in
// `site.test.mjs` — no deployment fact, nothing credential-shaped, the deployed base path, and both
// family-link guards — is a loop over the built pages. Which pages those are was a single
// non-recursive `readdirSync(dist)`, correct from X-63 until X-64 published the first page below
// the root. `capabilities/invoke.html` and `capabilities/subscribe.html` were then scanned by
// nothing at all, and the gate stayed green, because a scanner given fewer files does not fail —
// it passes sooner.
//
// A guard that is a loop is only as good as what it loops over, and nothing was checking that. The
// two tests here are that check, and they are deliberately about the *enumeration* rather than
// about any page's content:
//
//   the first compares what the suite scans against what VitePress actually publishes, so a page
//   that renders and is not scanned is a red suite. This is what makes the fix outlast the bug:
//   X-65 is chartered to fill `capabilities/`, and a page it adds tomorrow — at any depth — is
//   covered without anybody remembering that coverage is a thing to check; and
//
//   the second runs the walker against a tree built to defeat the old one. That is the convention
//   this repository applies to every scanner it owns (`console/test/components.test.mjs`,
//   `scripts/check-action-pins.sh --self-test`, `codeBlocksOf` in `site.test.mjs`): a scanner which
//   has not just proved it catches a violation is not evidence there are none.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { pages, publishedSources, walk } from './rendered.mjs'

test('every page this site publishes is one the suite scans', () => {
  // The load-bearing test of this file. Sources on one side, the enumeration on the other; a page
  // in the first and not the second is a page the content rules never read.
  const scanned = new Set(pages().map(({ name }) => name))
  const published = publishedSources()

  assert.ok(published.length > 0, 'no markdown sources were found, so this proves nothing')

  for (const page of published) {
    assert.ok(
      scanned.has(page),
      `${page} is published by this site and scanned by nothing.\n` +
        'Every content rule in `site.test.mjs` — no deployment fact, nothing credential-shaped, the ' +
        'base path, the family links — is a loop over the scanned pages, so a page missing from that ' +
        'list is a page with no rules applied to it at all, on a public site, with a green gate.\n' +
        `scanned: ${[...scanned].sort().join(', ')}`
    )
  }
})

test('the page walker finds a page nested below the root', () => {
  // The self-test. The fixture is the shape that broke it: one page at the root, one a directory
  // down — which is `capabilities/` — and one two directories down, which is nothing today and is
  // exactly what X-65 might add without thinking of this file.
  const dir = mkdtempSync(path.join(tmpdir(), 'flux-exchange-walk-'))
  try {
    mkdirSync(path.join(dir, 'capabilities', 'grouped'), { recursive: true })
    writeFileSync(path.join(dir, 'index.html'), '<html></html>')
    writeFileSync(path.join(dir, 'capabilities', 'invoke.html'), '<html></html>')
    writeFileSync(path.join(dir, 'capabilities', 'grouped', 'deep.html'), '<html></html>')
    // Not a page, and must not be returned as one.
    writeFileSync(path.join(dir, 'capabilities', 'notes.txt'), 'not a page')

    assert.deepEqual(
      walk(dir, '.html').sort(),
      ['capabilities/grouped/deep.html', 'capabilities/invoke.html', 'index.html'],
      'the walker does not see pages below the root; the content rules would skip them in silence'
    )

    // And the reading that actually matters: `pages()` returns them with their directory in the
    // name, so a failure message names the file somebody has to open.
    assert.deepEqual(
      pages(dir)
        .map(({ name }) => name)
        .sort(),
      ['capabilities/grouped/deep.html', 'capabilities/invoke.html', 'index.html']
    )
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
