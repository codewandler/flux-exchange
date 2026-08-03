// One declaration-driven connection form. App.vue owns every request; this component owns only
// rendering the versioned projection and turning its uncontrolled DOM controls into one request.

import { defineComponent, h, ref, watch, type PropType, type VNode } from 'vue'
import ConnectorPicker from './ConnectorPicker.mts'
import type { Connector } from './catalog.mts'
import {
  CONNECTION_PLAN_VERSION,
  type ConnectionPlan,
  type ConnectionPlanField,
  type ConnectionPlanOutcome,
  type ConnectionPlanState,
  type ConnectionPlanSubmission,
  type ServiceFailure,
  type ServiceRefusal,
} from './service.mts'

function failureSentence(reason: ServiceFailure): string {
  switch (reason.kind) {
    case 'unreachable':
      return `${reason.endpoint} could not be reached. ${reason.detail} Nothing was sent.`
    case 'refused':
      return `${reason.endpoint} answered ${reason.status}, with no sentence this console could read. ${reason.detail}`
    case 'unreadable':
      return `${reason.endpoint} answered ${reason.status} with a body this console could not read. ${reason.detail}`
  }
}

function refusalNotice(refusal: ServiceRefusal): VNode {
  return h('section', {
    class: 'failure', role: 'alert', 'data-connect': 'refused', 'data-status': String(refusal.status),
  }, [
    h('h3', { class: 'failure__title' }, `The service refused this, answering ${refusal.status}`),
    h('p', { class: 'failure__message' }, refusal.error),
  ])
}

/**
 * Read the controls the plan admits. Values are materialised only for the immediate request and
 * are keyed by the service's target identity; aliases sharing a target are consumed once.
 */
export function connectionPlanSubmission(plan: ConnectionPlan, data: FormData): ConnectionPlanSubmission {
  const name = data.get('name')
  const values: Record<string, string> = {}
  const consumed = new Set<string>()

  for (const field of plan.fields) {
    const target = field.target?.id
    if (!field.routable || target === undefined || target === 'connection.name' || consumed.has(target)) continue
    consumed.add(target)
    const value = data.get(target)
    if (typeof value === 'string' && value !== '') values[target] = value
  }

  const selected = plan.selection
  const chosenName = typeof name === 'string' ? name : ''
  return {
    version: CONNECTION_PLAN_VERSION,
    name: chosenName,
    ...(selected !== null && selected !== chosenName ? { current_name: selected } : {}),
    values,
  }
}

function fieldControl(field: ConnectionPlanField): VNode {
  const target = field.target
  if (target === null) {
    return h('p', { class: 'connect__unroutable', role: 'alert' }, field.reason)
  }

  const common = {
    id: field.identity,
    name: target.id,
    'data-plan-target': target.id,
    disabled: !field.routable,
  }
  if (field.choices !== undefined && field.choices.length > 0) {
    return h('select', { ...common, class: 'connect__select' }, [
      h('option', { value: '', selected: true }, field.set ? 'Keep the current choice' : 'Choose…'),
      ...field.choices.map((choice) => h('option', { key: choice.value, value: choice.value }, choice.label)),
    ])
  }

  const credentialTarget = target.id.startsWith('credential.')
  return h('input', {
    ...common,
    type: field.secret || credentialTarget ? 'password' : field.input === 'email' ? 'email' : 'text',
    ...(field.secret || credentialTarget
      ? { autocomplete: 'new-password', spellcheck: 'false' }
      : {}),
    placeholder: field.set ? 'leave blank to keep the current value' : field.help,
  })
}

/** Render every descriptor, while asking once for a target that several descriptors share. */
function planFields(plan: ConnectionPlan): VNode {
  const controls = new Map<string, ConnectionPlanField>()
  return h('div', { class: 'connect__fields' }, plan.fields.map((field) => {
    const target = field.target?.id ?? null
    const first = target === null ? null : controls.get(target)
    if (target !== null && first === undefined) controls.set(target, field)

    const status = field.required ? 'Required' : 'Optional'
    const metadata = [
      h('span', { class: 'connect__requirement' }, status),
      h('span', { class: 'connect__provenance', 'data-provenance': field.provenance }, field.provenance),
      field.service === null ? null : h('code', { class: 'connect__service' }, field.service),
      h('span', { class: 'connect__set' }, field.set ? 'Set' : 'Missing'),
    ]

    let control: VNode
    if (field.identity === 'connection.name') {
      control = h('input', {
        id: field.identity, name: 'name', type: 'text', required: true, value: plan.selection ?? '', autocomplete: 'off',
        'data-plan-target': 'connection.name',
      })
    } else if (first !== undefined && first !== null) {
      control = h('p', { class: 'connect__shared', 'data-shared-target': target }, [
        'Uses the same submitted value as ', h('strong', null, first.label), '.',
      ])
    } else {
      control = fieldControl(field)
    }

    return h('section', {
      key: field.identity,
      class: ['connect__field', !field.routable && 'connect__field--unroutable'],
      'data-plan-field': field.identity,
      'data-required': String(field.required),
      'data-routable': String(field.routable),
    }, [
      h('div', { class: 'connect__field-head' }, [
        h(first !== undefined && first !== null ? 'span' : 'label', {
          class: 'connect__label', for: first === undefined || first === null ? field.identity : undefined,
        }, field.label),
        ...metadata,
      ]),
      h('p', { class: 'connect__help' }, field.help),
      control,
    ])
  }))
}

