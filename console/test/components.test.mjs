// The boundary that makes the carried components mountable here at all.
//
// The fifteen components under `src/components/` were not written for this repository. They are the
// explorer from the flux-connectors documentation site, and the only reason they could be copied
// across instead of rewritten is that none of them imports its old host: since C-142 a component may
// import Vue, a sibling component, and the catalogue's typed contract, and nothing else.
//
// That was an asserted property there (`web/test/explorer.test.mjs`,
// `no_component_imports_the_site_framework`) and it has to stay an asserted property here, or it
// rots in exactly the way that is invisible: a component that imports Vite, or `node:fs`, or this
// console's fixture data, renders identically on this page and simply stops being mountable
// anywhere. Nothing about the rendered output would ever tell you.
//
// So this file is the port of that test, plus the two guards the move itself introduced:
//
//   - the scanner is checked against sources it *should* reject, so the test cannot degrade into a
//     vacuous pass by quietly failing to see imports at all;
//   - every CSS custom property the components read has to be defined by this console's token layer,
//     because an undefined custom property renders as nothing, silently.
//
// Node's built-in test runner, deliberately: this is a build-time property of the sources, needs no
// browser and no bundler, and adding a test framework to check it would be out of proportion.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const componentDir = path.join(consoleRoot, 'src', 'components')

/** Every carried component, by absolute path. */
function componentSources() {
  return readdirSync(componentDir)
    .filter((entry) => entry.endsWith('.vue'))
    .map((entry) => path.join(componentDir, entry))
}

const rel = (file) => path.relative(consoleRoot, file)

/**
 * The contents of every `<script>` block in a single-file component.
 *
 * Imports can only live in a script block, and restricting the scan to those blocks is what lets the
 * comment stripper below run safely: a `//` outside a string is a comment in JavaScript, but in a
 * template it is most likely the middle of a URL.
 */
function scriptBlocks(source) {
  return [...source.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)].map((match) => match[1])
}

/**
 * Script comments removed, string literals kept.
 *
 * String-aware rather than a `//.*$` sweep, because these files carry a great deal of comment and
 * some of it names modules in prose — `withBase` and `vitepress` both appear in the components'
 * comments today. Reading those as imports would fail the suite on documentation.
 *
 * A quote that opens no string makes the scanner read the rest of the block as string content, so it
 * keeps text rather than dropping it: every ambiguity here fails towards *keeping* code, which is
 * the safe direction for a guard.
 */
function withoutComments(source) {
  let kept = ''
  for (let i = 0; i < source.length; ) {
    const pair = source.slice(i, i + 2)
    if (pair === '//') {
      const end = source.indexOf('\n', i)
      kept += ' '
      i = end === -1 ? source.length : end
    } else if (pair === '/*') {
      const end = source.indexOf('*/', i + 2)
      kept += ' '
      i = end === -1 ? source.length : end + 2
    } else if (source[i] === "'" || source[i] === '"' || source[i] === '`') {
      const quote = source[i]
      const start = i++
      while (i < source.length && source[i] !== quote) i += source[i] === '\\' ? 2 : 1
      kept += source.slice(start, ++i)
    } else {
      kept += source[i++]
    }
  }
  return kept
}

/**
 * Every module a component reaches for, however it reaches.
 *
 * Wider than the specifier form the original test scanned, because every form below is a way to
 * couple a component to a host and the point of the guard is the coupling, not the syntax:
 *
 *   import x from 'm'      import type { T } from 'm'      import 'm'
 *   export { x } from 'm'  export * from 'm'
 *   await import('m')      require('m')
 *
 * Quotes may be single or double. `require` is included because a component that reached for CommonJS
 * would be just as unmountable, and it costs one alternative to say so.
 */
function importedModules(source) {
  const script = scriptBlocks(source).map(withoutComments).join('\n')
  const patterns = [
    /\bimport\s+[^'"();]*?\bfrom\s*['"]([^'"]+)['"]/g,
    /\bimport\s*['"]([^'"]+)['"]/g,
    /\bexport\s+[^'"();]*?\bfrom\s*['"]([^'"]+)['"]/g,
    /\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
    /\brequire\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
  ]
  const found = new Set()
  for (const pattern of patterns) {
    for (const match of script.matchAll(pattern)) found.add(match[1])
  }
  return [...found]
}

