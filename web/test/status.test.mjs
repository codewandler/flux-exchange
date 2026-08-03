// The status badge is derived from the descriptor, and this file is what makes that a fact (X-64).
//
// **Why this file is separate from `site.test.mjs`.** That one asks what a page may *publish* — no
// deployment fact, nothing credential-shaped — and answers it by scanning text. This one asks where
// a page's "is this built" *came from*, which is not a property of the text at all: a hand-written
// badge and a derived one render identical HTML. So the assertions here are about the derivation,
// and two of them can only be made by building the site again against a hypothetical descriptor.
//
// **The bar this file is held to.** This repository corrected five separate renderings of one false
// claim in a single week — that `invoke` was not built — each written honestly, each caught by a
// review rather than by a mechanism. A test that would pass against a badge somebody typed is worth
// nothing here, so the two central tests do not read the badge and check it looks right. They change
// the input and watch the output move:
//
//   `flipping a capability's served flag flips the rendered badge` builds the whole site against a
//   descriptor derived from `console/src/surfaces.mts` with one `served` flag flipped, and asserts
//   the rendered badge on an unedited page changed with it. That is the demonstration
//   `test/descriptor.test.mjs::the_descriptor_is_derived_and_not_a_coincidence` makes for the
//   document, made here for the page.
//
//   `a page for a capability the descriptor does not name fails the build` builds against a
//   descriptor with one capability deleted and asserts the build *fails*. A missing status rendering
//   as blank is the exact failure this story exists to prevent: absence must not read as "fine".
//
// **How they do it, and why not the obvious way.** Both drive VitePress in-process through its Node
// API and override the resolved `transformPageData` via `onAfterConfigResolve` — see [`buildWith`].
// The obvious way was an environment variable read by `.vitepress/descriptor.mts`, and review was
// right to reject it: that put a switch in *production* code which made a real build publish badges
// derived from arbitrary JSON while the currency guard went on passing, because the guard read the
// committed artifact and not what the badges read. A story whose claim is "a page cannot say a
// capability is live without the route table agreeing" cannot ship a documented way around itself.
// The hook used here exists only in the Node API; `npm run build` and `pages.yml` run the CLI.
//
// **Run after `npm run build`**, like `site.test.mjs`: the first assertions read `.vitepress/dist`.
// The two build-again tests render into their own temporary directories and leave `dist` alone.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { pages, walk, dist, webRoot, repoRoot } from './rendered.mjs'

/** Where the *service* keeps the document it serves — the one the site's badges derive from. */
const ARTIFACT = path.join(repoRoot, 'crates', 'exchange-server', 'src', 'routes', 'onboarding.json')

/** The directory whose pages are about a capability, and therefore must carry a derived status. */
const CAPABILITIES = path.join(webRoot, 'capabilities')

/** The committed descriptor, parsed — the answer every badge below is measured against. */
function descriptor() {
  assert.ok(existsSync(ARTIFACT), `${ARTIFACT} is missing — the site has nothing to derive a status from`)
  return JSON.parse(readFileSync(ARTIFACT, 'utf-8'))
}

/**
 * Every built capability page, as `{ id, name, html }`.
 *
 * Built on the shared enumerator in `rendered.mjs` rather than reading `capabilities/` directly.
 * This file had the same non-recursive flaw that let `site.test.mjs` publish an unscanned page — it
 * listed one directory and stopped — so a page at `capabilities/<group>/<page>.md` would have taken
 * a status from `statusFor` and been measured by neither suite. One walker, checked by
 * `coverage.test.mjs`, and both suites inherit the fix.
 *
 * Fails rather than returning nothing when the site publishes none. "There are no capability pages"
 * would otherwise make every assertion below vacuously true, which is the one way a suite about
 * derived status lies: it would stay green through the whole of X-65 adding pages that hardcode one.
 */
function capabilityPages(root = dist) {
  const found = pages(root)
    .filter(({ name }) => name.startsWith('capabilities/'))
    // The id is the path below `capabilities/`, so a grouped page is `grouped/deep` and cannot
    // collide with a root-level one of the same basename.
    .map((page) => ({ ...page, id: page.name.slice('capabilities/'.length).replace(/\.html$/, '') }))

  assert.ok(
    found.length > 0,
    'the site publishes no `capabilities/` pages — there is nothing carrying a derived status, so nothing here proves the badge is derived (X-64)'
  )
  return found
}

