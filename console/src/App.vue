<script setup lang="ts">
// The console's root: the one place that knows where data comes from and which view receives it.
// Catalogue views are exchange-owned, but they still receive completed data as props; the service
// client decides loading, failure and readiness before any catalogue view renders.
//
// Each thing this file reads has three states and each has its own view, because collapsing any two
// of them lies to the reader:
//
//   loading  the request is in flight — say so, and name what is being read
//   failed   nothing was read — name the endpoint that did not answer
//   ready    an answer, however small, including one with nothing in it
//
// The third and second are the pair that matters. A service that is not running and a service with
// nothing in it must never render the same page.
//
// **What surrounds all of that is `ConsoleShell.mts`**, and it is the reason X-34 exists. This
// console renders the connector catalogue, which is reference material — what this build *could*
// run — and while that was the whole surface, the console read as a connector browser rather than
// as the admin surface of a service that holds credentials and executes for many callers. The shell
// states the platform's surfaces instead, including the three that are not built, and `surfaces.mts`
// is the single statement of which is which.

import { computed, onMounted, onUnmounted, shallowRef, watch } from 'vue'
import { fragmentPath, useRoute } from './routing'
import {
  CONNECTORS_ENDPOINT,
  SIGNIN_ENDPOINT,
  channelConnectionLabels,
  applyConnectionPlan,
  createChannel,
  cancelWorkflowRun,
  createWorkflow,
  loadCatalogue,
  loadChannelDeclarations,
  loadChannels,
  loadConnections,
  loadActivity,
  loadApps,
  loadAppActivity,
  loadEditorCatalog,
  loadConnectionPlan,
  loadGrants,
  loadSession,
  loadSignInAvailability,
  loadWorkflows,
  installApp,
  inspectConnectionAuthority,
  publishWorkflow,
  previewGrant,
  replaceGrants,
  removeChannel,
  rotateCredential,
  saveWorkflow,
  saveWorkflowGraph,
  signOut,
  transitionConnectionAuthority,
  sendAppMessage,
  startWorkflowRun,
  validateWorkflowEdit,
  validateWorkflowGraph,
  updateChannel,
  type ActivityState,
  type AppActivity,
  type AppMutation,
  type AppsState,
  type InstallAppChoice,
  type CatalogueState,
  type ChannelDeclarationsState,
  type ChannelMutation,
  type ChannelsState,
  type ConnectionsState,
  type ConnectionPlanOutcome,
  type ConnectionPlanState,
  type ConnectionPlanSubmission,
  type ConnectionAuthorityOutcome,
  type ConnectionAuthorityInspectionOutcome,
  type ConnectionAuthorityInspectionRequest,
  type ConnectionAuthorityTransition,
  type EditorCatalogState,
  type GrantOutcome,
  type GrantsState,
  type PreviewState,
  type ProposedGrant,
  type RotationOutcome,
  type SessionState,
  type SignInAvailabilityState,
  type WorkflowDraft,
  type WorkflowMutation,
  type WorkflowsState,
} from './service.mts'
import { ONBOARDING_PATH } from './onboarding.mts'
import { SERVICE_ACCOUNTS_PATH } from './minting.mts'
import { mayGrant } from './granting.mts'
import { LatestConnectionRequest } from './connection-plan-state.mts'
import { surfaceOfRoute } from './surfaces.mts'
import { isDark, toggleTheme } from './theme'

import AgentOnboarding from './AgentOnboarding.mts'
import Activity from './Activity.vue'
import Apps from './Apps.vue'
import Agents from './Agents.mts'
import CatalogueFinder from './CatalogueFinder.mts'
import CatalogueFailure from './CatalogueFailure.mts'
import CatalogueOperation from './CatalogueOperation.mts'
import Connect from './Connect.mts'
import Channels from './Channels.vue'
import Connections from './Connections.mts'
import ConsoleShell from './ConsoleShell.mts'
import Grants from './Grants.mts'
import Invoke from './Invoke.mts'
import Journey from './Journey.mts'
import Workflows from './Workflows.vue'

