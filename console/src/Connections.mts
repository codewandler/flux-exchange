// What this tenant has wired up — the first of the console's two jobs, and the only one this build
// can do.
//
// **Addresses, never values.** `GET /api/connections` answers with the address each declared
// credential would live at and whether anything is held there; the value never leaves the store.
// That is the north star of the whole service — *the credential never crosses the boundary; the
// authority does* — and this element is where a reader would first notice if it stopped being true.
// So it renders `held` and the address, and there is deliberately nowhere here for a value to go.
//
// **The three states are the catalogue's three states**, for the same reason `service.mts` gives:
// a tenant that has connected nothing and a service that did not answer must never render alike.
// The signed-out case is not one of them — `App.vue` never asks this question without a principal,
// because the session already answered it.
//
// A render function rather than a single-file component, following `CatalogueFailure.mts`.

import { defineComponent, h, type PropType, type VNode } from 'vue'
import { CONNECTIONS_ENDPOINT, type Connection, type ConnectionsState } from './service.mts'
import type { ServiceFailure } from './service.mts'

/** What a failed listing says, naming the endpoint in every branch. */
function failure(reason: ServiceFailure): VNode {
  const sentence = () => {
    switch (reason.kind) {
      case 'unreachable':
        return `${reason.endpoint} could not be reached. ${reason.detail} Nothing was read, so this page is empty because the request failed — not because this tenant has connected nothing.`
      case 'refused':
        return `${reason.endpoint} answered ${reason.status}. ${reason.detail} No listing was read, so what is below is not "no connections" — it is nothing at all.`
      case 'unreadable':
        return `${reason.endpoint} answered ${reason.status} with a body this console could not read as a listing. ${reason.detail}`
    }
  }

  return h(
    'section',
    { class: 'failure', role: 'alert', 'data-connections': 'failed', 'data-failure': reason.kind },
    [
      h('h1', { class: 'failure__title' }, 'This tenant’s connections could not be read'),
      h('p', { class: 'failure__endpoint' }, ['Endpoint: ', h('code', null, reason.endpoint)]),
      h('p', { class: 'failure__message' }, sentence()),
    ]
  )
}

/** One connection: the connector, and each credential it declares as an address. */
function card(connection: Connection): VNode {
  const declared = connection.credentials.length
  const held = connection.credentials.filter((credential) => credential.held).length

  return h('article', { class: 'connection', 'data-connector': connection.connector }, [
    h('div', { class: 'connection__head' }, [
      h('h2', { class: 'connection__name' }, connection.vendor),
      h('code', { class: 'connection__id' }, connection.connector),
      h(
        'span',
        { class: 'connection__count' },
        `${held} of ${declared} ${declared === 1 ? 'credential' : 'credentials'} held`
      ),
    ]),

    connection.authority
      ? h('p', { class: 'connection__authority' }, [
          'Addressed under authority ',
          h('code', null, connection.authority),
        ])
      : null,

    h('table', { class: 'connection__credentials' }, [
      h('thead', null, [
        h('tr', null, [
          h('th', null, 'Credential'),
          h('th', null, 'Address'),
          h('th', null, 'Stored'),
        ]),
      ]),
      h(
        'tbody',
        null,
        connection.credentials.map((credential) =>
          h('tr', { key: credential.name }, [
            h('td', null, h('code', null, credential.name)),
            // The address and never the value. This host can name where a secret lives; it cannot
            // read one back out, and neither can this page.
            h('td', null, h('code', { class: 'connection__address' }, credential.address)),
            h(
              'td',
              { 'data-held': String(credential.held) },
              credential.held ? 'yes' : 'not stored'
            ),
          ])
        )
      ),
    ]),
  ])
}

export default defineComponent({
  name: 'Connections',
  props: {
    state: { type: Object as PropType<ConnectionsState>, required: true },
  },
  setup(props) {
    return () => {
      const state = props.state

      if (state.status === 'loading') {
        return h('p', { class: 'connections__loading' }, [
          'Reading this tenant’s connections from ',
          h('code', null, CONNECTIONS_ENDPOINT),
          '…',
        ])
      }

      if (state.status === 'failed') return failure(state.failure)

      return h('section', { 'data-connections': 'ready' }, [
        h('h1', null, 'Connections'),
        state.connections.length
          ? h('div', { class: 'connections' }, state.connections.map(card))
          : // A read that succeeded and found nothing. Distinguishable from the failure above by
            // more than a sentence: it names the endpoint that *did* answer.
            h('p', { class: 'connections__none' }, [
              'This tenant holds no credentials for any connector. ',
              h('code', null, CONNECTIONS_ENDPOINT),
              ' answered, and it listed nothing — so this is what is wired up, not a page that ',
              'failed to load.',
            ]),
        h('p', { class: 'connections__note' }, [
          'A connection is a set of addresses. This console can tell you where a credential lives ',
          'and whether something is stored there; it cannot read one back, and neither can the ',
          'service — that is what ',
          h('em', null, 'the credential never crosses the boundary'),
          ' means. Connecting a connector is done through ',
          h('code', null, 'POST /api/connections/{connector}'),
          '; there is no form for it here yet.',
        ]),
      ])
    }
  },
})
