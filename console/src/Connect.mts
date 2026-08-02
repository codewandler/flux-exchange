// Wiring a connector up — the first of the console's two jobs, and the one it could not do.
//
// X-34 built the surface that shows what is wired and said plainly that it built no way to wire
// anything: "`POST` needs per-connector credential inputs and belongs in its own story." This is
// that story, and three things about it matter more than the layout.
//
// **The inputs are the service's declaration, never a list kept here.** This element renders one
// input per name in the `Declaration` it is handed, and `service.mts` reads that from the service
// every time. A connector that gains a credential gains an input with nobody editing this console,
// which is the only version of this form that stays true.
//
// **It writes; it does not read values.** There is no branch here that renders a credential value,
// because nothing hands it one: `POST` answers with the same view `GET /api/connections` lists —
// addresses and `held` — and after a successful connect that is exactly what a reader sees, through
// the same renderer the read-only view uses. The value an operator typed lives in the input element
// they typed it into, is read once when the form is submitted, and is held nowhere else. *The
// credential never crosses the boundary; the authority does* is the north star of this whole
// service, and this is the surface where a reader would first notice if it stopped being true.
//
// **A refusal is the service's own sentence.** X-18, X-20 and X-29 were spent on those sentences.
// This element quotes `refusal.error` whole and adds nothing to it — except on the `409`, where it
// says one thing the service cannot: the remedy is X-39's rotation, and this console will not
// stitch delete-then-create together behind a button. That would put back exactly the window X-39
// exists to remove.
//
// A render function rather than a single-file component, following `Connections.mts` and
// `CatalogueFailure.mts`.

import { defineComponent, h, ref, watch, type PropType, type VNode } from 'vue'
import { connectionCard } from './Connections.mts'
import ConnectorPicker from './ConnectorPicker.mts'
import { fragmentPath } from './routing.ts'
import type { Connector } from './catalog.mts'
import { credentialEndpoint, type ConnectOutcome, type DeclarationState } from './service.mts'
import type { ServiceFailure, ServiceRefusal } from './service.mts'

/** What a failed read or write says, naming the endpoint in every branch. */
function failureSentence(reason: ServiceFailure): string {
  switch (reason.kind) {
    case 'unreachable':
      return `${reason.endpoint} could not be reached. ${reason.detail} Nothing was sent, so nothing was connected — and this is not the service saying no.`
    case 'refused':
      return `${reason.endpoint} answered ${reason.status}, with no sentence this console could read. ${reason.detail}`
    case 'unreadable':
      return `${reason.endpoint} answered ${reason.status} with a body this console could not read. ${reason.detail} What happened to the connection is therefore unknown; read the listing above before retrying.`
  }
}

/** A refusal, in the service's own words and nothing else. */
function refusalNotice(refusal: ServiceRefusal, extra: VNode[] = []): VNode {
  return h(
    'section',
    {
      class: 'failure',
      role: 'alert',
      'data-connect': 'refused',
      'data-status': String(refusal.status),
    },
    [
      h('h3', { class: 'failure__title' }, `The service refused this, answering ${refusal.status}`),
      // Verbatim, and whole. Every word of this was argued over in a story of its own; a console
      // that paraphrased it would be throwing that away and inventing a worse sentence.
      h('p', { class: 'failure__message' }, refusal.error),
      ...extra,
    ]
  )
}

/**
 * What to do about a connector that is already connected.
 *
 * The one thing the console adds to a refusal, and it is added rather than substituted: the
 * service's own sentence is above this, unedited. It is here because the service's `409` names
 * `DELETE` as the way out — it predates X-39 — and delete-then-create is precisely the window X-39
 * removed. An operator replacing a leaked secret should never be sent through a state where their
 * tenant has no connection at all.
 */