// `shallowRef` rather than `ref`: each of these is replaced wholesale and never edited, so making
// every operation in a catalogue deeply reactive would buy nothing and cost a walk of the document.
const catalogue = shallowRef<CatalogueState>({ status: 'loading' })
const session = shallowRef<SessionState>({ status: 'loading' })
const signInAvailability = shallowRef<SignInAvailabilityState>({ status: 'loading' })
const connections = shallowRef<ConnectionsState>({ status: 'loading' })
const workflows = shallowRef<WorkflowsState>({ status: 'loading' })
const editorCatalog = shallowRef<EditorCatalogState>({ status: 'loading' })
const activity = shallowRef<ActivityState>({ status: 'loading' })
const apps = shallowRef<AppsState>({ status: 'loading' })
const appActivity = shallowRef<AppActivity[]>([])
const appOutcome = shallowRef<AppMutation | null>(null)
const appBusy = shallowRef(false)
const channels = shallowRef<ChannelsState>({ status: 'loading' })
const channelDeclarations = shallowRef<ChannelDeclarationsState>({ status: 'loading' })
const channelOutcome = shallowRef<ChannelMutation | null>(null)
const channelBusy = shallowRef(false)
const workflowOutcome = shallowRef<WorkflowMutation | null>(null)
const workflowBusy = shallowRef(false)

const route = useRoute()

/** Whether a principal is resolved. `null` while the session is still unknown either way. */
const principal = computed(() => (session.value.status === 'ready' ? session.value.principal : null))
const signedIn = computed(() => principal.value !== null)
const channelConnections = computed(() => channelConnectionLabels(connections.value))

async function reloadCatalogue() {
  catalogue.value = { status: 'loading' }
  catalogue.value = await loadCatalogue()
}

async function reloadSession() {
  session.value = { status: 'loading' }
  session.value = await loadSession()
}

async function reloadSignInAvailability() {
  signInAvailability.value = { status: 'loading' }
  signInAvailability.value = await loadSignInAvailability()
}

async function reloadConnections() {
  connections.value = { status: 'loading' }
  connections.value = await loadConnections()
}

async function reloadWorkflows() {
  workflows.value = { status: 'loading' }
  workflows.value = await loadWorkflows()
}

async function reloadEditorCatalog() {
  editorCatalog.value = { status: 'loading' }
  editorCatalog.value = await loadEditorCatalog()
}

async function reloadActivity() {
  activity.value = { status: 'loading' }
  activity.value = await loadActivity()
}

async function reloadApps() {
  apps.value = { status: 'loading' }
  apps.value = await loadApps()
}

async function inspectApp(app: string) {
  appActivity.value = app ? await loadAppActivity(app) : []
}

async function installManagedApp(choice: InstallAppChoice) {
  if (appBusy.value) return
  appBusy.value = true
  appOutcome.value = await installApp(choice)
  appBusy.value = false
  if (appOutcome.value.status === 'installed') {
    await reloadApps()
    await inspectApp(choice.id)
  }
}

async function chatWithApp(app: string, message: string, session: string | null) {
  if (appBusy.value) return
  appBusy.value = true
  appOutcome.value = await sendAppMessage(app, message, session)
  appBusy.value = false
  await inspectApp(app)
}

async function reloadChannels() {
  channels.value = { status: 'loading' }
  channels.value = await loadChannels()
}

async function reloadChannelDeclarations() {
  if (catalogue.value.status !== 'ready') return
  channelDeclarations.value = { status: 'loading' }
  channelDeclarations.value = await loadChannelDeclarations(catalogue.value.catalog)
}

async function reloadWorkflowSurface() {
  workflowOutcome.value = null
  await Promise.all([reloadWorkflows(), reloadEditorCatalog(), reloadActivity()])
}

onMounted(async () => {
  // Both at once. The catalogue is anonymous and the session is not, so neither waits on the other
  // — and a slow identity provider must not delay the one view that never needed a principal.
  void reloadCatalogue()
  await Promise.all([reloadSession(), reloadSignInAvailability()])
})

// Connections are tenant data and `/api/connections` requires a principal, so this is not asked
// until one resolves. That is what keeps `ConnectionsState` at three states: a signed-out reader is
// never shown a listing that failed, because none was attempted — the gate below is shown instead.
watch(signedIn, async (resolved) => {
  if (resolved) {
    await reloadConnections()
    if (principal.value?.kind === 'user') await Promise.all([reloadWorkflowSurface(), reloadChannels(), reloadApps()])
  }
})

watch(() => catalogue.value.status, async (status) => {
  if (status === 'ready') await reloadChannelDeclarations()
})

async function createHeldChannel(connector: string, connection: string, binding: string, events: string[]) {
  if (channelBusy.value) return
  channelBusy.value = true
  channelOutcome.value = await createChannel(connector, connection, binding, events)
  channelBusy.value = false
  if (channelOutcome.value.status === 'saved') await reloadChannels()
}

async function updateHeldChannel(channel: Parameters<typeof updateChannel>[0], connection: string, events: string[]) {
  if (channelBusy.value) return
  channelBusy.value = true
  channelOutcome.value = await updateChannel(channel, connection, events)
  channelBusy.value = false
  if (channelOutcome.value.status === 'saved') await reloadChannels()
}

