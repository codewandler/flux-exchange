// The chrome around every screen: what this service is, what it can do, and who you are on it.
//
// **What this element is for.** Until this existed the console rendered the connector catalogue and
// a title bar, so it read as a connector browser — a document about connectors — rather than as the
// admin surface of a service that holds credentials and runs operations for many callers. The
// vision gives the console two jobs, *wire things up* and *see what happened*, and this is where
// the platform's shape is stated so a reader can see both of them at once. The surfaces themselves
// live in `surfaces.mts`; this file only renders them.
//
// **The half that is not built is the half this element is careful about.** `invoke`, `subscribe`
// and execution records appear here — hiding them would make the platform's own shape harder to see
// — and they appear as **inert entries with the reason beside them**. No href, `aria-disabled`, and
// a sentence saying what is missing. The temptation this repository has already had to remove
// several times is the plausible empty state: a screen headed "Activity" reading "No executions
// yet" is a claim that nothing has happened, and nothing has *run*. `test/shell.test.mjs` asserts
// the difference rather than trusting anyone to hold it.
//
// **Why a render function rather than a single-file component.** The same reason as
// `CatalogueFailure.mts`: the claims above are only worth anything if a
// test renders them, and `vue/server-renderer` renders a render function under plain `node --test`
// with no bundler and no new dependency. An SFC could only have been checked by grepping a
// template, which is not evidence that anything renders.
//
// It imports no theme module and holds no state. The theme toggle arrives through the `theme` slot,
// so this element can be rendered in a test where `window` does not exist.

import { defineComponent, h, type PropType, type VNode } from 'vue'
import { SIGNIN_ENDPOINT, type SessionState } from './service.mts'
import { SURFACES, type Surface } from './surfaces.mts'
import { fragmentPath } from './routing.ts'

/** The service this console administers. Its own name, not a product's. */
const SERVICE = 'flux-exchange'

/** Where the platform's surfaces are listed. Identity is rendered in the bar, beside the theme. */
const IDENTITY = 'identity'

/**
 * One surface as a navigation entry.
 *
 * A built one is an anchor through the fragment router — a bare `/connections` href would 404,
 * because this console is one static document (`routing.ts` argues it at length). An unbuilt one is
 * a `<span>`, which cannot be followed by construction rather than by a handler declining to.
 */
function entry(surface: Surface, active: string | null): VNode {
  const shared = {
    class: ['rail__entry', { 'rail__entry--here': surface.id === active }],
    'data-surface': surface.id,
    'data-built': String(surface.built),
  }

  if (surface.path) {
    return h(
      'a',
      {
        ...shared,
        href: fragmentPath(surface.path),
        title: surface.summary,
        // Read by a screen reader as the current page, and by `.rail__entry--here` as the underline.
        'aria-current': surface.id === active ? 'page' : undefined,
      },
      surface.label
    )
  }

  return h(
    'span',
    {
      ...shared,
      class: [...shared.class, 'rail__entry--absent'],
      'aria-disabled': 'true',
      // The reason, on the entry itself. A disabled control with no explanation reads as a broken
      // console rather than as a statement about the service.
      title: surface.absent,
    },
    [
      h('span', { class: 'rail__label' }, surface.label),
      // `no screen` and not `not built`: this entry is disabled because the console has nowhere to
      // send you, which for `invoke` is not the same claim as the service being unable to do it.
      // The tooltip carries which gap it is. See `surfaces.mts` on `built` versus `served`.
      h('span', { class: 'rail__tag' }, 'no screen'),
    ]
  )
}

/**
 * The one sentence that says what this deployment cannot do, built from the model.
 *
 * Derived rather than written out, so it cannot drift from the entries above it: the day `invoke`
 * lands, its entry becomes a link and it leaves this sentence in the same edit.
 */
function inventory(absent: readonly Surface[]): VNode {
  const names = absent.map((surface) => surface.label).join(', ')
  return h('p', { class: 'shell__inventory', 'data-shell': 'inventory' }, [
    h('strong', null, 'No screen: '),
    // "Not built" and "it calls nothing" were true when X-34 wrote this and stopped being true when
    // `invoke` shipped. What these entries have in common is that there is nowhere to click, which
    // is a statement about this console; which of them the *service* also cannot do is the
    // `served` flag, and the entry's own reason says so. See `surfaces.mts`.
    `${names}. This deployment holds credentials and serves a catalogue of what it could run. ` +
      'These entries lead nowhere rather than to a page that would imply otherwise — hover one ' +
      'for what is missing, which is not the same answer for all of them.',
  ])
}

