<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { VueFlow, type Edge, type Node } from '@vue-flow/core'
import type {
  ActivityState,
  EditorCatalogState,
  EditorGraph,
  EditorOperation,
  WorkflowDraft,
  WorkflowMutation,
  WorkflowsState,
} from './service.mts'

const props = defineProps<{
  state: WorkflowsState
  catalog: EditorCatalogState
  activity: ActivityState
  busy: boolean
  outcome: WorkflowMutation | null
}>()

const emit = defineEmits<{
  retry: []
  create: [id: string, title: string, source: string]
  save: [workflow: WorkflowDraft, source: string]
  saveGraph: [workflow: WorkflowDraft, graph: EditorGraph]
  validate: [workflow: WorkflowDraft, source: string]
  validateGraph: [workflow: WorkflowDraft, graph: EditorGraph]
  publish: [workflow: WorkflowDraft]
  run: [workflow: WorkflowDraft, params: unknown]
}>()

type Mode = 'tree' | 'freeform' | 'source'
const mode = ref<Mode>('tree')
const selectedId = ref('')
const title = ref('')
const source = ref('')
const params = ref('{}')
const query = ref('')
const newId = ref('')
const newTitle = ref('')
const newSource = ref('flow workflow\n  return true\n')
const history = ref<string[]>([])
const historyAt = ref(-1)
const canvasNodes = ref<Node[]>([])
const canvasEdges = ref<Edge[]>([])
const workingGraph = ref<EditorGraph | null>(null)
const editMode = ref<'source' | 'graph'>('source')
const selectedNodeId = ref('')
const nodeParams = ref('{}')
const switchBlockedId = ref('')
let nextNode = 1

const workflows = computed(() => props.state.status === 'ready' ? props.state.workflows : [])
const selected = computed(() => workflows.value.find((workflow) => workflow.id === selectedId.value) ?? null)
const dirty = computed(() => Boolean(selected.value) && (
  source.value !== selected.value?.source || title.value.trim() !== selected.value?.title || graphDirty.value
))
const graphDirty = computed(() => JSON.stringify(workingGraph.value) !== JSON.stringify(selected.value?.graph ?? null))
const displayGraph = computed(() => workingGraph.value)
const selectedNode = computed(() => workingGraph.value?.body.find((node) => node.id === selectedNodeId.value) ?? null)
const activeRun = computed(() => {
  if (props.activity.status !== 'ready' || !selected.value?.published_version) return null
  return props.activity.runs.find((run) =>
    run.workflow_id === selected.value?.id && run.version === selected.value?.published_version) ?? null
})
const nodePhases = computed(() => {
  const phases = new Map<string, string>()
  for (const entry of activeRun.value?.events ?? []) phases.set(entry.event.node_id, entry.event.phase)
  return phases
})
const operations = computed(() => {
  if (props.catalog.status !== 'ready') return []
  const needle = query.value.trim().toLowerCase()
  return props.catalog.catalog.operations.filter((operation) =>
    !needle || `${operation.id} ${operation.group} ${operation.description}`.toLowerCase().includes(needle))
})
const parsedParams = computed(() => {
  try {
    const value = JSON.parse(params.value)
    return value && typeof value === 'object' && !Array.isArray(value) ? value : null
  } catch {
    return null
  }
})

watch(workflows, (current) => {
  if (!selectedId.value && current.length) selectedId.value = current[0].id
  if (selectedId.value && !current.some((workflow) => workflow.id === selectedId.value)) {
    selectedId.value = current[0]?.id ?? ''
  }
}, { immediate: true })

watch(selected, (workflow) => {
  title.value = workflow?.title ?? ''
  source.value = workflow?.source ?? ''
  history.value = workflow ? [workflow.source] : []
  historyAt.value = workflow ? 0 : -1
  workingGraph.value = workflow?.graph ? structuredClone(workflow.graph) : null
  editMode.value = 'source'
  selectedNodeId.value = ''
  layoutGraph()
}, { immediate: true })

