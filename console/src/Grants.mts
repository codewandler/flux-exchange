// What this tenant may run, and where an operator changes it.
//
// X-13 closed the invocation gate fail-closed and X-62's service half gave it three routes. This is
// the screen, and one sentence from the story shapes every decision below:
//
//   > **A grant nobody can evaluate before saving is a grant somebody sets too wide.**
//
// So this is not a form that saves and then reports. **Saving is refused until the service has said
// what the grant would admit**, and what it said is on the screen next to the button —
// `the_save_is_refused_until_the_preview_has_answered` is what holds that, and it is the whole
// reason the preview route exists.
//
// **Nothing here decides admission.** The list of operations a selector admits is
// `POST /api/grants/preview`'s answer, from `OperationFacts::of` through
// `ConnectorSurface::admitted` — the projection the gate itself decides on. A TypeScript
// reimplementation would be a second answer to one question, and the one an operator read before
// saving would be the one that is *not* deciding.
//
// **And nothing here can name an operation.** The draft is a connector and three axes; there is no
// field, no control and no branch that could carry an id, which is `granting.mts`'s and
// `service.mts`'s half of the same rule. The route refuses six spellings with a `422`; this console's
// claim is the stronger one, that it has nowhere to put one.
//
// **This screen takes what it renders as props and emits what it wants done**, which is `App.vue`'s
// ordinary arrangement. A grant is a policy, carries no secret and is meant to be read back. What
// the boundary buys is that every claim below can be driven from fixtures with no transport at all.
//
// A render function rather than a single-file component, following `Agents.mts` and `Connect.mts`:
// the claims are only worth anything if a test drives them, and a render function mounts under a
// plain `node --test`. Its rules live in `grants.css` for the reason `shell.css` gives.

import { computed, defineComponent, h, ref, watch, type PropType, type VNode } from 'vue'
import ConnectorPicker from './ConnectorPicker.mts'
import type { Catalog, Connector } from './catalog.mts'
import { grantPreset, groupAdmitted, previewChange, type GrantPreset } from './journey-model.mts'
import { fragmentPath } from './routing.ts'
import {
  EFFECTS,
  RISK_LEVELS,
  blocking,
  mayGrant,
  replacing,
  unknownEffects,
  unknownRisks,
  without,
} from './granting.mts'
import { GRANTS_ENDPOINT, SIGNIN_ENDPOINT } from './service.mts'
import type {
  AdmittedOperation,
  ChannelDeclarationsState,
  GrantOutcome,
  GrantsState,
  HeldGrant,
  PreviewState,
  ProposedGrant,
  ServiceFailure,
  ServiceRefusal,
  SessionState,
} from './service.mts'

/** Which of the five things this screen can be, from the session and nothing else. */
type Gate = 'loading' | 'unknown' | 'anonymous' | 'may-not-grant' | 'may-grant'

/** What the session says this reader may do here. Follows `Agents.mts`, for the same route shape. */
function gateOf(session: SessionState): Gate {
  if (session.status === 'loading') return 'loading'
  // Not "signed out". This console does not know, and reporting an outage as a sign-out is the
  // collapse `ConsoleShell` and `CatalogueFailure` both exist to prevent.
  if (session.status === 'failed') return 'unknown'
  if (!session.principal) return 'anonymous'
  return mayGrant(session.principal) ? 'may-grant' : 'may-not-grant'
}

/** What a failed read or write says, naming the endpoint in every branch. Follows `Connect.mts`. */
function failureSentence(reason: ServiceFailure): string {
  switch (reason.kind) {
    case 'unreachable':
      return `${reason.endpoint} could not be reached. ${reason.detail} Nothing was sent, so nothing was granted or revoked — and this is not the service saying no.`
    case 'refused':
      return `${reason.endpoint} answered ${reason.status}, with no sentence this console could read. ${reason.detail}`
    case 'unreadable':
      return `${reason.endpoint} answered ${reason.status} with a body this console could not read. ${reason.detail} What this tenant may run is therefore unknown; re-read this page before changing anything.`
  }
}

/** A refusal, in the service's own words and nothing added to them. */
function refusalNotice(refusal: ServiceRefusal, extra: VNode[] = []): VNode {
  return h(
    'section',
    {
      class: 'failure',
      role: 'alert',
      'data-grants': 'refused',
      'data-status': String(refusal.status),
    },
    [
      // Verbatim, and whole. `routes::grants::refused` composes these sentences and carries the
      // argument in each; a console that paraphrased would be inventing a worse one.
      h('h3', { class: 'failure__title' }, `The service refused this, answering ${refusal.status}`),
      h('p', { class: 'failure__message' }, refusal.error),
      ...extra,
    ]
  )
}

