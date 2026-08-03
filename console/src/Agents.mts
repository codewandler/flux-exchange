// Creating a Service Account, and the one disclosure this console will ever make.
//
// X-36 shipped `POST /api/service-accounts` and deliberately no UI. X-41 published the page that tells an
// agent author how to get an identity, and the best answer it could give was "ask a human to
// `curl`". This is the human's screen, and it is shaped end to end by one property:
//
//   > **The token is shown once.** The Service Account store keeps a *verifier*, so this host is genuinely
//   > unable to say it a second time. That is the design, not a limitation to work around.
//
// Three consequences decide everything below.
//
// **This element mints for itself, which is a deliberate exception to how this console fetches.**
// `App.vue` is otherwise the one place that knows where data comes from: it reads, and screens are
// handed the result as props. A token must not go through that. `App.vue` outlives every screen —
// it is the root — so a token that reached it would still be in memory after the reader had
// navigated away, one `v-if` away from being rendered again, and the property above would be a
// claim about this file rather than about the console. So the mint happens here, the result lives
// in this `setup`'s own closure, and it is destroyed with the component instance. Navigating away
// is not a handler that remembers to clear something; it is the state ceasing to exist.
//
// **Management never implies retrieval.** Listing shows ids and expiry, and revocation takes an id.
// Neither response can carry a token or verifier. The one control that touches a newly minted token
// takes it off the page and cannot put it back because there is nothing left to retrieve.
//
// **The screen offers minting only to a principal this host would admit.** X-40 settled that: a
// `User`, and nothing else. Offering the button to an agent or a service would teach an operator
// that it is available and let them find the `403` themselves — and when the service refuses
// anyway, its own sentence is what they read, unedited, because `minting.mts`'s idea of who may
// mint is a courtesy and the route is the rule.
//
// X-34 recorded the trade this screen inherits, and it is worth stating where somebody reading the
// screen will meet it: a cookie-carried caller **does** receive a readable token here, unlike at
// `/api/session`. Cross-site is closed by `SameSite=Strict`. Same-origin script is not and cannot
// be — the token is on the page by construction, and there is no arrangement in which a human is
// shown one and script running as that human is not. The remedies are immediate revocation and a
// bounded expiry; that is why the expiry is stated on every mint and is never defaulted.
//
// A render function rather than a single-file component, following `Connect.mts` and
// `ConsoleShell.mts`: the claims above are only worth anything if a test drives them, and a render
// function mounts under a plain `node --test` with no bundler and no new dependency. Its rules live
// in `agents.css` for the reason `shell.css` gives.

import { defineComponent, h, ref, shallowRef, watch, type PropType, type VNode } from 'vue'
import {
  SERVICE_ACCOUNTS_ENDPOINT,
  loadServiceAccounts,
  mintServiceAccount,
  revokeServiceAccount,
  type MintOutcome,
  type MintedServiceAccount,
  type RevokeServiceAccountOutcome,
  type ServiceAccountSummary,
  type ServiceAccountsState,
} from './service.mts'
import { SIGNIN_ENDPOINT, type ServiceFailure, type ServiceRefusal, type SessionState } from './service.mts'
import {
  authorisation,
  expiryFromNow,
  mayMint,
  tokenStanding,
  writeClipboard,
  type Copied,
  type Standing,
} from './minting.mts'
import { ONBOARDING_PATH } from './onboarding.mts'
import { fragmentPath } from './routing.ts'

/** The service this screen mints against, by the one name it is published under. */
const SERVICE = 'flux-exchange'

/** Which of the five things this screen can be, from the session and nothing else. */
type Gate = 'loading' | 'unknown' | 'anonymous' | 'may-not-mint' | 'may-mint'

/** What the session says this reader may do here. */
function gateOf(session: SessionState): Gate {
  if (session.status === 'loading') return 'loading'
  // Not "signed out". This console does not know, and reporting an outage as a sign-out is the
  // collapse `ConsoleShell` and `CatalogueFailure` both exist to prevent.
  if (session.status === 'failed') return 'unknown'
  if (!session.principal) return 'anonymous'
  return mayMint(session.principal) ? 'may-mint' : 'may-not-mint'
}

