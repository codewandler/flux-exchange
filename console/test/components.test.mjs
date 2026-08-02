// X-86's local catalogue-view boundary.
//
// The views are no longer copied from flux-connectors, but the useful architecture survives:
// `service.mts` owns the network, App passes completed data down, and every colour comes from the
// console's token layer. These checks state those local properties without pretending the views are
// a portable package.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = (name) => readFileSync(path.join(consoleRoot, 'src', name), 'utf-8')

test('catalogue_views_receive_data_and_never_fetch_it', () => {
  for (const name of ['CatalogueFinder.mts', 'CatalogueOperation.mts']) {
    const code = source(name).replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, '')
    assert.doesNotMatch(
      code,
      /from\s+['"]\.\/service\.mts['"]|\bfetch\s*\(/,
      `${name} reaches the network; a failed catalogue must be decided before a view renders`
    )
  }

  const app = source('App.vue')
  assert.match(app, /<CatalogueFinder\s+:catalog="ready\.catalog"/, 'App does not pass the completed catalogue to the finder')
  assert.match(app, /<CatalogueOperation\s+:catalog="ready\.catalog"/, 'App does not pass the completed catalogue to operation detail')
})

test('the_exchange_owns_no_copied_explorer_directory', () => {
  const app = source('App.vue')
  assert.doesNotMatch(app, /\.\/components\//, 'App still imports the copied flux-connectors explorer')
  assert.doesNotMatch(
    source('catalog.mts'),
    /CoreCatalog|InboundEvent|FluxSource|PATH_RESOLVER/,
    'the local catalogue contract still carries documentation-only surface area'
  )
})

test('catalogue_styles_name_no_colour_and_every_token_exists', () => {
  const rules = source('catalogue.css').replace(/\/\*[\s\S]*?\*\//g, '')
  const literals = [...rules.matchAll(/#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g)].map((match) => match[0])
  assert.deepEqual(literals, [], 'catalogue.css names a colour instead of using the shared token layer')

  const defined = new Set()
  for (const sheet of ['tokens.css', 'app.css']) {
    for (const match of source(sheet).matchAll(/(--[A-Za-z0-9-]+)\s*:/g)) defined.add(match[1])
  }
  const read = [...rules.matchAll(/var\(\s*(--[A-Za-z0-9-]+)/g)].map((match) => match[1])
  assert.ok(read.length > 0, 'catalogue.css reads no design token; this test would pass vacuously')
  assert.deepEqual(
    [...new Set(read)].filter((name) => !defined.has(name)),
    [],
    'catalogue.css reads a custom property this console defines nowhere'
  )
})
