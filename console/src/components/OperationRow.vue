<script setup lang="ts">
// One row of the operation list, and the deep link to that operation's page.
//
// `data-defect` is the row's honest summary: `own` when this operation has a problem nothing else in
// the catalogue has, `none` when the only things standing in its way are catalogue- or
// provider-wide. A row never claims an operation works — nothing does yet — and never marks one
// broken for a condition it merely inherits.

import { computed, inject } from 'vue'
import {
  PATH_RESOLVER,
  identityPath,
  operationHref,
  ownIssues,
  ownsDefect,
  type Operation,
  type PathResolver,
} from '../catalog.mts'
import StatusBadge from './StatusBadge.vue'

const props = defineProps<{
  operation: Operation
  vendor: string
  /**
   * The service this operation belongs to, when its connector addresses more than one surface and
   * the vendor alone therefore does not say where the operation lives. `null` otherwise, and a null
   * service renders no attribute and no text rather than a placeholder.
   */
  service?: string | null
}>()

const resolvePath = inject<PathResolver>(PATH_RESOLVER, identityPath)

const own = computed(() => ownIssues(props.operation))
</script>

<template>
  <li
    class="row"
    :class="{ 'row--defect': ownsDefect(operation) }"
    :data-operation="operation.id"
    :data-service="service ?? undefined"
    :data-defect="ownsDefect(operation) ? 'own' : 'none'"
  >
    <div class="row__head">
      <a class="row__id" :href="resolvePath(operationHref(operation))">{{ operation.id }}</a>
      <StatusBadge :operation="operation" />
    </div>

    <p class="row__desc">{{ operation.description }}</p>

    <p class="row__meta">
      <span class="row__vendor">{{ vendor }}</span>
      <code v-if="service" class="row__service">{{ service }}</code>
      <span class="row__method">{{ operation.method }}</span>
      <code>{{ operation.path }}</code>
      <span>risk: {{ operation.risk }}</span>
      <span>{{ operation.idempotency }}</span>
    </p>

    <p v-for="issue in own" :key="issue.code" class="row__defect">{{ issue.summary }}</p>
  </li>
</template>

<style scoped>
/*
 * `min-width: 0` is load-bearing, not tidying. The list is a grid, and a grid item's automatic
 * minimum size is `auto` — its min-content — so a row is never allowed to be narrower than its
 * longest unbreakable run. An operation path like `/v0/{baseId}/{tableIdOrName}/{recordId}` has no
 * break opportunity in it, so on a phone every row forced its track wider than the viewport and the
 * whole page scrolled sideways by ~193px. `min-width: 0` lets the track shrink; `overflow-wrap` on
 * the path below is what then makes the path itself break rather than simply overflow the row.
 */
.row {
  min-width: 0;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  padding: 12px 16px;
  margin: 0;
  list-style: none;
}

.row--defect {
  border-color: var(--vp-c-danger-1);
}

.row__head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.row__id {
  font-family: var(--vp-font-family-mono);
  font-size: 15px;
  font-weight: 600;
  color: var(--vp-c-brand-1);
  text-decoration: none;
}

.row__id:hover {
  text-decoration: underline;
}

.row__desc {
  margin: 6px 0 0;
  font-size: 14px;
}

.row__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin: 6px 0 0;
  font-size: 12px;
  color: var(--vp-c-text-2);
}

/* The request path is one token with no break opportunity a browser will take on its own. It is the
   longest thing in the row, so it decides the row's min-content width unless it is allowed to break
   mid-token. Paired with `min-width: 0` on `.row` above. */
.row__meta code {
  overflow-wrap: anywhere;
}

.row__vendor,
.row__method {
  font-weight: 600;
  color: var(--vp-c-text-1);
}

.row__service {
  font-size: 12px;
  color: var(--vp-c-text-1);
}

.row__defect {
  margin: 8px 0 0;
  padding: 8px 12px;
  border-radius: 6px;
  background-color: var(--vp-c-danger-soft);
  color: var(--vp-c-danger-1);
  font-size: 13px;
  line-height: 1.5;
}
</style>
