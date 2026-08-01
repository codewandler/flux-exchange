<script setup lang="ts">
// One provider: vendor, operation count, auth scheme, hosts, and a status line that does not
// flatter.
//
// The status line counts the operations that own a defect, not the ones that "work" — see
// `data/catalog.mts`. Whatever holds the whole provider back is stated once, as a banner on the
// card, because repeating it on each of its operations would say nothing about any of them.
//
// The services block appears only for a connector that publishes services of its own. A connector
// that addresses a single surface publishes only the reserved service, which every address elides,
// so its card is exactly what it was before services existed — a row naming a service the address
// does not contain would be noise on most of the grid and wrong on all of it.

import { computed } from 'vue'
import {
  defectCount,
  hasInboundSurface,
  namedServices,
  providerAddress,
  providerIssues,
  serviceApiVersion,
  type Provider,
} from '../catalog.mts'
import InboundSurface from './InboundSurface.vue'
import IssueNotice from './IssueNotice.vue'

const props = defineProps<{ provider: Provider }>()

const defects = computed(() => defectCount(props.provider.operations))
const issues = computed(() => providerIssues(props.provider))
const services = computed(() => namedServices(props.provider))
const address = computed(() => providerAddress(props.provider))

/**
 * The provider's headline status, derived so it flips on its own.
 *
 * Today every operation of every provider has `works: false`, so every card reads "not live yet" —
 * which is the truth, and is the badge refusing to flatter. Close the auth seam and these cards
 * change with the next build, with no edit here. What the badge deliberately does not do is count
 * "0 of 9 working": that number is the same for every provider and tells a visitor nothing about
 * which one to pick.
 */
const live = computed(() => props.provider.operations.filter((op) => op.status.works).length)

const headline = computed(() => {
  if (live.value === 0) return { label: 'Not live yet', tone: 'warn' }
  if (live.value < props.provider.operation_count) {
    return { label: `${live.value} operations live`, tone: 'warn' }
  }
  return { label: 'Live', tone: 'ok' }
})
</script>

<template>
  <section :id="provider.id" class="card" :data-provider-defects="defects">
    <header class="card__head">
      <h3 class="card__vendor">{{ provider.vendor }}</h3>
      <code class="card__id">{{ provider.id }}</code>
      <span class="card__badge" :class="`card__badge--${headline.tone}`">
        {{ headline.label }}
      </span>
    </header>

    <p class="card__desc">{{ provider.description }}</p>

    <dl class="card__facts">
      <div>
        <dt>Operations</dt>
        <dd>{{ provider.operation_count }}</dd>
      </div>
      <div>
        <dt>Auth</dt>
        <dd v-if="provider.auth.schemes.length">{{ provider.auth.schemes.join(', ') }}</dd>
        <dd v-else class="card__warn">not configured</dd>
      </div>
      <div>
        <dt>Base URL</dt>
        <dd><code>{{ provider.base_url }}</code></dd>
      </div>
      <div>
        <dt>Hosts</dt>
        <dd class="card__hosts">
          <code v-for="host in provider.hosts" :key="host">{{ host }}</code>
        </dd>
      </div>
      <div v-if="address">
        <dt>Address</dt>
        <dd><code>{{ address }}</code></dd>
      </div>
      <div>
        <dt>Operation-specific issues</dt>
        <dd :class="defects ? 'card__warn' : 'card__ok'">
          {{ defects }} of {{ provider.operation_count }} operations
        </dd>
      </div>
    </dl>

    <div v-if="services.length" class="card__services">
      <h4 class="card__services-title">Services</h4>
      <ul class="services">
        <li
          v-for="service in services"
          :key="service.name"
          class="service"
          :data-service-of="provider.id"
          :data-service="service.name"
        >
          <code class="service__name">{{ service.name }}</code>
          <span class="service__count">
            {{ service.operation_count }} operation{{ service.operation_count === 1 ? '' : 's' }}
          </span>
          <span v-if="serviceApiVersion(provider, service)" class="service__version">
            {{ serviceApiVersion(provider, service) }}
          </span>
          <code v-if="service.gid" class="service__gid">{{ service.gid }}</code>
        </li>
      </ul>
    </div>

    <!-- Only for a connector that describes one: a heading over an empty list would tell a visitor
         that sixteen connectors have an inbound surface they have not filled in, when in fact they
         declare none. -->
    <InboundSurface v-if="hasInboundSurface(provider)" :provider="provider" />

    <IssueNotice
      title="Connector-wide availability limitation"
      tone="inherited"
      banner="provider"
      :provider="provider.id"
      :issues="issues"
    />
  </section>