async function removeHeldChannel(channel: Parameters<typeof removeChannel>[0]) {
  if (channelBusy.value) return
  channelBusy.value = true
  channelOutcome.value = await removeChannel(channel)
  channelBusy.value = false
  if (channelOutcome.value.status === 'removed') await reloadChannels()
}

async function createWorkflowDraft(id: string, title: string, source: string) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await createWorkflow(id, title, source)
  workflowBusy.value = false
  if (workflowOutcome.value.status === 'saved') await reloadWorkflows()
}

async function saveWorkflowDraft(workflow: WorkflowDraft, source: string) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await saveWorkflow(workflow, source)
  workflowBusy.value = false
  if (workflowOutcome.value.status === 'saved') await reloadWorkflows()
}

async function saveWorkflowVisual(workflow: WorkflowDraft, graph: NonNullable<WorkflowDraft['graph']>) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await saveWorkflowGraph(workflow, graph)
  workflowBusy.value = false
  if (workflowOutcome.value.status === 'saved') await reloadWorkflows()
}

async function validateWorkflowDraft(workflow: WorkflowDraft, source: string) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await validateWorkflowEdit(workflow, source)
  workflowBusy.value = false
}

async function validateWorkflowVisual(workflow: WorkflowDraft, graph: NonNullable<WorkflowDraft['graph']>) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await validateWorkflowGraph(workflow, graph)
  workflowBusy.value = false
}

async function publishWorkflowDraft(workflow: WorkflowDraft) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await publishWorkflow(workflow)
  workflowBusy.value = false
  if (workflowOutcome.value.status === 'published') await reloadWorkflows()
}

async function runWorkflow(workflow: WorkflowDraft, params: unknown) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  workflowOutcome.value = await startWorkflowRun(workflow, params)
  workflowBusy.value = false
  await reloadActivity()
}

async function cancelRun(run: { id: string }) {
  if (workflowBusy.value) return
  workflowBusy.value = true
  await cancelWorkflowRun(run.id)
  workflowBusy.value = false
  await reloadActivity()
}

const activityPoll = window.setInterval(() => {
  if (
    principal.value?.kind === 'user' &&
    (route.value.name === 'activity' || route.value.name === 'workflows') &&
    activity.value.status === 'ready' &&
    activity.value.runs.some((run) => run.status === 'running')
  ) void reloadActivity()
}, 1500)
onUnmounted(() => window.clearInterval(activityPoll))

// ---------------------------------------------------------------------------------------------
// Wiring a connector up. The console's other job, and until X-44 the one it could not do.
//
// The selected connector and label, the value-free plan, and the last value-free result are all
// safe to retain. The one deliberate exception is the exact normalized origin returned by the
// configured-operator review route: it is held only for that plan revision and cleared before any
// transition, retry or navigation. **What the operator typed is not among them.** A value
// lives in the input element it was typed into and in the request body, and this component holds
// none of it — see `Connect.mts`, which is where that is enforced rather than merely intended.
// ---------------------------------------------------------------------------------------------

const chosen = shallowRef<string | null>(null)
const selectedConnectionLabel = shallowRef<string | null>(null)
const connectionPlan = shallowRef<ConnectionPlanState | null>(null)
const outcome = shallowRef<ConnectionPlanOutcome | null>(null)
const authorityOutcome = shallowRef<ConnectionAuthorityOutcome | null>(null)
const authorityBusy = shallowRef('')
const authorityInspection = shallowRef<ConnectionAuthorityInspectionOutcome | null>(null)
const authorityInspecting = shallowRef('')
const connecting = shallowRef(false)
const connectionPlanRequests = new LatestConnectionRequest()
const connectionApplyRequests = new LatestConnectionRequest()
const authorityRequests = new LatestConnectionRequest()
const authorityInspectionRequests = new LatestConnectionRequest()

function invalidateAuthority(): void {
  authorityRequests.invalidate()
  authorityInspectionRequests.invalidate()
  authorityBusy.value = ''
  authorityInspecting.value = ''
  authorityOutcome.value = null
  authorityInspection.value = null
}

watch(
  () => route.value.name,
  (next, previous) => {
    // App.vue outlives every screen. The operator-only normalized origin does not: leaving the
    // connection surface clears both the rendered value and any in-flight inspection answer.
    if (previous === 'connections' && next !== 'connections') invalidateAuthority()
  },
)

