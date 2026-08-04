// Service Account metadata and owner-local creation guidance.
//
// The browser may list and revoke value-free account metadata. Creation is different: its response
// is the one-shot FXSA handoff, so only the verified owner-local helper is allowed to receive it.
// This view therefore never posts a create request, parses a token, or holds credential material.

import { defineComponent, h, shallowRef, watch, type PropType, type VNode } from 'vue'
import {
  SERVICE_ACCOUNTS_ENDPOINT,
  SIGNIN_ENDPOINT,
  loadServiceAccounts,
  revokeServiceAccount,
  type RevokeServiceAccountOutcome,
  type ServiceAccountSummary,
  type ServiceAccountsState,
  type SessionState,
} from './service.mts'
import { authorisation, mayMint, tokenStanding, type Standing } from './minting.mts'
import { ONBOARDING_PATH } from './onboarding.mts'
import { fragmentPath } from './routing.ts'

type Gate = 'loading' | 'unknown' | 'anonymous' | 'may-not-mint' | 'may-mint'

function gateOf(session: SessionState): Gate {
  if (session.status === 'loading') return 'loading'
  if (session.status === 'failed') return 'unknown'
  if (!session.principal) return 'anonymous'
  return mayMint(session.principal) ? 'may-mint' : 'may-not-mint'
}

function instant(seconds: number): string {
  return new Date(seconds * 1000).toISOString().replace('T', ' ').replace('.000Z', ' UTC')
}

function standingEntry(entry: Standing): VNode {
  return h('li', {
    class: ['agents__standing-entry', { 'agents__standing-entry--pending': !entry.can }],
    'data-standing': entry.step.id,
    'data-available': String(entry.can),
  }, [
    h('h3', { class: 'agents__standing-title' }, [
      entry.step.title,
      entry.can ? null : h('span', { class: 'agents__tag' }, 'not yet'),
    ]),
    h('p', { class: 'agents__standing-summary' }, entry.step.summary),
    entry.can ? null : h('p', { class: 'agents__standing-reason' }, entry.reason),
  ].filter((node): node is VNode => node !== null))
}

function standingPanel(): VNode {
  const note = authorisation()
  return h('section', { class: 'agents__standing', 'data-agents': 'standing' }, [
    h('h2', null, 'What a Service Account can and cannot do today'),
    h('ul', { class: 'agents__standing-list' }, tokenStanding().map(standingEntry)),
    note ? h('p', { class: 'agents__authorisation', 'data-agents': 'authorisation' }, note) : null,
  ].filter((node): node is VNode => node !== null))
}