function rotationNotice(connector: string, declared: string[]): VNode {
  return h('div', { class: 'connect__rotation', 'data-connect': 'rotation' }, [
    h('p', null, [
      'To replace a value that has leaked, rotate it in place rather than disconnecting first: a ',
      h('code', null, 'PUT'),
      ' to the credential replaces it in a single write, and there is no moment in between where ',
      'this tenant has no connection.',
    ]),
    h(
      'ul',
      null,
      declared.map((credential) =>
        h('li', { key: credential }, [
          h('code', null, `PUT ${credentialEndpoint(connector, credential)}`),
        ])
      )
    ),
    h('p', { class: 'connect__aside' }, [
      'Use Rotate on the connection dashboard. It replaces one held value atomically and never ',
      'offers disconnect-then-reconnect.',
    ]),
  ])
}

/** The declaration, as inputs — one per name the service published, in its order. */
function credentialInputs(connector: string, declared: string[]): VNode {
  if (declared.length === 0) {
    return h('p', { class: 'connect__note', 'data-connect': 'declares-nothing' }, [
      h('code', null, connector),
      ' declares no credential, so there is nothing to store for it.',
    ])
  }

  return h(
    'div',
    { class: 'connect__fields' },
    declared.map((credential) =>
      h('label', { class: 'connect__field', key: credential }, [
        h('span', { class: 'connect__label' }, h('code', null, credential)),
        h('input', {
          // The name the service declared, which is also the key `POST` expects. There is no
          // translation step here and there must not be one: a name this console invented would
          // address a credential no operation reads.
          'data-credential': credential,
          name: credential,
          type: 'password',
          // Not `current-password`. A password manager offering the value it saved last time is the
          // same leak by another route, and this field is never a login.
          autocomplete: 'new-password',
          spellcheck: 'false',
          // Deliberately no `value` binding of any kind. The element is where the operator's value
          // lives and the only place it lives; binding it to state here would make this console
          // hold a credential, and a held credential is a rendered credential one refactor later.
          placeholder: 'paste the value',
        }),
      ])
    )
  )
}

/** The declaration's four states, each said as itself. */
function declarationBody(state: DeclarationState, connector: string, retry: () => void): VNode {
  switch (state.status) {
    case 'loading':
      return h('div', { class: 'connections__skeleton', 'aria-label': `Reading what ${connector} declares` }, [
        h('span', { class: 'skeleton' }), h('span', { class: 'skeleton' }),
      ])

    case 'ready':
      return credentialInputs(state.declaration.connector, state.declaration.credentials)

    case 'refused':
      return refusalNotice(state.refusal)

    case 'failed':
      // Not an empty form. A connector whose declaration could not be read and a connector that
      // declares nothing must never render alike — the second is a fact about the connector, and
      // the first is a fact about the network.
      return h(
        'section',
        { class: 'failure', role: 'alert', 'data-connect': 'declaration-failed' },
        [
          h('h3', { class: 'failure__title' }, 'What this connector declares could not be read'),
          h('p', { class: 'failure__endpoint' }, [
            'Endpoint: ',
            h('code', null, state.failure.endpoint),
          ]),
          h('p', { class: 'failure__message' }, failureSentence(state.failure)),
          h('button', { type: 'button', class: 'failure__retry', onClick: retry }, 'Retry declaration'),
        ]
      )
  }
}

/** What the last attempt did, if there has been one. */
function outcomeBody(outcome: ConnectOutcome, connector: string, declared: string[]): VNode {
  if (outcome.status === 'connected') {
    return h('section', { 'data-connect': 'connected' }, [
      h('h3', { class: 'connect__done' }, 'Connected'),
      // The same renderer the read-only listing uses, deliberately: addresses and `held`, and
      // structurally nowhere for a value to appear.
      connectionCard(outcome.connection),
      h('p', { class: 'connect__next' }, [
        h('a', { href: fragmentPath(`/grants?connector=${encodeURIComponent(connector)}`) }, 'Next: grant this connector'),
      ]),
    ])
  }

  if (outcome.status === 'refused') {
    const extra = outcome.refusal.status === 409 ? [rotationNotice(connector, declared)] : []
    return refusalNotice(outcome.refusal, extra)
  }

  return h('section', { class: 'failure', role: 'alert', 'data-connect': 'failed' }, [
    h('h3', { class: 'failure__title' }, 'This connection was not made'),
    h('p', { class: 'failure__endpoint' }, ['Endpoint: ', h('code', null, outcome.failure.endpoint)]),
    h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
  ])
}

