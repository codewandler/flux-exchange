<script setup lang="ts">
// The console's root: the one place that knows where data comes from, and the one place that answers
// the components' path question.
//
// Every component under `src/components/` takes what it renders as a prop. That is not an accident of
// how they were written — it is the property that let them be lifted out of the flux-connectors docs
// site without a rewrite, and `test/components.test.mjs` holds it. So the catalogue is fetched *here*
// and passed down, and no component under `src/components/` knows a network exists.
//
// The catalogue has three states and each has its own view, because collapsing any two of them lies
// to the reader:
//
//   loading  the request is in flight — say so, and name what is being read
//   failed   nothing was read — `CatalogueFailure.mts`, which names the endpoint
//   ready    a catalogue, however small, including one with no connectors in it
//
// The third and second are the pair that matters. A service that is not running and a service with
// nothing in it must never render the same page.

import { computed, onMounted, provide, shallowRef } from 'vue'
import { PATH_RESOLVER } from './catalog.mts'
import { fragmentPath, useRoute } from './routing'
import { CONNECTORS_ENDPOINT, loadCatalogue, type CatalogueState } from './service.mts'
import { isDark, toggleTheme } from './theme'

import CatalogueFailure from './CatalogueFailure.mts'
import OperationFacts from './OperationFacts.mts'
import CatalogExplorer from './components/CatalogExplorer.vue'
import CatalogSnapshot from './components/CatalogSnapshot.vue'
import OperationDetail from './components/OperationDetail.vue'

// The components' one port. Their default is identity, which would be wrong here: this console is a
// single static document, so `/operations/<id>` as a bare href would 404. See `src/routing.ts`.
provide(PATH_RESOLVER, fragmentPath)

// `shallowRef` rather than `ref`: the catalogue is replaced wholesale and never edited, so making
// every operation in it deeply reactive would buy nothing and cost a walk of the entire document.
const state = shallowRef<CatalogueState>({ status: 'loading' })

onMounted(async () => {
  state.value = await loadCatalogue()
})

// Narrowed once here rather than in the template, so the type checker can see it inside `v-if`.
const ready = computed(() => (state.value.status === 'ready' ? state.value : null))
const failure = computed(() => (state.value.status === 'failed' ? state.value.failure : null))

const route = useRoute()

/** The served facts about the operation on screen, when the route names one this catalogue has. */
const facts = computed(() =>
  ready.value && route.value.name === 'operation' ? (ready.value.served[route.value.id] ?? null) : null
)

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
      <!-- Named, so a request that never comes back is visibly a request and not a blank page. -->
      <p v-if="state.status === 'loading'" class="console__loading">
        Reading the catalogue from <code>{{ CONNECTORS_ENDPOINT }}</code
        >…
      </p>

      <CatalogueFailure v-else-if="failure" :failure="failure" />

      <template v-else-if="ready">
        <template v-if="route.name === 'explorer'">
          <h1>Catalogue</h1>
          <CatalogSnapshot :catalog="ready.catalog" />
          <CatalogExplorer :catalog="ready.catalog" />
        </template>

        <template v-else-if="route.name === 'operation'">
          <h1><code>{{ route.id }}</code></h1>
          <OperationDetail :catalog="ready.catalog" :id="route.id" />
          <OperationFacts v-if="facts" :operation="facts" />
        </template>

        <!--
          The served catalogue carries no Flux core entries — `core` is `null` — so a `/core/…` link
          resolves to a statement that this source publishes none, rather than to a blank page or to
          the explorer. A link that no longer resolves should say so.
        -->
        <template v-else-if="route.name === 'core'">
          <h1><code>{{ route.entry }}</code></h1>
          <p>
            The flux-exchange catalogue publishes connectors and their operations. It publishes no
            Flux core entries, so nothing here answers to
            <code>/core/{{ route.kind }}/{{ route.entry }}</code
            >. <a :href="fragmentPath('/')">Back to the catalogue</a>.
          </p>
        </template>

        <template v-else>
          <h1>Not found</h1>
          <p>
            Nothing in this console answers to <code>{{ route.path }}</code
            >. <a :href="fragmentPath('/')">Back to the catalogue</a>.
          </p>
        </template>
      </template>
    </main>

    <footer class="console__foot">
      <p>
        Read from <code>{{ CONNECTORS_ENDPOINT }}</code> ·
        <a href="https://github.com/codewandler/flux-exchange">codewandler/flux-exchange</a>
      </p>
    </footer>
  </div>
</template>

<style scoped>
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

.console__loading {
  margin: 32px 0;
  color: var(--vp-c-text-2);
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
