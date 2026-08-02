// Invoke one catalogue operation with the exact parameter object its published schema describes.

import { computed, defineComponent, h, ref, watch, type PropType, type VNode } from 'vue'
import type { Catalog, Operation } from './catalog.mts'
import { bodyFromSchema, validateBody } from './invoking.mts'
import {
  SIGNIN_ENDPOINT,
  invokeOperation,
  type Connection,
  type HeldGrant,
  type InvokeOutcome,
  type SessionState,
} from './service.mts'
import { fragmentPath } from './routing.ts'

function operations(catalog: Catalog): Operation[] {
  return catalog.connectors.flatMap((connector) => connector.operations)
}

export default defineComponent({
  name: 'Invoke',
  props: {
    catalog: { type: Object as PropType<Catalog>, required: true },
    session: { type: Object as PropType<SessionState>, required: true },
    connections: { type: Array as PropType<Connection[]>, default: () => [] },
    grants: { type: Array as PropType<HeldGrant[]>, default: () => [] },
    initialOperation: { type: String, default: '' },
  },
  emits: ['retrySession'],
  setup(props, { emit }) {
    const chosen = ref('')
    const query = ref('')
    const body = ref('{}')
    const problems = ref<string[]>([])
    const busy = ref(false)
    const outcome = ref<InvokeOutcome | null>(null)
    const elapsed = ref<number | null>(null)

    const all = computed(() => operations(props.catalog))
    const found = computed(() => all.value.find((operation) => operation.id === chosen.value) ?? null)
    const matches = computed(() => {
      const terms = query.value.toLowerCase().trim().split(/\s+/).filter(Boolean)
      return all.value.filter((operation) => {
        const connector = props.catalog.connectors.find((entry) => entry.operations.includes(operation))
        const facts = `${operation.id} ${operation.description} ${operation.service} ${connector?.vendor ?? ''}`.toLowerCase()
        return terms.every((term) => facts.includes(term))
      }).slice(0, 20)
    })

    function select(operation: Operation) {
      chosen.value = operation.id
      query.value = operation.id
      body.value = JSON.stringify(bodyFromSchema(operation.inputSchema), null, 2)
      problems.value = []
      outcome.value = null
      elapsed.value = null
      if (typeof window !== 'undefined') {
        window.history.replaceState(
          window.history.state,
          '',
          fragmentPath(`/invoke?operation=${encodeURIComponent(operation.id)}`)
        )
      }
    }

    watch(() => props.initialOperation, (id) => {
      const operation = all.value.find((entry) => entry.id === id)
      if (operation) select(operation)
    }, { immediate: true })

    async function run() {
      const operation = found.value
      if (!operation || busy.value) return
      let params: unknown
      try {
        params = JSON.parse(body.value)
      } catch (error) {
        problems.value = [`Invalid JSON: ${error instanceof Error ? error.message : String(error)}`]
        return
      }
      problems.value = validateBody(operation.inputSchema, params)
      if (problems.value.length || typeof params !== 'object' || params === null || Array.isArray(params)) return
      busy.value = true
      outcome.value = null
      const started = performance.now()
      outcome.value = await invokeOperation(operation.id, params as Record<string, unknown>)
      elapsed.value = Math.max(0, Math.round(performance.now() - started))
      busy.value = false
    }

    function outcomeView(value: InvokeOutcome): VNode {
      if (value.status === 'invoked') {
        return h('section', { class: 'invoke__result', role: 'status', 'data-invoke': 'result' }, [
          h('h2', null, value.result.isError ? 'The operation reported an error' : 'Result'),
          h('p', { class: 'invoke__elapsed' }, `${elapsed.value ?? 0} ms from browser to response.`),
          h('pre', { class: 'invoke__content' }, value.result.content),
          value.result.view ? h('details', null, [h('summary', null, 'Model-facing view'), h('pre', null, value.result.view)]) : null,
        ])
      }
      if (value.status === 'refused') {
        return h('section', { class: 'failure', role: 'alert', 'data-invoke': 'refused' }, [
          h('h2', { class: 'failure__title' }, `Invocation refused (${value.refusal.status})`),
          h('p', { class: 'failure__message' }, value.refusal.message),
          h('p', { class: 'invoke__facts' }, `Sent: ${value.refusal.sent} · Retryable: ${value.refusal.retryable ? 'yes' : 'no'} · ${elapsed.value ?? 0} ms`),
          value.refusal.supplyAt
            ? h('p', null, h('a', { href: fragmentPath(value.refusal.supplyAt) }, 'Supply the missing connection setting'))
            : null,
          h('button', { type: 'button', class: 'failure__retry', disabled: busy.value, onClick: run }, 'Retry invocation'),
        ])
      }
      return h('section', { class: 'failure', role: 'alert', 'data-invoke': 'failed' }, [
        h('h2', { class: 'failure__title' }, 'The invocation result could not be read'),
        h('p', { class: 'failure__message' }, value.failure.detail),
        h('button', { type: 'button', class: 'failure__retry', disabled: busy.value, onClick: run }, 'Retry invocation'),
      ])
    }

    return () => {
      if (props.session.status === 'loading') return h('div', { class: 'connections__skeleton', 'aria-label': 'Reading session' }, [h('span', { class: 'skeleton skeleton--title' }), h('span', { class: 'skeleton' })])
      if (props.session.status === 'failed') return h('section', { class: 'failure', role: 'alert' }, [
        h('h1', { class: 'failure__title' }, 'The session could not be read'),
        h('p', { class: 'failure__message' }, `${props.session.failure.endpoint} did not answer. This console does not know whether you are signed in.`),
        h('button', { type: 'button', class: 'failure__retry', onClick: () => emit('retrySession') }, 'Retry session'),
      ])
      const principal = props.session.principal
      if (!principal) return h('section', { class: 'gate' }, [
        h('h1', null, 'Sign in to invoke an operation'),
        h('p', null, 'Invocation runs for the tenant the service resolves from your principal.'),
        h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT }, 'Sign in'),
      ])

      return h('section', { class: 'invoke', 'data-page': 'invoke' }, [
        h('h1', null, 'Invoke'),
        h('p', { class: 'invoke__lead' }, 'Choose an operation, review its declared parameter object, and run it as this tenant.'),
        props.connections.length === 0
          ? h('p', { class: 'invoke__prerequisite' }, ['No connector is connected. ', h('a', { href: fragmentPath('/connections') }, 'Connect one first.')])
          : props.grants.length === 0
            ? h('p', { class: 'invoke__prerequisite' }, ['This tenant has no grants. ', h('a', { href: fragmentPath('/grants') }, 'Grant a connector first.')])
            : null,
        h('label', { class: 'invoke__search' }, [
          h('span', null, 'Operation'),
          h('input', {
            type: 'search', value: query.value, placeholder: 'Search operations',
            onInput: (event: Event) => { query.value = (event.target as HTMLInputElement).value },
          }),
        ]),
        query.value && (!found.value || query.value !== found.value.id)
          ? h('ul', { class: 'invoke__choices' }, matches.value.map((operation) => h('li', { key: operation.id },
              h('button', { type: 'button', onClick: () => select(operation) }, [
                h('code', null, operation.id), h('span', null, operation.description),
              ])
            )))
          : null,
        found.value ? h('form', { class: 'invoke__form', onSubmit: (event: Event) => { event.preventDefault(); void run() } }, [
          h('p', { class: 'invoke__description' }, found.value.description),
          h('label', { class: 'invoke__body' }, [
            h('span', null, 'Parameter object (JSON)'),
            h('textarea', {
              value: body.value, rows: 12, spellcheck: 'false',
              onInput: (event: Event) => { body.value = (event.target as HTMLTextAreaElement).value; problems.value = [] },
            }),
          ]),
          problems.value.length ? h('ul', { class: 'invoke__problems', role: 'alert' }, problems.value.map((problem) => h('li', { key: problem }, problem))) : null,
          h('button', { type: 'submit', class: 'invoke__submit', disabled: busy.value }, busy.value ? 'Invoking…' : 'Invoke operation'),
        ]) : null,
        outcome.value ? outcomeView(outcome.value) : null,
      ])
    }
  },
})
