// One declaration-driven connection form. App.vue owns every request; this component owns only
// rendering the versioned projection and turning its uncontrolled DOM controls into one request.

import { defineComponent, h, ref, watch, type PropType, type VNode } from 'vue'
import ConnectorPicker from './ConnectorPicker.mts'
import type { Connector } from './catalog.mts'
import {
  type ConnectionAuthorityAction,
  type ConnectionAuthorityInspectionOutcome,
  type ConnectionAuthorityInspectionRequest,
  type ConnectionAuthorityOutcome,
  type ConnectionAuthorityTransition,
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
 * Read the controls the plan admits. The value-free BEGIN and raw ordinal secret frames are built
 * separately; aliases sharing a target are consumed once.
 */
export function connectionPlanSubmission(plan: ConnectionPlan, data: FormData): ConnectionPlanSubmission {
  const name = data.get('name')
  const begin: ConnectionPlanSubmission['begin'] = {
    authorities: [],
    connector: plan.connector,
    label: typeof name === 'string' ? name : '',
    plan_revision: plan.plan_revision,
    settings: [],
    targets: [],
  }
  const secrets: ConnectionPlanSubmission['secrets'] = []
  const consumed = new Set<string>()
  const requiredTargets = new Set(plan.fields.flatMap((field) =>
    field.required && field.target !== null ? [field.target.id] : []))

  for (const field of plan.fields) {
    const target = field.target
    if (!field.routable || target === null || consumed.has(target.id)) continue
    consumed.add(target.id)
    const value = target.id === 'connection.name' ? begin.label : data.get(target.id)
    const selected = requiredTargets.has(target.id) || typeof value === 'string' && value !== ''
    if (!selected) continue
    begin.targets.push({ target: target.id, revision: target.revision })
    if (target.id === 'connection.name') continue
    if (field.secret) {
      secrets.push({ target: target.id, value: new TextEncoder().encode(typeof value === 'string' ? value : '') })
      continue
    }
    begin.settings.push({ target: target.id, value: typeof value === 'string' ? value : '' })
    if (field.authority !== null) {
      begin.authorities.push({ target: target.id, revision: field.authority.revision })
    }
  }

  return { begin, secrets }
}

function fieldControl(field: ConnectionPlanField, required: boolean): VNode {
  const target = field.target
  if (target === null) {
    return h('p', { class: 'connect__unroutable', role: 'alert' }, field.reason ?? '')
  }

  const common = {
    id: field.identity,
    name: target.id,
    'data-plan-target': target.id,
    disabled: !field.routable,
    required,
  }
  if (field.choices !== null && field.choices.length > 0) {
    return h('select', { ...common, class: 'connect__select' }, [
      h('option', { value: '', selected: true }, field.set ? 'Keep the current choice' : 'Choose…'),
      ...field.choices.map((choice) => h('option', { key: choice.value, value: choice.value }, choice.label)),
    ])
  }

  const credentialTarget = target.id.startsWith('credential.')
  // Catalogue formats are extensible. Preserve the two browser-native validation modes this
  // consumer understands and render every other open text-like format as text.
  const inputType = field.input === 'email' || field.input === 'url' ? field.input : 'text'
  return h('input', {
    ...common,
    type: field.secret || credentialTarget ? 'password' : inputType,
    ...(field.secret || credentialTarget
      ? { autocomplete: 'new-password', spellcheck: 'false' }
      : {}),
    placeholder: field.set ? 'leave blank to keep the current value' : field.help,
  })
}

/** Render the value-free review state and only the actions advertised for this exact revision. */
function authorityBody(
  plan: ConnectionPlan,
  field: ConnectionPlanField,
  busy: string,
  inspection: ConnectionAuthorityInspectionOutcome | null,
  inspecting: string,
  transition: (identity: string, request: ConnectionAuthorityTransition) => void,
  inspect: (identity: string, request: ConnectionAuthorityInspectionRequest) => void,
): VNode | null {
  const authority = field.authority
  if (authority === null) return null

  const explanation = {
    unset: 'No authority has been proposed, so the runtime has no origin to use.',
    proposed: 'The runtime cannot use it until an operator approves this revision.',
    approved: 'This revision is approved, so the runtime may use it without disclosing its value here.',
    revoked: 'This proposal is revoked, so the runtime cannot use it.',
  }[authority.state]
  const labels = {
    unset: 'No proposal', proposed: 'Approval required', approved: 'Approved', revoked: 'Revoked',
  }[authority.state]
  const action = (choice: ConnectionAuthorityAction): void => {
    if (plan.selection === null || field.service === null || field.binds === null || authority.revision === null) return
    transition(field.identity, {
      connector: plan.connector,
      label: plan.selection,
      service: field.service,
      field: field.binds,
      revision: authority.revision,
      action: choice,
    })
  }
  const working = busy === field.identity
  const reviewing = inspecting === field.identity
  const approve = authority.actions.includes('approve') ? 'approve' : undefined
  const revoke = authority.actions.includes('revoke') ? 'revoke' : undefined
  const inspected = inspection?.status === 'answered' && plan.selection !== null && field.service !== null &&
    field.binds !== null && inspection.result.connector === plan.connector &&
    inspection.result.label === plan.selection && inspection.result.service === field.service &&
    inspection.result.field === field.binds && inspection.result.authority.state === authority.state &&
    inspection.result.authority.revision === authority.revision
    ? inspection.result
    : null
  const inspectProposal = (): void => {
    if (authority.state !== 'proposed' || plan.selection === null || field.service === null ||
        field.binds === null || authority.revision === null) return
    inspect(field.identity, {
      connector: plan.connector,
      label: plan.selection,
      service: field.service,
      field: field.binds,
      state: authority.state,
      revision: authority.revision,
    })
  }

  return h('aside', {
    class: ['connect__authority', `connect__authority--${authority.state}`],
    'data-authority-state': authority.state,
  }, [
    h('div', { class: 'connect__authority-head' }, [
      h('strong', { class: 'connect__authority-badge' }, labels),
      authority.revision === null
        ? null
        : h('span', { class: 'connect__authority-revision' }, `Revision ${authority.revision}`),
    ]),
    h('p', { class: 'connect__authority-help' }, explanation),
    authority.actions.length === 0 ? null : h('div', { class: 'connect__authority-actions' }, [
      approve === undefined
        ? null
        : h('button', {
          type: 'button', disabled: working || reviewing,
          onClick: inspectProposal,
        }, reviewing ? 'Reading proposal…' : 'Review proposed origin'),
      inspected === null ? null : h('p', { class: 'connect__authority-proposal' }, [
        `Normalized origin for revision ${inspected.authority.revision}: `,
        h('code', { 'data-authority-origin': 'reviewed' }, inspected.authority.origin),
      ]),
      approve === undefined || inspected === null
        ? null
        : h('button', {
          type: 'button', disabled: working || reviewing,
          onClick: () => action(approve),
        }, working ? 'Changing authority…' : 'Approve proposed authority'),
      revoke === undefined
        ? null
        : h('button', {
          type: 'button', disabled: working || reviewing,
          onClick: () => action(revoke),
        }, working ? 'Changing authority…' : authority.state === 'proposed' ? 'Revoke proposal' : 'Revoke authority'),
    ]),
  ])
}

/** Render every descriptor, while asking once for a target that several descriptors share. */
function planFields(
  plan: ConnectionPlan,
  authorityBusy: string,
  authorityInspection: ConnectionAuthorityInspectionOutcome | null,
  authorityInspecting: string,
  transition: (identity: string, request: ConnectionAuthorityTransition) => void,
  inspect: (identity: string, request: ConnectionAuthorityInspectionRequest) => void,
): VNode {
  const controls = new Map<string, ConnectionPlanField>()
  const requiredTargets = new Set(plan.fields.flatMap((field) =>
    field.required && field.target !== null ? [field.target.id] : []))
  return h('div', { class: 'connect__fields' }, plan.fields.map((field) => {
    const target = field.target?.id ?? null
    const first = target === null ? null : controls.get(target)
    if (target !== null && first === undefined) controls.set(target, field)

    const status = field.required ? 'Required' : 'Optional'
    const metadata = [
      h('span', { class: 'connect__requirement' }, status),
      h('span', { class: 'connect__provenance', 'data-provenance': field.provenance }, field.provenance),
      field.service === null ? null : h('code', { class: 'connect__service' }, field.service),
      h('span', { class: 'connect__set' }, field.set === null ? 'Not reported' : field.set ? 'Set' : 'Missing'),
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
      control = fieldControl(field, plan.selection === null && target !== null && requiredTargets.has(target))
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
      authorityBody(plan, field, authorityBusy, authorityInspection, authorityInspecting, transition, inspect),
    ])
  }))
}