export default defineComponent({
  name: 'ServiceAccounts',
  props: {
    session: { type: Object as PropType<SessionState>, required: true },
  },
  setup(props) {
    const accountsState = shallowRef<ServiceAccountsState | null>(null)
    const revocation = shallowRef<({ id: string } & RevokeServiceAccountOutcome) | null>(null)
    const revoking = shallowRef<string | null>(null)

    async function refreshAccounts(): Promise<void> {
      accountsState.value = await loadServiceAccounts()
    }

    async function revokeAccount(id: string): Promise<void> {
      if (revoking.value !== null) return
      revoking.value = id
      const outcome = await revokeServiceAccount(id)
      revocation.value = { id, ...outcome }
      if (outcome.status === 'revoked' && accountsState.value?.status === 'ready') {
        accountsState.value = {
          status: 'ready',
          accounts: accountsState.value.accounts.filter((entry) => entry.id !== id),
        }
      }
      revoking.value = null
    }

    watch(
      () => gateOf(props.session),
      (gate) => {
        if (gate === 'may-mint' && accountsState.value === null) void refreshAccounts()
      },
      { immediate: true },
    )

    function ownerLocalCreation(): VNode {
      return h('section', { class: 'agents__gate', 'data-agents': 'gate', 'data-state': 'may-mint' }, [
        h('h2', null, 'Create through the owner-local helper'),
        h('p', { class: 'agents__note' }, [
          'Service Account creation returns a one-shot credential through a closed native capability. ',
          'The browser cannot receive that handoff and never asks for it as HTTP JSON.',
        ]),
        h('p', null, [
          'Use ',
          h('code', null, 'flux-exchange local service-account-mint --id <id> --expires-at <unix-seconds>'),
          ' from the authenticated OS owner session. The verified helper opens the private local-management endpoint and transfers the result directly.',
        ]),
      ])
    }

    function withheldGate(gate: Gate): VNode {
      if (gate === 'loading') {
        return h('section', { class: 'agents__gate', 'data-agents': 'gate', 'data-state': gate }, [
          h('p', { class: 'agents__note' }, 'Reading your session…'),
        ])
      }
      if (gate === 'unknown') {
        return h('section', { class: 'agents__gate', 'data-agents': 'gate', 'data-state': gate }, [
          h('h2', null, 'This console cannot tell whether you are signed in'),
          h('p', { class: 'agents__note' }, 'The session request failed, so this page does not guess who may manage Service Accounts.'),
        ])
      }
      if (gate === 'anonymous') {
        return h('section', { class: 'agents__gate', 'data-agents': 'gate', 'data-state': gate }, [
          h('h2', null, 'Sign in to manage Service Accounts'),
          h('p', null, h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT }, 'Sign in')),
        ])
      }
      if (gate === 'may-not-mint') {
        return h('section', { class: 'agents__gate', 'data-agents': 'gate', 'data-state': gate }, [
          h('h2', null, 'Only a signed-in person may manage Service Accounts'),
          h('p', { class: 'agents__note' }, 'A Service Account cannot create or revoke a successor identity.'),
        ])
      }
      return ownerLocalCreation()
    }

    function accountsPanel(): VNode {
      const held = accountsState.value
      const last = revocation.value
      return h('section', { class: 'agents__accounts', 'data-agents': 'accounts' }, [
        h('h2', null, 'Current Service Accounts'),
        held === null
          ? h('p', { role: 'status' }, 'Reading Service Accounts…')
          : held.status === 'failed'
            ? h('p', { class: 'failure', role: 'alert' }, [
                h('code', null, held.failure.endpoint),
                ` could not be read: ${held.failure.detail}`,
              ])
            : held.accounts.length === 0
              ? h('p', { 'data-agents': 'accounts-empty' }, 'This tenant has no live Service Accounts.')
              : h('ul', { class: 'agents__accounts-list' }, held.accounts.map((account: ServiceAccountSummary) =>
                  h('li', { class: 'agents__account', 'data-account': account.id }, [
                    h('span', null, [h('code', null, account.id), ` — stops resolving ${instant(account.expiresAt)}`]),
                    h('button', {
                      type: 'button',
                      'data-agents': 'revoke',
                      'data-account': account.id,
                      disabled: revoking.value !== null,
                      onClick: () => revokeAccount(account.id),
                    }, revoking.value === account.id ? 'Revoking…' : 'Revoke'),
                  ]),
                )),
        last?.status === 'revoked'
          ? h('p', { role: 'status', 'data-agents': 'revoked' }, [h('code', null, last.id), ' no longer authenticates.'])
          : null,
        last?.status === 'refused'
          ? h('p', { class: 'failure', role: 'alert', 'data-agents': 'refused' }, last.refusal.error)
          : null,
        last?.status === 'failed'
          ? h('p', { class: 'failure', role: 'alert', 'data-agents': 'revoke-failed' }, last.failure.detail)
          : null,
      ].filter((node): node is VNode => node !== null))
    }

    return () => {
      const gate = gateOf(props.session)
      return h('section', { class: 'agents', 'data-page': 'agents' }, [
        h('h1', null, 'Service Accounts'),
        h('p', { class: 'agents__lead' }, [
          'This browser reads metadata from ',
          h('code', null, SERVICE_ACCOUNTS_ENDPOINT),
          ' and can revoke an identity. Creation stays on the private owner-local path; see ',
          h('a', { href: fragmentPath(ONBOARDING_PATH) }, 'Connect an agent'),
          '.',
        ]),
        withheldGate(gate),
        gate === 'may-mint' ? accountsPanel() : null,
        standingPanel(),
      ].filter((node): node is VNode => node !== null))
    }
  },
})