watch(displayGraph, () => layoutGraph(), { deep: true })
watch(nodePhases, () => layoutGraph(), { deep: true })
watch(() => props.outcome, (outcome) => {
  if (outcome?.status !== 'validated' || outcome.id !== selected.value?.id) return
  source.value = outcome.workflow.source
  workingGraph.value = outcome.workflow.graph ? structuredClone(outcome.workflow.graph) : null
  layoutGraph()
}, { deep: true })
watch(selectedNode, (node) => {
  if (!node || node.kind !== 'call') {
    nodeParams.value = '{}'
    return
  }
  const first = Array.isArray(node.args) ? node.args[0] : null
  nodeParams.value = JSON.stringify(
    first && typeof first === 'object' && first.kind === 'lit' && typeof first.value === 'object'
      ? first.value
      : {},
    null,
    2,
  )
})

function label(node: Record<string, unknown>): string {
  const kind = typeof node.kind === 'string' ? node.kind : 'node'
  if (kind === 'call' && typeof node.op === 'string') return node.op
  if (kind === 'return') return 'Return'
  if (kind === 'when') return 'When'
  if (kind === 'parallel') return 'Parallel'
  if (kind === 'repeat') return `Repeat × ${String(node.max ?? '?')}`
  if (kind === 'each') return 'For each'
  return kind
}

function layoutGraph() {
  const graph = displayGraph.value
  if (!graph) {
    canvasNodes.value = []
    canvasEdges.value = []
    return
  }
  const nodes: Node[] = []
  const edges: Edge[] = []
  let row = 0
  const walk = (items: Array<Record<string, unknown>>, depth: number, parent?: string) => {
    let previous: string | undefined
    for (const item of items) {
      const id = typeof item.id === 'string' ? item.id : `node-${row}`
      nodes.push({ id, position: { x: depth * 260, y: row++ * 116 }, data: { label: label(item), kind: item.kind, phase: nodePhases.value.get(id) } })
      if (previous) edges.push({ id: `${previous}-${id}`, source: previous, target: id })
      else if (parent) edges.push({ id: `${parent}-${id}`, source: parent, target: id })
      previous = id
      for (const key of ['then', 'otherwise', 'body']) {
        const children = item[key]
        if (Array.isArray(children)) walk(children as Array<Record<string, unknown>>, depth + 1, id)
      }
      if (Array.isArray(item.branches)) {
        for (const branch of item.branches as Array<Record<string, unknown>>) {
          if (Array.isArray(branch.body)) walk(branch.body as Array<Record<string, unknown>>, depth + 1, id)
        }
      }
    }
  }
  walk(graph.body, 0)
  canvasNodes.value = nodes
  canvasEdges.value = edges
}

function choose(workflow: WorkflowDraft) {
  if (workflow.id === selectedId.value) return
  if (dirty.value) {
    switchBlockedId.value = workflow.id
    return
  }
  selectedId.value = workflow.id
}

function discardAndOpen() {
  if (!switchBlockedId.value) return
  selectedId.value = switchBlockedId.value
  switchBlockedId.value = ''
}

function remember(value: string) {
  editMode.value = 'source'
  source.value = value
  if (history.value[historyAt.value] === value) return
  history.value = history.value.slice(0, historyAt.value + 1)
  history.value.push(value)
  historyAt.value = history.value.length - 1
}

function undo() {
  if (historyAt.value <= 0) return
  historyAt.value -= 1
  source.value = history.value[historyAt.value]
}

function redo() {
  if (historyAt.value >= history.value.length - 1) return
  historyAt.value += 1
  source.value = history.value[historyAt.value]
}

function editorKey(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey)) return
  if (event.key.toLowerCase() === 's' && selected.value) {
    event.preventDefault()
    save()
  } else if (event.key.toLowerCase() === 'z') {
    event.preventDefault()
    event.shiftKey ? redo() : undo()
  }
}

function save() {
  if (!selected.value || !title.value.trim()) return
  const workflow = { ...selected.value, title: title.value.trim() }
  if (editMode.value === 'graph' && graphDirty.value && workingGraph.value) emit('saveGraph', workflow, structuredClone(workingGraph.value))
  else emit('save', workflow, source.value)
}

function validateCurrent() {
  if (!selected.value) return
  if (editMode.value === 'graph' && graphDirty.value && workingGraph.value) emit('validateGraph', selected.value, structuredClone(workingGraph.value))
  else emit('validate', selected.value, source.value)
}