/** An instant the operator can check before they send it. UTC, because a tenant is not a timezone. */
function instant(seconds: number): string {
  return new Date(seconds * 1000).toISOString().replace('T', ' ').replace('.000Z', ' UTC')
}

/** What a failed write says, naming the endpoint in every branch. Follows `Connect.mts`. */
function failureSentence(reason: ServiceFailure): string {
  switch (reason.kind) {
    case 'unreachable':
      return `${reason.endpoint} could not be reached. ${reason.detail} Nothing was sent, so no Service Account was created — and this is not the service saying no.`
    case 'refused':
      return `${reason.endpoint} answered ${reason.status}, with no sentence this console could read. ${reason.detail}`
    case 'unreadable':
      return `${reason.endpoint} answered ${reason.status} with a body this console could not read. ${reason.detail} Whether a Service Account now exists is therefore unknown, and if one does, its token was in that body and is gone.`
  }
}

/** A refusal, in the service's own words and nothing added to them. */
function refusalNotice(refusal: ServiceRefusal): VNode {
  return h(
    'section',
    { class: 'failure', role: 'alert', 'data-agents': 'refused', 'data-status': String(refusal.status) },
    [
      h('h3', { class: 'failure__title' }, `The service refused this, answering ${refusal.status}`),
      // Verbatim. `routes::refuse_kind` composes the `403` from the kinds the route declares, so
      // this sentence is the authority on who may mint and this console's own list is not.
      h('p', { class: 'failure__message' }, refusal.error),
    ]
  )
}

/** What a copy did, when one has been tried. Nothing at all before that. */
function copyNotice(copied: Copied | null): VNode | null {
  if (copied === null) return null

  if (copied.ok) {
    return h(
      'span',
      { class: 'agents__copy-ok', 'data-agents': 'copied', role: 'status' },
      'On your clipboard.'
    )
  }

  // Loud, and never a shrug. A clipboard write that silently failed is the one failure on this
  // screen an operator cannot recover from: they navigate away believing they have the token.
  return h('p', { class: 'agents__copy-failed', 'data-agents': 'copy-failed', role: 'alert' }, [
    h(
      'strong',
      null,
      'This token did not reach your clipboard, so do not navigate away yet.'
    ),
    h('span', { class: 'agents__copy-why' }, ` ${copied.reason} Select it above and copy it by hand.`),
  ])
}

/** The one screen in this console that renders a credential value, and everything it owes for it. */
function mintedPanel(
  minted: MintedServiceAccount,
  copied: Copied | null,
  copy: () => void,
  discard: () => void
): VNode {
  return h('section', { class: 'agents__minted', 'data-agents': 'minted' }, [
    h('h2', { class: 'agents__minted-title' }, 'Store this token now — it is shown once'),

    h(
      'p',
      { class: 'agents__once' },
      `${SERVICE} does not keep this token. It keeps a verifier: enough to check a token presented ` +
        'later, and not enough to reconstruct one. So this host cannot show it to you again — not ' +
        'on this page, not on another, not through any route it serves — and nothing here is ' +
        'holding a copy back. If it is lost, revoke this Service Account and create another; do not look it up.'
    ),

    // The disclosure. In text, in one place, and deliberately not in an attribute: a value in
    // markup is a value in a copied `outerHTML`, in a devtools screenshot, and in anything that
    // serialises the document.
    h('p', { class: 'agents__token-line' }, [
      h('code', { class: 'agents__token', 'data-agents': 'token' }, minted.token),
    ]),

    h('div', { class: 'agents__copy' }, [
      h(
        'button',
        { type: 'button', class: 'agents__copy-button', 'data-agents': 'copy', onClick: copy },
        'Copy'
      ),
      copyNotice(copied),
    ]),

    h('dl', { class: 'agents__facts' }, [
      h('dt', null, 'Service Account'),
      h('dd', null, h('code', null, minted.principal.id)),
      h('dt', null, 'Kind'),
      h('dd', null, minted.principal.kind),
      // The tenant, because every authority this token could ever act under is derived from it, and
      // because it is the one field the operator did not supply — it was read from who they are.
      h('dt', null, 'Tenant'),
      h('dd', null, h('code', null, minted.principal.tenant)),
      h('dt', null, 'Stops resolving'),
      h('dd', null, instant(minted.expiresAt)),
    ]),

    h(
      'p',
      { class: 'agents__revocation' },
      'If this token is lost or exposed, use Revoke below. Revocation removes its verifier, so the ' +
        'token stops authenticating immediately; expiry remains the backstop and is never defaulted.'
    ),

    // One-way. The reader has stored it and does not want it on a screen behind them; there is
    // nothing that puts it back, because after this there is nothing left to put back.
    h(
      'button',
      { type: 'button', class: 'agents__discard', 'data-agents': 'discard', onClick: discard },
      'I have stored it — take it off this page'
    ),
  ])
}

