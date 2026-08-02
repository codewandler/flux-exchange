// What this site published, enumerated once, for every suite that scans it.
//
// **Why this is a module rather than a function in each test file.** It was a function in each test
// file, and that is precisely how X-64 published an IP address, a `host:port` endpoint and a bearer
// token to a live public site with a fully green gate.
//
// `site.test.mjs` read the built pages with a single non-recursive `readdirSync(dist)`. That was
// total coverage for as long as every page sat at the root of `dist`, which was true from X-63
// until X-64 added `capabilities/`. The moment a page was published one directory down, it fell out
// of *every* content rule at once — no deployment fact, nothing credential-shaped, the base-path
// check, and both of X-77's family-link guards — and nothing anywhere said so, because a scanner
// that scans fewer files does not fail. It reports success faster.
//
// So there is now one enumerator, and the rule it has to satisfy is not "recurse" — recursion was
// only the first round's shape. Round two found the second: the enumerator still carried a list of
// directories to skip, shared with the *source* walk, so `dist/test/` and `dist/scripts/` went
// unread and a bearer token published with the gate green all over again. Skipping on the way in is
// a content decision; skipping on the way out is a blind spot, and the two must not share a list.
// [`pages`] now excludes nothing at all.
//
// The rule this file has to satisfy is: **the suite must not be able to silently stop covering
// pages.** Three things enforce it, all in `coverage.test.mjs`:
//
//   `pages()` is measured against the markdown sources VitePress actually publishes, so a page that
//   renders and is not scanned fails the suite rather than passing quietly. A directory added by
//   X-65 is covered the day it is added, by nobody remembering anything; and
//
//   the walker is run against a fixture tree with a page nested two deep, in the convention this
//   repository uses everywhere — `console/test/components.test.mjs`, `check-action-pins.sh`,
//   `codeBlocksOf` — that a scanner which has not just proved it catches a violation is not
//   evidence there are none; and
//
//   that same fixture names its directories `test`, `scripts`, `node_modules`, `public` and
//   `.vitepress`, so the output walk is proved to descend into the exact five names the source walk
//   skips. That test fails on the code this file replaced, which is the only reason to trust it.

import assert from 'node:assert/strict'
import { readdirSync, readFileSync, existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { NOT_CONTENT } from '../.vitepress/content.mts'

export const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const repoRoot = path.resolve(webRoot, '..')
export const dist = path.join(webRoot, '.vitepress', 'dist')

/**
 * Every file under `root` matching `extension`, recursively, as slash-separated relative paths.
 *
 * **`skip` is a parameter, and it defaults to skipping nothing.** That default is the correction
 * round two of review forced, and it is worth stating plainly because the bug was subtle and the
 * consequence was not. The exclusion used to be a constant closed over by this function, so it
 * applied to *both* callers — and while skipping `test/` and `scripts/` when predicting which pages
 * should exist is correct, skipping `dist/test/` and `dist/scripts/` when reading which pages *do*
 * exist is a hole. Both halves went blind in the same five places at once, which is exactly why
 * `coverage.test.mjs` could not see it: the predicted set and the scanned set agreed, because
 * neither contained the page. A bearer token published to the live site with the gate green.
 *
 * The rule that falls out, and it is the one to keep: **a claim about what should be published is
 * never a licence to skip reading something that was.** Excluding on the way in is a content
 * decision; excluding on the way out is a blind spot.
 */
function walk(root, extension, { skip = [] } = {}, prefix = '') {
  const excluded = new Set(skip)
  const found = []
  for (const entry of readdirSync(path.join(root, prefix), { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isDirectory()) {
      if (prefix === '' && excluded.has(entry.name)) continue
      found.push(...walk(root, extension, { skip }, relative))
    } else if (entry.name.endsWith(extension)) {
      found.push(relative)
    }
  }
  return found
}

/** The walker, exported so `coverage.test.mjs` can run it against a tree it built on purpose. */
export { walk }

/**
 * Every built page, as `{ name, html }` — `name` relative to `dist`, so a nested page is
 * `capabilities/invoke.html` rather than `invoke.html`.
 *
 * **Total, with no exclusions of any kind.** Every rule in `site.test.mjs` is a loop over this, so
 * anything omitted here is published with no rules applied to it at all. There is no directory whose
 * contents this may skip: if the build put an `.html` file in the output, a reader can reach it, and
 * a reader reaching it is the entire premise of the content rules.
 *
 * `root` is a parameter because `status.test.mjs` renders hypothetical builds into temporary
 * directories and has to scan those the same way. It defaults to the real output, which is what
 * every caller in `site.test.mjs` wants.
 */
export function pages(root = dist) {
  assert.ok(
    existsSync(root),
    `${root} does not exist — run \`npm run build\` before \`npm test\`; these assertions read the rendered site`
  )
  const names = walk(root, '.html')
  assert.ok(names.length > 0, 'the build produced no HTML pages')
  return names.map((name) => ({ name, html: readFileSync(path.join(root, name), 'utf-8') }))
}

/**
 * Every page VitePress publishes, named as it lands in `dist` — derived from the markdown sources
 * rather than from the output.
 *
 * This is the half that makes coverage checkable. Reading `dist` tells you what the walker found;
 * reading the sources tells you what it *should* have found, and the two disagreeing is the failure
 * that went unnoticed. `404.html` has no source and is not listed: it is generated by the theme.
 *
 * The exclusions come from `.vitepress/content.mts`, which is also what the config's `srcExclude`
 * is built from — one statement of what is content, read by the publisher and by this predictor, so
 * they cannot drift into agreeing wrongly.
 */
export function publishedSources() {
  return walk(webRoot, '.md', { skip: NOT_CONTENT })
    .filter((source) => source !== 'README.md')
    .map((source) => source.replace(/\.md$/, '.html'))
}
