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
// **`FLUX_EXCHANGE_DESCRIPTOR_FIXTURE` is how they do it**, and it is test-only plumbing — see
// `.vitepress/descriptor.mts`, which reads it. It replaces the document the *badges* derive from and
// deliberately does **not** replace the one the currency check reads, so pointing the build at a
// hypothetical never disables the guard that the committed artifact is current.
//
// **Run after `npm run build`**, like `site.test.mjs`: the first assertions read `.vitepress/dist`.
// The two build-again tests render into their own temporary directories and leave `dist` alone.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, readdirSync, existsSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = path.resolve(webRoot, '..')
const dist = path.join(webRoot, '.vitepress', 'dist')

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
 * Fails rather than returning nothing when the site publishes none. "There are no capability pages"
 * would otherwise make every assertion below vacuously true, which is the one way a suite about
 * derived status lies: it would stay green through the whole of X-65 adding pages that hardcode one.
 */
function capabilityPages(root = dist) {
  assert.ok(
    existsSync(root),
    `${root} does not exist — run \`npm run build\` before \`npm test\`; these assertions read the rendered site`
  )
  const built = path.join(root, 'capabilities')
  assert.ok(
    existsSync(built),
    `the site publishes no \`capabilities/\` pages — there is nothing carrying a derived status, so nothing here proves the badge is derived (X-64)`
  )
  const names = readdirSync(built).filter((name) => name.endsWith('.html'))
  assert.ok(names.length > 0, `${built} holds no rendered page`)
  return names.map((name) => ({
    id: name.replace(/\.html$/, ''),
    name: `capabilities/${name}`,
    html: readFileSync(path.join(built, name), 'utf-8'),
  }))
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

test('the site publishes a page for both answers the badge can give', () => {
  // Without this, a suite could be green on a site whose every capability page happens to be live,
  // and "the badge renders `Not built`" would never once have been executed. One of each is the
  // smallest sample that exercises the component's two branches.
  const document = descriptor()
  const named = new Map(document.capabilities.map((capability) => [capability.id, capability]))
  const live = capabilityPages().map(({ id }) => named.get(id)?.live)

  assert.ok(live.includes(true), 'no capability page is for a live capability')
  assert.ok(
    live.includes(false),
    'no capability page is for a capability this build does not have, so nothing on this site has ever rendered the `Not built` badge'
  )
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
function scratch(use) {
  const dir = mkdtempSync(path.join(tmpdir(), 'flux-exchange-site-'))
  try {
    return use(dir)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/**
 * Build the whole site against `document` as its descriptor, into `outDir`.
 *
 * Returns `{ ok, output }` rather than throwing, because one of the two callers is asserting that
 * the build *fails* and needs to read what it said while failing.
 *
 * **No page is edited and no file in the repository is written.** The fixture is a temporary file
 * and the output goes to a temporary directory, so a crashed run leaves a clean tree — which matters
 * more than it sounds: the alternative shape, writing over the committed artifact and restoring it
 * afterwards, leaves a repository claiming a build that does not exist if the process dies.
 */
function buildWith(document, dir) {
  const fixture = path.join(dir, 'onboarding.json')
  writeFileSync(fixture, `${JSON.stringify(document, null, 2)}\n`, 'utf-8')
  const out = path.join(dir, 'dist')

  try {
    const output = execFileSync('npx', ['vitepress', 'build', '--outDir', out], {
      cwd: webRoot,
      encoding: 'utf-8',
      stdio: 'pipe',
      env: { ...process.env, FLUX_EXCHANGE_DESCRIPTOR_FIXTURE: fixture },
    })
    return { ok: true, output, out }
  } catch (failure) {
    return { ok: false, output: `${failure.stdout ?? ''}${failure.stderr ?? ''}`, out }
  }
}

test("flipping a capability's served flag flips the rendered badge", { timeout: 300_000 }, async () => {
  // **The deliverable.** Not "the badge matches the descriptor" — a hardcoded badge on a page whose
  // author copied today's answer satisfies that. This drives the flag at the far end of the chain
  // and watches the rendered HTML move: `served` in `console/src/surfaces.mts` → `available()` in
  // `onboarding.mts` → `live` in the descriptor → the badge in the page chrome.
  //
  // `subscribe` and not `invoke`, for `descriptor.test.mjs`'s reason: `invoke` is served, so it
  // cannot stand in for a capability this build does not have.
  const { descriptorJson } = await import(path.join(repoRoot, 'console', 'src', 'descriptor.mts'))
  const { SURFACES } = await import(path.join(repoRoot, 'console', 'src', 'surfaces.mts'))

  const SUBJECT = 'subscribe'
  const page = `${SUBJECT}.html`

  // Where it stands today, read off the site that is already built. If this is not `false` the
  // mutation below proves nothing, so it is asserted rather than assumed.
  const before = capabilityPages().find(({ id }) => id === SUBJECT)
  assert.ok(before, `the site publishes no capability page for \`${SUBJECT}\` (X-64)`)
  assert.equal(
    attribute(badge(before.name, before.html).element, 'data-live'),
    'false',
    `\`${SUBJECT}\` is live in this build, so flipping its surface to served cannot demonstrate anything`
  )

  // The same site, as a build whose service serves it — with no edit to any page, none to the
  // component, and none here.
  const hypothetical = JSON.parse(
    descriptorJson(
      SURFACES.map((surface) => (surface.id === SUBJECT ? { ...surface, served: true } : surface))
    )
  )
  assert.equal(
    hypothetical.capabilities.find((capability) => capability.id === SUBJECT).live,
    true,
    'flipping `served` did not make the capability live in the derived document; the chain is broken before it reaches the site'
  )

  scratch((dir) => {
    const built = buildWith(hypothetical, dir)
    assert.ok(built.ok, `the site did not build against a descriptor where \`${SUBJECT}\` is live:\n${built.output}`)

    const html = readFileSync(path.join(built.out, 'capabilities', page), 'utf-8')
    assert.equal(
      attribute(badge(page, html).element, 'data-live'),
      'true',
      `marking \`${SUBJECT}\` served did not change the badge on its page. The status is not derived from the descriptor — it is written somewhere, and this site is one stale edit from the sixth rendering of a false claim.`
    )
  })
})

test('a page for a capability the descriptor does not name fails the build', { timeout: 300_000 }, () => {
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

  scratch((dir) => {
    const built = buildWith(without, dir)
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
  assert.ok(existsSync(CAPABILITIES), `${CAPABILITIES} is missing`)
  const sources = readdirSync(CAPABILITIES).filter((name) => name.endsWith('.md'))
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