function planBody(
  state: ConnectionPlanState,
  connector: string,
  retry: () => void,
  select: (label: string) => void,
  authorityBusy: string,
  authorityInspection: ConnectionAuthorityInspectionOutcome | null,
  authorityInspecting: string,
  transition: (identity: string, request: ConnectionAuthorityTransition) => void,
  inspect: (identity: string, request: ConnectionAuthorityInspectionRequest) => void,
): VNode {
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
        planFields(plan, authorityBusy, authorityInspection, authorityInspecting, transition, inspect),
      ])
    }
  }
}

function authorityOutcomeBody(outcome: ConnectionAuthorityOutcome): VNode {
  if (outcome.status === 'refused') return refusalNotice(outcome.refusal)
  if (outcome.status === 'failed') {
    return h('section', { class: 'failure', role: 'alert', 'data-authority-outcome': 'failed' }, [
      h('h3', { class: 'failure__title' }, 'The authority change could not be read'),
      h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
    ])
  }
  if ('outcome' in outcome.result) {
    return h('section', {
      class: 'failure', role: 'alert', 'data-authority-outcome': 'partial',
    }, [
      h('h3', { class: 'failure__title' }, 'The authority change may have happened'),
      h('p', { class: 'failure__message' },
        `Revision ${outcome.result.authority.revision} may have happened. Re-read the authority state before retrying.`),
    ])
  }
  return h('p', {
    class: 'connect__authority-result', role: 'status',
    'data-authority-outcome': outcome.result.authority.state,
  }, outcome.result.authority.state === 'approved'
    ? `Revision ${outcome.result.authority.revision} is approved.`
    : `Revision ${outcome.result.authority.revision} is revoked.`)
}

