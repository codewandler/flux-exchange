<script setup lang="ts">
// One value from a tool contract, as a chip.
//
// The tone is derived from the value, not passed in — so `risk: high` cannot be rendered calm at one
// call site and alarming at another. Anything the map does not recognise stays neutral rather than
// guessing, because a wrong colour on a safety field is worse than no colour: an unrecognised risk
// level painted green would read as an assurance nobody made.

import { computed } from 'vue'

const props = defineProps<{ value: string }>()

/** Values that carry a warning, keyed by the vocabulary the tool contract actually uses. */
const ALARMING = ['high', 'destructive', 'non_idempotent']
const CAUTIONARY = ['medium', 'conditional']
const REASSURING = ['low', 'none', 'idempotent']

const tone = computed(() => {
  const value = props.value.toLowerCase()
  if (ALARMING.includes(value)) return 'alarming'
  if (CAUTIONARY.includes(value)) return 'cautionary'
  if (REASSURING.includes(value)) return 'reassuring'
  return 'neutral'
})
</script>

<template>
  <span class="chip" :class="`chip--${tone}`">{{ value }}</span>
</template>

<style scoped>
.chip {
  display: inline-block;
  border-radius: 10px;
  padding: 1px 10px;
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  font-weight: 600;
  line-height: 20px;
  white-space: nowrap;
}

.chip--alarming {
  background-color: var(--vp-c-danger-soft);
  color: var(--vp-c-danger-1);
}

.chip--cautionary {
  background-color: var(--vp-c-warning-soft);
  color: var(--vp-c-warning-1);
}

.chip--reassuring {
  background-color: var(--vp-c-success-soft);
  color: var(--vp-c-success-1);
}

.chip--neutral {
  background-color: var(--vp-c-default-soft);
  color: var(--vp-c-text-2);
}
</style>
