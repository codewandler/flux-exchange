// The derived "is this built" badge, rendered into the chrome of every capability page (X-64).
//
// **Why this is a component in the layout and not a line in the markdown.** The five renderings this
// story exists to prevent went wrong the same way twice over: an author wrote a caveat, the caveat
// went stale, and — the part that made it dangerous — the caveat and the claim it qualified drifted
// apart on the page, so a reader met the claim first and often only. A badge an author types into a
// paragraph is both failures waiting again. This one is rendered by the layout, above the page's own
// heading, from `pageData.frontmatter.capabilityStatus`, which `.vitepress/config.mts` stamps on at
// build time from the agent descriptor. There is nothing here for an author to write and nothing for
// them to forget to update.
//
// **Why a render function rather than a single-file component**, matching `console/src/`: the same
// reason stated at length in `console/src/CatalogueFailure.mts` — the assertion that this renders is
// made by reading real HTML, and a template grepped for a string is not evidence that anything
// renders. `web/test/status.test.mjs` reads `.vitepress/dist`, which is this component
// server-rendered by VitePress.
//
// `data-capability` and `data-live` are the contract, the way `data-step` and `data-available` are
// on the console's onboarding page. A class name is a styling decision somebody is entitled to
// change; those two attributes are what a test addresses and what they mean is fixed.

import { defineComponent, h } from 'vue'
import { useData } from 'vitepress'

import type { CapabilityStatus } from '../descriptor.mts'

/** What the badge says, in the two states the descriptor can be in. */
const VERDICT = {
  live: 'Built',
  planned: 'Not built',
} as const

export default defineComponent({
  name: 'CapabilityStatus',
  setup() {
    const { frontmatter } = useData()

    return () => {
      const status = frontmatter.value.capabilityStatus as CapabilityStatus | undefined

      // Every page that should have one has one: `statusFor` fails the build for a page under
      // `capabilities/` that does not. So a page reaching here without a status is a page that is
      // not about a capability — the overview, the boundary argument, getting started — and it
      // renders no chrome rather than an empty box.
      if (!status) return null

      const verdict = status.live ? VERDICT.live : VERDICT.planned

      return h(
        'aside',
        {
          class: ['capability-status', status.live ? 'is-live' : 'is-planned'],
          'data-capability': status.id,
          'data-live': String(status.live),
          'aria-label': `Status of the ${status.title} capability`,
        },
        [
          h('p', { class: 'capability-status__verdict' }, [
            h('strong', null, verdict),
            // The endpoint belongs beside the verdict rather than in the prose below it, for the
            // reason the verdict is up here at all: a reader who takes one thing off this page
            // should take away what to call and whether it answers.
            status.call ? h('code', { class: 'capability-status__call' }, status.call) : null,
          ]),

          // The reason, in the descriptor's own words. Rendered rather than summarised: a page
          // paraphrasing why something is not built is a sixth rendering of the same claim, which
          // is what this mechanism exists to stop.
          status.withheld ? h('p', { class: 'capability-status__withheld' }, status.withheld) : null,

          h('p', { class: 'capability-status__source' }, [
            'Derived from this build’s ',
            h('code', null, 'GET /api/onboarding'),
            ' descriptor, whose answers are held to the service’s own route table. It is not written on this page.',
          ]),
        ]
      )
    }
  },
})