/** A connector was chosen: ask for its complete plan, and forget the last attempt. */
async function chooseConnector(connector: string) {
  if (connecting.value) return
  const id = connector || null
  connectionApplyRequests.invalidate()
  invalidateAuthority()
  chosen.value = id
  selectedConnectionLabel.value = null
  outcome.value = null

  if (id === null) {
    connectionPlanRequests.invalidate()
    connectionPlan.value = null
    return
  }

  const ticket = connectionPlanRequests.begin(id, null)
  connectionPlan.value = { status: 'loading' }
  const state = await loadConnectionPlan(id, null)
  // The reader may have moved on while that was in flight; a plan for a connector nobody is
  // looking at any more would render under the wrong heading.
  if (connectionPlanRequests.admits(ticket, chosen.value, selectedConnectionLabel.value)) {
    connectionPlan.value = state
  }
}

/** Select only an operator-owned label; no field value has a URL representation. */
async function selectConnectionLabel(label: string) {
  if (connecting.value) return
  const connector = chosen.value
  if (connector === null) return
  const selection = label || null
  const ticket = connectionPlanRequests.begin(connector, selection)
  connectionApplyRequests.invalidate()
  invalidateAuthority()
  selectedConnectionLabel.value = selection
  outcome.value = null
  connectionPlan.value = { status: 'loading' }
  const state = await loadConnectionPlan(connector, selection)
  if (connectionPlanRequests.admits(ticket, chosen.value, selectedConnectionLabel.value)) {
    connectionPlan.value = state
  }
}

async function retryConnectionPlan() {
  const connector = chosen.value
  if (connector === null) return
  invalidateAuthority()
  const selection = selectedConnectionLabel.value
  const ticket = connectionPlanRequests.begin(connector, selection)
  connectionPlan.value = { status: 'loading' }
  const state = await loadConnectionPlan(connector, selection)
  if (connectionPlanRequests.admits(ticket, chosen.value, selectedConnectionLabel.value)) {
    connectionPlan.value = state
  }
}

/** Run the hosted FXLM ceremony, then read a fresh value-free projection. */
async function connectChosen(
  connector: string,
  selection: string | null,
  submission: ConnectionPlanSubmission
) {
  if (chosen.value !== connector || selectedConnectionLabel.value !== selection || connecting.value) return
  const ticket = connectionApplyRequests.begin(connector, selection)
  invalidateAuthority()

  connecting.value = true
  const answered = await applyConnectionPlan(connector, submission)
  if (!connectionApplyRequests.admits(ticket, chosen.value, selectedConnectionLabel.value)) return
  outcome.value = answered
  connecting.value = false

  if (outcome.value.status === 'answered') {
    const label = outcome.value.result.label
    selectedConnectionLabel.value = label
    const planTicket = connectionPlanRequests.begin(connector, label)
    connectionPlan.value = { status: 'loading' }
    const state = await loadConnectionPlan(connector, label)
    if (connectionPlanRequests.admits(planTicket, chosen.value, selectedConnectionLabel.value)) {
      connectionPlan.value = state
    }
    await reloadConnections()
  }
}

/** Change only the exact value-free revision and action the current plan advertised. */
async function changeConnectionAuthority(identity: string, transition: ConnectionAuthorityTransition) {
  if (connecting.value || authorityBusy.value !== '') return
  const current = connectionPlan.value
  if (current?.status !== 'ready' || current.plan.connector !== chosen.value ||
      current.plan.selection !== selectedConnectionLabel.value || transition.connector !== chosen.value ||
      transition.label !== selectedConnectionLabel.value) return

  const field = current.plan.fields.find((candidate) => candidate.identity === identity)
  const authority = field?.authority
  if (field?.service === null || field?.service !== transition.service || field.binds !== transition.field ||
      authority?.revision !== transition.revision || !authority.actions.includes(transition.action)) return
  if (transition.action === 'approve') {
    const reviewed = authorityInspection.value
    if (reviewed?.status !== 'answered' || reviewed.result.connector !== transition.connector ||
        reviewed.result.label !== transition.label || reviewed.result.service !== transition.service ||
        reviewed.result.field !== transition.field || reviewed.result.authority.state !== 'proposed' ||
        reviewed.result.authority.revision !== transition.revision) return
  }

  authorityInspectionRequests.invalidate()
  authorityInspecting.value = ''
  authorityInspection.value = null
  const ticket = authorityRequests.begin(transition.connector, transition.label)
  authorityBusy.value = identity
  authorityOutcome.value = null
  const answered = await transitionConnectionAuthority(transition)
  if (!authorityRequests.admits(ticket, chosen.value, selectedConnectionLabel.value)) return
  authorityBusy.value = ''
  authorityOutcome.value = answered

  if (answered.status === 'answered') {
    const planTicket = connectionPlanRequests.begin(transition.connector, transition.label)
    connectionPlan.value = { status: 'loading' }
    const state = await loadConnectionPlan(transition.connector, transition.label)
    if (connectionPlanRequests.admits(planTicket, chosen.value, selectedConnectionLabel.value)) {
      connectionPlan.value = state
    }
  }
}