function planBody(state: ConnectionPlanState, connector: string, retry: () => void, select: (label: string) => void): VNode {
  switch (state.status) {
    case 'loading':
      return h('div', { class: 'connections__skeleton', 'aria-label': `Reading ${connector}'s connection plan` }, [
        h('span', { class: 'skeleton' }), h('span', { class: 'skeleton' }),
      ])
    case 'refused':
      return refusalNotice(state.refusal)
    case 'failed':
      return h('section', { class: 'failure', role: 'alert', 'data-connect': 'plan-failed' }, [
        h('h3', { class: 'failure__title' }, 'The connection plan could not be read'),
        h('p', { class: 'failure__message' }, failureSentence(state.failure)),
        h('button', { type: 'button', class: 'failure__retry', onClick: retry }, 'Retry plan'),
      ])
    case 'ready': {
      const plan = state.plan
      return h('div', { 'data-plan-state': plan.state }, [
        h('p', { class: ['connect__state', `connect__state--${plan.state}`] },
          plan.state === 'complete' ? 'Complete' : 'Incomplete'),
        plan.labels.length === 0 ? h('p', { class: 'connect__note' }, 'No labelled connections exist yet.') : h('label', {
          class: 'connect__field connect__labels',
        }, [
          h('span', { class: 'connect__label' }, 'Existing labels'),
          h('select', {
            class: 'connect__select', 'data-connect': 'labels', value: plan.selection ?? '',
            onChange: (event: Event) => select((event.target as HTMLSelectElement).value),
          }, [
            h('option', { value: '' }, 'Create a new label'),
            ...plan.labels.map((label) => h('option', { key: label, value: label }, label)),
          ]),
        ]),
        planFields(plan),
      ])
    }
  }
}

function resultBody(outcome: ConnectionPlanOutcome): VNode {
  if (outcome.status === 'refused') return refusalNotice(outcome.refusal)
  if (outcome.status === 'failed') {
    return h('section', { class: 'failure', role: 'alert', 'data-connect': 'failed' }, [
      h('h3', { class: 'failure__title' }, 'The apply result could not be read'),
      h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
    ])
  }

  const result = outcome.result
  const title = {
    complete: 'Connection complete', incomplete: 'Connection incomplete',
    refused: 'Connection refused', partial: 'Connection partially applied',
  }[result.outcome]
  return h('section', {
    class: ['connect__result', `connect__result--${result.outcome}`],
    role: result.outcome === 'complete' ? 'status' : 'alert',
    'data-outcome': result.outcome,
  }, [
    h('h3', null, title),
    result.steps.length === 0 ? null : h('ol', { class: 'connect__steps' }, result.steps.map((step) =>
      h('li', { key: step.target, 'data-step-outcome': step.outcome }, [
        h('code', null, step.target), ` — ${step.outcome}`, step.reason ? `: ${step.reason}` : '',
      ])
    )),
    h('p', null, result.plan.state === 'complete'
      ? 'The fresh plan reports every required field set.'
      : 'The fresh plan still reports missing or unroutable required fields.'),
  ])
}

export default defineComponent({
  name: 'Connect',
  props: {
    connectors: { type: Array as PropType<string[]>, required: true },
    catalogConnectors: { type: Array as PropType<Connector[]>, default: () => [] },
    connected: { type: Array as PropType<string[]>, default: () => [] },
    chosen: { type: String as PropType<string | null>, default: null },
    plan: { type: Object as PropType<ConnectionPlanState | null>, default: null },
    outcome: { type: Object as PropType<ConnectionPlanOutcome | null>, default: null },
    busy: { type: Boolean, default: false },
  },
  emits: ['choose', 'select-label', 'submit', 'retry'],
  setup(props, { emit }) {
    const element = ref<HTMLFormElement | null>(null)
    watch(() => props.outcome, (outcome) => {
      // Once any answer arrives, submitted values have no reason to remain in the DOM. The fresh
      // value-free plan says what survived and the operator can retry only what is still missing.
      if (outcome !== null) element.value?.reset()
    })

    function submit(event: Event): void {
      event.preventDefault()
      if (props.plan?.status !== 'ready') return
      const plan = props.plan.plan
      emit(
        'submit',
        plan.connector,
        plan.selection,
        connectionPlanSubmission(plan, new FormData(event.currentTarget as HTMLFormElement))
      )
    }

    return () => {
      const picker = props.catalogConnectors.length > 0
        ? props.catalogConnectors
        : props.connectors.map((id) => ({ id, vendor: id, description: '', operationCount: 0, channelCount: 0, operations: [] }))
      const ready = props.plan?.status === 'ready'

      return h('section', { class: 'connect', 'data-connect': 'panel' }, [
        h('h2', { class: 'connect__title' }, 'Connect a connector'),
        h('p', { class: 'connect__intro' }, [
          'This form is exactly the versioned plan the service returned. Stored values never come back; ',
          'only whether each declared field is set.',
        ]),
        h('form', { class: 'connect__form', 'data-connect': 'form', ref: element, onSubmit: submit }, [
          h(ConnectorPicker, {
            connectors: picker, connected: props.connected, value: props.chosen ?? '', label: 'Connector',
            'onChoose': (id: string) => emit('choose', id),
          }),
          props.chosen !== null && props.plan !== null
            ? planBody(props.plan, props.chosen, () => emit('retry'), (label) => emit('select-label', label))
            : null,
          h('button', {
            type: 'submit', class: 'connect__submit', 'data-connect': 'submit',
            disabled: props.busy || !ready,
          }, props.busy ? 'Applying…' : 'Apply connection plan'),
        ]),
        props.outcome === null ? null : resultBody(props.outcome),
      ])
    }
  },
})
