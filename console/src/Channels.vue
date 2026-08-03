<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type {
  ChannelDeclaration,
  ChannelDeclarationsState,
  ChannelMutation,
  ChannelsState,
  HeldChannel,
} from './service.mts'

const props = defineProps<{
  state: ChannelsState
  declarations: ChannelDeclarationsState
  busy: boolean
  outcome: ChannelMutation | null
}>()

const emit = defineEmits<{
  retry: []
  create: [connector: string, binding: string, events: string[]]
  update: [channel: HeldChannel, events: string[]]
  remove: [channel: HeldChannel]
}>()

const connector = ref('')
const binding = ref('')
const selectedEvents = ref<string[]>([])
const edits = ref<Record<string, string[]>>({})

const socketDeclarations = computed(() =>
  props.declarations.status === 'ready'
    ? props.declarations.declarations.filter((item) => item.transport === 'socket')
    : []
)
const connectors = computed(() => [...new Set(socketDeclarations.value.map((item) => item.connector))])
const bindings = computed(() => socketDeclarations.value.filter((item) => item.connector === connector.value))
const declaration = computed(() => bindings.value.find((item) => item.name === binding.value) ?? null)

watch(connectors, (values) => {
  if (!values.includes(connector.value)) connector.value = values[0] ?? ''
}, { immediate: true })

watch(bindings, (values) => {
  if (!values.some((item) => item.name === binding.value)) binding.value = values[0]?.name ?? ''
}, { immediate: true })

watch(declaration, (value) => {
  selectedEvents.value = value?.events.filter((event) => event.default).map((event) => event.name) ?? []
}, { immediate: true })

watch(() => props.state, (state) => {
  if (state.status !== 'ready') return
  const next: Record<string, string[]> = {}
  for (const channel of state.channels) next[channel.id] = [...channel.events]
  edits.value = next
}, { immediate: true, deep: true })

function declaredFor(channel: HeldChannel): ChannelDeclaration | null {
  return socketDeclarations.value.find((item) =>
    item.connector === channel.connector && item.name === channel.binding) ?? null
}

function toggle(target: string[], event: string, checked: boolean) {
  const set = new Set(target)
  if (checked) set.add(event)
  else set.delete(event)
  return [...set]
}

function create() {
  if (!connector.value || !binding.value || !selectedEvents.value.length) return
  emit('create', connector.value, binding.value, [...selectedEvents.value])
}

function changed(channel: HeldChannel): boolean {
  return JSON.stringify(edits.value[channel.id] ?? []) !== JSON.stringify(channel.events)
}
</script>

