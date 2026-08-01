<script setup lang="ts">
// The console's root: the one place that knows where data comes from, and the one place that answers
// the components' path question.
//
// Every component under `src/components/` takes what it renders as a prop. That is not an accident of
// how they were written — it is the property that let them be lifted out of the flux-connectors docs
// site without a rewrite, and `test/components.test.mjs` holds it. So the fixture is imported *here*
// and passed down, and the day a real catalogue exists this file is the only one that changes.

import { computed, provide } from 'vue'
import { PATH_RESOLVER } from './catalog.mts'
import { fragmentPath, useRoute } from './routing'
import { fixtureCatalog } from './fixtures/catalog'
import { isDark, toggleTheme } from './theme'

import CatalogExplorer from './components/CatalogExplorer.vue'
import CatalogSnapshot from './components/CatalogSnapshot.vue'
import CoreDetail from './components/CoreDetail.vue'
import OperationDetail from './components/OperationDetail.vue'

// The components' one port. Their default is identity, which would be wrong here: this console is a
// single static document, so `/operations/<id>` as a bare href would 404. See `src/routing.ts`.
provide(PATH_RESOLVER, fragmentPath)

const catalog = fixtureCatalog
const route = useRoute()

const title = computed(() => {
  switch (route.value.name) {
    case 'operation':
      return route.value.id
    case 'core':
      return route.value.entry
    case 'unknown':
      return 'Not found'
    default:
      return 'Catalogue'
  }
})
</script>

<template>
  <!--
    The banner. Deliberately the first thing in the document, deliberately not dismissible, and
    deliberately worded as a statement rather than a caveat: there is no flux-exchange service, no
    API and no generated catalogue. Everything below is `src/fixtures/catalog.ts`. If this console
    ever gains a real source, this element is what has to change with it.
  -->
  <div class="fixture-banner" role="status">
    <strong>Fixture data.</strong>
    No flux-exchange service exists yet. Every connector, operation, credential and event below is
    invented, lives in <code>src/fixtures/catalog.ts</code>, and reaches nothing. Nothing on this page
    was fetched, and nothing on it can be called.
  </div>

  <div class="console">
    <header class="console__head">
      <div>
        <a class="console__brand" :href="fragmentPath('/')">flux-exchange console</a>
        <span class="console__where">{{ title }}</span>
      </div>
      <button
        class="console__theme"
        type="button"
        :aria-pressed="isDark"
        @click="toggleTheme()"
      >
        {{ isDark ? 'Light' : 'Dark' }}
      </button>
    </header>

    <main>
      <template v-if="route.name === 'explorer'">
        <h1>Catalogue</h1>
        <CatalogSnapshot :catalog="catalog" />
        <CatalogExplorer :catalog="catalog" />
      </template>

      <template v-else-if="route.name === 'operation'">
        <h1><code>{{ route.id }}</code></h1>
        <OperationDetail :catalog="catalog" :id="route.id" />
      </template>

      <template v-else-if="route.name === 'core'">
        <h1><code>{{ route.entry }}</code></h1>
        <CoreDetail :catalog="catalog" :kind="route.kind" :name="route.entry" />
      </template>

      <template v-else>
        <h1>Not found</h1>
        <p>
          Nothing in this console answers to <code>{{ route.path }}</code
          >. <a :href="fragmentPath('/')">Back to the catalogue</a>.
        </p>
      </template>
    </main>

    <footer class="console__foot">
      <p>
        <code>{{ catalog.generator }}</code> · schema version {{ catalog.schema_version }} ·
        <a href="https://github.com/codewandler/flux-exchange">codewandler/flux-exchange</a>
      </p>
    </footer>
  </div>
</template>

<style scoped>
.fixture-banner {
  padding: 12px 24px;
  font-size: 14px;
  line-height: 22px;
  color: var(--vp-c-warning-1);
  background-color: var(--vp-c-warning-soft);
  border-bottom: 1px solid var(--vp-c-divider);
}

.fixture-banner code {
  color: inherit;
}

.console {
  max-width: 1104px;
  margin: 0 auto;
  padding: 0 24px 64px;
}

.console__head {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: baseline;
  justify-content: space-between;
  padding: 20px 0;
  border-bottom: 1px solid var(--vp-c-divider);
}

.console__brand {
  font-weight: 600;
  color: var(--vp-c-text-1);
  text-decoration: none;
}

.console__where {
  margin-left: 10px;
  font-size: 13px;
  color: var(--vp-c-text-3);
}

.console__theme {
  padding: 4px 12px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  background-color: var(--vp-c-bg-soft);
  cursor: pointer;
}

.console__foot {
  margin-top: 48px;
  padding-top: 16px;
  border-top: 1px solid var(--vp-c-divider);
  font-size: 12px;
  color: var(--vp-c-text-3);
}

.console__foot code {
  color: inherit;
}
</style>
