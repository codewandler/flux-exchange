<script setup lang="ts">
import { computed, inject, ref } from 'vue'
import {
  PATH_RESOLVER,
  allCoreEntries,
  coreEntryHref,
  identityPath,
  type CoreCatalog,
  type CoreEntry,
  type PathResolver,
} from '../catalog.mts'

const props = defineProps<{ core: CoreCatalog }>()

const resolvePath = inject<PathResolver>(PATH_RESOLVER, identityPath)
const query = ref('')
const kind = ref('')
const availability = ref('')

const entries = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return allCoreEntries(props.core).filter((entry) => {
    if (kind.value && entry.kind !== kind.value) return false
    if (availability.value && entry.availability !== availability.value) return false
    return (
      !needle ||
      entry.name.toLowerCase().includes(needle) ||
      entry.title.toLowerCase().includes(needle) ||
      entry.description.toLowerCase().includes(needle) ||
      entry.category.some((part) => part.toLowerCase().includes(needle))
    )
  })
})

function callable(entry: CoreEntry): boolean {
  return entry.kind !== 'capability' || entry.callable
}
</script>

<template>
  <p class="intro">
    These are Flux-owned operations, language nodes, and network capabilities. They are separate
    from vendor connectors, but share this explorer and publish dereferenceable JSON specifications.
    A no-op is intentionally absent: sequencing needs no placeholder, and returning a value is the
    language <code>return</code> node rather than a synthetic operation.
  </p>

  <div class="filters" role="search" aria-label="Filter Flux core entries">
    <label>
      Search
      <input v-model="query" type="search" placeholder="Name, category, or description" />
    </label>
    <label>
      Kind
      <select v-model="kind">
        <option value="">All kinds</option>
        <option value="operation">Operations</option>
        <option value="node">Language nodes</option>
        <option value="capability">Capabilities</option>
      </select>
    </label>
    <label>
      Availability
      <select v-model="availability">
        <option value="">All states</option>
        <option value="available">Available</option>
        <option value="planned">Planned</option>
      </select>
    </label>
  </div>

  <p class="result"><strong>{{ entries.length }}</strong> entries</p>
  <div class="entries">
    <article
      v-for="entry in entries"
      :key="entry.$id"
      class="entry"
      :data-core-kind="entry.kind"
      :data-core-name="entry.name"
      :data-availability="entry.availability"
      :data-callable="callable(entry)"
    >
      <div class="entry__head">
        <a :href="resolvePath(coreEntryHref(entry))"><code>{{ entry.name }}</code></a>
        <span class="badge">{{ entry.kind }}</span>
        <span class="badge" :class="`badge--${entry.availability}`">
          {{ entry.availability }}
        </span>
      </div>
      <p>{{ entry.description }}</p>
      <p class="entry__meta">
        {{ entry.category.join(' / ') }}
        <template v-if="entry.kind === 'capability' && !entry.callable"> · not callable</template>
      </p>
      <a class="spec" :href="entry.$id">JSON specification</a>
    </article>
  </div>
</template>

<style scoped>
.intro,
.result,
.entry__meta {
  color: var(--vp-c-text-2);
  font-size: 14px;
}

.filters {
  display: grid;
  grid-template-columns: minmax(240px, 2fr) repeat(2, minmax(150px, 1fr));
  gap: 12px;
  margin: 16px 0;
}

label {
  color: var(--vp-c-text-2);
  font-size: 12px;
}

input,
select {
  width: 100%;
  margin-top: 4px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 7px;
  background: var(--vp-c-bg);
  color: var(--vp-c-text-1);
  padding: 8px 10px;
}

.entries {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 12px;
}

.entry {
  border: 1px solid var(--vp-c-divider);
  border-radius: 10px;
  padding: 14px 16px;
}

.entry__head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 7px;
}

.entry p {
  margin: 9px 0;
}

.badge {
  border-radius: 9px;
  background: var(--vp-c-default-soft);
  color: var(--vp-c-text-2);
  font-size: 11px;
  padding: 1px 7px;
}

.badge--available {
  background: var(--vp-c-green-soft);
  color: var(--vp-c-green-1);
}

.badge--planned {
  background: var(--vp-c-warning-soft);
  color: var(--vp-c-warning-1);
}

.spec {
  font-size: 12px;
}

@media (max-width: 700px) {
  .filters {
    grid-template-columns: 1fr;
  }
}
</style>