/**
 * The status element a page rendered, or a failure naming the page.
 *
 * Addressed by `data-capability`, the way `console/test/descriptor.test.mjs` addresses a step by
 * `data-step`: an attribute the component renders on purpose is a contract, and a class name is a
 * styling decision somebody is entitled to change.
 */
function badge(name, html) {
  const found = /<[a-z]+[^>]*\sdata-capability="([^"]*)"[^>]*>/.exec(html)
  assert.ok(
    found,
    `${name} renders no status badge — a capability page with no status is the absence this story exists to stop reading as "fine" (X-64)`
  )
  return { element: found[0], capability: found[1], at: found.index }
}

/** One attribute off an element. */
function attribute(element, name) {
  const value = new RegExp(`${name}="([^"]*)"`).exec(element)
  return value ? value[1] : null
}

// ---------------------------------------------------------------------------------------------
// The badge on every page says what the descriptor says.
// ---------------------------------------------------------------------------------------------

test('every capability page renders the status the descriptor publishes', () => {
  const document = descriptor()
  const named = new Map(document.capabilities.map((capability) => [capability.id, capability]))

  for (const { id, name, html } of capabilityPages()) {
    const rendered = badge(name, html)
    const capability = named.get(rendered.capability)
    assert.ok(
      capability,
      `${name} carries a badge for \`${rendered.capability}\`, which the descriptor does not name — the build should have refused this page`
    )
    assert.equal(
      rendered.capability,
      id,
      `${name} is about \`${rendered.capability}\` and is served at \`${id}\`; a reader following a link about one capability must not land on another's status`
    )
    assert.equal(
      attribute(rendered.element, 'data-live'),
      String(capability.live),
      `${name}: the descriptor says live=${capability.live} for \`${id}\` and the page renders the other one`
    )
  }
})

test('the status is in the page chrome, above the prose', () => {
  // The five renderings went wrong because the caveat and the claim drifted apart on the page. A
  // badge three screens under the sentence it qualifies is the same failure with extra steps, so
  // this asserts position rather than presence: before the page's own heading, and therefore before
  // any sentence a reader could take as a claim.
  for (const { name, html } of capabilityPages()) {
    const rendered = badge(name, html)
    const heading = html.search(/<h1\b/)
    assert.ok(heading >= 0, `${name} renders no <h1>`)
    assert.ok(
      rendered.at < heading,
      `${name} renders its status badge below its own heading — the caveat and the claim have already drifted apart`
    )
  }
})

// ---------------------------------------------------------------------------------------------
// The committed artifact is current, and the *site* build is what says so.
// ---------------------------------------------------------------------------------------------

test('the descriptor the site derives from is the one the console derives today', () => {
  // The seam this whole mechanism rests on. The artifact is a committed copy of a derivation, so it
  // can rot — and a site deriving its badges from a stale copy is stale in the one place it claimed
  // not to be. `console/test/descriptor.test.mjs` already fails on this drift; the site must not
  // assume that suite ran, because `web/` and `console/` are separate trees that CI can run apart.
  const derived = execFileSync(process.execPath, [path.join(webRoot, 'scripts', 'derive-descriptor.mjs')], {
    encoding: 'utf-8',
  })
  assert.equal(
    readFileSync(ARTIFACT, 'utf-8'),
    derived,
    'the committed descriptor is not what `console/src/descriptor.mts` derives today — run `node scripts/agent-descriptor.mjs` from `console/`. Until then every badge on this site is derived from a description of a build that no longer exists.'
  )
})

// ---------------------------------------------------------------------------------------------
// The two demonstrations. Both build the site again; neither edits a page.
// ---------------------------------------------------------------------------------------------

