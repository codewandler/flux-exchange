<script setup lang="ts">
import { computed, inject } from 'vue'
import {
  PATH_RESOLVER,
  allCoreEntries,
  coreEntryById,
  coreEntryHref,
  identityPath,
  type Catalog,
  type CoreEntry,
  type PathResolver,
} from '../catalog.mts'
import SchemaBlock from './SchemaBlock.vue'
import SpecChip from './SpecChip.vue'

const props = defineProps<{ catalog: Catalog; kind: string; name: string }>()

const resolvePath = inject<PathResolver>(PATH_RESOLVER, identityPath)

const entry = computed<CoreEntry | undefined>(() => {
  const singular = props.kind === 'capabilities' ? 'capability' : props.kind.replace(/s$/, '')
  return props.catalog.core
    ? allCoreEntries(props.catalog.core).find(
        (candidate) => candidate.kind === singular && candidate.name === props.name
      )
    : undefined
})

function operationHref(id: string): string | undefined {
  if (!props.catalog.core) return undefined
  const operation = coreEntryById(props.catalog.core, id)
  return operation ? coreEntryHref(operation) : undefined
}
</script>

<template>
  <p v-if="!entry" class="missing">
    No Flux core <code>{{ kind }}</code> entry named <code>{{ name }}</code> is published.
  </p>

  <article
    v-else
    :data-core-kind="entry.kind"
    :data-core-name="entry.name"
    :data-availability="entry.availability"
    :data-callable="entry.kind === 'capability' ? entry.callable : true"
  >
    <p class="lede">{{ entry.description }}</p>
    <p class="chips">
      <span class="chip">{{ entry.kind }}</span>
      <span class="chip">{{ entry.category.join(' / ') }}</span>
      <span class="chip" :class="`chip--${entry.availability}`">{{ entry.availability }}</span>
      <span v-if="entry.kind === 'capability'" class="chip">
        {{ entry.callable ? 'callable' : 'not callable' }}
      </span>
    </p>

    <div v-if="entry.availability === 'planned'" class="planned">
      This capability is a published design target, not an operation available to a Flux program.
    </div>

    <template v-if="entry.kind === 'operation'">
      <h2>Tool contract</h2>
      <dl class="facts facts--chips">
        <dt>Risk</dt>
        <dd><SpecChip :value="entry.tool_spec.risk" /></dd>
        <dt>Idempotency</dt>
        <dd><SpecChip :value="entry.tool_spec.idempotency" /></dd>
        <dt>Effects</dt>
        <dd>
          <SpecChip v-if="!entry.tool_spec.effects.length" value="none" />
          <SpecChip v-for="effect in entry.tool_spec.effects" :key="effect" :value="effect" />
        </dd>
        <dt>Access</dt>
        <dd>
          <SpecChip v-if="!entry.tool_spec.access.length" value="none" />
          <SpecChip v-for="kind in entry.tool_spec.access" :key="kind" :value="kind" />
        </dd>
        <template v-if="entry.tool_spec.group">
          <dt>Group</dt>
          <dd><SpecChip :value="entry.tool_spec.group" /></dd>
        </template>
      </dl>
      <h2>Input schema</h2>
      <SchemaBlock :schema="entry.tool_spec.input_schema" />
    </template>

    <template v-else-if="entry.kind === 'node'">
      <h2>Flux AST schema</h2>
      <p>
        This node is defined by the anchored schema
        <a :href="entry.schema_ref"><code>{{ entry.schema_ref }}</code></a>.
      </p>
    </template>

    <template v-else>
      <h2>Operations</h2>
      <p v-if="!entry.operation_ids.length">
        No callable operation is published for this capability.
      </p>
      <ul v-else>
        <li v-for="id in entry.operation_ids" :key="id">
          <a v-if="operationHref(id)" :href="resolvePath(operationHref(id)!)"><code>{{ id }}</code></a>
          <a v-else :href="id"><code>{{ id }}</code></a>
        </li>
      </ul>
    </template>

    <h2>Specification</h2>
    <p>
      Canonical JSON: <a :href="entry.$id"><code>{{ entry.$id }}</code></a>
    </p>
    <p>
      Validated by <a :href="entry.$schema"><code>{{ entry.$schema }}</code></a>.
    </p>
  </article>
</template>

<style scoped>
.lede {
  color: var(--vp-c-text-2);
  font-size: 16px;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.chip {
  border-radius: 10px;
  background: var(--vp-c-default-soft);
  color: var(--vp-c-text-2);
  font-size: 12px;
  padding: 2px 9px;
}

.chip--available {
  background: var(--vp-c-green-soft);
  color: var(--vp-c-green-1);
}

.chip--planned {
  background: var(--vp-c-warning-soft);
  color: var(--vp-c-warning-1);
}

.planned {
  border-left: 4px solid var(--vp-c-warning-1);
  background: var(--vp-c-warning-soft);
  padding: 12px 16px;
  margin: 16px 0;
}

.facts {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 7px 18px;
}

.facts dt {
  color: var(--vp-c-text-2);
}

.facts dd {
  margin: 0;
}

/* A value row holds one chip or several (effects and access are lists), so it wraps rather than
   widening the grid track and pushing the layout — the min-content failure C-100 spent a round on. */
.facts--chips {
  align-items: center;
  gap: 8px 18px;
  min-width: 0;
}

.facts--chips dd {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
}

pre {
  overflow-x: auto;
}

.missing {
  color: var(--vp-c-danger-1);
}
</style>