export default defineComponent({
  name: 'Connect',
  props: {
    /** Every connector the catalogue lists, by id. The console enumerates nothing itself. */
    connectors: { type: Array as PropType<string[]>, required: true },
    /** Human catalogue facts for the searchable picker. */
    catalogConnectors: { type: Array as PropType<Connector[]>, default: () => [] },
    connected: { type: Array as PropType<string[]>, default: () => [] },
    /** The connector being connected, or `null` before one is chosen. */
    chosen: { type: String as PropType<string | null>, default: null },
    /** What it declares, or `null` when nothing is chosen. */
    declaration: { type: Object as PropType<DeclarationState | null>, default: null },
    /** What the last attempt did, or `null` when there has not been one. */
    outcome: { type: Object as PropType<ConnectOutcome | null>, default: null },
    /** Whether a connect is in flight, so the form cannot be submitted twice. */
    busy: { type: Boolean, default: false },
  },
  emits: ['choose', 'submit', 'retry'],
  setup(props, { emit }) {
    // The form element, held only so a successful connect can clear it. There is no reactive mirror
    // of what was typed, which is the point: the values exist in the DOM inputs and in the request
    // body, and in no third place this console could later render.
    const element = ref<HTMLFormElement | null>(null)

    // Cleared on success and never on a refusal. A refused attempt leaves what the operator typed
    // where they typed it, so a `413` or a typo does not cost them two long secrets — and the value
    // has still never been anywhere but the input.
    watch(
      () => props.outcome,
      (outcome) => {
        if (outcome?.status === 'connected') element.value?.reset()
      }
    )

    /** The names the service declared, or none when it has not answered with any. */
    const declared = (): string[] =>
      props.declaration?.status === 'ready' ? props.declaration.declaration.credentials : []

    function submit(event: Event): void {
      event.preventDefault()
      const form = event.currentTarget as HTMLFormElement
      const entries = new FormData(form)

      // Read by declared name, so nothing a form could carry that the connector did not declare is
      // ever sent. An empty box is an omission rather than an empty credential: a connection may
      // legitimately carry a subset of what is declared, and storing `""` would put a value nothing
      // can use at an address an operation reads.
      const values: Record<string, string> = {}
      for (const credential of declared()) {
        const value = entries.get(credential)
        if (typeof value === 'string' && value !== '') values[credential] = value
      }

      emit('submit', values)
    }

    return () => {
      const chosen = props.chosen
      const names = declared()
      const picker = props.catalogConnectors.length
        ? props.catalogConnectors
        : props.connectors.map((id) => ({ id, vendor: id, description: '', operationCount: 0, operations: [] }))

      return h('section', { class: 'connect', 'data-connect': 'panel' }, [
        h('h2', { class: 'connect__title' }, 'Connect a connector'),

        h('p', { class: 'connect__intro' }, [
          'A value entered here is sent to ',
          h('code', null, '/api/connections'),
          ' and stored by the service at the address its connector declares. It is never read back ',
          '— not by this page and not by anything else. What you see afterwards is where the ',
          'credential lives and whether something is held there.',
        ]),

        h('form', { class: 'connect__form', 'data-connect': 'form', ref: element, onSubmit: submit }, [
          h(ConnectorPicker, {
            connectors: picker,
            connected: props.connected,
            value: chosen ?? '',
            label: 'Connector',
            'onChoose': (id: string) => emit('choose', id),
          }),

          chosen && props.declaration ? declarationBody(props.declaration, chosen, () => emit('retry')) : null,

          h(
            'button',
            {
              type: 'submit',
              class: 'connect__submit',
              'data-connect': 'submit',
              disabled: props.busy || names.length === 0,
            },
            props.busy ? 'Connecting…' : 'Connect'
          ),
        ]),

        props.outcome && chosen ? outcomeBody(props.outcome, chosen, names) : null,
      ])
    }
  },
})
