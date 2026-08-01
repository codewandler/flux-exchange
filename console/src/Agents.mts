// Minting an agent, and the one disclosure this console will ever make.
//
// X-36 shipped `POST /api/agents` and deliberately no UI. X-41 published the page that tells an
// agent author how to get an identity, and the best answer it could give was "ask a human to
// `curl`". This is the human's screen, and it is shaped end to end by one property:
//
//   > **The token is shown once.** `crate::agent` keeps a *verifier*, so this host is genuinely
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
// **There is no affordance that implies retrieval.** No "show again", no reveal toggle, no history,
// no listing. The one control that touches the token after it is shown takes it *off* the page and
// cannot put it back, because there is nothing left to put back. `service.mts` has no function that
// could fetch one, which is the same statement one layer down.
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
// shown one and script running as that human is not. The remedy is revocation, which is X-38 and is
// not built; that is why the expiry is stated on every mint and is never defaulted.
//
// A render function rather than a single-file component, following `Connect.mts` and
// `ConsoleShell.mts`: the claims above are only worth anything if a test drives them, and a render
// function mounts under a plain `node --test` with no bundler and no new dependency. Its rules live
// in `agents.css` for the reason `shell.css` gives.

import { defineComponent, h, ref, shallowRef, type PropType, type VNode } from 'vue'
import { AGENTS_ENDPOINT, mintAgent, type MintOutcome, type MintedAgent } from './service.mts'
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
      return `${reason.endpoint} could not be reached. ${reason.detail} Nothing was sent, so no agent was created — and this is not the service saying no.`
    case 'refused':
      return `${reason.endpoint} answered ${reason.status}, with no sentence this console could read. ${reason.detail}`
    case 'unreadable':
      return `${reason.endpoint} answered ${reason.status} with a body this console could not read. ${reason.detail} Whether an agent now exists is therefore unknown, and if one does, its token was in that body and is gone.`
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
  minted: MintedAgent,
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
        'holding a copy back. If it is lost, the remedy is to mint another agent, not to look it up.'
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
      h('dt', null, 'Agent'),
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
      'There is no way to revoke this token — not from this console, and not from this host, which ' +
        'serves no route that would. Until there is, a token that leaks stops working when the ' +
        'expiry above passes and not before. That is the whole reason the lifetime is yours to ' +
        'state and is never defaulted for you.'
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
  name: 'Agents',
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
      result.value = await mintAgent({ id: id.value.trim(), expiresAt: expiryFromNow(chosen) })
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
          'What to call this agent within your tenant. It is a name, not an address.',
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
            'and this console will not choose one either — a token that outlives the reason it ' +
            'was made is the thing revocation would exist to fix, and revocation is not built.',
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
          busy.value ? 'Minting…' : 'Mint an agent'
        ),
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
            h('h2', null, 'Sign in to mint an agent'),
            h('p', { class: 'agents__note' }, [
              'An agent is a principal of exactly one tenant, and the tenant is read from whoever ',
              'this service resolves you to be — never from anything this page could ask for. So ',
              'there is nobody to mint for until you sign in.',
            ]),
            h('p', null, h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT }, 'Sign in')),
          ]

        case 'may-not-mint':
          return [
            h('h2', null, 'Only a signed-in person may create an agent'),
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
        h('h1', null, 'Mint an agent'),

        h('p', { class: 'agents__lead' }, [
          `${SERVICE}'s primary caller is an agent, not a human. An agent does not sign in: a `,
          'person who is already signed in mints it and hands over the token, exactly once. This ',
          'is where that happens — the call is ',
          h('code', null, `POST ${AGENTS_ENDPOINT}`),
          ', and what an agent author does with the result is on ',
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
              h('h3', { class: 'failure__title' }, 'No agent was minted'),
              h('p', { class: 'failure__endpoint' }, [
                'Endpoint: ',
                h('code', null, outcome.failure.endpoint),
              ]),
              h('p', { class: 'failure__message' }, failureSentence(outcome.failure)),
            ])
          : null,

        standingPanel(),
      ].filter((node): node is VNode => node !== null))
    }
  },
})