/** A selector in words, in the vocabulary the service takes it back in. */
function selectorLine(grant: HeldGrant): VNode {
  const bounds: VNode[] = []

  bounds.push(
    grant.selector.maxRisk === null
      ? h('span', null, 'any risk')
      : h('span', null, ['at most ', h('code', null, grant.selector.maxRisk)])
  )
  if (grant.selector.effectsWithin !== null) {
    bounds.push(h('span', null, ['effects within ', h('code', null, grant.selector.effectsWithin.join(', ') || 'none')]))
  }
  if (grant.selector.idempotency !== null) {
    bounds.push(h('span', null, [h('code', null, grant.selector.idempotency), ' only']))
  }

  return h(
    'p',
    { class: 'grants__selector', 'data-grants': 'selector' },
    bounds.flatMap((bound, at) => (at === 0 ? [bound] : [' · ', bound]))
  )
}

/** One admitted operation, with the facts it was admitted **on** — which is why the list is here. */
function admittedEntry(operation: AdmittedOperation): VNode {
  return h('li', { class: 'grants__admitted-entry', key: operation.id, 'data-admits': operation.id }, [
    h('code', { class: 'grants__admitted-id' }, operation.id),
    h('span', { class: 'grants__tag' }, operation.risk),
    h('span', { class: 'grants__tag grants__tag--quiet' }, operation.idempotency),
  ])
}

/**
 * What a grant admits, read against what the connector declares.
 *
 * The count is stated with its denominator on purpose: *3 admitted* means nothing until a reader
 * knows whether the connector declares four or four hundred, and a bound that turns out to admit
 * everything is exactly the mistake this panel exists to make visible.
 */
function admittedPanel(grant: HeldGrant): VNode {
  return h('div', { class: 'grants__admitted', 'data-grants': 'admits' }, [
    h(
      'p',
      { class: 'grants__count', 'data-count': String(grant.admits.length), 'data-declares': String(grant.declares) },
      `Admits ${grant.admits.length} of the ${grant.declares} operations ${grant.connector} declares.`
    ),
    grant.admits.length === 0
      ? h(
          'p',
          { class: 'grants__none' },
          'Nothing. A grant that admits no operation is held and runs nothing — narrower than not holding it at all only in that it is visible.'
        )
      : h('ul', { class: 'grants__admitted-list' }, grant.admits.map(admittedEntry)),
  ])
}

function inboundPanel(grant: HeldGrant): VNode | null {
  const inbound = grant.inbound ?? []
  if (!inbound.length) return null
  return h('div', { class: 'grants__inbound', 'data-grants': 'inbound' }, [
    h('p', { class: 'grants__count' }, 'Inbound channel events'),
    h('ul', { class: 'grants__inbound-list' }, inbound.map((entry) =>
      h('li', { key: entry.binding }, [
        h('code', null, entry.binding),
        h('span', null, entry.events.join(', ')),
      ])
    )),
  ])
}

/**
 * A grant this surface could not have written, shown as stored.
 *
 * The service marks it `expressible: false` and refuses to let this console replace the set while it
 * is held. That refusal is right — the only evidence of a silent drop would be an operation running
 * that used to be refused — but a `409` is not an answer to anybody, so this is where it becomes
 * one: what is in the file, what would be lost, and where to change it.
 */
function inexpressibleNotice(grant: HeldGrant): VNode {
  const names = (title: string, ids: string[]) =>
    ids.length === 0
      ? null
      : h('p', { class: 'grants__exempt-line' }, [
          h('strong', null, `${title}: `),
          ...ids.flatMap((id, at) => (at === 0 ? [h('code', null, id)] : [', ', h('code', null, id)])),
        ])

  return h('div', { class: 'grants__inexpressible', 'data-grants': 'inexpressible' }, [
    h('p', { class: 'grants__inexpressible-reason' }, grant.reason),
    grant.exempt ? names('Always admitted', grant.exempt.always) : null,
    grant.exempt ? names('Never admitted', grant.exempt.never) : null,
    h(
      'p',
      { class: 'grants__aside' },
      'This screen writes a connector and a predicate, and nothing that names an operation — so it ' +
        'cannot write this grant back. Changing any grant from here would replace the whole set and ' +
        'drop what is listed above, so the service refuses the write while this is held. Edit it ' +
        'where it was written, in the grant file this deployment configured.'
    ),
  ].filter((node): node is VNode => node !== null))
}