function authorityInspectionFailure(outcome: ConnectionAuthorityInspectionOutcome): VNode | null {
  if (outcome.status === 'answered') return null
  if (outcome.status === 'refused') return refusalNotice(outcome.refusal)
  return h('section', { class: 'failure', role: 'alert', 'data-authority-inspection': 'failed' }, [
    h('h3', { class: 'failure__title' }, 'The proposed origin could not be read'),
    h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
  ])
}

function resultBody(outcome: ConnectionPlanOutcome): VNode {
  if (outcome.status === 'refused') return refusalNotice(outcome.refusal)
  if (outcome.status === 'failed') {
    return h('section', { class: 'failure', role: 'alert', 'data-connect': 'failed' }, [
      h('h3', { class: 'failure__title' }, 'The apply result could not be read'),
      h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
    ])
  }

  return h('section', {
    class: ['connect__result', 'connect__result--complete'],
    role: 'status',
    'data-outcome': 'complete',
  }, [
    h('h3', null, outcome.result.replayed ? 'Connection already committed' : 'Connection committed'),
    h('p', null, 'The value-free receipt is durable. Refresh the plan to read current non-secret state.'),
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
    authorityOutcome: { type: Object as PropType<ConnectionAuthorityOutcome | null>, default: null },
    authorityBusy: { type: String, default: '' },
    authorityInspection: { type: Object as PropType<ConnectionAuthorityInspectionOutcome | null>, default: null },
    authorityInspecting: { type: String, default: '' },
    busy: { type: Boolean, default: false },
  },
  emits: ['choose', 'select-label', 'submit', 'retry', 'authority', 'inspect-authority'],
  setup(props, { emit }) {
    const element = ref<HTMLFormElement | null>(null)
    watch(() => props.outcome, (outcome) => {
      // Only a durable receipt makes the proposal safe to forget. A lost response has no receipt id,
      // so the contract requires a byte-identical proposal replay; keeping the uncontrolled DOM
      // controls lets the operator retry without creating a secret-bearing reactive mirror.
      if (outcome?.status === 'answered') element.value?.reset()
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
      const ready = props.plan?.status === 'ready' && props.plan.plan.selection === null

      return h('section', { class: 'connect', 'data-connect': 'panel' }, [
        h('h2', { class: 'connect__title' }, 'Connect a connector'),
        h('p', { class: 'connect__intro' }, [
          'This form follows the value-free v2 plan. Secret controls cross only as raw local-management frames; ',
          'they never enter JSON.',
        ]),
        h('form', { class: 'connect__form', 'data-connect': 'form', ref: element, onSubmit: submit }, [
          h(ConnectorPicker, {
            connectors: picker, connected: props.connected, value: props.chosen ?? '', label: 'Connector',
            'onChoose': (id: string) => emit('choose', id),
          }),
          props.chosen !== null && props.plan !== null
            ? planBody(
              props.plan,
              props.chosen,
              () => emit('retry'),
              (label) => emit('select-label', label),
              props.authorityBusy,
              props.authorityInspection,
              props.authorityInspecting,
              (identity, request) => emit('authority', identity, request),
              (identity, request) => emit('inspect-authority', identity, request),
            )
            : null,
          h('button', {
            type: 'submit', class: 'connect__submit', 'data-connect': 'submit',
            disabled: props.busy || !ready,
          }, props.busy ? 'Connecting…' : ready ? 'Connect' : 'Select “Create a new label” to connect'),
        ]),
        props.authorityInspection === null ? null : authorityInspectionFailure(props.authorityInspection),
        props.authorityOutcome === null ? null : authorityOutcomeBody(props.authorityOutcome),
        props.outcome === null ? null : resultBody(props.outcome),
      ])
    }
  },
})