<template>
  <section class="channels" aria-labelledby="channels-title">
    <header class="channels__header">
      <div>
        <p class="eyebrow">Inbound connector events</p>
        <h1 id="channels-title">Channels</h1>
        <p>Exchange keeps each generated WebSocket open for this tenant. Agents subscribe separately through one authenticated socket.</p>
      </div>
      <button type="button" :disabled="busy" @click="emit('retry')">Refresh</button>
    </header>

    <div v-if="state.status === 'loading'" class="channels__loading" aria-live="polite">Reading this tenant’s channels…</div>
    <section v-else-if="state.status === 'failed'" class="failure" role="alert">
      <h2>Channels could not be read</h2>
      <p><code>{{ state.failure.endpoint }}</code> — {{ state.failure.detail }}</p>
      <button type="button" @click="emit('retry')">Try again</button>
    </section>

    <template v-else>
      <div class="channel-summary" aria-live="polite">
        <strong>{{ state.channels.length }}</strong>
        <span>{{ state.channels.length === 1 ? 'persistent channel' : 'persistent channels' }}</span>
      </div>

      <div class="channel-grid">
        <article v-for="channel in state.channels" :key="channel.id" class="channel-card">
          <header>
            <div>
              <p class="eyebrow">{{ channel.connector }}</p>
              <h2>{{ channel.binding }}</h2>
              <code>{{ channel.id }}</code>
            </div>
            <span class="channel-status" :data-status="channel.status" aria-label="Channel status">
              {{ channel.status }}
            </span>
          </header>

          <fieldset v-if="declaredFor(channel)" :disabled="busy">
            <legend>Delivered events</legend>
            <label v-for="event in declaredFor(channel)?.events" :key="event.name">
              <input
                type="checkbox"
                :checked="edits[channel.id]?.includes(event.name)"
                @change="edits[channel.id] = toggle(edits[channel.id] ?? [], event.name, ($event.target as HTMLInputElement).checked)"
              />
              <span><strong>{{ event.name }}</strong><small>{{ event.description }}</small></span>
            </label>
          </fieldset>
          <p v-else class="channel-declaration-missing" role="alert">This build no longer declares this binding. It cannot be edited safely.</p>

          <div class="channel-card__actions">
            <button
              type="button"
              :disabled="busy || !changed(channel) || !(edits[channel.id]?.length)"
              @click="emit('update', channel, edits[channel.id] ?? [])"
            >Save event selection</button>
            <details>
              <summary>Remove…</summary>
              <p>This stops the vendor socket and removes its persisted declaration.</p>
              <button type="button" class="button-danger" :disabled="busy" @click="emit('remove', channel)">Delete channel</button>
            </details>
          </div>
        </article>

        <p v-if="!state.channels.length" class="channels-empty">No channel is configured for this tenant yet.</p>
      </div>

      <section class="channel-create" aria-labelledby="channel-create-title">
        <div>
          <p class="eyebrow">New persistent channel</p>
          <h2 id="channel-create-title">Open a generated WebSocket binding</h2>
          <p>The connector and binding are catalogue choices. Tenant, connection, endpoint, credentials and placement are derived by the host.</p>
        </div>

        <div v-if="declarations.status === 'loading'" aria-live="polite">Reading generated channel declarations…</div>
        <div v-else-if="declarations.status === 'failed'" class="failure" role="alert">
          <p><code>{{ declarations.failure.endpoint }}</code> — {{ declarations.failure.detail }}</p>
        </div>
        <p v-else-if="!socketDeclarations.length" class="channels-empty">This catalogue declares no generated WebSocket channel.</p>
        <form v-else @submit.prevent="create">
          <label>Connector
            <select v-model="connector">
              <option v-for="item in connectors" :key="item" :value="item">{{ item }}</option>
            </select>
          </label>
          <label>Binding
            <select v-model="binding">
              <option v-for="item in bindings" :key="item.name" :value="item.name">{{ item.name }}</option>
            </select>
          </label>
          <p v-if="declaration" class="channel-description">{{ declaration.description }}</p>
          <fieldset v-if="declaration">
            <legend>Select declared events</legend>
            <label v-for="event in declaration.events" :key="event.name">
              <input
                type="checkbox"
                :checked="selectedEvents.includes(event.name)"
                @change="selectedEvents = toggle(selectedEvents, event.name, ($event.target as HTMLInputElement).checked)"
              />
              <span><strong>{{ event.name }}</strong><small>{{ event.description }}</small></span>
            </label>
          </fieldset>
          <p v-if="!selectedEvents.length" class="input-error" role="alert">Select at least one declared event.</p>
          <button type="submit" class="button-primary" :disabled="busy || !selectedEvents.length">Create channel</button>
        </form>
      </section>
    </template>

    <p v-if="outcome?.status === 'refused'" class="channel-outcome channel-outcome--bad" role="alert">{{ outcome.refusal.error }}</p>
    <p v-else-if="outcome?.status === 'failed'" class="channel-outcome channel-outcome--bad" role="alert">{{ outcome.failure.detail }}</p>
    <p v-else-if="outcome?.status === 'saved'" class="channel-outcome" role="status">Channel saved; its current lifecycle state is {{ outcome.channel.status }}.</p>
    <p v-else-if="outcome?.status === 'removed'" class="channel-outcome" role="status">Channel removed.</p>
  </section>
</template>