/** Read the exact normalized proposal only through the configured-operator authority surface. */
async function inspectAuthority(identity: string, request: ConnectionAuthorityInspectionRequest) {
  if (connecting.value || authorityBusy.value !== '' || authorityInspecting.value !== '') return
  const current = connectionPlan.value
  if (current?.status !== 'ready' || current.plan.connector !== chosen.value ||
      current.plan.selection !== selectedConnectionLabel.value || request.connector !== chosen.value ||
      request.label !== selectedConnectionLabel.value) return

  const field = current.plan.fields.find((candidate) => candidate.identity === identity)
  const authority = field?.authority
  if (field?.service === null || field?.service !== request.service || field.binds !== request.field ||
      authority?.state !== request.state || authority.revision !== request.revision ||
      authority.state !== 'proposed' || !authority.actions.includes('approve')) return

  const ticket = authorityInspectionRequests.begin(request.connector, request.label)
  authorityInspecting.value = identity
  authorityInspection.value = null
  const answered = await inspectConnectionAuthority(request)
  if (!authorityInspectionRequests.admits(ticket, chosen.value, selectedConnectionLabel.value)) return
  authorityInspecting.value = ''
  authorityInspection.value = answered
}

const rotating = shallowRef('')
const rotationOutcome = shallowRef<RotationOutcome | null>(null)

async function rotateHeld(connector: string, credential: string, value: string) {
  if (rotating.value) return
  rotating.value = `${connector}/${credential}`
  rotationOutcome.value = await rotateCredential(connector, credential, value)
  rotating.value = ''
  if (rotationOutcome.value.status === 'rotated') await reloadConnections()
}

// ---------------------------------------------------------------------------------------------
// Grants: what this tenant may run. The half of X-62 the service already had.
//
// Read here and passed down, which is this file's ordinary arrangement — and deliberately *not*
// `Agents.mts`'s exception. That one exists because a minted token must not reach this component,
// which is the root and outlives every screen. A grant is a policy: it carries no secret, it is
// meant to be read back, and holding it here costs nothing.
//
// Four pieces of state. The draft is **not** among them: it lives in `Grants.mts`, which emits a
// proposal when it changes and a whole set when it is saved.
// ---------------------------------------------------------------------------------------------

const grants = shallowRef<GrantsState>({ status: 'loading' })
const preview = shallowRef<PreviewState | null>(null)
const grantOutcome = shallowRef<GrantOutcome | null>(null)
const granting = shallowRef(false)

/**
 * Which question the preview panel is currently waiting on.
 *
 * Every edit asks a new one, and answers can arrive out of order — an operator who widens a bound
 * and then narrows it again must not be shown the wider answer because it was slower. Only the
 * newest question's answer is kept, which is `chooseConnector`'s rule made explicit because here
 * the question changes on every keystroke-equivalent rather than once per connector.
 */
let asked = 0

// The route requires a `User` on the read as well as the write, so this is not asked of a caller
// this host would refuse: a `403` rendered as a failed read would tell an agent's operator that the
// service is broken. `Grants.mts` says why there is no listing instead.
watch(
  () => mayGrant(principal.value),
  async (may) => {
    if (may) await reloadGrants()
  }
)

async function reloadGrants() {
  grants.value = { status: 'loading' }
  grants.value = await loadGrants()
}

/** Ask what a proposed grant would admit. Reads the preview route; writes nothing. */
async function previewProposed(proposed: ProposedGrant) {
  const question = ++asked
  preview.value = { status: 'loading' }
  const answer = await previewGrant(proposed)
  if (question === asked) preview.value = answer
}

/** Replace the whole set, then re-read — what is shown is the service's answer, not what was sent. */
async function saveGrants(next: ProposedGrant[]) {
  if (granting.value) return

  granting.value = true
  grantOutcome.value = await replaceGrants(next)
  granting.value = false

  const outcome = grantOutcome.value
  if (outcome.status === 'saved') grants.value = { status: 'ready', grants: outcome.grants, editable: outcome.editable }
}

/** Sign out, then reload — the session cookie is gone and every view on the page depends on it. */
async function endSession() {
  await signOut()
  window.location.reload()
}

// Narrowed once here rather than in the template, so the type checker can see it inside `v-if`.
const ready = computed(() => (catalogue.value.status === 'ready' ? catalogue.value : null))
const failure = computed(() =>
  catalogue.value.status === 'failed' ? catalogue.value.failure : null
)
const sessionFailure = computed(() =>
  session.value.status === 'failed' ? session.value.failure : null
)