/** Who the service says you are, or the honest absence of an answer. */
function identity(session: SessionState, emit: (event: 'signOut' | 'retrySession') => void): VNode {
  const surface = SURFACES.find((candidate) => candidate.id === IDENTITY)
  const box = (state: string, children: unknown[]) =>
    h(
      'div',
      {
        class: 'shell__identity',
        'data-surface': IDENTITY,
        'data-built': String(surface ? surface.built : true),
        'data-session': state,
        // A labelled group rather than a bare div: identity is one of the platform's surfaces and
        // it is the one with no page of its own, so the label it is listed under has to reach a
        // reader who is not looking at the header.
        role: 'group',
        'aria-label': surface ? surface.label : IDENTITY,
      },
      children as VNode[]
    )

  // Named, so a request that never comes back is visibly a request and not an empty header.
  if (session.status === 'loading') {
    return box('loading', [h('span', { class: 'shell__quiet' }, 'Reading your session…')])
  }

  // Not "signed out". The console does not know, and saying it does would report an outage as a
  // sign-out — the same collapse `CatalogueFailure` exists to prevent, one endpoint over.
  if (session.status === 'failed') {
    return box('failed', [
      h('span', { class: 'shell__quiet' }, [
        'Session unknown — ',
        h('code', null, session.failure.endpoint),
        ' did not answer.',
      ]),
      h('button', { class: 'shell__signout', type: 'button', onClick: () => emit('retrySession') }, 'Retry'),
      h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT, 'data-shell': 'signin' }, 'Sign in'),
    ])
  }

  if (!session.principal) {
    return box('anonymous', [
      h('span', { class: 'shell__quiet' }, 'Not signed in'),
      // An anchor and never a fetch: `/api/signin` answers `303` to the identity provider, and a
      // fetch would follow that redirect inside the page while the address bar never moved.
      h('a', { class: 'shell__signin', href: SIGNIN_ENDPOINT, 'data-shell': 'signin' }, 'Sign in'),
    ])
  }

  const { kind, id, tenant } = session.principal
  return box(kind || 'principal', [
    h('span', { class: 'shell__who', title: surface ? surface.summary : undefined }, [
      h('span', { class: 'shell__who-id' }, id),
      // The tenant, given equal weight. Every credential address on this console is derived from
      // it, so a header naming the person but not the tenant cannot explain what is being shown.
      h('span', { class: 'shell__who-tenant' }, tenant),
    ]),
    h(
      'button',
      {
        class: 'shell__signout',
        type: 'button',
        'data-shell': 'signout',
        onClick: () => emit('signOut'),
      },
      'Sign out'
    ),
  ])
}

export default defineComponent({
  name: 'ConsoleShell',
  props: {
    /** What `/api/session` said. Read, never invented. */
    session: { type: Object as PropType<SessionState>, required: true },
    /** The surface the reader is on, or `null` for a route that belongs to none. */
    active: { type: String as PropType<string | null>, default: null },
  },
  emits: ['signOut', 'retrySession'],
  setup(props, { emit, slots }) {
    const rail = SURFACES.filter((surface) => surface.id !== IDENTITY)
    const absent = rail.filter((surface) => !surface.built)
    const built = rail.filter((surface) => surface.built)

    return () =>
      h('header', { class: 'shell', 'data-shell': 'head' }, [
        h('div', { class: 'shell__bar' }, [
          h(
            'a',
            {
              class: 'shell__brand',
              // Home is connections, not the catalogue — see `routing.ts`.
              href: fragmentPath('/connections'),
            },
            [SERVICE, h('span', { class: 'shell__brand-tag' }, 'console')]
          ),
          h('div', { class: 'shell__bar-end' }, [
            identity(props.session, emit),
            // The theme toggle belongs to the app, which owns the only module that touches
            // `window`. Slotted so this element stays renderable where there is no document.
            slots.theme ? h('div', { class: 'shell__theme' }, slots.theme()) : null,
          ]),
        ]),

        h(
          'nav',
          { class: 'rail', 'data-shell': 'surfaces', 'aria-label': 'Platform surfaces' },
          [
            ...built.map((surface) => entry(surface, props.active)),
            h('span', { class: 'rail__desktop-future' }, absent.map((surface) => entry(surface, props.active))),
            absent.length ? h('details', { class: 'rail__future' }, [
              h('summary', null, `Future (${absent.length})`),
              h('div', { class: 'rail__future-list' }, absent.map((surface) => entry(surface, props.active))),
            ]) : null,
          ]
        ),

        absent.length ? inventory(absent) : null,
      ])
  },
})
