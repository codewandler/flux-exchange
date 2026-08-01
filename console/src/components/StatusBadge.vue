<script setup lang="ts">
// The badge an operation carries everywhere it appears.
//
// It never says "works". Nothing in this catalogue can make a live API call yet, so a green tick
// would be a lie — and a red cross on all 25 would be true and worthless. The badge answers the one
// question that varies between operations: does this one have a problem of its own?

import { computed } from 'vue'
import { ownIssues, type Operation } from '../catalog.mts'

const props = defineProps<{ operation: Operation }>()

const own = computed(() => ownIssues(props.operation))
</script>

<template>
  <span v-if="own.length" class="badge badge--defect" :title="own[0].summary">
    Known limitation
    <span v-if="own.length > 1">&times;{{ own.length }}</span>
  </span>
  <span v-else class="badge badge--clear" title="No limitation specific to this operation">
    No operation-specific issue
  </span>
</template>

<style scoped>
.badge {
  display: inline-block;
  border-radius: 10px;
  padding: 1px 10px;
  font-size: 12px;
  font-weight: 600;
  line-height: 20px;
  white-space: nowrap;
}

.badge--defect {
  background-color: var(--vp-c-danger-soft);
  color: var(--vp-c-danger-1);
}

.badge--clear {
  background-color: var(--vp-c-default-soft);
  color: var(--vp-c-text-2);
}
</style>
