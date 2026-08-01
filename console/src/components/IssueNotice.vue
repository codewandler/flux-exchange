<script setup lang="ts">
// One block of published conditions, rendered from the catalogue's own text.
//
// Used at all three scopes. The tone is the only thing that differs, and it is deliberate: a defect
// the operation owns is a warning about *this* operation, while a catalogue- or provider-wide
// condition is context — stated in full, once, rather than repeated as 25 individual failures.
//
// C-206 added a third tone, `clear`, for a condition that is not a reason to fail at all: a vendor
// that requires no credential is a *fact*, and rendering it in the same red as a defect would tell a
// visitor to worry about the one thing here that needs nothing from them. A `Note` is an `Issue`
// without the parameter list, so both render through this one block rather than through two that
// would drift apart.

import type { Issue, Note } from '../catalog.mts'

defineProps<{
  title: string
  issues: (Issue | Note)[]
  tone: 'defect' | 'inherited' | 'clear'
  /** Marks this block as a banner over a whole set rather than a note about one operation. */
  banner?: 'catalog' | 'provider'
  /** The provider a banner covers, when it covers one. */
  provider?: string
}>()

/** The parameters a condition implicates — a note has none, and never grows an empty list to fake one. */
function params(issue: Issue | Note): string[] {
  return 'params' in issue ? issue.params : []
}
</script>

<template>
  <section
    v-if="issues.length"
    class="notice"
    :class="`notice--${tone}`"
    :data-banner="banner"
    :data-provider="provider"
  >
    <h4 class="notice__title">{{ title }}</h4>
    <ul class="notice__list">
      <li v-for="issue in issues" :key="issue.code" class="notice__item" :data-code="issue.code">
        <p class="notice__summary">{{ issue.summary }}</p>
        <p v-if="params(issue).length" class="notice__meta">
          <span class="notice__params">
            Affected parameters:
            <code v-for="name in params(issue)" :key="name">{{ name }}</code>
          </span>
        </p>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.notice {
  border: 1px solid transparent;
  border-radius: 8px;
  padding: 12px 16px;
  margin: 16px 0;
}

.notice--defect {
  background-color: var(--vp-c-danger-soft);
  border-color: var(--vp-c-danger-1);
}

.notice--inherited {
  background-color: var(--vp-c-default-soft);
  border-color: var(--vp-c-divider);
}

.notice--clear {
  background-color: var(--vp-c-tip-soft);
  border-color: var(--vp-c-tip-1);
}

.notice--clear .notice__title {
  color: var(--vp-c-tip-1);
}

.notice__title {
  margin: 0 0 8px;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0.02em;
}

.notice--defect .notice__title {
  color: var(--vp-c-danger-1);
}

.notice__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.notice__item + .notice__item {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--vp-c-divider);
}

.notice__summary {
  margin: 0;
  font-size: 14px;
  line-height: 1.6;
}

.notice__meta {
  margin: 6px 0 0;
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.notice__params code {
  font-size: 11px;
}
</style>
