<script setup lang="ts">
// The explorer: every provider, every operation, and an honest account of what stands in the way.
//
// The headline count is the number of operations that own a defect. It is **not** "N of 25 working":
// `works` is false for all 25 today because the auth seam has not landed in flux, and a site that
// made 20 operations working exactly as designed look broken would be as dishonest as one that hid
// the five real gaps.
//
// So the presentation follows `scope`, which the emitter puts on every issue for this reason:
//
//   catalog  → a banner here, above everything
//   provider → a banner on that provider's card
//   operation → a badge and the reason, on the operation, wherever it appears

import { computed } from 'vue'
import {
  allOperations,
  catalogIssues,
  defectCount,
  type Catalog,
} from '../catalog.mts'
import IssueNotice from './IssueNotice.vue'
import CoreExplorer from './CoreExplorer.vue'
import OperationList from './OperationList.vue'
import ProviderCard from './ProviderCard.vue'

const props = defineProps<{ catalog: Catalog }>()

const operations = computed(() => allOperations(props.catalog))
const defects = computed(() => defectCount(operations.value))
const wide = computed(() => catalogIssues(props.catalog))
const coreEntries = computed(() =>
  props.catalog.core
    ? props.catalog.core.operations.length +
      props.catalog.core.nodes.length +
      props.catalog.core.capabilities.length
    : 0
)
const planned = computed(
  () => props.catalog.core?.capabilities.filter((entry) => entry.availability === 'planned').length ?? 0
)
</script>

<template>
  <p class="summary" :data-defect-count="defects">
    <strong>{{ catalog.providers.length }}</strong> connectors ·
    <strong>{{ operations.length }}</strong> connector operations ·
    <template v-if="catalog.core">
      <strong>{{ coreEntries }}</strong> Flux core entries ·
      <strong>{{ planned }}</strong> planned network capabilities ·
    </template>
    <strong>{{ defects }}</strong> with an operation-specific limitation.
    Browse Flux's built-ins or choose a connector below.
  </p>

  <IssueNotice
    title="Catalogue-wide availability limitation"
    tone="inherited"
    banner="catalog"
    :issues="wide"
  />

  <template v-if="catalog.core">
    <h2 id="core">Flux core</h2>
    <CoreExplorer :core="catalog.core" />
  </template>

  <h2 id="providers">Connectors</h2>
  <div class="providers">
    <ProviderCard
      v-for="provider in catalog.providers"
      :key="provider.id"
      :provider="provider"
    />
  </div>

  <h2 id="operations">Operations</h2>
  <OperationList :providers="catalog.providers" />
</template>

<style scoped>
.summary {
  font-size: 14px;
  color: var(--vp-c-text-2);
}

/*
 * 240px, down from the 320px this grid carried when the catalogue held six connectors. `auto-fit`
 * fits `floor((width + gap) / (min + gap))` tracks, so on the 1025px the explorer now gets, 320px
 * bought three columns and a fourth needs a minimum of 244px or less.
 *
 * 244px used to be below what a card could render: the header's 274px min-content put a 314px floor
 * under the card, and twelve of sixteen badges escaped their border at 244px. That floor is gone —
 * `.card__head` wraps now — so the grid is free to use the width. 240px yields four 244px columns at
 * the full layout width, three from about 1180px, two from 768px and one on a phone.
 */
.providers {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 16px;
  margin: 16px 0;
}
</style>
