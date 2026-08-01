// The page an agent author lands on: what this service is, and what it will and will not do for
// them today.
//
// **What this element renders is not written here.** Every claim about what an agent can do comes
// from `onboarding.mts`, which derives it from `surfaces.mts` — the same declaration the navigation
// reads. This file decides layout and nothing else. That split is the whole point: copy describing a
// platform that changes weekly is false within a release, and `test/onboarding.test.mjs` asserts the
// page cannot present a step whose backing surface the console marks unbuilt.
//
// **It takes no props, and that is a claim rather than an omission.** The page is reachable signed
// out — an agent that must authenticate to learn how to authenticate is a closed loop — so it must
// carry nothing belonging to a tenant: no connector list, no principal, no address, no count. A
// component with no input cannot render any of them, which is a stronger guarantee than reading the
// copy carefully. Neither this file nor `onboarding.mts` imports `service.mts`, the one module here
// that knows a network exists, so nothing is read either.
//
// **Why a render function rather than a single-file component.** The same reason as
// `ConsoleShell.mts` and `CatalogueFailure.mts`: the claims above are only worth anything if a test
// renders them, and `vue/server-renderer` renders a render function under plain `node --test` with
// no bundler and no new dependency. Its rules live in `onboarding.css` for the reason `shell.css`
// gives — a separate file is what makes the "names no colour of its own" scan precise.

import { defineComponent, h, type VNode } from 'vue'
import { STEPS, available, pendingSummary, withheld, type Step } from './onboarding.mts'

/** The service this page is about, by the one name it is published under. */
const SERVICE = 'flux-exchange'

/** How to do it: the call, who makes it, and the sentence not to skim. */
function instruction(step: Step): VNode[] {
  // Never reached for a withheld step — `onboarding.mts` holds `call === null` for those, and the
  // tests hold it both ways. The guard is here so the type narrows, not as a second opinion.
  if (!step.call) return []
  const { method, endpoint, caller, note, warn } = step.call

  return [
    h('p', { class: 'onboard__call' }, [
      h('code', { class: 'onboard__endpoint' }, `${method} ${endpoint}`),
    ]),
    h('p', { class: 'onboard__caller' }, caller),
    h('p', { class: 'onboard__note' }, note),
    warn ? h('p', { class: 'onboard__warn' }, h('strong', null, warn)) : null,
  ].filter((node): node is VNode => node !== null)
}

/**
 * One step, in the state this build puts it in.
 *
 * A withheld step is not hidden and not softened. It is named, tagged, and given the reason beside
 * it — the same treatment the navigation gives an unbuilt surface, and for the same reason: a
 * missing step a reader has to infer is worse than one that says what is missing.
 */
function entry(step: Step): VNode {
  const can = available(step)

  return h(
    'li',
    {
      class: ['onboard__step', { 'onboard__step--pending': !can }],
      'data-step': step.id,
      'data-available': String(can),
    },
    [
      h('h2', { class: 'onboard__title' }, [
        step.title,
        can ? null : h('span', { class: 'onboard__tag' }, 'not yet'),
      ]),
      h('p', { class: 'onboard__summary' }, step.summary),
      ...(can
        ? instruction(step)
        : [h('p', { class: 'onboard__withheld' }, withheld(step))]),
    ]
  )
}

export default defineComponent({
  name: 'AgentOnboarding',
  setup() {
    return () =>
      h('section', { class: 'onboard', 'data-page': 'connect' }, [
        h('h1', null, 'Connect an agent'),

        h('p', { class: 'onboard__lead' }, [
          `${SERVICE} holds credentials, terminates channels, runs operations for many callers and `,
          'records what happened. Its primary caller is an agent, not a human: people sign in to ',
          'wire things up, and agents are what call operations all day.',
        ]),

        h('p', { class: 'onboard__lead' }, [
          'Below is what that means for an agent arriving at ',
          h('em', null, 'this'),
          ' deployment — which of it exists, which of it does not, and the one step that works ',
          'today. It is short because most of the platform is not built, and saying so is cheaper ',
          'than letting you find out.',
        ]),

        // Derived, so the day a surface lands this sentence loses the step in the same edit.
        h('p', { class: 'onboard__pending', 'data-onboard': 'pending' }, pendingSummary()),

        h('ol', { class: 'onboard__steps' }, STEPS.map(entry)),

        h('section', { class: 'onboard__aside' }, [
          h('h2', null, 'What a token from here is, and is not'),
          h('p', null, [
            "An agent's token grants access to operations, never to credentials. A caller names an ",
            'operation and gets a result: it cannot name a host, because the URL comes from the ',
            "operation's own compiled Flux; it cannot name a credential, because the address is ",
            "derived from the tenant and the connector's declared authority; and it cannot name a ",
            'tenant, because that is read from the resolved principal and from nothing a caller ',
            'controls. So a stolen token is a bounded set of operations against one tenant, and ',
            'never a vendor secret. That rule is why the list above is the shape it is.',
          ]),
        ]),

        h('section', { class: 'onboard__aside' }, [
          h('h2', null, 'What this page is'),
          h('p', null, [
            'It is derived rather than written. What it says an agent can do comes from the same ',
            'declaration the navigation at the top of this console reads, so a step here cannot ',
            'claim a surface that navigation marks unbuilt — and a surface that regresses takes its ',
            'step off this page with it.',
          ]),
          h('p', null, [
            'It also knows nothing about you. It reads nothing over the network, signed in or not, ',
            'and names no connector, tenant, principal, address or count. It describes the shape of ',
            'this service and never its contents.',
          ]),
        ]),
      ])
  },
})