/** One grant a tenant holds: what it selects, what that currently admits, and how to revoke it. */
function grantCard(grant: HeldGrant, revoke: (() => void) | null): VNode {
  return h(
    'article',
    { class: 'grants__card', 'data-grant': grant.connector, 'data-expressible': String(grant.expressible) },
    [
      h('header', { class: 'grants__card-head' }, [
        h('h3', { class: 'grants__connector' }, [h('code', null, grant.connector), ` — ${grant.vendor}`]),
        revoke
          ? h(
              'button',
              { type: 'button', class: 'grants__revoke', 'data-grants': 'revoke', onClick: revoke },
              'Revoke'
            )
          : null,
      ].filter((node): node is VNode => node !== null)),
      selectorLine(grant),
      grant.expressible ? null : inexpressibleNotice(grant),
      inboundPanel(grant),
      admittedPanel(grant),
    ].filter((node): node is VNode => node !== null)
  )
}

export default defineComponent({
  name: 'Grants',
  props: {
    /** What `/api/session` said. Decides whether there is a form at all. */
    session: { type: Object as PropType<SessionState>, required: true },
    /** What this tenant holds, as the service answered. */
    grants: { type: Object as PropType<GrantsState>, required: true },
    /** Every connector the catalogue lists. The console enumerates none of its own. */
    connectors: { type: Array as PropType<string[]>, required: true },
    catalogConnectors: { type: Array as PropType<Connector[]>, default: () => [] },
    connected: { type: Array as PropType<string[]>, default: () => [] },
    catalog: { type: Object as PropType<Catalog>, default: null },
    channelDeclarations: {
      type: Object as PropType<ChannelDeclarationsState>,
      default: () => ({ status: 'ready', declarations: [] }),
    },
    initialConnector: { type: String, default: '' },
    /** Every risk level the catalogue publishes, so a level this console cannot offer is visible. */
    catalogueRisks: { type: Array as PropType<string[]>, default: () => [] },
    /** Every effect the catalogue publishes, for `catalogueRisks`' reason. */
    catalogueEffects: { type: Array as PropType<string[]>, default: () => [] },
    /** What the service says the draft would admit, or `null` before it has been asked. */
    preview: { type: Object as PropType<PreviewState | null>, default: null },
    /** What the last write did, or `null` when there has not been one. */
    outcome: { type: Object as PropType<GrantOutcome | null>, default: null },
    /** Whether a write is in flight, so the form cannot be submitted twice. */
    busy: { type: Boolean, default: false },
  },
  emits: ['preview', 'save', 'retry'],
  setup(props, { emit }) {
    // ---------------------------------------------------------------------------------------
    // The draft. A connector and three axes, and there is nowhere here for an operation id.
    // ---------------------------------------------------------------------------------------

    const connector = ref(props.initialConnector)
    const preset = ref<GrantPreset>('read-only')
    /** `''` is "any risk" — the axis absent, which is how the service spells unbounded. */
    const maxRisk = ref('low')
    const idempotency = ref('')
    /** Whether the effects axis bounds anything at all. `false` sends no `effects_within`. */
    const boundEffects = ref(false)
    const effects = ref<string[]>([])
    const inbound = ref<Record<string, string[]>>({})

    const declaredInbound = computed(() =>
      props.channelDeclarations.status === 'ready'
        ? props.channelDeclarations.declarations.filter((entry) =>
            entry.connector === connector.value && entry.transport === 'socket')
        : []
    )

    function loadInbound(id: string): void {
      const existing = held.value.find((grant) => grant.connector === id)
      inbound.value = Object.fromEntries(
        (existing?.inbound ?? []).map((entry) => [entry.binding, [...entry.events]])
      )
    }

    function chooseConnector(id: string): void {
      connector.value = id
      loadInbound(id)
      if (id) applyPreset(preset.value)
      else changed()
    }

    function toggleInbound(binding: string, event: string, checked: boolean): void {
      const selected = new Set(inbound.value[binding] ?? [])
      if (checked) selected.add(event)
      else selected.delete(event)
      inbound.value = { ...inbound.value, [binding]: [...selected] }
      changed()
    }

    function applyPreset(next: GrantPreset): void {
      preset.value = next
      const selector = grantPreset(next, props.catalogueEffects)
      maxRisk.value = selector.maxRisk ?? ''
      idempotency.value = selector.idempotency ?? ''
      boundEffects.value = selector.effectsWithin !== null
      effects.value = selector.effectsWithin ?? []
      changed()
    }

    /** What the operator has stated, or `null` before they have chosen a connector. */
    const draft = (): ProposedGrant | null =>
      connector.value === ''
        ? null
        : {
            connector: connector.value,
            selector: {
              maxRisk: maxRisk.value === '' ? null : maxRisk.value,
              effectsWithin: boundEffects.value ? [...effects.value] : null,
              idempotency: idempotency.value === '' ? null : idempotency.value,
            },
            ...(() => {
              const selected = props.channelDeclarations.status === 'ready'
                ? declaredInbound.value.flatMap((entry) => {
                    const events = inbound.value[entry.name] ?? []
                    return events.length ? [{ binding: entry.name, events: [...events] }] : []
                  })
                : held.value
                    .find((grant) => grant.connector === connector.value)
                    ?.inbound?.map((entry) => ({ binding: entry.binding, events: [...entry.events] })) ?? []
              return selected.length ? { inbound: selected } : {}
            })(),
          }

    /**
     * Ask the service what the draft would admit.
     *
     * Called on **every** change, including the one that clears the connector — which emits nothing
     * and leaves the panel to render "choose a connector". The answer is the service's; this
     * function's only job is that the question is asked again whenever the answer would differ.
     */
    function changed(): void {
      const proposed = draft()
      if (proposed !== null) emit('preview', proposed)
    }

    const held = computed<HeldGrant[]>(() =>
      props.grants.status === 'ready' ? props.grants.grants : []
    )
    const blocked = computed<HeldGrant[]>(() => blocking(held.value))

    watch(() => props.initialConnector, (next) => {
      if (next) { connector.value = next; loadInbound(next); applyPreset('read-only') }
    }, { immediate: true })

    watch(() => props.grants, () => {
      if (connector.value) {
        loadInbound(connector.value)
        changed()
      }
    }, { deep: true })

    /**
     * Whether the preview on screen is an answer about the draft on screen.
     *
     * The connector is compared because a preview for another one is the clearest way this could be
     * wrong; `App.vue` discards answers that arrive after a newer question, so this is the second
     * half of the same guard rather than the whole of it. Anything but `ready` is not an answer.
     */
    const evaluated = computed<HeldGrant | null>(() => {
      const preview = props.preview
      if (preview?.status !== 'ready') return null
      return preview.grant.connector === connector.value ? preview.grant : null
    })

    /**
     * Whether saving is offered — and the third clause is this screen's whole argument.
     *
     * A draft, a readable set that this surface can replace faithfully, **and a preview of the draft
     * itself**. A form that could be submitted before the service had said what it admits would be
     * the "saves and then tells you what happened" this screen exists not to be.
     */
    const savable = computed<boolean>(
      () =>
        !props.busy &&
        gateOf(props.session) === 'may-grant' &&
        props.grants.status === 'ready' &&
        props.grants.editable &&
        draft() !== null &&
        evaluated.value !== null
    )

    function save(event: Event): void {
      event.preventDefault()
      const proposed = draft()
      if (!savable.value || proposed === null) return

      const next = replacing(held.value, proposed)
      // `null` is `granting.mts` refusing to compose a set that would drop what it cannot express.
      // `savable` has already checked `editable`, so this is the belt to that brace rather than a
      // path a reader can reach — and it refuses rather than sending something narrower.
      if (next === null) return

      emit('save', next)
    }

    function revoke(id: string): void {
      const next = without(held.value, id)
      if (next === null || props.busy) return
      emit('save', next)
    }

    // ---------------------------------------------------------------------------------------
    // The views.
    // ---------------------------------------------------------------------------------------

    /** A labelled chooser over a written-out vocabulary, plus the "bounds nothing" option. */
    const chooser = (
      name: string,
      label: string,
      hint: string,
      value: string,
      any: string,
      options: readonly string[]
    ): VNode =>
      h('label', { class: 'grants__field' }, [
        h('span', { class: 'grants__label' }, label),
        h(
          'select',
          {
            class: 'grants__select',
            'data-grants': name,
            value,
            onChange: (event: Event) => {
              const chosen = (event.target as HTMLSelectElement).value
              preset.value = 'custom'
              if (name === 'max-risk') maxRisk.value = chosen
              if (name === 'idempotency') idempotency.value = chosen
              changed()
            },
          },
          [
            h('option', { value: '', selected: value === '' }, any),
            ...options.map((option) =>
              h('option', { key: option, value: option, selected: option === value }, option)
            ),
          ]
        ),
        h('span', { class: 'grants__hint' }, hint),
      ])

    /** The effects axis: a bound that is off by default, because most grants do not want one. */
    function effectsField(): VNode {
      return h('fieldset', { class: 'grants__field grants__field--effects' }, [
        h('legend', { class: 'grants__label' }, 'Effects'),
        h('label', { class: 'grants__check' }, [
          h('input', {
            type: 'checkbox',
            'data-grants': 'bound-effects',
            checked: boundEffects.value,
            onChange: (event: Event) => {
              preset.value = 'custom'
              boundEffects.value = (event.target as HTMLInputElement).checked
              changed()
            },
          }),
          h('span', null, 'Admit only operations whose effects are within a set I choose'),
        ]),
        boundEffects.value
          ? h(
              'div',
              { class: 'grants__effects' },
              EFFECTS.map((effect) =>
                h('label', { class: 'grants__check', key: effect }, [
                  h('input', {
                    type: 'checkbox',
                    'data-effect': effect,
                    checked: effects.value.includes(effect),
                    onChange: (event: Event) => {
                      preset.value = 'custom'
                      const on = (event.target as HTMLInputElement).checked
                      effects.value = on
                        ? [...effects.value, effect]
                        : effects.value.filter((name) => name !== effect)
                      changed()
                    },
                  }),
                  h('code', null, effect),
                ])
              )
            )
          : null,
        h(
          'span',
          { class: 'grants__hint' },
          'A subset test, not a bound: an operation is admitted only if every effect it declares is ' +
            'in this set. Everything this build carries declares `network` and nothing else, so a ' +
            'set without it admits nothing today — and a set with it refuses the first operation ' +
            'that ever reports another.'
        ),
      ].filter((node): node is VNode => node !== null))
    }

    /** What the service said the draft admits — the panel this whole screen is arranged around. */
    function previewPanel(): VNode {
      const preview = props.preview

      if (connector.value === '') {
        return h(
          'p',
          { class: 'grants__note', 'data-grants': 'preview-idle' },
          'Choose a connector, and this will say which of its operations the grant would admit — before anything is saved.'
        )
      }
      if (preview === null || preview.status === 'loading') {
        return h('div', { class: 'connections__skeleton', 'data-grants': 'preview-loading', 'aria-label': 'Reading grant preview' }, [
          h('span', { class: 'skeleton' }), h('span', { class: 'skeleton' }),
        ])
      }
      if (preview.status === 'refused') return refusalNotice(preview.refusal)
      if (preview.status === 'failed') {
        // Not an empty list. A preview that could not be read and a grant that admits nothing must
        // never render alike: the second is a fact about the selector, the first about the network.
        return h('section', { class: 'failure', role: 'alert', 'data-grants': 'preview-failed' }, [
          h('h3', { class: 'failure__title' }, 'What this grant would admit could not be read'),
          h('p', { class: 'failure__endpoint' }, ['Endpoint: ', h('code', null, preview.failure.endpoint)]),
          h('p', { class: 'failure__message' }, failureSentence(preview.failure)),
          h('button', { type: 'button', class: 'failure__retry', onClick: changed }, 'Retry preview'),
        ])
      }

      const answer = evaluated.value
      if (answer === null) {
        return h('div', { class: 'grants__note', 'data-grants': 'preview-stale' }, [
          h('p', null, 'This preview belongs to an older draft.'),
          h('button', { type: 'button', class: 'failure__retry', onClick: changed }, 'Refresh preview'),
        ])
      }

      return h('section', { class: 'grants__preview', 'data-grants': 'preview' }, [
        h('h3', null, 'This grant would admit'),
        (() => {
          const current = held.value.find((grant) => grant.connector === answer.connector)
          const change = previewChange(current?.admits.map((operation) => operation.id) ?? [], answer.admits.map((operation) => operation.id))
          return h('p', { class: [`grants__delta`, `grants__delta--${change}`], 'data-change': change },
            current ? `Compared with the saved grant, this is ${change}.` : `This adds authority for ${answer.admits.length} operations.`)
        })(),
        h(
          'p',
          { class: 'grants__derived' },
          'Answered by the service from what each operation declares — the same projection the gate ' +
            'decides on when something is actually run. This console computes none of it.'
        ),
        props.catalog
          ? h('div', { class: 'grants__groups' }, groupAdmitted(props.catalog, answer.admits.map((operation) => operation.id)).map((group) =>
              h('section', { key: `${group.connector}/${group.service}`, class: 'grants__group' }, [
                h('h4', null, [h('code', null, group.service), ` · ${group.connector}`]),
                ...group.risks.map((risk) => h('details', { key: risk.risk, open: risk.operations.length < 6 }, [
                  h('summary', null, `${risk.risk} risk · ${risk.operations.length}`),
                  h('ul', null, risk.operations.map((id) => h('li', { key: id }, h('code', null, id)))),
                ])),
              ])))
          : admittedPanel(answer),
      ])
    }

    /** Why there is no form, said as itself. Never an empty form and never a disabled one. */
    function withheldGate(gate: Gate): VNode[] {
      const session = props.session

      switch (gate) {
        case 'loading':
          return [h('p', { class: 'grants__note' }, 'Reading your session…')]

        case 'unknown':
          return [
            h('h2', null, 'This console cannot tell whether you are signed in'),
            h('p', { class: 'grants__note' }, [
              session.status === 'failed' ? h('code', null, session.failure.endpoint) : '',
              ' did not answer, so this page is not saying that you may not change what this tenant ',
              'runs. It is saying it does not know, and that is not a thing to attempt on a guess.',
            ]),
          ]

        case 'anonymous':
          return [
            h('h2', null, 'Sign in to see what this tenant may run'),
            h('p', { class: 'grants__note' }, [
              'A grant belongs to a tenant, and the tenant is read from whoever this service ',
              'resolves you to be — never from anything this page could ask for.',
            ]),
            h('p', null, h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT }, 'Sign in')),
          ]

        case 'may-not-grant':
          return [
            h('h2', null, 'Only a signed-in person may read or change what this tenant runs'),
            h('p', { class: 'grants__note' }, [
              'You are signed in as ',
              h('code', null, session.status === 'ready' && session.principal ? session.principal.kind : ''),
              ', and this host admits only a user here — on the read as well as the write. Editing a ',
              'grant decides which operations run at all, for every principal of this tenant, which ',
              'is more authority than supplying a credential. And a refusal at ',
              h('code', null, 'invoke'),
              ' deliberately never says which bound turned it down, so that a token cannot enumerate ',
              'a tenant’s policy one call at a time; a listing open to every kind would hand the ',
              'whole of it over in one request.',
            ]),
          ]

        case 'may-grant':
          return [form()]
      }
    }

    /** The editor. Offered only where the gate says it may be. */
    function form(): VNode {
      const unknown = unknownRisks(props.catalogueRisks)
      const unknownEffect = unknownEffects(props.catalogueEffects)
      const stopped = props.grants.status === 'ready' && !props.grants.editable

      const inboundFields = (): VNode => {
        if (!connector.value) {
          return h('p', { class: 'grants__hint' }, 'Choose a connector to see its inbound channel bindings.')
        }
        if (props.channelDeclarations.status === 'loading') {
          return h('p', { class: 'grants__hint', 'aria-live': 'polite' }, 'Reading inbound channel declarations…')
        }
        if (props.channelDeclarations.status === 'failed') {
          return h('p', { class: 'grants__stale', role: 'alert' },
            `Inbound declarations could not be read from ${props.channelDeclarations.failure.endpoint}. Existing inbound authority is preserved, but cannot be edited safely.`)
        }
        if (!declaredInbound.value.length) {
          return h('p', { class: 'grants__hint' }, 'This connector declares no generated WebSocket channel.')
        }
        return h('div', { class: 'grants__inbound-editor' }, declaredInbound.value.map((binding) =>
          h('fieldset', { key: binding.name }, [
            h('legend', null, [h('code', null, binding.name), ` — ${binding.description}`]),
            ...binding.events.map((event) => h('label', { key: event.name }, [
              h('input', {
                type: 'checkbox',
                checked: (inbound.value[binding.name] ?? []).includes(event.name),
                onChange: (changed: Event) => toggleInbound(
                  binding.name,
                  event.name,
                  (changed.target as HTMLInputElement).checked
                ),
              }),
              h('span', null, [h('strong', null, event.name), h('small', null, event.description)]),
            ])),
          ])
        ))
      }

      return h('form', { class: 'grants__form', 'data-grants': 'form', onSubmit: save }, [
        h('h2', null, 'Grant a connector'),

        stopped
          ? h('p', { class: 'grants__note', 'data-grants': 'blocked', role: 'alert' }, [
              'Saving is not offered while this tenant holds a grant this screen cannot write back — ',
              `${blocked.value.map((grant) => grant.connector).join(', ')} above. Replacing the set `,
              'here would drop what it names, so the service refuses the write rather than losing it.',
            ])
          : null,

        h(ConnectorPicker, {
          connectors: props.catalogConnectors.length
            ? props.catalogConnectors
            : props.connectors.map((id) => ({ id, vendor: id, description: '', operationCount: 0, channelCount: 0, operations: [] })),
          connected: props.connected,
          value: connector.value,
          label: 'Connector',
          'onChoose': chooseConnector,
        }),

        h('label', { class: 'sr-only', 'aria-hidden': 'true' }, [
          h('span', { class: 'grants__label' }, 'Connector'),
          h(
            'select',
            {
              class: 'grants__select',
              'data-grants': 'connector',
              value: connector.value,
              onChange: (event: Event) => {
                chooseConnector((event.target as HTMLSelectElement).value)
              },
            },
            [
              h('option', { value: '', selected: connector.value === '' }, 'Choose a connector…'),
              ...props.connectors.map((id) =>
                h('option', { key: id, value: id, selected: id === connector.value }, id)
              ),
            ]
          ),
          h(
            'span',
            { class: 'grants__hint' },
            'One grant per connector. Saving replaces any grant this tenant already holds for it, ' +
              'and leaves the others exactly as they are.'
          ),
        ]),

        h('fieldset', { class: 'grants__presets' }, [
          h('legend', { class: 'grants__label' }, 'Starting policy'),
          ...([
            ['read-only', 'Read only', 'Low-risk operations only.'],
            ['no-destructive', 'No destructive effects', 'Excludes delete and money effects published by this build.'],
            ['custom', 'Custom', 'Edit every metadata bound yourself.'],
          ] as const).map(([id, label, description]) => h('label', { class: ['grants__preset', preset.value === id ? 'grants__preset--active' : ''], key: id }, [
            h('input', { type: 'radio', name: 'preset', value: id, checked: preset.value === id, onChange: () => applyPreset(id) }),
            h('strong', null, label),
            h('span', null, description),
          ])),
        ]),

        h('div', { class: ['grants__custom', preset.value === 'custom' ? '' : 'grants__custom--preset'] }, [

        chooser(
          'max-risk',
          'At most this risk',
          'At or below, on the level each operation declares. Leave it unbounded and every level is admitted.',
          maxRisk.value,
          'any risk',
          RISK_LEVELS
        ),

        effectsField(),

        chooser(
          'idempotency',
          'Idempotency',
          'Admit only operations that declare this. Rarely what you want; leave it unbounded unless you mean it.',
          idempotency.value,
          'any',
          ['idempotent', 'conditional', 'not_idempotent']
        ),
        ]),

        h('section', { class: 'grants__inbound-editor-section', 'aria-labelledby': 'inbound-grant-title' }, [
          h('h3', { id: 'inbound-grant-title' }, 'Inbound channel events'),
          h('p', { class: 'grants__hint' },
            'Optional and closed: a subscriber receives only the declared events selected here. This does not start or stop the vendor channel.'),
          inboundFields(),
        ]),

        unknown.length > 0
          ? h('p', { class: 'grants__stale', 'data-grants': 'unknown-risks', role: 'alert' }, [
              'This build’s catalogue publishes a risk level this console does not offer as a bound: ',
              ...unknown.flatMap((risk, at) => (at === 0 ? [h('code', null, risk)] : [', ', h('code', null, risk)])),
              '. The widest bound offered here therefore admits less than the widest that exists, so ' +
                'read the list below rather than the name of the level you chose.',
            ])
          : null,

        unknownEffect.length > 0
          ? h('p', { class: 'grants__stale', 'data-grants': 'unknown-effects', role: 'alert' }, [
              'This build’s catalogue publishes an effect this console does not offer: ',
              ...unknownEffect.flatMap((effect, at) =>
                at === 0 ? [h('code', null, effect)] : [', ', h('code', null, effect)]
              ),
              '. A bound set here cannot include it, so bounding effects would refuse those operations.',
            ])
          : null,

        previewPanel(),

        h(
          'button',
          {
            type: 'submit',
            class: 'grants__submit',
            'data-grants': 'save',
            disabled: !savable.value,
          },
          props.busy ? 'Saving…' : 'Save this grant'
        ),

        savable.value
          ? null
          : h(
              'p',
              { class: 'grants__hint', 'data-grants': 'why-not' },
              stopped
                ? 'Saving is refused while a grant this screen cannot write back is held.'
                : 'Saving is offered once the service has said what this grant would admit. A grant nobody has evaluated is a grant set too wide.'
            ),
      ].filter((node): node is VNode => node !== null))
    }

    /** What this tenant holds today. The state every deployment starts in is the important one. */
    function heldPanel(): VNode {
      const state = props.grants

      if (state.status === 'loading') {
        return h('div', { class: 'connections__skeleton', 'aria-label': `Reading grants from ${GRANTS_ENDPOINT}` }, [
          h('span', { class: 'skeleton skeleton--title' }), h('span', { class: 'skeleton' }),
        ])
      }

      if (state.status === 'failed') {
        return h('section', { class: 'failure', role: 'alert', 'data-grants': 'failed-read' }, [
          h('h3', { class: 'failure__title' }, 'What this tenant may run could not be read'),
          h('p', { class: 'failure__endpoint' }, ['Endpoint: ', h('code', null, state.failure.endpoint)]),
          h('p', { class: 'failure__message' }, failureSentence(state.failure)),
          h('button', { type: 'button', class: 'failure__retry', onClick: () => emit('retry') }, 'Retry grants'),
        ])
      }

      if (state.grants.length === 0) {
        return h('section', { class: 'grants__empty', 'data-grants': 'empty' }, [
          h('h2', null, 'This tenant has been granted nothing, so it runs nothing'),
          h(
            'p',
            null,
            'That is not a broken state — it is the one every deployment starts in. An operation ' +
              'runs only if a grant admits it, and until one exists every invocation is refused ' +
              'with 403 before any credential is read. Grant a connector below.'
          ),
        ])
      }

      return h('section', { class: 'grants__held', 'data-grants': 'held' }, [
        h('h2', null, 'What this tenant may run'),
        ...state.grants.map((grant) =>
          grantCard(grant, state.editable ? () => revoke(grant.connector) : null)
        ),
      ])
    }

    /** What the last write did. */
    function outcomePanel(outcome: GrantOutcome): VNode | null {
      if (outcome.status === 'saved') {
        return h('p', { class: 'grants__saved', 'data-grants': 'saved', role: 'status' }, [
          'Saved. What is listed above is what the service answered with, not what this page sent.',
          ' ', h('a', { href: fragmentPath('/invoke') }, 'Next: invoke an admitted operation.'),
        ])
      }

      if (outcome.status === 'refused') {
        // The one thing the console adds to a refusal, and it is added rather than substituted.
        // A `409` is this tenant holding a grant this surface cannot express, and the service names
        // the connector but cannot show what would have been lost — the read above can.
        const extra =
          outcome.refusal.status === 409 && blocked.value.length > 0
            ? [
                h('div', { class: 'grants__blocked', 'data-grants': 'blocked-detail' }, [
                  h('p', null, 'This is what is in the way, as the listing above shows it:'),
                  h(
                    'ul',
                    null,
                    blocked.value.map((grant) =>
                      h('li', { key: grant.connector }, [
                        h('code', null, grant.connector),
                        ' — ',
                        grant.exempt
                          ? `always ${grant.exempt.always.join(', ') || 'none'}; never ${grant.exempt.never.join(', ') || 'none'}`
                          : grant.reason,
                      ])
                    )
                  ),
                  h(
                    'p',
                    null,
                    'Nothing was changed. Edit that grant where it was written — this screen writes ' +
                      'a connector and a predicate, and a grant that names operations is not one it ' +
                      'can express.'
                  ),
                ]),
              ]
            : []
        return refusalNotice(outcome.refusal, extra)
      }

      return h('section', { class: 'failure', role: 'alert', 'data-grants': 'failed-write' }, [
        h('h3', { class: 'failure__title' }, 'Nothing was changed'),
        h('p', { class: 'failure__endpoint' }, ['Endpoint: ', h('code', null, outcome.failure.endpoint)]),
        h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
      ])
    }

    return () => {
      const gate = gateOf(props.session)
      const outcome = props.outcome

      return h('section', { class: 'grants', 'data-page': 'grants' }, [
        h('h1', null, 'Grants'),

        h('p', { class: 'grants__lead' }, [
          'An operation runs for this tenant only if a grant admits it — decided from what the ',
          'operation declares, its risk, its effects and its idempotency, and never from a list of ',
          'operation names. A grant written as a list stops covering a connector the moment that ',
          'connector gains an operation; a grant written as ',
          h('code', null, 'at most low'),
          ' covers the new one correctly on the day it lands. The call behind this screen is ',
          h('code', null, `PUT ${GRANTS_ENDPOINT}`),
          '.',
        ]),

        gate === 'may-grant' ? heldPanel() : null,

        h(
          'section',
          { class: 'grants__gate', 'data-grants': 'gate', 'data-state': gate },
          withheldGate(gate)
        ),

        outcome ? outcomePanel(outcome) : null,
      ].filter((node): node is VNode => node !== null))
    }
  },
})
