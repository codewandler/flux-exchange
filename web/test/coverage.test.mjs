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
import { mkdirSync, mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { pages, publishedSources, walk } from './rendered.mjs'
import { NOT_CONTENT, SRC_EXCLUDE } from '../.vitepress/content.mts'

const webRoot = path.resolve(import.meta.dirname, '..')

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

test('this site publishes nothing out of its own machinery', () => {
  // The second defence, asserted against the real build rather than against the config that is
  // supposed to produce it. `srcExclude` is a glob list and a glob that stops matching fails silently
  // — the page simply appears. Until round two of X-64's review, a markdown file in `web/test/` or
  // `web/scripts/` rendered to a public page, and the content rules were not reading that far either.
  //
  // Note what this does *not* rely on: if this ever regresses, the page still gets scanned, because
  // `pages()` excludes nothing. Two independent defences, and this is the one that keeps contributor
  // files off the public site at all.
  for (const { name } of pages()) {
    const directory = name.split('/')[0]
    assert.ok(
      !NOT_CONTENT.includes(directory),
      `the site published ${name}. \`${directory}/\` is this site's own machinery, not content — ` +
        '`srcExclude` in `.vitepress/config.mts` is no longer keeping it off the public site.'
    )
  }
})

test('the built site is read with no directory excluded, including the ones sources skip', () => {
  // **Round two of X-64's review, as a test.** The walker took a list of directories to skip and it
  // was shared: correct for the source walk, which is predicting what *should* publish, and a hole
  // in the output walk, which is reading what *did*. `dist/test/` and `dist/scripts/` went unread,
  // and `coverage.test.mjs` could not see it because the predicted set and the scanned set agreed —
  // both omitted the page. An IP address, a `host:port` endpoint and a bearer token published to
  // the live site with all 25 tests green.
  //
  // So this asserts the asymmetry directly, on the five names that were skipped. It fails against
  // the implementation it replaced, which is the only thing that makes it evidence.
  const dir = mkdtempSync(path.join(tmpdir(), 'flux-exchange-total-'))
  try {
    const expected = []
    for (const directory of NOT_CONTENT) {
      mkdirSync(path.join(dir, directory), { recursive: true })
      writeFileSync(path.join(dir, directory, 'leak.html'), '<html></html>')
      expected.push(`${directory}/leak.html`)
    }
    writeFileSync(path.join(dir, 'index.html'), '<html></html>')
    expected.push('index.html')

    assert.deepEqual(
      pages(dir)
        .map(({ name }) => name)
        .sort(),
      expected.sort(),
      'the output walk skips a directory. Every content rule is a loop over `pages()`, so a page it ' +
        'does not return is a page published with no rule applied to it — which is how a bearer ' +
        'token reached the public site with this suite green. Excluding on the way in is a content ' +
        'decision; excluding on the way out is a blind spot.'
    )

    // The other half of the asymmetry: the *source* walk does skip them, and must, or predicting
    // which pages exist would mean walking `node_modules`.
    assert.deepEqual(
      walk(dir, '.html', { skip: NOT_CONTENT }),
      ['index.html'],
      'the source walk no longer honours its exclusions'
    )
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('what the site refuses to publish is the same list the suite predicts from', () => {
  // The drift this closes is not two lists disagreeing — it is two lists agreeing wrongly, which is
  // the shape that survived a full round of review. `content.mts` states it once; the config builds
  // `srcExclude` from it and `rendered.mjs` predicts from it. This asserts the config actually reads
  // the constant rather than having quietly grown a literal beside it.
  const config = readFileSync(path.join(webRoot, '.vitepress', 'config.mts'), 'utf-8')

  assert.match(
    config,
    /srcExclude:\s*SRC_EXCLUDE\b/,
    'the config no longer builds `srcExclude` from `content.mts` — what the site publishes and what ' +
      'the suite predicts are now two lists, and they will fail by agreeing rather than by differing'
  )
  for (const directory of NOT_CONTENT) {
    assert.ok(
      SRC_EXCLUDE.includes(`${directory}/**`),
      `\`${directory}\` is treated as non-content by the suite and is not excluded from publishing`
    )
  }
  assert.ok(SRC_EXCLUDE.includes('README.md'), 'the contributor readme is no longer excluded')
})