function selectNode(event: { node: Node }) {
  selectedNodeId.value = event.node.id
}

function addOperation(operation: EditorOperation) {
  if (!workingGraph.value) return
  editMode.value = 'graph'
  const id = `node-${Date.now().toString(36)}-${nextNode++}`
  workingGraph.value.body.push({
    id,
    source_path: `body[${workingGraph.value.body.length}]`,
    kind: 'call',
    op: operation.id,
    args: [{ kind: 'lit', value: {} }],
  })
  selectedNodeId.value = id
  layoutGraph()
}

function removeSelected() {
  if (!workingGraph.value || !selectedNodeId.value) return
  editMode.value = 'graph'
  workingGraph.value.body = workingGraph.value.body.filter((node) => node.id !== selectedNodeId.value)
  selectedNodeId.value = ''
  layoutGraph()
}

function moveSelected(by: number) {
  if (!workingGraph.value) return
  const at = workingGraph.value.body.findIndex((node) => node.id === selectedNodeId.value)
  const to = at + by
  if (at < 0 || to < 0 || to >= workingGraph.value.body.length) return
  editMode.value = 'graph'
  const [node] = workingGraph.value.body.splice(at, 1)
  workingGraph.value.body.splice(to, 0, node)
  layoutGraph()
}

function applyNodeParams() {
  if (!selectedNode.value || selectedNode.value.kind !== 'call') return
  try {
    const value = JSON.parse(nodeParams.value)
    if (!value || typeof value !== 'object' || Array.isArray(value)) return
    editMode.value = 'graph'
    selectedNode.value.args = [{ kind: 'lit', value }]
  } catch {
    // The disabled state below keeps an invalid object local until the author fixes it.
  }
}

const nodeParamsValid = computed(() => {
  try {
    const value = JSON.parse(nodeParams.value)
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
  } catch {
    return false
  }
})

function create() {
  if (!newId.value.trim() || !newTitle.value.trim()) return
  emit('create', newId.value.trim(), newTitle.value.trim(), newSource.value)
}

async function showSource() {
  mode.value = 'source'
  await nextTick()
  document.querySelector<HTMLTextAreaElement>('.workflow-source')?.focus()
}
</script>

