// What this site is not allowed to publish, asserted over the rendered HTML (X-63).
//
// The site is a public page about a service that holds other people's credentials, and its design
// (docs/designs/public-docs-site.md) names three rules it publishes under. Two of them are shapes a
// machine can recognise and are checked here; the other two are review's job and say so below,
// because a suite that implies coverage it does not have is worse than one that admits the gap.
//
//   checked here — no deployment-specific fact (a host, an address, a port, an instance);
//   checked here — nothing credential-shaped, however obviously fake, because a copyable example is
//                  a copied example. Both of those are read twice per page — the prose, and every
//                  fenced example as the clipboard would carry it. See [`codeBlocksOf`] for why the
//                  second reading is not redundant, and X-69 for how it was missing;
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
import { readFileSync, existsSync } from 'node:fs'
import path from 'node:path'

import { pages, dist, webRoot, repoRoot } from './rendered.mjs'

// The base GitHub actually serves this project site under. Pinned as a literal rather than read out
// of the config, so that flipping the config is a two-file change somebody has to mean.
//
// Flip this **only** once `gh api repos/codewandler/flux-exchange/pages --jq .cname` reports a
// domain — not when a CNAME file lands. Next door, a committed CNAME was taken as evidence the
// custom domain was live, the base went to '/', and every bundled asset 404'd on a site that still
// served from the project URL.
const DEPLOYED_BASE = '/flux-exchange/'

// `pages()` is imported rather than defined here, and that is X-64's rework rather than tidying.
//
// It used to be `readdirSync(dist)` — no recursion — which was total coverage while every page sat
// at the root of `dist`, and stopped being so the moment `capabilities/` was added. Every rule below
// is a loop over `pages()`, so the two pages one directory down were scanned by none of them, on a
// live public site, with this suite green. One enumerator now, in `rendered.mjs`, and
// `coverage.test.mjs` holds it to covering everything the site actually publishes.

/** The entities this file's patterns care about, decoded. */
function decode(html) {
  return html
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
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
  return decode(
    html
      .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, ' ')
      .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, ' ')
      .replace(/<[^>]+>/g, ' ')
  )
}

/**
 * Every fenced code block of a page, as the text the copy button would put on a clipboard.
 *
 * **This exists because [`textOf`] cannot see a code block, and X-69 is the story that found out.**
 * The highlighter wraps each token in its own `<span>`, so `textOf` — which replaces a tag with a
 * *space*, correctly, or every sentence would run into the next — renders `export FOO=bar` as
 * `export FOO = bar`. Every rule below that looks for a value on the right-hand side of an `=` then
 * misses it. The site had no fenced block on any page until X-69 added the first one, so the rule
 * about what an *example* may contain had never once been asked about an example.
 *
 * Tags are therefore stripped with **no** separator here, which reconstructs the block verbatim,
 * and `the code-block reader reconstructs what a reader would copy` below runs this against
 * highlighter markup it must see through.
 */
function codeBlocksOf(html) {
  return [
    ...html.matchAll(/<div class="language-[^"]*">[\s\S]*?<pre[^>]*>([\s\S]*?)<\/pre>/g),
  ].map((match) => decode(match[1].replace(/<[^>]+>/g, '')))
}

/**
 * Shapes that can only be a fact about somebody's running host.
 *
 * Hoisted out of the test that scans pages so [`codeBlocksOf`]'s own self-test can hold them, in
 * the shape `console/test/components.test.mjs` uses: a scanner that has not just proved it catches
 * a violation is not evidence there are none.
 */
const DEPLOYMENT_FACTS = [
  [/\b\d{1,3}(?:\.\d{1,3}){3}\b/, 'an IP address'],
  [/\blocalhost\b/i, 'localhost'],
  [/\b127\.0\.0\.1\b/, 'the loopback address'],
  [/:\d{2,5}\/(?:api|health)\b/, 'a host:port endpoint'],
  [/\bhttps?:\/\/[A-Za-z0-9.-]+:\d+/, 'a URL naming a port'],
]

/**
 * Shapes a credential takes, "however obviously fake" being the point: a reader copies the shape,
 * fills in a real value, and the example has done its damage. So these match the *shape*, not a
 * known-bad value.
 */
