<script setup lang="ts">
// The filterable operation list.
//
// Every option in every select is read out of the catalogue — add a provider, a service, a risk tier
// or an idempotency class and the filters grow to cover it with no edit here.
//
// The service filter is the one that depends on another: choosing a connector narrows the services
// to that connector's, because every other connector's service is an option that could not match a
// single row. Choosing a service with no connector chosen stays valid, and is the useful direction —
// it is how a visitor finds one surface of a large vendor without knowing which vendor publishes it.
//
// The last filter is the one that earns its place, and it is deliberately **not** "does it work".
// `works` is false for all 25 operations today because no provider can make a live call yet, so
// filtering on it sorts nothing from nothing. Filtering on whether an operation owns a defect
// separates the five with a problem of their own from the twenty waiting on the same seam as
// everything else.
//
// Filtering narrows a list that is already fully rendered, so with JavaScript switched off every
// operation is still on the page, in the catalogue's own order — the state below only ever removes
// rows and reorders them, and the server renders it unset.
//
// That state lives in the query string (C-102), because a *view* is worth sharing and had no address
// at all: "every destructive operation of one connector" could not be sent to anyone. The pair that
// does it is pure and lives in `data/catalog.mts`; what is left here is the wiring, and the wiring
// has two rules.
//
//   - **Read the URL on mount, not during setup.** There is no `location` while the page is being
//     rendered, and reading one during setup would also make the server's markup disagree with the
//     client's first render. So the list renders unfiltered and unsorted, and the URL is applied once
//     the browser has it.
//   - **Replace, never push.** A pushed entry per change means the back button walks back through
//     every keystroke of a search instead of leaving the explorer. VitePress ships its own router
//     (there is no Vue Router here) and it offers `go`, which pushes; the honest equivalent is
//     `history.replaceState`, carrying the router's own state object through untouched so the two
//     stay in agreement.

import { computed, onMounted, ref, watch } from 'vue'
import {
  SORTS,
  compareOperations,
  decodeView,
  emptyView,
  encodeView,
  facet,
  narrowView,
  operationService,
  ownsDefect,
  serviceFacet,
  type Operation,
  type Provider,
} from '../catalog.mts'
import OperationRow from './OperationRow.vue'

const props = defineProps<{ providers: Provider[] }>()

const ANY = ''

/**
 * Whether there is a browser to write a URL into.
 *
 * VitePress exports this as `inBrowser` and it is exactly this expression (C-142). Importing it was
 * the only thing tying this component to the framework, for a guard the component can state itself —
 * and it has to be a guard rather than an `onMounted`, because the watcher below fires during the
 * server render too.
 */
const inBrowser = typeof window !== 'undefined'

/** What each sort is called on the control. The values are the URL's vocabulary; these are prose. */
const SORT_LABELS: Record<string, string> = {
  catalog: 'Catalogue order',
  id: 'Operation id',
  risk: 'Risk',
}

const view = ref(emptyView())

/** Every operation with the vendor that owns it, flattened in catalogue order. */
const entries = computed(() =>
  props.providers.flatMap((owner) =>
    owner.operations.map((operation) => ({ operation, owner }))
  )
)

const operations = computed<Operation[]>(() => entries.value.map((entry) => entry.operation))

const risks = computed(() => facet(operations.value, (operation) => operation.risk))
const idempotencies = computed(() =>
  facet(operations.value, (operation) => operation.idempotency)
)

/**
 * The service options, narrowed to the chosen connector.
 *
 * The narrowing leaves nothing to choose for a connector that addresses a single surface, so the
 * control is disabled rather than removed: a filter that disappeared under the cursor would move
 * every control beside it.
 */
const services = computed(() => serviceFacet(props.providers, view.value.provider))

const matching = computed(() =>
  entries.value.filter(({ operation, owner }) => {
    const { query, provider, service, risk, idempotency, defect } = view.value
    if (provider !== ANY && owner.id !== provider) return false
    if (service !== ANY && operation.service !== service) return false
    if (risk !== ANY && operation.risk !== risk) return false
    if (idempotency !== ANY && operation.idempotency !== idempotency) return false
    if (defect === 'own' && !ownsDefect(operation)) return false
    if (defect === 'none' && ownsDefect(operation)) return false

    const needle = query.trim().toLowerCase()
    if (!needle) return true
    return (
      operation.id.toLowerCase().includes(needle) ||
      operation.description.toLowerCase().includes(needle) ||
      operation.path.toLowerCase().includes(needle)
    )
  })
)

// Sorting is over the whole row, and the comparator is over the operation: the vendor travels with
// the operation only so the row can name it, and never decides where the row goes.
const shown = computed(() => {
  const compare = compareOperations(view.value.sort)
  return [...matching.value].sort((a, b) => compare(a.operation, b.operation))
})

/**
 * State the view in the address bar, replacing the current entry.
 *
 * `history.state` is carried through rather than cleared, because it is VitePress's router's and not
 * ours — it holds the scroll position the router restores.
 */