/**
 * Every connector the catalogue lists, by id.
 *
 * The connect form offers these and no others, and the console enumerates none of its own — the
 * same rule the credential inputs follow one level down. Empty while the catalogue is still being
 * read, which leaves the form's chooser empty rather than short: a partial list of connectors would
 * read as the complete one.
 */
const connectors = computed(() => ready.value?.catalog.connectors.map((connector) => connector.id) ?? [])
const catalogConnectors = computed(() => ready.value?.catalog.connectors ?? [])
const readyConnections = computed(() => connections.value.status === 'ready' ? connections.value.connections : [])
const connected = computed(() => readyConnections.value.map((connection) => connection.connector))
const readyGrants = computed(() => grants.value.status === 'ready' ? grants.value.grants : [])

/**
 * Every risk level and every effect the catalogue actually publishes.
 *
 * Handed to the grants screen so that a vocabulary it does not offer as a bound is something it can
 * *say*, rather than something that silently narrows the widest grant an operator can write. The
 * console keeps the ordered list because `max_risk` means "at or below" and an order cannot be
 * recovered from a set of strings; this is what keeps that list from going stale in silence.
 */
const catalogueRisks = computed(() => [
  ...new Set(
    (ready.value?.catalog.connectors ?? []).flatMap((connector) =>
      connector.operations.map((operation) => operation.risk)
    )
  ),
])
const catalogueEffects = computed(() => [
  ...new Set(
    (ready.value?.catalog.connectors ?? []).flatMap((connector) =>
      connector.operations.flatMap((operation) => operation.effects)
    )
  ),
])

/** Which surface of the platform the reader is on, so the rail can say so. */
const active = computed(() => surfaceOfRoute(route.value.name))
</script>