/**
 * The complete set of module specifiers a component may name.
 *
 *   `vue`               the framework the components are written in
 *   `./Something.vue`   a sibling in this same directory
 *   `../catalog.mts`    the catalogue's typed contract, which is data shape and no data
 *
 * Note what the third one is *not*: it is not a loader, not a fetch, not this console's fixtures.
 * Everything a component renders arrives as a prop or as injected context, and a component that
 * reached for its own data could not be attached to a second host — which is the entire reason this
 * set could be lifted out of the flux-connectors site and dropped in here unchanged.
 *
 * The path is `../catalog.mts` and not `../../../data/catalog.mts` because the module moved with
 * them; the rule did not change, only the layout it is stated against.
 */
const ALLOWED = /^(vue|\.\/[A-Za-z][A-Za-z0-9]*\.vue|\.\.\/catalog\.mts)$/

test('no_component_imports_the_site_framework', () => {
  const sources = componentSources()
  assert.ok(sources.length > 0, 'no components were found; this test would pass vacuously')

  const imports = new Map(
    sources.map((file) => [file, importedModules(readFileSync(file, 'utf-8'))])
  )

  // Stated first and on its own, because it is the coupling the original story was about and the one
  // most likely to come back: someone reaches for a router or a site helper for a link, and five
  // components quietly acquire a host.
  const framework = /^(vitepress|vitepress\/|vite$|vite\/|vue-router|@vitejs\/)/
  const coupled = sources.filter((file) => imports.get(file).some((module) => framework.test(module)))
  assert.deepEqual(
    coupled.map(rel),
    [],
    `${coupled.length} component(s) import a host framework directly, so they can only ever be mounted in one app: ${coupled.map(rel).join(', ')}`
  )

  // And nothing else either. A component that reaches for the filesystem, for the network, or for
  // this console's fixture data cannot be attached anywhere, whatever it imports it from.
  for (const file of sources) {
    for (const module of imports.get(file)) {
      assert.match(
        module,
        ALLOWED,
        `${rel(file)} imports \`${module}\` — a component takes what it renders as props or injected context, and may otherwise import only Vue, a sibling component, or the catalogue's typed contract (\`../catalog.mts\`)`
      )
    }
  }
})

// The guard on the guard.
//
// `no_component_imports_the_site_framework` passes today, and a scanner that had silently stopped
// seeing imports would pass in exactly the same way — green, and worth nothing. So the scanner is
// run against sources that must be rejected, and the assertion is that it sees them.
//
// Each case below is a real way the boundary has been crossed or could be: the host framework, the
// filesystem, a build-time data loader, this console's own fixtures, a transport, and every import
// syntax that reaches them.
test('the import scanner sees the imports it exists to catch', () => {
  const component = (script) => `<script setup lang="ts">\n${script}\n</script>\n<template><p /></template>\n`

  const forbidden = [
    ["import { withBase } from 'vitepress'", 'vitepress'],
    ['import { useRouter } from "vue-router"', 'vue-router'],
    ["import { readFileSync } from 'node:fs'", 'node:fs'],
    ["import { data } from '../catalog.data.mts'", '../catalog.data.mts'],
    ["import { fixtureCatalog } from '../fixtures/catalog'", '../fixtures/catalog'],
    ["import { routes } from '../routing'", '../routing'],
    ["import axios from 'axios'", 'axios'],
    // Side-effect, re-export, dynamic and CommonJS forms: the coupling is the point, not the syntax.
    ["import 'vitepress/dist/client/theme-default/styles/vars.css'", 'vitepress/dist/client/theme-default/styles/vars.css'],
    ["export { withBase } from 'vitepress'", 'vitepress'],
    ["const m = await import('vitepress')", 'vitepress'],
    ["const m = require('node:fs')", 'node:fs'],
    // Type-only is not an exemption: `import type` still names a module a host has to provide.
    ["import type { Router } from 'vue-router'", 'vue-router'],
  ]

  for (const [line, module] of forbidden) {
    const seen = importedModules(component(line))
    assert.ok(seen.includes(module), `the scanner did not see \`${module}\` in: ${line}`)
    assert.ok(
      !ALLOWED.test(module),
      `the scanner saw \`${module}\` but the allowlist would let it through`
    )
  }

  // And the three that are allowed are still recognised as imports and still permitted — a scanner
  // that rejected everything would be useless in the opposite direction.
  for (const [line, module] of [
    ["import { computed } from 'vue'", 'vue'],
    ["import StatusBadge from './StatusBadge.vue'", './StatusBadge.vue'],
    ["import { ownIssues } from '../catalog.mts'", '../catalog.mts'],
  ]) {
    const seen = importedModules(component(line))
    assert.ok(seen.includes(module), `the scanner did not see the permitted import \`${module}\``)
    assert.match(module, ALLOWED, `\`${module}\` is permitted but the allowlist rejects it`)
  }

  // Comments are not imports. The components carry a lot of prose and some of it names the very
  // modules above; reading those as imports would fail the suite on its own documentation.
  const commented = importedModules(
    component("// import { withBase } from 'vitepress'\n/* import 'node:fs' */\nimport { computed } from 'vue'")
  )
  assert.deepEqual(commented, ['vue'], 'a module named in a comment was read as an import')

  // And it sees all three permitted kinds in the *real* sources. If the extraction of `<script>`
  // blocks ever broke, every scan would return nothing and the whole guard would pass on air —
  // silently, and looking exactly as green as it does now.
  //
  // Asserted across the set rather than per file, because a component with no imports at all is a
  // legitimate thing to be: `FluxSource.vue` renders a string it is handed and needs nothing.
  const actual = new Set(
    componentSources().flatMap((file) => importedModules(readFileSync(file, 'utf-8')))
  )
  assert.ok(actual.has('vue'), 'the scanner found no `vue` import in any component')
  assert.ok(actual.has('../catalog.mts'), 'the scanner found no catalogue import in any component')
  assert.ok(
    [...actual].some((module) => module.startsWith('./') && module.endsWith('.vue')),
    'the scanner found no sibling-component import in any component'
  )
})

