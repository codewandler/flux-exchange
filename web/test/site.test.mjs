// What this site is not allowed to publish, asserted over the rendered HTML (X-63).
//
// The site is a public page about a service that holds other people's credentials, and its design
// (docs/designs/public-docs-site.md) names three rules it publishes under. Two of them are shapes a
// machine can recognise and are checked here; the other two are review's job and say so below,
// because a suite that implies coverage it does not have is worse than one that admits the gap.
//
//   checked here — no deployment-specific fact (a host, an address, a port, an instance);
//   checked here — nothing credential-shaped, however obviously fake, because a copyable example is
//                  a copied example;
//   NOT checked  — "nothing beyond what `GET /api/onboarding` already discloses". That ceiling is a
//                  judgement about a field list, not a pattern.
//   NOT checked  — "no claim that a capability is or is not live". A sentence claiming one is
//                  ordinary English and no regex separates it from a definition. **This is the
//                  interesting one**, and it is exactly what X-64 replaces with a derived badge; the
//                  three pages here are written to make no such claim in the meantime.
//
// It also pins the deployed base path, which is the mistake ../flux-connectors already paid for
// once, and the wiring that makes the build a gate rather than a habit.
//
// Node's built-in test runner, and a hand-rolled reader for the slice of HTML and YAML this asks
// about — the site has exactly one dependency and this adds none.
//
// **Run after `npm run build`.** These read `.vitepress/dist`; with no build present they fail
// against a site that was never rendered.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync, existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = path.resolve(webRoot, '..')
const dist = path.join(webRoot, '.vitepress', 'dist')

// The base GitHub actually serves this project site under. Pinned as a literal rather than read out
// of the config, so that flipping the config is a two-file change somebody has to mean.
//
// Flip this **only** once `gh api repos/codewandler/flux-exchange/pages --jq .cname` reports a
// domain — not when a CNAME file lands. Next door, a committed CNAME was taken as evidence the
// custom domain was live, the base went to '/', and every bundled asset 404'd on a site that still
// served from the project URL.
const DEPLOYED_BASE = '/flux-exchange/'

/** Every built page, as `{ name, html }`. */
function pages() {
  assert.ok(
    existsSync(dist),
    `${dist} does not exist — run \`npm run build\` before \`npm test\`; these assertions read the rendered site`
  )
  const names = readdirSync(dist).filter((name) => name.endsWith('.html'))
  assert.ok(names.length > 0, 'the build produced no HTML pages')
  return names.map((name) => ({ name, html: readFileSync(path.join(dist, name), 'utf-8') }))
}

/**
 * The human-readable text of a page: script and style bodies removed, tags stripped, entities for
 * the characters this file's patterns care about decoded.
 *
 * Scanning text rather than markup is deliberate. The bundler's own asset names are long opaque
 * strings by design (`style.aucfAaaG.css`), and a scan for "long opaque string" over raw HTML would
 * flag every build. What a reader could copy off the page is the text.
 */
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
}

/** Every `href`/`src` value in a page. */
function linksOf(html) {
  return [...html.matchAll(/(?:href|src)="([^"]*)"/g)].map((match) => match[1])
}

test('every root-relative URL resolves under the base GitHub serves this site from', () => {
  for (const { name, html } of pages()) {
    for (const link of linksOf(html)) {
      if (!link.startsWith('/')) continue
      assert.ok(
        link.startsWith(DEPLOYED_BASE),
        `${name} links to ${link}, which is outside the deployed base ${DEPLOYED_BASE} — that asset 404s on the published site`
      )
    }
  }
})

test('the site links off-site only to the flux family on GitHub', () => {
  // An absolute URL on a page about a credential-holding service is either a family repository or a
  // fact about somebody's deployment. There is no third kind we want to publish, so the allow-list
  // is a host list rather than a pattern.
  const allowed = new Set(['github.com'])

  for (const { name, html } of pages()) {
    for (const url of html.matchAll(/https?:\/\/([A-Za-z0-9.-]+)/g)) {
      const host = url[1]
      assert.ok(
        allowed.has(host),
        `${name} links to ${host}, which is not one of ${[...allowed].join(', ')} — an off-site host on this site is usually a deployment fact`
      )
    }
  }
})

test('no page publishes a deployment-specific fact', () => {
  // The site describes the software, never an instance. Each pattern is a shape that can only be
  // about somebody's running host.
  const forbidden = [
    [/\b\d{1,3}(?:\.\d{1,3}){3}\b/, 'an IP address'],
    [/\blocalhost\b/i, 'localhost'],
    [/\b127\.0\.0\.1\b/, 'the loopback address'],
    [/:\d{2,5}\/(?:api|health)\b/, 'a host:port endpoint'],
    [/\bhttps?:\/\/[A-Za-z0-9.-]+:\d+/, 'a URL naming a port'],
  ]

  for (const { name, html } of pages()) {
    const text = textOf(html)
    for (const [pattern, what] of forbidden) {
      const hit = text.match(pattern)
      assert.equal(
        hit,
        null,
        `${name} publishes ${what} (${hit?.[0]}) — this site describes the software, never a deployment`
      )
    }
  }
})