const CREDENTIAL_SHAPES = [
  [/-----BEGIN [A-Z ]+-----/, 'a PEM block'],
  [/\bBearer\s+[A-Za-z0-9._~+/=-]{8,}/, 'a bearer token'],
  [/\b(?:sk|pk|rk)-[A-Za-z0-9]{8,}/, 'an API key'],
  [/\bgh[pousr]_[A-Za-z0-9]{8,}/, 'a GitHub token'],
  [/\bxox[baprs]-[A-Za-z0-9-]{8,}/, 'a Slack token'],
  [/\bAKIA[0-9A-Z]{16}\b/, 'an AWS access key id'],
  [/\beyJ[A-Za-z0-9_-]{8,}\./, 'a JWT'],
  // A configuration example with a value on the right-hand side, in any of the three spellings a
  // reader could paste: `FOO=bar`, `"secret": "…"`, `password: …`.
  //
  // **`FOO=<something>` is exempt, and that exemption is X-69's.** A page that tells somebody how
  // to arm a development identity has to name the variable that arms it, and the reason this rule
  // exists — a copyable example is a copied example — is the reason a *placeholder* is the right
  // spelling rather than a banned one: `FLUX_EXCHANGE_DEV_IDENTITY=<kind:id@tenant>` is the
  // grammar, not a value, and pasting it verbatim makes the process refuse to start and name the
  // entry rather than arming something. The rule is still "no value": anything but an
  // angle-bracket placeholder, quoted or bare, fires. `the environment-variable rule admits a
  // placeholder and still catches a value` is what holds the exemption to that width.
  [
    /\b[A-Z][A-Z0-9_]{3,}=(?!["']?<[^<>\n]+>["']?(?:[\s,;)]|$))\S/,
    'an environment variable with a value',
  ],
  [
    /\b(?:secret|password|passwd|token|api_key|apikey|credential)\s*[:=]\s*["'][^"']+["']/i,
    'a configured secret',
  ],
  // Any long opaque run in prose. The pages are English; a 32-character unbroken token is not.
  [/\b[A-Za-z0-9_-]{32,}\b/, 'an opaque value'],
]

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

test('the site links off-site only to the flux family', () => {
  // An absolute URL on a page about a credential-holding service is either something of the flux
  // family's or a fact about somebody's deployment. There is no third kind we want to publish, so
  // the allow-list is a host list rather than a pattern.
  //
  // Two hosts, because the family has two kinds of address and X-77 is about telling them apart:
  // `github.com` holds the repositories, `codewandler.github.io` serves the sites they publish.
  // Which of the two a given link should use is the subject question [`subjectIsTheProject`] asks —
  // this test only says that no third host appears.
  const allowed = new Set(['github.com', 'codewandler.github.io'])

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
  // The site describes the software, never an instance.
  for (const { name, html } of pages()) {
    for (const scanned of [textOf(html), ...codeBlocksOf(html)]) {
      for (const [pattern, what] of DEPLOYMENT_FACTS) {
        const hit = scanned.match(pattern)
        assert.equal(
          hit,
          null,
          `${name} publishes ${what} (${hit?.[0]}) — this site describes the software, never a deployment`
        )
      }
    }
  }
})

test('no page publishes anything credential-shaped, in its prose or in an example', () => {
  // Both readings of every page: the prose, and each fenced block as the clipboard would carry it.
  // The second is not redundant — see [`codeBlocksOf`] for the reading the first cannot make.
  for (const { name, html } of pages()) {
    for (const scanned of [textOf(html), ...codeBlocksOf(html)]) {
      for (const [pattern, what] of CREDENTIAL_SHAPES) {
        const hit = scanned.match(pattern)
        assert.equal(
          hit,
          null,
          `${name} publishes what looks like ${what} (${hit?.[0]}) — a copyable example is a copied example`
        )
      }
    }
  }
})

test('the code-block reader reconstructs what a reader would copy', () => {
  // Run against highlighter markup it must see through, rather than trusting that it does. The
  // fixture is one line of real `vitepress build` output with the colours shortened: every token in
  // its own span, which is exactly what defeats a text scan that separates tags with a space.
  const highlighted =
    '<div class="language-sh vp-adaptive-theme"><button title="Copy Code" class="copy"></button>' +
    '<span class="lang">sh</span><pre class="shiki vp-code" tabindex="0"><code><span class="line">' +
    '<span style="--shiki-light:#D73A49;">export</span><span style="--shiki-light:#24292E;"> A_SETTING</span>' +
    '<span style="--shiki-light:#D73A49;">=</span><span style="--shiki-light:#032F62;">&quot;a value&quot;</span>' +
    '</span></code></pre></div>'

  assert.deepEqual(codeBlocksOf(highlighted), ['export A_SETTING="a value"'])

  // The half that makes this reader worth having: the same markup, read as prose, hides the
  // assignment from the rule that is supposed to catch it.
  const [envRule] = CREDENTIAL_SHAPES.filter(([, what]) => what === 'an environment variable with a value')
  assert.ok(envRule, 'the environment-variable rule is no longer in CREDENTIAL_SHAPES')
  assert.match(codeBlocksOf(highlighted)[0], envRule[0])
  assert.doesNotMatch(
    textOf(highlighted),
    envRule[0],
    'prose-reading now catches a tokenised assignment; if that is really true, say so and simplify — it was not true when this was written'
  )
})

test('the environment-variable rule admits a placeholder and still catches a value', () => {
  // The one exemption X-69 added, held to its width. A grammar is publishable; a value is not.
  const [[pattern]] = CREDENTIAL_SHAPES.filter(
    ([, what]) => what === 'an environment variable with a value'
  )

  for (const caught of [
    'export FLUX_EXCHANGE_GRANTS=/home/somebody/grants',
    'FLUX_EXCHANGE_DEV_IDENTITY="user:alice@acme"',
    'A_SETTING=<a placeholder>then-a-value',
    'A_SETTING=<>',
  ]) {
    assert.match(caught, pattern, `${caught} names a value and must not publish`)
  }

  for (const admitted of [
    'export FLUX_EXCHANGE_DEV_IDENTITY="<kind:id@tenant>"',
    'export FLUX_EXCHANGE_GRANTS=<a path outside every checkout>',
    'A_SETTING=<a placeholder>, and prose after it',
  ]) {
    assert.doesNotMatch(admitted, pattern, `${admitted} is a grammar rather than a value`)
  }
})

// ---------------------------------------------------------------------------------------------
// The getting-started page (X-69)
// ---------------------------------------------------------------------------------------------

/** Where the page a stranger starts from is served. `cleanUrls`, so the link carries no suffix. */
const GETTING_STARTED = { file: 'getting-started.html', link: `${DEPLOYED_BASE}getting-started` }

/** The built getting-started page, or a failure that says what is missing rather than `undefined`. */
function gettingStarted() {
  const page = pages().find(({ name }) => name === GETTING_STARTED.file)
  assert.ok(
    page,
    `the site publishes no ${GETTING_STARTED.file} — a visitor can read what this service refuses to do and cannot learn how to start it (X-69)`
  )
  return page
}

test('the loopback constraint is inside the block a reader would copy, not under it', () => {
  // The one thing on this page that must not go wrong. A roster handle is a credential with no
  // secret in it, which is why `admit_bind` refuses every non-loopback address while the
  // development identity is armed — *a reachable bind whose authentication is a name anybody can
  // guess is worse than no authentication, because the surface in front of it believes every
  // caller.* Somebody skimming for a command reads the block and nothing else, so the constraint
  // has to be *in* it; a page that explains local sign-in and mentions loopback three screens later
  // is a page that gets a secret-free roster onto a public address.
  const { name, html } = gettingStarted()
  const blocks = codeBlocksOf(html)
  assert.ok(blocks.length > 0, `${name} carries no example at all`)

  const starting = blocks.filter((block) => /cargo run/.test(block))
  assert.ok(
    starting.length > 0,
    `${name} has no block that starts the service, so there is nothing for the constraint to be inside of`
  )

  for (const block of starting) {
    assert.match(
      block,
      /loopback/i,
      `${name} starts the service in a block that does not say the bind is loopback-only — whoever copies it meets the constraint at startup instead, or does not meet it at all`
    )
    assert.match(
      block,
      /deploy/i,
      `${name} starts the service in a block that does not say this is not how you deploy it`
    )
  }
})

test('the getting-started page says what must be true before anything will run', () => {
  // Fail-closed on invocation is the correct behaviour and it looks exactly like an outage. A page
  // that ends at "you are signed in" sends its reader into a refusal with nothing to act on.
  const { name, html } = gettingStarted()
  const text = textOf(html)

  for (const [needle, why] of [
    ['FLUX_EXCHANGE_GRANTS', 'the setting that has to name a file before anything runs'],
    ['not_granted', 'the refusal a reader meets when a store is bound and the tenant holds nothing'],
  ]) {
    assert.ok(text.includes(needle), `${name} does not name ${needle} — ${why}`)
  }
})

test('the getting-started page reaches the reader: the nav on every page, and the landing hero', () => {
  // Sidebar-only would satisfy "the page exists" and not "a visitor finds it". The nav is on every
  // page and the hero action is what a first-time visitor is offered before they read anything.
  // `404.html` is excluded and it is the only exclusion: VitePress renders it without the theme
  // shell, so it carries no nav on any page of any site built this way — asserting over it would be
  // asserting about the framework rather than about this site's navigation.
  for (const { name, html } of pages().filter(({ name }) => name !== '404.html')) {
    assert.ok(
      linksOf(html).includes(GETTING_STARTED.link),
      `${name} does not link to ${GETTING_STARTED.link} — the nav is what puts the page in front of somebody who did not come looking for it`
    )
  }

  const { html } = pages().find(({ name }) => name === 'index.html')
  assert.match(
    html,
    new RegExp(`<a class="[^"]*VPButton[^"]*"[^>]*href="${GETTING_STARTED.link}"`),
    'the landing page offers no hero action for the getting-started page — a nav entry is not the same as being handed the page'
  )
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

// ---------------------------------------------------------------------------------------------
// The family links (X-77)
// ---------------------------------------------------------------------------------------------

/**
 * The siblings this site sends a reader to, each with the repository it lives in and the
 * documentation site it publishes.
 *
 * Verified 2026-08-02: both sites answer 200, and `gh api repos/codewandler/<name> --jq .cname`
 * reports no cname for either — so the `codewandler.github.io` address *is* the address, rather than
 * a redirect to a nicer domain that ought to be linked instead.
 */
const FAMILY = [
  {
    name: 'flux',
    repo: 'https://github.com/codewandler/flux',
    site: 'https://codewandler.github.io/flux/',
  },
  {
    name: 'flux-connectors',
    repo: 'https://github.com/codewandler/flux-connectors',
    site: 'https://codewandler.github.io/flux-connectors/',
  },
]

/**
 * Every `<a>` of a page as `{ href, text }`, with the text read the way a reader sees it.
 *
 * Anchors do not nest, so the non-greedy body is safe. [`textOf`] on the body is what turns the
 * social icon's `<svg>` into the empty string, which is the correct reading of it: a link offering
 * no words is not a link about a project.
 */
function anchorsOf(html) {
  return [...html.matchAll(/<a\b([^>]*)>([\s\S]*?)<\/a>/g)].map(([, attrs, body]) => ({
    href: decode(attrs.match(/href="([^"]*)"/)?.[1] ?? ''),
    text: textOf(body).replace(/\s+/g, ' ').trim(),
  }))
}

/**
 * Is this anchor about a sibling **project** — what it is and what it does — rather than about the
 * repository that project happens to live in?
 *
 * **The discriminator is the link's subject, not its hostname**, and this is the paragraph to read
 * before adding a github.com link to this site rather than the story that added the rule. A page may
 * point at github.com as often as it likes provided what it points *at* is genuinely the repository:
 * `getting-started`'s clone URL, `surface`'s pointer to the itemized inventory in the README,
 * `index`'s `#what-exists-today` deep link, the `Releases (GitHub)` nav entry and the social icon all
 * mean the repository and are correct. Swapping those for a landing page would send somebody looking
 * for a clone URL somewhere that has none. Only a link whose subject is *what a sibling is or does*
 * has a better destination, and that is the site the sibling publishes.
 *
 * Two questions decide it, in order:
 *
 *   1. Does the URL address something *inside* the repository — a path, a fragment, a releases page?
 *      No documentation site serves those, so github.com is the only address there is.
 *   2. Otherwise it is a bare repository URL and the words decide. Offered under the project's own
 *      name it is a link about the project; offered as "repository", or as an icon carrying no words
 *      at all, it is a link about the repository.
 *
 * The second question is deliberately narrow: it recognises the project's own name and nothing else,
 * so `[the flux engine](https://github.com/codewandler/flux)` would slip past it. Widening it means
 * guessing which English phrases name a project, and a guard that fires on a legitimate repository
 * link is a guard people learn to route around. `the subject rule admits a repository link and still
 * catches a family link` holds it to exactly this width.
 */
function subjectIsTheProject({ href, text }, sibling) {
  if (href !== sibling.repo && href !== `${sibling.repo}/`) return false
  return text.toLowerCase() === sibling.name
}

test('a link about a sibling project goes to that project’s site, not to its repository', () => {
  // The three sites are one product, so following a family link has to keep the reader inside the
  // family's documentation instead of dropping them into a source tree.
  //
  // This cannot be the build's job. `ignoreDeadLinks: false` is what makes a wrong link fail
  // `npm run build`, and it resolves **internal** links only — an external link to the wrong host
  // answers 200 and would publish for as long as nobody noticed.
  for (const { name, html } of pages()) {
    for (const anchor of anchorsOf(html)) {
      for (const sibling of FAMILY) {
        assert.ok(
          !subjectIsTheProject(anchor, sibling),
          `${name} offers "${anchor.text}" as ${anchor.href}, which is ${sibling.name}'s repository — a link about what ${sibling.name} is belongs at ${sibling.site}, the documentation site it publishes`
        )
      }
    }
  }
})

test('the family is reachable from every page, not only from the overview', () => {
  // "Three sites read as one product" is a property of the whole site, not of its landing page: a
  // reader who arrives at /surface from a search result should be able to reach the siblings too.
  // The nav and the footer are the two places that appear on every page.
  //
  // `404.html` is excluded for the same reason as the getting-started nav test above, and it is the
  // only exclusion: VitePress renders it without the theme shell, so it carries neither.
  for (const { name, html } of pages().filter(({ name }) => name !== '404.html')) {
    const links = linksOf(html)
    for (const { name: sibling, site } of FAMILY) {
      assert.ok(
        links.includes(site),
        `${name} does not link ${site} — ${sibling} is reachable from some pages and not from others, which is three repositories again`
      )
    }
  }
})

test('the subject rule admits a repository link and still catches a family link', () => {
  // The discriminator, held to its width in both directions, in the shape the environment-variable
  // rule above uses: a scanner that has not just proved it catches a violation is not evidence there
  // are none.
  const [flux, connectors] = FAMILY

  for (const caught of [
    { href: flux.repo, text: 'flux' },
    { href: `${flux.repo}/`, text: 'Flux' },
    { href: connectors.repo, text: 'flux-connectors' },
  ]) {
    const sibling = caught.href.startsWith(connectors.repo) ? connectors : flux
    assert.ok(
      subjectIsTheProject(caught, sibling),
      `"${caught.text}" → ${caught.href} is a link about the project and must go to ${sibling.site}`
    )
  }

  for (const admitted of [
    { href: `${flux.repo}/releases`, text: 'Releases (GitHub)' },
    { href: `${flux.repo}#what-exists-today`, text: 'what exists today' },
    { href: flux.repo, text: 'repository' },
    { href: flux.repo, text: '' },
    { href: flux.site, text: 'flux' },
    // The sibling's name is a prefix of the other's repository URL; exact comparison, not
    // `startsWith`, is what keeps `flux-connectors` from being read as `flux`.
    { href: connectors.repo, text: 'flux-connectors' },
  ]) {
    assert.ok(
      !subjectIsTheProject(admitted, flux),
      `"${admitted.text}" → ${admitted.href} means the repository, or another sibling, and must not trip the family rule`
    )
  }
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