/** One thing a token from here would be presented for, in the state this build puts it in. */
function standingEntry(entry: Standing): VNode {
  return h(
    'li',
    {
      class: ['agents__standing-entry', { 'agents__standing-entry--pending': !entry.can }],
      'data-standing': entry.step.id,
      'data-available': String(entry.can),
    },
    [
      h('h3', { class: 'agents__standing-title' }, [
        entry.step.title,
        entry.can ? null : h('span', { class: 'agents__tag' }, 'not yet'),
      ]),
      h('p', { class: 'agents__standing-summary' }, entry.step.summary),
      entry.can ? null : h('p', { class: 'agents__standing-reason' }, entry.reason),
    ].filter((node): node is VNode => node !== null)
  )
}

/**
 * What a token from here can and cannot do today.
 *
 * Derived, never written — see `minting.mts`. Shown whether or not anything has been minted,
 * because it is what an operator needs *before* deciding to create a principal, not after.
 */
function standingPanel(): VNode {
  const note = authorisation()

  return h('section', { class: 'agents__standing', 'data-agents': 'standing' }, [
    h('h2', null, 'What a token from here can and cannot do today'),
    h(
      'p',
      { class: 'agents__derived' },
      'Derived from the same declaration the navigation at the top of this console reads, so ' +
        'nothing below can claim a surface this build marks unbuilt — and a surface that regresses ' +
        'takes its claim off this page with it.'
    ),
    h('ul', { class: 'agents__standing-list' }, tokenStanding().map(standingEntry)),
    note ? h('p', { class: 'agents__authorisation', 'data-agents': 'authorisation' }, note) : null,
  ].filter((node): node is VNode => node !== null))
}