/** A scratch directory that cleans up after itself, whatever the assertion inside does. */
async function scratch(use) {
  const dir = mkdtempSync(path.join(tmpdir(), 'flux-exchange-site-'))
  try {
    return await use(dir)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/**
 * Build the whole site against `document` as its descriptor, into a temporary directory.
 *
 * Returns `{ ok, output }` rather than throwing, because one of the two callers is asserting that
 * the build *fails* and needs to read what it said while failing.
 *
 * **The hypothetical descriptor is injected here, in this process, and there is no way to inject one
 * into a real build.** That is review's correction rather than a preference. An environment variable
 * used to do this job, read by `.vitepress/descriptor.mts` — production code — which meant a real
 * build could be pointed at arbitrary JSON and publish badges derived from it while
 * `assertDescriptorIsCurrent()` went on passing, because that guard reads the committed artifact and
 * not what the badges read. The story's whole claim is that a page cannot say a capability is live
 * without the route table agreeing, and that was a documented way around it sitting in the shipped
 * path.
 *
 * `readDescriptor()` now reads one file and nothing else. The hypothetical arrives through
 * VitePress's own `onAfterConfigResolve` hook, which exists only in the Node API — `npm run build`
 * and `pages.yml` run the CLI and have no way to reach it.
 *
 * **It is still the production derivation being exercised, not a copy of it.** The real
 * `.vitepress/config.mts` is loaded and its `assertDescriptorIsCurrent()` runs, so even a
 * hypothetical build proves the committed artifact is current; only the resolved
 * `transformPageData` is replaced, and it is replaced with a call to the same `statusFor` the real
 * config calls, given a different document to resolve against.
 *
 * **No page is edited and no file in the repository is written.** The output goes to a temporary
 * directory, so a crashed run leaves a clean tree — which matters more than it sounds: the
 * alternative shape, writing over the committed artifact and restoring it afterwards, leaves a
 * repository claiming a build that does not exist if the process dies.
 */
async function buildWith(document, dir) {
  const out = path.join(dir, 'dist')
  const { build } = await import('vitepress')
  const { statusFor } = await import(path.join(webRoot, '.vitepress', 'descriptor.mts'))

  try {
    await build(webRoot, {
      outDir: out,
      onAfterConfigResolve(siteConfig) {
        siteConfig.transformPageData = (pageData) => {
          pageData.frontmatter.capabilityStatus = statusFor(
            pageData.relativePath,
            pageData.frontmatter,
            document
          )
        }
      },
    })
    return { ok: true, output: '', out }
  } catch (failure) {
    return { ok: false, output: String(failure?.message ?? failure), out }
  }
}

test("flipping a capability's served flag flips the rendered badge", { timeout: 300_000 }, async () => {
  // **The deliverable.** Not "the badge matches the descriptor" — a hardcoded badge on a page whose
  // author copied today's answer satisfies that. This drives the flag at the far end of the chain
  // and watches the rendered HTML move: `served` in `console/src/surfaces.mts` → `available()` in
  // `onboarding.mts` → `live` in the descriptor → the badge in the page chrome.
  //
  // `subscribe` is the newest live route and therefore catches the exact transition that removed
  // the site's last real `Not built` page. The demonstration flips in either direction; it must not
  // depend on the production tree permanently retaining an absent capability just to test a badge.
  const { descriptorJson } = await import(path.join(repoRoot, 'console', 'src', 'descriptor.mts'))
  const { SURFACES } = await import(path.join(repoRoot, 'console', 'src', 'surfaces.mts'))

  const SUBJECT = 'subscribe'
  const page = `${SUBJECT}.html`

  // Where it stands today, read off the site that is already built. The mutation below is the
  // opposite, derived from this value rather than hard-coding yesterday's status.
  const before = capabilityPages().find(({ id }) => id === SUBJECT)
  assert.ok(before, `the site publishes no capability page for \`${SUBJECT}\` (X-64)`)
  const beforeLive = attribute(badge(before.name, before.html).element, 'data-live') === 'true'

  // The same site with the surface's served fact inverted — with no edit to any page or component.
  const hypothetical = JSON.parse(
    descriptorJson(
      SURFACES.map((surface) => (surface.id === SUBJECT ? { ...surface, served: !beforeLive } : surface))
    )
  )
  assert.equal(
    hypothetical.capabilities.find((capability) => capability.id === SUBJECT).live,
    !beforeLive,
    'flipping `served` did not flip the capability in the derived document; the chain is broken before it reaches the site'
  )

  await scratch(async (dir) => {
    const built = await buildWith(hypothetical, dir)
    assert.ok(built.ok, `the site did not build against the flipped \`${SUBJECT}\` descriptor:\n${built.output}`)

    const html = readFileSync(path.join(built.out, 'capabilities', page), 'utf-8')
    assert.equal(
      attribute(badge(page, html).element, 'data-live'),
      String(!beforeLive),
      `flipping \`${SUBJECT}\` did not change the badge on its page. The status is not derived from the descriptor — it is written somewhere, and this site is one stale edit from the sixth rendering of a false claim.`
    )

    assert.match(
      html,
      beforeLive ? /Not built/ : /Live/,
      'the flipped build did not exercise the badge label for the opposite state'
    )
  })
})

test('a page for a capability the descriptor does not name fails the build', { timeout: 300_000 }, async () => {
  // The other half, and the one absence hides in. A page whose capability has left the descriptor
  // must stop the build; rendering it with a blank status tells a reader nothing is wrong, which is
  // the failure this story exists to prevent. `invoke` is the subject because it is the capability
  // this repository has already published a false claim about, five times.
  const SUBJECT = 'invoke'
  const document = descriptor()
  assert.ok(
    document.capabilities.some((capability) => capability.id === SUBJECT),
    `the descriptor no longer names \`${SUBJECT}\`, so deleting it below removes nothing`
  )
  assert.ok(
    capabilityPages().some(({ id }) => id === SUBJECT),
    `the site publishes no capability page for \`${SUBJECT}\`, so there is no page to be orphaned (X-64)`
  )

  const without = {
    ...document,
    capabilities: document.capabilities.filter((capability) => capability.id !== SUBJECT),
  }

  await scratch(async (dir) => {
    const built = await buildWith(without, dir)
    assert.equal(
      built.ok,
      false,
      `the site built cleanly with a page for \`${SUBJECT}\` and no \`${SUBJECT}\` in the descriptor. That page published a capability page with no status, and a missing status reads as "fine".`
    )
    assert.match(
      built.output,
      new RegExp(SUBJECT),
      'the build failed without naming the capability that is missing, so whoever hits this cannot tell which page is orphaned'
    )
    assert.ok(
      !existsSync(path.join(built.out, 'capabilities', `${SUBJECT}.html`)),
      `the build failed and rendered \`${SUBJECT}\` anyway`
    )
  })
})

// ---------------------------------------------------------------------------------------------
// The rule that makes the two above cover pages nobody has written yet.
// ---------------------------------------------------------------------------------------------

test('every page under capabilities/ names the capability it is about', () => {
  // What keeps this mechanism load-bearing through X-65. The two tests above measure the pages that
  // exist; this one measures the *rule*, so a page added later cannot opt out of a status by simply
  // omitting the frontmatter key — that omission is the quietest possible way back to a page whose
  // liveness is whatever its prose says.
  //
  // Walked recursively, for the reason `capabilityPages` is: a page at
  // `capabilities/<group>/<page>.md` is still a page under `capabilities/`, `statusFor` will still
  // give it a status, and a one-level listing would have left it holding one nothing checked.
  assert.ok(existsSync(CAPABILITIES), `${CAPABILITIES} is missing`)
  const sources = walk(CAPABILITIES, '.md')
  assert.ok(sources.length > 0, `${CAPABILITIES} holds no page`)

  const named = new Set(descriptor().capabilities.map((capability) => capability.id))

  for (const source of sources) {
    const front = /^---\n([\s\S]*?)\n---/.exec(readFileSync(path.join(CAPABILITIES, source), 'utf-8'))
    assert.ok(front, `capabilities/${source} has no frontmatter, so it declares no capability`)
    const declared = /^capability:\s*(\S+)\s*$/m.exec(front[1])
    assert.ok(
      declared,
      `capabilities/${source} declares no \`capability:\` — a page in this directory is about one, and its status is derived from that key`
    )
    assert.ok(
      named.has(declared[1]),
      `capabilities/${source} is about \`${declared[1]}\`, which the descriptor does not name`
    )
  }
})