function publish(search: string) {
  if (!inBrowser) return
  const url = new URL(window.location.href)
  url.search = search
  window.history.replaceState(window.history.state, '', url)
}

// One watcher does both halves. A value the catalogue no longer offers is dropped first — a chosen
// service that the chosen connector does not publish would filter every operation away and read as
// an empty catalogue — and the write back re-enters here with the narrowed view, which is already
// narrow and so falls through to the URL. Comparing the encodings rather than the objects is what
// makes that terminate: the encoding is canonical, so `narrowView` reaches a fixed point in one step.
watch(
  view,
  (current) => {
    const narrowed = narrowView(current, props.providers)
    const search = encodeView(narrowed)
    if (search !== encodeView(current)) {
      view.value = narrowed
      return
    }
    publish(search)
  },
  { deep: true }
)

// Restoring a shared link, once — and only in the browser, so the server still renders every
// operation unfiltered.
onMounted(() => {
  view.value = narrowView(decodeView(window.location.search), props.providers)
})

function reset() {
  view.value = emptyView()
}
</script>

<template>
  <div class="filters">
    <label class="filters__field filters__field--wide">
      <span>Search</span>
      <input v-model="view.query" type="search" placeholder="id, description or path" />
    </label>

    <label class="filters__field">
      <span>Connector</span>
      <select v-model="view.provider">
        <option :value="ANY">Any</option>
        <option v-for="owner in providers" :key="owner.id" :value="owner.id">
          {{ owner.vendor }}
        </option>
      </select>
    </label>

    <label class="filters__field">
      <span>Service</span>
      <select v-model="view.service" :disabled="!services.length">
        <option :value="ANY">Any</option>
        <option v-for="value in services" :key="value" :value="value">{{ value }}</option>
      </select>
    </label>

    <label class="filters__field">
      <span>Risk</span>
      <select v-model="view.risk">
        <option :value="ANY">Any</option>
        <option v-for="value in risks" :key="value" :value="value">{{ value }}</option>
      </select>
    </label>

    <label class="filters__field">
      <span>Idempotency</span>
      <select v-model="view.idempotency">
        <option :value="ANY">Any</option>
        <option v-for="value in idempotencies" :key="value" :value="value">{{ value }}</option>
      </select>
    </label>

    <label class="filters__field">
      <span>Operation issues</span>
      <select v-model="view.defect">
        <option :value="ANY">Any</option>
        <option value="own">Has a known limitation</option>
        <option value="none">No operation-specific issue</option>
      </select>
    </label>

    <label class="filters__field">
      <span>Sort</span>
      <select v-model="view.sort">
        <option v-for="value in SORTS" :key="value" :value="value">{{ SORT_LABELS[value] }}</option>
      </select>
    </label>

    <button class="filters__reset" type="button" @click="reset">Reset</button>
  </div>

  <p class="count">
    Showing <strong>{{ shown.length }}</strong> of {{ operations.length }} operations.
  </p>

  <ul class="list">
    <OperationRow
      v-for="{ operation, owner } in shown"
      :key="operation.id"
      :operation="operation"
      :vendor="owner.vendor"
      :service="operationService(owner, operation)"
    />
  </ul>

  <p v-if="!shown.length" class="count">Nothing matches those filters.</p>
</template>

<style scoped>
.filters {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 12px;
  margin: 16px 0;
}

/*
 * The bar has grown from five controls to eight (a service filter from C-101, a sort from C-102),
 * and it was wrapping to three rows at *every* width, 2560px included. The cause is that a flex
 * item's automatic minimum size is `min-content`, and a `<select>`'s min-content is its **widest
 * option** — "No operation-specific issue" is ~190px on its own. So the controls could never shrink,
 * whatever basis they were given, and eight of them do not fit anywhere.
 *
 * `min-width: 0` releases that floor, which is what lets the bases below actually govern. Flex
 * decides where to break on the *bases*, not on the grown widths, so the bases are sized to leave
 * the Reset button room on the same line: 160 + 6x88 + a ~62px button + 7x12px gaps = 834px, inside
 * both the 1025px the explorer gets at full width and the 865px it gets at 1280. The controls then
 * grow to fill. A truncated `<select>` still reads correctly because its own label sits directly
 * above it and the selected value is short in the common case ("Any").
 */
.filters__field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1 1 88px;
  min-width: 0;
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.filters__field--wide {
  flex: 2 1 160px;
}

.filters__field input,
.filters__field select {
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  padding: 5px 8px;
  font-size: 14px;
  color: var(--vp-c-text-1);
  background-color: var(--vp-c-bg);
  width: 100%;
  /* Same reason as the field above: without this the control refuses to go below its widest
     option and pushes the whole bar to another row. */
  min-width: 0;
}

.filters__reset {
  /* Never grow, never shrink: it is the one control whose width means nothing. */
  flex: 0 0 auto;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 13px;
  color: var(--vp-c-text-2);
}

.count {
  font-size: 13px;
  color: var(--vp-c-text-2);
  margin: 8px 0;
}

.list {
  display: grid;
  gap: 12px;
  padding: 0;
  margin: 0;
  list-style: none;
}
</style>
