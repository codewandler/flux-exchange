<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { ActivityState, WorkflowRun } from './service.mts'

const props = defineProps<{ state: ActivityState; busy: boolean }>()
const emit = defineEmits<{ retry: []; cancel: [run: WorkflowRun] }>()
const selectedId = ref('')
const runs = computed(() => props.state.status === 'ready' ? props.state.runs : [])
const selected = computed(() => runs.value.find((run) => run.id === selectedId.value) ?? runs.value[0] ?? null)
watch(runs, (value) => {
  if (!selectedId.value && value.length) selectedId.value = value[0].id
  if (selectedId.value && !value.some((run) => run.id === selectedId.value)) selectedId.value = value[0]?.id ?? ''
}, { immediate: true })

function phaseLabel(phase: string): string {
  return phase.replace('_', ' ')
}
</script>

<template>
  <section class="activity" aria-labelledby="activity-title">
    <header class="activity__header">
      <div>
        <p class="eyebrow">Durable execution record</p>
        <h1 id="activity-title">Activity</h1>
        <p>Value-free node status for immutable workflow versions. Arguments, results and credentials never appear in the trace.</p>
      </div>
      <button type="button" :disabled="busy" @click="emit('retry')">Refresh</button>
    </header>
    <p v-if="state.status === 'loading'" aria-live="polite">Reading workflow activity…</p>
    <div v-else-if="state.status === 'failed'" class="failure" role="alert">
      <h2>Activity could not be read</h2><p>{{ state.failure.detail }}</p><button type="button" @click="emit('retry')">Try again</button>
    </div>
    <div v-else-if="!runs.length" class="activity-empty">
      <h2>No workflow runs yet</h2>
      <p>This is a durable empty answer from the service, not a failed read.</p>
    </div>
    <div v-else class="activity-layout">
      <nav class="run-list" aria-label="Workflow runs">
        <button v-for="run in runs" :key="run.id" type="button" :aria-current="selected?.id === run.id ? 'true' : undefined" @click="selectedId = run.id">
          <span class="run-status" :data-status="run.status">{{ run.status }}</span>
          <strong>{{ run.workflow_id }} <small>v{{ run.version }}</small></strong>
          <code>{{ run.id }}</code>
          <time :datetime="new Date(run.created_at_ms).toISOString()">{{ new Date(run.created_at_ms).toLocaleString() }}</time>
        </button>
      </nav>
      <article v-if="selected" class="run-detail">
        <header>
          <div><p class="eyebrow">{{ selected.workflow_id }} · version {{ selected.version }}</p><h2>{{ selected.id }}</h2></div>
          <button v-if="selected.status === 'running'" type="button" :disabled="busy" @click="emit('cancel', selected)">Cancel run</button>
        </header>
        <p class="run-status run-status--large" :data-status="selected.status">{{ selected.status }}</p>
        <p v-if="selected.error" class="workflow-outcome--bad">{{ selected.error }}</p>
        <pre v-if="selected.result" class="run-result">{{ selected.result }}</pre>
        <h3>Node timeline</h3>
        <ol v-if="selected.events.length" class="event-list">
          <li v-for="entry in selected.events" :key="entry.sequence">
            <span>{{ entry.sequence }}</span>
            <code>{{ entry.event.node_id }}</code>
            <strong :data-phase="entry.event.phase">{{ phaseLabel(entry.event.phase) }}</strong>
            <small>#{{ entry.event.occurrence }} · {{ entry.event.source_path }}<template v-if="entry.event.branch"> · {{ entry.event.branch }}</template></small>
          </li>
        </ol>
        <p v-else>No editor-addressable node has emitted an event yet.</p>
      </article>
    </div>
  </section>
</template>