<template>
  <section class="workflows" aria-labelledby="workflows-title">
    <header class="workflows__header">
      <div>
        <p class="eyebrow">Tenant automation</p>
        <h1 id="workflows-title">Workflows</h1>
        <p>Author as source or inspect the same versioned Flux as a graph. Only publication creates an operation.</p>
      </div>
      <button type="button" :disabled="busy" @click="emit('retry')">Refresh</button>
    </header>

    <div v-if="state.status === 'loading'" class="workflow-loading" aria-live="polite">Reading workflow drafts…</div>
    <div v-else-if="state.status === 'failed'" class="failure" role="alert">
      <h2>Workflows could not be read</h2>
      <p><code>{{ state.failure.endpoint }}</code> — {{ state.failure.detail }}</p>
      <button type="button" @click="emit('retry')">Try again</button>
    </div>

    <div v-else class="workflow-layout">
      <aside class="workflow-list" aria-label="Workflow drafts">
        <div class="workflow-list__heading">
          <h2>Drafts</h2><span>{{ workflows.length }}</span>
        </div>
        <button
          v-for="workflow in workflows"
          :key="workflow.id"
          type="button"
          class="workflow-card"
          :class="{ 'workflow-card--selected': selectedId === workflow.id }"
          @click="choose(workflow)"
        >
          <strong>{{ workflow.title }}</strong>
          <code>{{ workflow.id }}</code>
          <span>Draft r{{ workflow.revision }}</span>
          <span v-if="workflow.published_version">Published v{{ workflow.published_version }}</span>
          <span v-else>Not published</span>
        </button>
        <div v-if="switchBlockedId" class="workflow-switch-warning" role="alert">
          <strong>Unsaved changes</strong>
          <p>Save this draft before opening another, or discard the local edits.</p>
          <div>
            <button type="button" @click="switchBlockedId = ''">Keep editing</button>
            <button type="button" class="button-danger" @click="discardAndOpen">Discard and open</button>
          </div>
        </div>
        <p v-if="!workflows.length" class="workflow-empty">No draft exists for this tenant yet.</p>

        <details class="workflow-create" :open="!workflows.length">
          <summary>New workflow</summary>
          <label>Identifier <input v-model="newId" placeholder="incident-triage" /></label>
          <label>Title <input v-model="newTitle" placeholder="Incident triage" /></label>
          <label>Starting Flux <textarea v-model="newSource" rows="6"></textarea></label>
          <button type="button" :disabled="busy || !newId.trim() || !newTitle.trim()" @click="create">Create draft</button>
        </details>
      </aside>

      <div v-if="selected" class="workflow-editor">
        <div class="workflow-editor__bar">
          <div>
            <input v-model="title" class="workflow-title" aria-label="Workflow title" maxlength="160" />
            <div class="workflow-editor__identity">
              <code>{{ selected.id }}</code>
              <span class="draft-state" :data-dirty="dirty">{{ dirty ? 'Draft modified' : 'Draft saved' }}</span>
            </div>
          </div>
          <div class="mode-switch" aria-label="Editor mode">
            <button v-for="choice in (['tree', 'freeform', 'source'] as Mode[])" :key="choice" type="button" :aria-pressed="mode === choice" @click="mode = choice">
              {{ choice === 'freeform' ? 'Freeform' : choice[0].toUpperCase() + choice.slice(1) }}
            </button>
          </div>
        </div>

        <div v-if="displayGraph === null && mode !== 'source'" class="source-only" role="status">
          <strong>This draft is source-only.</strong>
          Its exact bytes are preserved because the upstream graph schema cannot represent every construct or comment.
          <button type="button" @click="showSource">Open source</button>
        </div>

        <div v-if="mode !== 'source' && displayGraph" class="workflow-canvas" :data-mode="mode">
          <VueFlow
            v-model:nodes="canvasNodes"
            v-model:edges="canvasEdges"
            :nodes-draggable="mode === 'freeform'"
            :nodes-connectable="false"
            :zoom-on-double-click="false"
            fit-view-on-init
            @node-click="selectNode"
          >
            <template #node-default="slotProps">
              <div class="flow-node" :data-kind="slotProps.data.kind" :data-phase="slotProps.data.phase">
                <small>{{ slotProps.data.kind }}</small>
                <strong>{{ slotProps.data.label }}</strong>
                <span v-if="slotProps.data.phase" class="flow-node__phase">{{ slotProps.data.phase.replace('_', ' ') }}</span>
              </div>
            </template>
          </VueFlow>
          <p class="canvas-hint">
            {{ mode === 'tree' ? 'Deterministic top-down layout.' : 'Drag nodes to arrange this local view; control flow still executes from Flux source.' }}
            <template v-if="activeRun"> Latest v{{ activeRun.version }} run: {{ activeRun.status }}.</template>
          </p>
        </div>

        <section v-if="mode !== 'source' && selectedNode" class="node-inspector" aria-labelledby="node-inspector-title">
          <div>
            <p class="eyebrow">Selected node</p>
            <h2 id="node-inspector-title">{{ label(selectedNode) }}</h2>
            <code>{{ selectedNode.id }}</code>
          </div>
          <label v-if="selectedNode.kind === 'call'">
            Parameter object
            <textarea v-model="nodeParams" rows="5" spellcheck="false" :aria-invalid="!nodeParamsValid"></textarea>
            <span v-if="!nodeParamsValid" class="input-error" role="alert">Parameter JSON must be an object.</span>
          </label>
          <div class="node-inspector__actions">
            <button type="button" @click="moveSelected(-1)">Move earlier</button>
            <button type="button" @click="moveSelected(1)">Move later</button>
            <button v-if="selectedNode.kind === 'call'" type="button" :disabled="!nodeParamsValid" @click="applyNodeParams">Apply parameters</button>
            <button type="button" class="button-danger" @click="removeSelected">Remove node</button>
          </div>
        </section>

        <div v-else-if="mode === 'source'" class="source-editor">
          <div class="source-toolbar">
            <span>Flux source</span>
            <button type="button" :disabled="historyAt <= 0" @click="undo">Undo</button>
            <button type="button" :disabled="historyAt >= history.length - 1" @click="redo">Redo</button>
          </div>
          <textarea
            class="workflow-source"
            :value="source"
            spellcheck="false"
            aria-label="Flux workflow source"
            @input="remember(($event.target as HTMLTextAreaElement).value)"
            @keydown="editorKey"
          ></textarea>
        </div>

        <div class="workflow-diagnostics" aria-live="polite">
          <p v-for="diagnostic in selected.diagnostics" :key="`${diagnostic.code}-${diagnostic.path}`">
            <code>{{ diagnostic.code }}</code> {{ diagnostic.message }}
          </p>
        </div>

        <div class="workflow-actions">
          <span class="workflow-action-note" aria-live="polite">
            {{ dirty ? 'Save this draft before publishing.' : 'Ready to publish this saved revision.' }}
          </span>
          <button type="button" :disabled="busy" @click="validateCurrent">Validate</button>
          <button type="button" :disabled="busy || !dirty || !title.trim()" @click="save">Save draft</button>
          <button type="button" class="button-primary" :disabled="busy || dirty" @click="emit('publish', selected)">Publish r{{ selected.revision }}</button>
        </div>

        <section class="workflow-run-panel" aria-labelledby="run-workflow-title">
          <div>
            <h2 id="run-workflow-title">Run published version</h2>
            <p v-if="selected.published_version">Targets immutable v{{ selected.published_version }}.</p>
            <p v-else>Publish this draft before it can run.</p>
            <p>Requires a grant for <code>workflow.{{ selected.id }}</code> and a separate grant for every connector it calls.</p>
          </div>
          <label>
            Parameters
            <textarea v-model="params" rows="4" spellcheck="false" :aria-invalid="parsedParams === null"></textarea>
            <span v-if="parsedParams === null" class="input-error" role="alert">Parameter JSON must be an object.</span>
          </label>
          <button type="button" :disabled="busy || !selected.published_version || parsedParams === null" @click="emit('run', selected, parsedParams)">Start run</button>
        </section>

        <p v-if="outcome?.status === 'refused'" class="workflow-outcome workflow-outcome--bad" role="alert">{{ outcome.refusal.error }}</p>
        <p v-else-if="outcome?.status === 'failed'" class="workflow-outcome workflow-outcome--bad" role="alert">{{ outcome.failure.detail }}</p>
        <p v-else-if="outcome" class="workflow-outcome" role="status">
          <template v-if="outcome.status === 'published'">Published immutable version {{ outcome.version }}.</template>
          <template v-else-if="outcome.status === 'started'">Run {{ outcome.run.id }} started.</template>
          <template v-else-if="outcome.status === 'validated'">Validation succeeded{{ outcome.workflow.graph ? ' with a graph.' : ' in source-only mode.' }}</template>
          <template v-else>Draft saved.</template>
        </p>
      </div>

      <aside class="operation-palette" aria-label="Workflow operation palette">
        <h2>Operation palette</h2>
        <input v-model="query" type="search" placeholder="Search operations" aria-label="Search workflow operations" />
        <p v-if="catalog.status === 'loading'">Reading executable operations…</p>
        <p v-else-if="catalog.status === 'failed'" role="alert">{{ catalog.failure.detail }}</p>
        <template v-else>
          <p v-if="!operations.length" class="palette-empty" role="status">
            No operations match{{ query.trim() ? ` “${query.trim()}”` : '' }}.
          </p>
          <details v-for="kind in ['connector', 'cognition']" :key="kind" open>
            <summary>{{ kind === 'connector' ? 'Connectors' : 'Pure cognition' }}</summary>
            <article v-for="operation in operations.filter((item) => item.kind === kind)" :key="operation.id" class="palette-operation">
              <code>{{ operation.id }}</code>
              <span>{{ operation.group }} · {{ operation.risk }}</span>
              <p>{{ operation.description }}</p>
              <button type="button" :disabled="busy || !displayGraph" @click="addOperation(operation)">Add to end</button>
            </article>
          </details>
        </template>
      </aside>
    </div>
  </section>
</template>