// The visual half of "mountable somewhere else".
//
// The components name no colour. Every one of them is written against CSS custom properties, which is
// what let this console adopt the docs site's look by defining variables instead of editing fifteen
// files (`src/tokens.css`). The cost of that indirection is that a variable this console forgets to
// define does not fail — it resolves to nothing, and a badge renders as unstyled text on a page that
// otherwise looks fine. So it is checked.
test('every_variable_the_components_use_is_defined', () => {
  const referenced = new Map()
  for (const file of componentSources()) {
    const source = readFileSync(file, 'utf-8')
    for (const match of source.matchAll(/var\(\s*(--[A-Za-z0-9-]+)/g)) {
      if (!referenced.has(match[1])) referenced.set(match[1], rel(file))
    }
  }
  assert.ok(referenced.size > 0, 'no custom properties were found; this test would pass vacuously')

  // Collected per selector, and only the **light** blocks count.
  //
  // `.dark` is an override layer: it restates the properties whose value changes and no others, so a
  // property defined *only* under `.dark` is undefined in light mode — which is the more common mode
  // and the one a reader arrives in by default. Counting a `.dark` declaration as a definition would
  // let exactly that ship, so the base has to carry every property and dark may only override.
  //
  // Both stylesheets, because `app.css` legitimately defines a few on top of the token layer.
  const base = new Set()
  const darkOnly = new Set()
  for (const sheet of ['tokens.css', 'app.css']) {
    const css = readFileSync(path.join(consoleRoot, 'src', sheet), 'utf-8')
    for (const block of css.matchAll(/([^{}]*)\{([^{}]*)\}/g)) {
      const dark = /(^|[\s,])\.dark\b/.test(block[1])
      for (const declaration of block[2].matchAll(/(--[A-Za-z0-9-]+)\s*:/g)) {
        ;(dark ? darkOnly : base).add(declaration[1])
      }
    }
  }

  const missing = [...referenced].filter(([name]) => !base.has(name))
  assert.deepEqual(
    missing.map(
      ([name, file]) =>
        `${name} (first read by ${file}${darkOnly.has(name) ? '; defined under .dark only, so it is undefined in light mode' : ''})`
    ),
    [],
    'these custom properties are read by a component and defined by no light-mode rule in this console, so they resolve to nothing and that element renders unstyled'
  )
})
