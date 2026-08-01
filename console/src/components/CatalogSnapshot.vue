<script setup lang="ts">
import { computed, inject } from 'vue'
import {
  PATH_RESOLVER,
  allOperations,
  identityPath,
  type Catalog,
  type PathResolver,
} from '../catalog.mts'

const props = defineProps<{ catalog: Catalog }>()

const resolvePath = inject<PathResolver>(PATH_RESOLVER, identityPath)
const operationCount = computed(() => allOperations(props.catalog).length)
const coreCount = computed(() =>
  props.catalog.core
    ? props.catalog.core.operations.length +
      props.catalog.core.nodes.length +
      props.catalog.core.capabilities.length
    : 0
)

function availability(provider: Catalog['providers'][number]): string {
  const live = provider.operations.filter((operation) => operation.status.works).length
  if (live === 0) return 'Not live yet'
  if (live === provider.operation_count) return 'Live'
  return `${live} operations live`
}
</script>

<template>
  <section class="snapshot">
    <p class="snapshot__count">
      <strong>{{ catalog.providers.length }}</strong> connectors ·
      <strong>{{ operationCount }}</strong> connector operations
      <template v-if="catalog.core">
        · <strong>{{ coreCount }}</strong> Flux core entries
      </template>
    </p>
    <ul class="snapshot__providers">
      <li v-for="provider in catalog.providers" :key="provider.id">
        <a :href="resolvePath(`/explorer#${provider.id}`)">{{ provider.vendor }}</a>
        <span>{{ provider.operation_count }} operations</span>
        <span class="snapshot__status">{{ availability(provider) }}</span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.snapshot {
  margin: 20px 0 32px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 12px;
  overflow: hidden;
}

.snapshot__count {
  margin: 0;
  padding: 14px 18px;
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-2);
}

.snapshot__providers {
  margin: 0;
  padding: 0;
  list-style: none;
}

.snapshot__providers li {
  display: grid;
  grid-template-columns: minmax(120px, 1fr) auto auto;
  gap: 12px;
  align-items: center;
  padding: 12px 18px;
}

.snapshot__providers li + li {
  border-top: 1px solid var(--vp-c-divider);
}

.snapshot__providers span {
  color: var(--vp-c-text-2);
  font-size: 13px;
}

.snapshot__status {
  border-radius: 12px;
  padding: 2px 9px;
  background: var(--vp-c-warning-soft);
  color: var(--vp-c-warning-1) !important;
  font-weight: 600;
}

@media (max-width: 520px) {
  .snapshot__providers li {
    grid-template-columns: 1fr auto;
  }

  .snapshot__status {
    grid-column: 1 / -1;
    justify-self: start;
  }
}
</style>
