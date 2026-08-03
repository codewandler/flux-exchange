<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { AppActivity, AppMutation, AppsState, Connection, InstallAppChoice } from './service.mts'

const props = defineProps<{
  state: AppsState
  connections: Connection[]
  activity: AppActivity[]
  outcome: AppMutation | null
  busy: boolean
}>()

const emit = defineEmits<{
  retry: []
  install: [choice: InstallAppChoice]
  chat: [app: string, message: string, session: string | null]
  inspect: [app: string]
}>()

const appId = ref('slack-assistant')
const profile = ref('demo-model')
const reply = ref('Hello from your installed Flux App.')
const connection = ref('')
const risk = ref('high')
const scopes = ref('chat:reply')
const selectedLayers = ref<string[]>(['reply'])
const selectedApp = ref('')
const message = ref('')
const session = ref<string | null>(null)

const packageEntry = computed(() => props.state.status === 'ready'
  ? props.state.packages.find((item) => item.id === 'exchange-apps/slack-bot') ?? props.state.packages[0]
  : undefined)
const slackConnections = computed(() => props.connections.filter((item) => item.connector === 'slack' && item.label))
const installed = computed(() => props.state.status === 'ready' ? props.state.apps : [])

watch(slackConnections, (items) => {
  if (!connection.value) connection.value = items[0]?.label ?? ''
}, { immediate: true })
watch(installed, (items) => {
  if (!selectedApp.value) selectedApp.value = items[0]?.id ?? ''
}, { immediate: true })
watch(() => props.outcome, (outcome) => {
  if (outcome?.status === 'answered') session.value = outcome.session
}, { deep: true })

function install() {
  if (!packageEntry.value || !connection.value) return
  emit('install', {
    id: appId.value.trim(), package: packageEntry.value.id, version: packageEntry.value.version,
    connection: connection.value, access_layers: selectedLayers.value,
    risk_ceiling: risk.value, scopes: scopes.value.split(',').map((item) => item.trim()).filter(Boolean),
    profile: profile.value.trim(), static_reply: reply.value,
  })
}

function chat() {
  if (!selectedApp.value || !message.value.trim()) return
  emit('chat', selectedApp.value, message.value, session.value)
  message.value = ''
}
</script>

<template>
  <section class="apps">
    <header class="apps__header">
      <div><p class="eyebrow">Managed Agents</p><h1>Installed Flux Apps</h1></div>
      <span v-if="outcome?.status === 'answered'" class="apps__activation">{{ outcome.activation || 'active' }}</span>
    </header>

    <div v-if="state.status === 'loading'" class="connections__skeleton" aria-label="Reading installed Apps">
      <span class="skeleton skeleton--title"></span><span class="skeleton"></span>
    </div>
    <section v-else-if="state.status === 'failed'" class="failure">
      <h2>Installed Apps could not be read</h2><p>{{ state.failure.detail }}</p>
      <button type="button" @click="emit('retry')">Retry</button>
    </section>
    <template v-else>
      <article class="apps__panel">
        <h2>Install the Slack bot template</h2>
        <p>The package is immutable. These choices freeze its Connection, operation layer, model, risk ceiling and scopes.</p>
        <p v-if="!packageEntry">This build publishes no Slack-bot-style App Package.</p>
        <form v-else class="apps__form" @submit.prevent="install">
          <label>App name <input v-model="appId" required /></label>
          <label>Slack Connection
            <select v-model="connection" required>
              <option value="" disabled>Choose a labelled Slack Connection</option>
              <option v-for="item in slackConnections" :key="item.label ?? ''" :value="item.label ?? ''">{{ item.label }}</option>
            </select>
          </label>
          <fieldset><legend>Optional access layers</legend>
            <label v-for="layer in packageEntry.requirements.access_layers" :key="layer.name" class="apps__check">
              <input v-model="selectedLayers" type="checkbox" :value="layer.name" :disabled="layer.required" />
              {{ layer.name }} · {{ layer.connector }} <span v-if="layer.required">required</span>
            </label>
          </fieldset>
          <label>Model Profile <input v-model="profile" required /></label>
          <label>Demo model reply <textarea v-model="reply" rows="2"></textarea></label>
          <label>Risk ceiling
            <select v-model="risk"><option>low</option><option>medium</option><option>high</option><option>destructive</option></select>
          </label>
          <label>Scopes <input v-model="scopes" placeholder="chat:reply, support:triage" /></label>
          <button type="submit" :disabled="busy || !connection">{{ busy ? 'Installing…' : 'Review and install' }}</button>
        </form>
      </article>

      <article class="apps__panel">
        <h2>Talk to an installed App</h2>
        <label>Installed App
          <select v-model="selectedApp" @change="emit('inspect', selectedApp)">
            <option v-for="item in installed" :key="item.id" :value="item.id">{{ item.id }} · {{ item.activation }}</option>
          </select>
        </label>
        <form class="apps__chat" @submit.prevent="chat">
          <input v-model="message" placeholder="Message the Managed Agent" aria-label="Message" />
          <button type="submit" :disabled="busy || !selectedApp">Send</button>
        </form>
        <p v-if="outcome?.status === 'answered'" class="apps__reply">{{ outcome.reply }}</p>
        <p v-else-if="outcome?.status === 'refused'" class="failure">{{ outcome.refusal.error }}</p>
        <p v-else-if="outcome?.status === 'failed'" class="failure">{{ outcome.failure.detail }}</p>
        <p v-if="session" class="apps__session">Conversation <code>{{ session }}</code></p>
        <h3>Activation activity</h3>
        <ol class="apps__activity">
          <li v-for="item in activity" :key="item.id"><strong>{{ item.outcome }}</strong> · {{ item.kind }} · <code>{{ item.delivery }}</code></li>
          <li v-if="!activity.length">No managed run has completed yet.</li>
        </ol>
      </article>
    </template>
  </section>
</template>

<style src="./apps.css"></style>