test('no page publishes anything credential-shaped', () => {
  // "However obviously fake" is the point: a reader copies the shape, fills in a real value, and the
  // example has done its damage. So these match the *shape*, not a known-bad value.
  const forbidden = [
    [/-----BEGIN [A-Z ]+-----/, 'a PEM block'],
    [/\bBearer\s+[A-Za-z0-9._~+/=-]{8,}/, 'a bearer token'],
    [/\b(?:sk|pk|rk)-[A-Za-z0-9]{8,}/, 'an API key'],
    [/\bgh[pousr]_[A-Za-z0-9]{8,}/, 'a GitHub token'],
    [/\bxox[baprs]-[A-Za-z0-9-]{8,}/, 'a Slack token'],
    [/\bAKIA[0-9A-Z]{16}\b/, 'an AWS access key id'],
    [/\beyJ[A-Za-z0-9_-]{8,}\./, 'a JWT'],
    // A configuration example with a value on the right-hand side, in any of the three spellings a
    // reader could paste: `FOO=bar`, `"secret": "…"`, `password: …`.
    [/\b[A-Z][A-Z0-9_]{3,}=\S/, 'an environment variable with a value'],
    [/\b(?:secret|password|passwd|token|api_key|apikey|credential)\s*[:=]\s*["'][^"']+["']/i, 'a configured secret'],
    // Any long opaque run in prose. The pages are English; a 32-character unbroken token is not.
    [/\b[A-Za-z0-9_-]{32,}\b/, 'an opaque value'],
  ]

  for (const { name, html } of pages()) {
    const text = textOf(html)
    for (const [pattern, what] of forbidden) {
      const hit = text.match(pattern)
      assert.equal(
        hit,
        null,
        `${name} publishes what looks like ${what} (${hit?.[0]}) — a copyable example is a copied example`
      )
    }
  }
})

test('the contributor readme is not a published page', () => {
  // `srcExclude` in the config. Without it, web/README.md — build instructions, the internal story
  // ids, the layout table — renders at /README on a public site.
  //
  // `pages()` first, so this cannot pass by there being no build at all: "README.html is absent" is
  // true of an empty directory, and that is the one way an assertion about an absent file lies.
  pages()
  assert.ok(existsSync(path.join(webRoot, 'README.md')), 'web/README.md is missing')
  assert.ok(
    !existsSync(path.join(dist, 'README.html')),
    'web/README.md rendered into the site — `srcExclude` no longer covers it, and the contributor readme is now a public page'
  )
})

test('a dead internal link still fails the build rather than publishing', () => {
  // The gate this whole story rests on, and a one-word edit turns it off. Verified by hand at the
  // time of writing: appending a link to a page that does not exist makes `npm run build` exit
  // non-zero with "Found dead link /channels in file surface.md". Nothing here re-runs a build —
  // that costs a second build per test run — so this asserts the setting that makes the failure
  // possible.
  const config = readFileSync(path.join(webRoot, '.vitepress', 'config.mts'), 'utf-8')
  assert.match(
    config,
    /ignoreDeadLinks:\s*false/,
    'ignoreDeadLinks is no longer `false` — a broken internal link would now publish instead of failing the build'
  )
  assert.match(
    config,
    new RegExp(`base\\s*=\\s*'${DEPLOYED_BASE}'`),
    `the site's base is no longer '${DEPLOYED_BASE}'; flip it only once the Pages API reports a cname, and update DEPLOYED_BASE here in the same change`
  )
})

test('the site build is a gate on pull requests, and only `main` deploys', () => {
  // Read as text rather than parsed as YAML: this asks four yes/no questions of one file, and a
  // hand-rolled YAML parser to answer them would be more code than the workflow. The limitation is
  // real — a restructured workflow could satisfy these strings without meaning them — so each
  // assertion names the property it stands for.
  const workflow = path.join(repoRoot, '.github', 'workflows', 'pages.yml')
  assert.ok(existsSync(workflow), '.github/workflows/pages.yml is missing — nothing builds or publishes the site')
  const yaml = readFileSync(workflow, 'utf-8')

  const uncommented = yaml
    .split('\n')
    .filter((line) => !line.trim().startsWith('#'))
    .join('\n')

  assert.match(
    uncommented,
    /^\s{2}pull_request:\s*$/m,
    'pages.yml no longer runs on `pull_request` — a change that breaks the site would reach `main` before anything said so'
  )
  assert.match(
    uncommented,
    /run:\s*npm run build/,
    'pages.yml no longer builds the site, so nothing fails on a dead link'
  )
  assert.match(
    uncommented,
    /if:.*github\.event_name\s*!=\s*'pull_request'/,
    "pages.yml's deploy job no longer excludes pull requests — a fork's pull request would publish"
  )
  assert.match(
    uncommented,
    /if:.*github\.ref\s*==\s*'refs\/heads\/main'/,
    "pages.yml's deploy job no longer requires `main` — a manual run on a feature branch would publish it"
  )

  // The one-time repository setting a workflow cannot do for itself. If this comment is lost, the
  // knowledge that `deploy` fails with "Get Pages site failed" until somebody clicks Source =
  // GitHub Actions is lost with it, and there is nowhere else it lives.
  assert.match(
    yaml,
    /Settings\s*→\s*Pages/,
    'pages.yml no longer records the one-time Settings → Pages → Source = GitHub Actions step, which no workflow can perform for itself'
  )
})

test("AGENTS.md documents the site build as part of this repository's gate", () => {
  // The failure this guards against is documentation drifting behind enforcement, which is how the
  // sibling repository ended up believing for months that a suite ran in CI when nothing ran it.
  const agents = readFileSync(path.join(repoRoot, 'AGENTS.md'), 'utf-8')

  for (const command of ['npm ci', 'npm run build', 'npm test']) {
    assert.ok(agents.includes(command), `AGENTS.md's gate no longer documents \`${command}\``)
  }
  assert.match(
    agents,
    /\.github\/workflows\/pages\.yml/,
    "AGENTS.md's gate does not name the workflow that enforces the site build"
  )
  assert.ok(agents.includes('cd web'), "AGENTS.md's gate does not tell anyone to run the site build in `web/`")
})