</template>

<style scoped>
.card {
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  padding: 16px 20px;
}

/*
 * The header is what set the floor on card width, and therefore on how many columns the grid could
 * have. Vendor name, id and status badge sat on one unwrappable flex line whose min-content came to
 * 274px, so a card could not go below 314px and the grid could not fit a fourth track in 1025px.
 * Letting it wrap removes that floor: the badge drops to its own line on a narrow card instead of
 * escaping the border, and the header is unchanged on a wide one, where it still fits on one line.
 */
.card__head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
}

.card__vendor {
  margin: 0;
  font-size: 18px;
  line-height: 1.4;
  border: 0;
  padding: 0;
}

.card__id {
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.card__badge {
  margin-left: auto;
  border-radius: 10px;
  padding: 1px 10px;
  font-size: 12px;
  font-weight: 600;
  line-height: 20px;
  white-space: nowrap;
}

.card__badge--warn {
  background-color: var(--vp-c-warning-soft);
  color: var(--vp-c-warning-1);
}

.card__badge--ok {
  background-color: var(--vp-c-tip-soft);
  color: var(--vp-c-tip-1);
}

.card__desc {
  margin: 6px 0 0;
  font-size: 14px;
  color: var(--vp-c-text-2);
}

.card__facts {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 8px 16px;
  margin: 12px 0 0;
}

.card__facts dt {
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--vp-c-text-3);
}

.card__facts dd {
  margin: 2px 0 0;
  font-size: 14px;
}

.card__facts code {
  font-size: 12px;
  /* A single value with no break opportunity of its own must break rather than push its grid track
     wider than the card. This is the *within a value* half; `.card__hosts` below is the *between
     values* half. Its reach is exactly this `<dl>` — so it covers the Address fact, and it does NOT
     cover a service's gid, which lives in `.card__services` below. That one needs no cover today:
     `.service` already wraps between its chips, and a review measured no spill from it at 1280,
     1366 or 1440. If a long gid ever does escape, this is the rule to mirror there. */
  overflow-wrap: anywhere;
}

/* The hosts cell renders one `<code>` per host with **no whitespace between them**, so the markup
   offers no soft-wrap opportunity: two adjacent hostnames concatenate into one unbreakable ~298px
   inline box. The 609px single-column card the 688px page forced absorbed it; the 424.5px
   two-column track C-100 introduced does not, and it escaped the page — 29px of horizontal overflow
   at 1280 and 8px at 1366, against 0 at the merge base.
   Wrapping restores the break the missing whitespace never provided, and the gap replaces the
   separation. Scoped to this one cell rather than to `.card__facts dd` so the text-only facts keep
   their block layout. */
.card__hosts {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.card__services {
  margin: 12px 0 0;
}

.card__services-title {
  margin: 0;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--vp-c-text-3);
}

.services {
  display: grid;
  gap: 4px;
  margin: 4px 0 0;
  padding: 0;
  list-style: none;
}

.service {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 4px 10px;
  margin: 0;
  font-size: 13px;
  color: var(--vp-c-text-2);
}

.service__name {
  font-size: 12px;
  color: var(--vp-c-text-1);
}

.service__version {
  border-radius: 10px;
  padding: 0 8px;
  font-size: 11px;
  line-height: 18px;
  background-color: var(--vp-c-default-soft);
}

.service__gid {
  font-size: 12px;
}

.card__warn {
  color: var(--vp-c-danger-1);
  font-weight: 600;
}

.card__ok {
  color: var(--vp-c-text-1);
}
</style>