<template>
  <div class="console" :class="{ 'console--wide': route.name === 'workflows' || route.name === 'activity' || route.name === 'channels' || route.name === 'apps' }">
    <ConsoleShell :session="session" :sign-in-availability="signInAvailability" :active="active" @sign-out="endSession" @retry-session="reloadSession">
      <template #theme>
        <button type="button" :aria-pressed="isDark" @click="toggleTheme()">
          {{ isDark ? 'Light' : 'Dark' }}
        </button>
      </template>
    </ConsoleShell>

    <main>
      <!--
        How to connect an agent, and the head of this chain on purpose.

        `docs/vision.md` calls the agent this platform's primary caller, and this is the one screen
        that depends on nothing: no session, no catalogue, no principal. Putting it first says so
        structurally — a branch at the head of the chain has no predecessor that could gate it — and
        `the_page_is_reachable_without_a_session` asserts it stays there. An agent that must
        authenticate to learn how to authenticate is a closed loop.

        It is passed nothing, because it describes the shape of this service and never its contents.
      -->
      <template v-if="route.name === 'connect'">
        <AgentOnboarding />
      </template>

      <!--
        Where an operator mints one, and the one screen in this console that renders a credential
        value.

        It is passed the session and nothing else, and it reads `/api/service-accounts` for itself rather than
        being handed a result — deliberately, and against this file's usual arrangement. This
        component is the root, so it outlives every screen; anything it were handed would still be
        in memory after the reader had navigated away, and what the mint answers with is the one
        value on this host that cannot be shown a second time. Holding it here would make that a
        claim about `App.vue` instead of a property of the view. `Agents.mts` sets out the whole of
        it.
      -->
      <template v-else-if="route.name === 'service-accounts'">
        <Agents :session="session" />
      </template>

      <!--
        Connections — the first of the console's two jobs, and where a reader lands.

        Behind a principal, because a connection is tenant data. The gate is not an empty state: it
        says why there is nothing here and offers the one thing that changes it. `/api/signin`
        answers `303` to the identity provider, so it is an anchor the browser navigates and never
        a fetch.
      -->
      <template v-else-if="route.name === 'connections'">
        <div v-if="session.status === 'loading'" class="connections__skeleton" aria-label="Reading session">
          <span class="skeleton skeleton--title"></span><span class="skeleton"></span>
        </div>

        <!--
          The listing first, then the form. A reader lands on what is wired up — X-34's decision,
          unchanged — and the thing that changes it sits directly beneath, where the result of using
          it appears. On a successful connect the listing above is re-read, so the addresses are the
          service's answer rather than this page's memory of what was sent.
        -->
        <template v-else-if="signedIn">
          <Journey :connections="readyConnections" :grants="readyGrants" active="connect" />
          <Connections
            :state="connections"
            :rotating="rotating"
            :rotation-outcome="rotationOutcome"
            @retry="reloadConnections"
            @rotate="rotateHeld"
          />
          <Connect
            :connectors="connectors"
            :catalog-connectors="catalogConnectors"
            :connected="connected"
            :chosen="chosen"
            :plan="connectionPlan"
            :outcome="outcome"
            :authority-outcome="authorityOutcome"
            :authority-busy="authorityBusy"
            :authority-inspection="authorityInspection"
            :authority-inspecting="authorityInspecting"
            :busy="connecting"
            @choose="chooseConnector"
            @select-label="selectConnectionLabel"
            @submit="connectChosen"
            @retry="retryConnectionPlan"
            @authority="changeConnectionAuthority"
            @inspect-authority="inspectAuthority"
          />
        </template>

        <section v-else class="gate">
          <h1>Sign in to see this tenant's connections</h1>
          <p>
            A connection belongs to a tenant, and the tenant is read from whoever this service
            resolves you to be — never from anything this page could ask for. So there is nothing
            here to show until you sign in.
          </p>
          <p v-if="sessionFailure">
            <code>{{ sessionFailure.endpoint }}</code> did not answer, so this console cannot tell
            whether you are signed in. It is not saying that you are not.
          </p>
          <button v-if="sessionFailure" type="button" class="failure__retry" @click="reloadSession">
            Retry session
          </button>
          <p><a class="shell__signin" :href="SIGNIN_ENDPOINT">Sign in</a></p>
        </section>
      </template>

      <!--
        Grants — what this tenant may run, and the screen X-13's gate had been waiting for.

        Directly after connections in the rail because the two are one job in two steps: a tenant
        with a connection and no grant runs nothing at all. The screen is handed the session and
        renders its own gate from it — the route admits a `User` on the read as well as the write,
        so an agent's operator is told why rather than being shown a listing that failed.

        The connector list is the catalogue's, so this console enumerates none of its own; the risk
        and effect vocabularies are passed too, so a level the catalogue publishes and the screen
        cannot offer is something it states rather than something it silently drops.
      -->
      <template v-else-if="route.name === 'grants'">
        <Journey :connections="readyConnections" :grants="readyGrants" active="grant" />
        <Grants
          :session="session"
          :grants="grants"
          :connectors="connectors"
          :catalog-connectors="catalogConnectors"
          :connected="connected"
          :catalog="ready?.catalog"
          :channel-declarations="channelDeclarations"
          :initial-connector="route.connector"
          :catalogue-risks="catalogueRisks"
          :catalogue-effects="catalogueEffects"
          :preview="preview"
          :outcome="grantOutcome"
          :busy="granting"
          @preview="previewProposed"
          @save="saveGrants"
          @retry="reloadGrants"
        />
      </template>

      <template v-else-if="route.name === 'invoke'">
        <div v-if="catalogue.status === 'loading'" class="connections__skeleton" aria-label="Reading operation schemas">
          <span class="skeleton skeleton--title"></span><span class="skeleton"></span><span class="skeleton"></span>
        </div>
        <CatalogueFailure v-else-if="failure" :failure="failure" @retry="reloadCatalogue" />
        <template v-else-if="ready">
          <Journey :connections="readyConnections" :grants="readyGrants" active="invoke" />
          <Invoke
            :catalog="ready.catalog"
            :session="session"
            :connections="readyConnections"
            :grants="readyGrants"
            :initial-operation="route.operation"
            @retry-session="reloadSession"
          />
        </template>
      </template>

      <template v-else-if="route.name === 'workflows'">
        <Workflows
          v-if="principal?.kind === 'user'"
          :state="workflows"
          :catalog="editorCatalog"
          :activity="activity"
          :busy="workflowBusy"
          :outcome="workflowOutcome"
          @retry="reloadWorkflowSurface"
          @create="createWorkflowDraft"
          @save="saveWorkflowDraft"
          @save-graph="saveWorkflowVisual"
          @validate="validateWorkflowDraft"
          @validate-graph="validateWorkflowVisual"
          @publish="publishWorkflowDraft"
          @run="runWorkflow"
        />
        <section v-else class="gate">
          <h1>Sign in as a user to author workflows</h1>
          <p>Workflow drafts and versions belong to the tenant on the resolved user principal.</p>
          <p><a class="shell__signin" :href="SIGNIN_ENDPOINT">Sign in</a></p>
        </section>
      </template>

      <template v-else-if="route.name === 'activity'">
        <Activity
          v-if="principal?.kind === 'user'"
          :state="activity"
          :busy="workflowBusy"
          @retry="reloadActivity"
          @cancel="cancelRun"
        />
        <section v-else class="gate">
          <h1>Sign in as a user to inspect workflow activity</h1>
          <p>Run records are tenant data and are never selected by a tenant field on this page.</p>
          <p><a class="shell__signin" :href="SIGNIN_ENDPOINT">Sign in</a></p>
        </section>
      </template>

      <template v-else-if="route.name === 'channels'">
        <Channels
          v-if="principal?.kind === 'user'"
          :state="channels"
          :declarations="channelDeclarations"
          :connections="channelConnections"
          :busy="channelBusy"
          :outcome="channelOutcome"
          @retry="reloadChannels"
          @create="createHeldChannel"
          @update="updateHeldChannel"
          @remove="removeHeldChannel"
        />
        <section v-else class="gate">
          <h1>Sign in as a user to manage channels</h1>
          <p>Persistent channels belong to the tenant on the resolved user principal.</p>
          <p><a class="shell__signin" :href="SIGNIN_ENDPOINT">Sign in</a></p>
        </section>
      </template>

      <template v-else-if="route.name === 'apps'">
        <Apps
          v-if="principal?.kind === 'user'"
          :state="apps"
          :connections="readyConnections"
          :activity="appActivity"
          :outcome="appOutcome"
          :busy="appBusy"
          @retry="reloadApps"
          @install="installManagedApp"
          @chat="chatWithApp"
          @inspect="inspectApp"
        />
        <section v-else class="gate">
          <h1>Sign in as a user to install Apps</h1>
          <p>App bindings and Managed Agent runs belong to the tenant on the resolved principal.</p>
          <p><a class="shell__signin" :href="SIGNIN_ENDPOINT">Sign in</a></p>
        </section>
      </template>

      <!-- Everything below is the catalogue: reference material, and no longer the front door. -->

      <!-- Named, so a request that never comes back is visibly a request and not a blank page. -->
      <div v-else-if="catalogue.status === 'loading'" class="connections__skeleton" aria-label="Reading catalogue">
        <span class="skeleton skeleton--title"></span><span class="skeleton"></span><span class="skeleton"></span>
      </div>

      <CatalogueFailure v-else-if="failure" :failure="failure" @retry="reloadCatalogue" />

      <template v-else-if="ready">
        <template v-if="route.name === 'explorer'">
          <h1>Catalogue</h1>
          <CatalogueFinder :catalog="ready.catalog" :initial-view="route.view" />
        </template>

        <template v-else-if="route.name === 'operation'">
          <h1><code>{{ route.id }}</code></h1>
          <CatalogueOperation :catalog="ready.catalog" :id="route.id" :return-view="route.returnView" />
        </template>

        <!--
          The served catalogue carries no Flux core entries — `core` is `null` — so a `/core/…` link
          resolves to a statement that this source publishes none, rather than to a blank page or to
          the explorer. A link that no longer resolves should say so.
        -->
        <template v-else-if="route.name === 'core'">
          <h1><code>{{ route.entry }}</code></h1>
          <p>
            The flux-exchange catalogue publishes connectors and their operations. It publishes no
            Flux core entries, so nothing here answers to
            <code>/core/{{ route.kind }}/{{ route.entry }}</code
            >. <a :href="fragmentPath('/explorer')">Back to the catalogue</a>.
          </p>
        </template>

        <template v-else>
          <h1>Not found</h1>
          <p>
            Nothing in this console answers to <code>{{ route.path }}</code
            >. <a :href="fragmentPath('/explorer')">Back to the catalogue</a>.
          </p>
        </template>
      </template>
    </main>

    <!--
      The footer, and not the rail. Connecting an agent is a reference an agent author reaches for
      once, and minting one is something an operator does with the identity they already have; the
      rail states what this platform *is*, and an entry there would claim a seventh surface. The two
      destinations come first, and in that order: the reference explains what an agent is for, and
      the screen beside it is where one is created.
    -->
    <footer class="console__foot">
      <p>
        <a :href="fragmentPath(ONBOARDING_PATH)">Connect an agent</a> ·
        <a :href="fragmentPath(SERVICE_ACCOUNTS_PATH)">Service Accounts</a> · Catalogue read from
        <code>{{ CONNECTORS_ENDPOINT }}</code> ·
        <a href="https://github.com/codewandler/flux-exchange">codewandler/flux-exchange</a>
      </p>
    </footer>
  </div>
</template>

<style scoped>
.console {
  max-width: 1104px;
  margin: 0 auto;
  padding: 0 24px 64px;
}

.console--wide {
  max-width: 1560px;
}

.console__loading {
  margin: 32px 0;
  color: var(--vp-c-text-2);
}

.console__foot {
  margin-top: 48px;
  padding-top: 16px;
  border-top: 1px solid var(--vp-c-divider);
  font-size: 12px;
  color: var(--vp-c-text-3);
}

.console__foot code {
  color: inherit;
}
</style>
