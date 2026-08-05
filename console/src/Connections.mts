// The actionable connection dashboard: status and safe next actions, with addresses never values.

import { defineComponent, h, type PropType, type VNode } from 'vue'
import { fragmentPath } from './routing.ts'
import {
  CONNECTIONS_ENDPOINT,
  type Connection,
  type ConnectionsState,
  type ServiceFailure,
} from './service.mts'

function failure(reason: ServiceFailure, retry?: () => void): VNode {
  const sentence = reason.kind === 'unreachable'
    ? `${reason.endpoint} could not be reached. ${reason.detail}`
    : reason.kind === 'unreadable'
      ? `${reason.endpoint} answered ${reason.status} with a body this console could not read. ${reason.detail}`
      : `${reason.endpoint} answered ${reason.status}. ${reason.detail}`
  return h('section', { class: 'failure', role: 'alert', 'data-connections': 'failed' }, [
    h('h1', { class: 'failure__title' }, 'This tenant’s connections could not be read'),
    h('p', { class: 'failure__message' }, sentence),
    retry ? h('button', { type: 'button', class: 'failure__retry', onClick: retry }, 'Retry connections') : null,
  ])
}

/** The single renderer used for list entries and a just-created connection. */
export function connectionCard(connection: Connection): VNode {
  const declared = connection.credentials.length
  const held = connection.credentials.filter((credential) => credential.held).length
  const state = declared === 0 || held === declared ? 'connected' : held === 0 ? 'needs attention' : 'partially connected'

  const credentials = h('table', { class: 'connection__credentials' }, [
    h('thead', null, h('tr', null, [h('th', null, 'Credential'), h('th', null, 'Address'), h('th', null, 'Stored'), h('th', null, 'Action')])),
    h('tbody', null, connection.credentials.map((credential) => {
      return h('tr', { key: credential.name }, [
        h('td', null, h('code', null, credential.name)),
        h('td', null, h('code', { class: 'connection__address' }, credential.address)),
        h('td', { 'data-held': String(credential.held) }, credential.held ? 'yes' : 'not stored'),
        h('td', null, credential.held ? 'Rotate with owner-local management' : 'Acquire with owner-local management'),
      ])
    })),
  ])

  return h('article', { class: 'connection', 'data-connector': connection.connector, 'data-state': state.replace(' ', '-') }, [
    h('div', { class: 'connection__head' }, [
      h('div', null, [h('h2', { class: 'connection__name' }, connection.vendor), h('code', { class: 'connection__id' }, connection.connector)]),
      h('span', { class: 'connection__status' }, state),
    ]),
    h('p', { class: 'connection__summary' }, `${held} of ${declared} ${declared === 1 ? 'credential' : 'credentials'} held`),
    h('div', { class: 'connection__actions' }, [
      h('a', { href: fragmentPath(`/grants?connector=${encodeURIComponent(connection.connector)}`) }, 'Review grant'),
      h('a', { href: fragmentPath('/invoke') }, 'Invoke operation'),
    ]),
    connection.authority ? h('p', { class: 'connection__authority' }, ['Addressed under authority ', h('code', null, connection.authority)]) : null,
    h('details', { class: 'connection__details' }, [h('summary', null, `Credentials (${held}/${declared} held)`), credentials]),
  ])
}

export default defineComponent({
  name: 'Connections',
  props: {
    state: { type: Object as PropType<ConnectionsState>, required: true },
  },
  emits: ['retry'],
  setup(props, { emit }) {
    return () => {
      if (props.state.status === 'loading') return h('div', { class: 'connections__skeleton', 'aria-label': 'Reading connections' }, [
        h('span', { class: 'skeleton skeleton--title' }), h('span', { class: 'skeleton' }), h('span', { class: 'skeleton' }),
      ])
      if (props.state.status === 'failed') return failure(props.state.failure, () => emit('retry'))
      return h('section', { 'data-connections': 'ready' }, [
        h('h1', null, 'Connections'),
        props.state.connections.length
          ? h('div', { class: 'connections' }, props.state.connections.map(connectionCard))
          : h('section', { class: 'connections__none' }, [h('h2', null, 'No connections yet'), h('p', null, [h('code', null, CONNECTIONS_ENDPOINT), ' answered with an empty list. Choose a connector below to begin.'])]),
        h('p', { class: 'connections__note' }, ['This dashboard shows credential addresses and whether something is held there. It never reads or renders a credential value.']),
      ])
    }
  },
})