export default defineComponent({
  name: 'ServiceAccounts',
  props: {
    /**
     * What `/api/session` said. The only input this screen takes, and deliberately so: a minted
     * token must be produced inside this view and held nowhere above it.
     */
    session: { type: Object as PropType<SessionState>, required: true },
  },
  setup(props) {
    // ---------------------------------------------------------------------------------------
    // Everything this screen knows, and the whole of where a token can be.
    //
    // These four live in this closure and nowhere else. There is no store, no module-level
    // variable, no prop and no emit carrying any of them upward — so unmounting this component
    // is what "the token is gone" means, and it is not something any code here has to remember
    // to do. `test/agents.test.mjs` mounts, mints, unmounts and mounts again to hold it.
    // ---------------------------------------------------------------------------------------

    /** What the operator typed. A name and a lifetime — neither is a credential. */
    const id = ref('')
    const days = ref('')

    /** What the last mint did, or `null` before there has been one. Carries the token. */
    const result = shallowRef<MintOutcome | null>(null)

    /** Listed identities carry only id and expiry; no token can enter this state. */
    const accountsState = shallowRef<ServiceAccountsState | null>(null)

    /** The last revocation result and the id it concerned. */
    const revocation = shallowRef<
      ({ id: string } & RevokeServiceAccountOutcome) | null
    >(null)

    /** The id whose verifier is currently being removed. */
    const revoking = ref<string | null>(null)

    /** Whether a mint is in flight, so the form cannot be submitted twice. */
    const busy = ref(false)

    /** What the last copy did, or `null` before one has been tried. */
    const copied = shallowRef<Copied | null>(null)

    /** Whether the form has enough to send. The expiry is never supplied by this console. */
    const lifetime = (): number | null => {
      const value = Number(days.value)
      return days.value.trim() !== '' && Number.isFinite(value) && value > 0 ? value : null
    }
    const ready = (): boolean => id.value.trim() !== '' && lifetime() !== null

    async function submit(event: Event): Promise<void> {
      event.preventDefault()
      const chosen = lifetime()
      if (busy.value || chosen === null || !ready()) return

      // The previous token leaves the page before the request goes out, rather than lingering
      // beside a spinner for a second one. Two tokens on a screen is one of them being lost.
      result.value = null
      copied.value = null
      busy.value = true
      const outcome = await mintServiceAccount({
        id: id.value.trim(),
        expiresAt: expiryFromNow(chosen),
      })
      result.value = outcome
      if (outcome.status === 'minted' && accountsState.value?.status === 'ready') {
        const added: ServiceAccountSummary = {
          id: outcome.minted.principal.id,
          expiresAt: outcome.minted.expiresAt,
        }
        accountsState.value = {
          status: 'ready',
          accounts: [...accountsState.value.accounts.filter((entry) => entry.id !== added.id), added]
            .sort((left, right) => left.id.localeCompare(right.id)),
        }
      }
      busy.value = false
    }

    async function copy(): Promise<void> {
      const outcome = result.value
      if (outcome?.status !== 'minted') return
      copied.value = await writeClipboard(outcome.minted.token)
    }

    /** Take the token off the page. There is deliberately nothing that undoes this. */
    function discard(): void {
      result.value = null
      copied.value = null
    }

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
        if (result.value?.status === 'minted' && result.value.minted.principal.id === id) discard()
      }
      revoking.value = null
    }

    // The session normally arrives after the component mounts. Load only once it proves this is a
    // signed-in human; anonymous and Service Account callers are not invited to probe the route.
    watch(
      () => gateOf(props.session),
      (gate) => {
        if (gate === 'may-mint' && accountsState.value === null) void refreshAccounts()
      },
      { immediate: true }
    )

    /** A labelled box. The name and the lifetime, which the operator states and this console does not. */
    const field = (name: string, label: string, hint: string, value: string, attributes: object) =>
      h('label', { class: 'agents__field' }, [
        h('span', { class: 'agents__label' }, label),
        h('input', {
          class: 'agents__input',
          'data-agents': name,
          name,
          value,
          // Never `type="password"`: these are not secrets, and an operator has to be able to read
          // back the name they are about to give an agent in their tenant.
          type: 'text',
          autocomplete: 'off',
          spellcheck: 'false',
          ...attributes,
        }),
        h('span', { class: 'agents__hint' }, hint),
      ])

    function form(): VNode {
      const chosen = lifetime()

      return h('form', { class: 'agents__form', 'data-agents': 'mint-form', onSubmit: submit }, [
        field(
          'id',
          'Name',
          'What to call this Service Account within your tenant. It is a name, not an address.',
          id.value,
          {
            placeholder: 'ci-runner',
            onInput: (event: Event) => (id.value = (event.target as HTMLInputElement).value),
          }
        ),
        field(
          'days',
          'Lifetime, in days',
          'Deliberately empty. This host refuses a mint with no expiry rather than choosing one, ' +
            'and this console will not choose one either. Revocation ends a token now; expiry is ' +
            'the backstop when nobody notices that it should be ended.',
          days.value,
          {
            inputmode: 'numeric',
            placeholder: '30',
            onInput: (event: Event) => (days.value = (event.target as HTMLInputElement).value),
          }
        ),
        chosen !== null
          ? h('p', { class: 'agents__expiry', 'data-agents': 'expiry' }, [
              'The token will stop resolving at ',
              h('code', null, instant(expiryFromNow(chosen))),
              ', which is what will be sent.',
            ])
          : null,
        h(
          'button',
          {
            type: 'submit',
            class: 'agents__submit',
            'data-agents': 'mint',
            disabled: busy.value || !ready(),
          },
          busy.value ? 'Creating…' : 'Create Service Account'
        ),
      ].filter((node): node is VNode => node !== null))
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
              : h(
                  'ul',
                  { class: 'agents__accounts-list' },
                  held.accounts.map((account) =>
                    h('li', { class: 'agents__account', 'data-account': account.id }, [
                      h('span', null, [
                        h('code', null, account.id),
                        ` — stops resolving ${instant(account.expiresAt)}`,
                      ]),
                      h(
                        'button',
                        {
                          type: 'button',
                          'data-agents': 'revoke',
                          'data-account': account.id,
                          disabled: revoking.value !== null,
                          onClick: () => revokeAccount(account.id),
                        },
                        revoking.value === account.id ? 'Revoking…' : 'Revoke'
                      ),
                    ])
                  )
                ),
        last?.status === 'revoked'
          ? h('p', { role: 'status', 'data-agents': 'revoked' }, [
              h('code', null, last.id),
              ' no longer authenticates.',
            ])
          : null,
        last?.status === 'refused' ? refusalNotice(last.refusal) : null,
        last?.status === 'failed'
          ? h('p', { class: 'failure', role: 'alert', 'data-agents': 'revoke-failed' }, [
              h('code', null, last.failure.endpoint),
              ` could not be completed: ${last.failure.detail}`,
            ])
          : null,
      ].filter((node): node is VNode => node !== null))
    }

    /** Why there is no form, said as itself. Never an empty form and never a disabled one. */
    function withheldGate(gate: Gate): VNode[] {
      const session = props.session

      switch (gate) {
        case 'loading':
          return [h('p', { class: 'agents__note' }, 'Reading your session…')]

        case 'unknown':
          return [
            h('h2', null, 'This console cannot tell whether you are signed in'),
            h('p', { class: 'agents__note' }, [
              session.status === 'failed' ? h('code', null, session.failure.endpoint) : '',
              ' did not answer, so this page is not saying that you may not mint. It is saying it ',
              'does not know, and creating a principal is not something to attempt on a guess.',
            ]),
          ]

        case 'anonymous':
          return [
            h('h2', null, 'Sign in to create a Service Account'),
            h('p', { class: 'agents__note' }, [
              'A Service Account is a principal of exactly one tenant, and the tenant is read from whoever ',
              'this service resolves you to be — never from anything this page could ask for. So ',
              'there is nobody to mint for until you sign in.',
            ]),
            h('p', null, h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT }, 'Sign in')),
          ]

        case 'may-not-mint':
          return [
            h('h2', null, 'Only a signed-in person may create a Service Account'),
            h('p', { class: 'agents__note' }, [
              'You are signed in as ',
              h('code', null, session.status === 'ready' && session.principal ? session.principal.kind : ''),
              ', and this host admits only a user here. That is a decision rather than an ',
              'oversight: revoking a token has to end the access it gave, and that holds only if ',
              'every minter is itself revocable by this host’s operator. A person’s account is ',
              'disabled at the identity provider; nothing in this deployment mints, verifies or ',
              'revokes anything else. A principal that could mint successors would leave an ',
              'operator revoking a leaked token, watching it stop working, and being wrong with no ',
              'way to find out.',
            ]),
          ]

        case 'may-mint':
          return [form()]
      }
    }

    return () => {
      const gate = gateOf(props.session)
      const outcome = result.value

      return h('section', { class: 'agents', 'data-page': 'agents' }, [
        h('h1', null, 'Service Accounts'),

        h('p', { class: 'agents__lead' }, [
          'A Service Account is a durable non-human identity for an App, Agent or automation. A ',
          'signed-in person creates it and hands over the token exactly once. This is where that ',
          'happens — the call is ',
          h('code', null, `POST ${SERVICE_ACCOUNTS_ENDPOINT}`),
          ', and how an App or Agent uses the result is on ',
          h('a', { href: fragmentPath(ONBOARDING_PATH) }, 'Connect an agent'),
          '.',
        ]),

        h(
          'section',
          { class: 'agents__gate', 'data-agents': 'gate', 'data-state': gate },
          withheldGate(gate)
        ),

        outcome?.status === 'minted'
          ? mintedPanel(outcome.minted, copied.value, copy, discard)
          : null,
        outcome?.status === 'refused' ? refusalNotice(outcome.refusal) : null,
        outcome?.status === 'failed'
          ? h('section', { class: 'failure', role: 'alert', 'data-agents': 'failed' }, [
              h('h3', { class: 'failure__title' }, 'No Service Account was created'),
              h('p', { class: 'failure__endpoint' }, [
                'Endpoint: ',
                h('code', null, outcome.failure.endpoint),
              ]),
              h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
            ])
          : null,

        gate === 'may-mint' ? accountsPanel() : null,

        standingPanel(),
      ].filter((node): node is VNode => node !== null))
    }
  },
})
