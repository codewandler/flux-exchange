// The visible Connect → Grant → Invoke path, derived from current server answers.

import { defineComponent, h, type PropType } from 'vue'
import { setupJourney, type JourneyStepId } from './journey-model.mts'
import { fragmentPath } from './routing.ts'
import type { Connection, HeldGrant } from './service.mts'

export default defineComponent({
  name: 'Journey',
  props: {
    connections: { type: Array as PropType<Connection[]>, default: () => [] },
    grants: { type: Array as PropType<HeldGrant[]>, default: () => [] },
    active: { type: String as PropType<JourneyStepId>, required: true },
  },
  setup(props) {
    return () => h('nav', { class: 'journey', 'aria-label': 'Connector setup progress' }, [
      h('ol', { class: 'journey__steps' }, setupJourney({
        connections: props.connections,
        grants: props.grants,
        active: props.active,
      }).map((step, index) => h('li', {
        key: step.id,
        class: ['journey__step', `journey__step--${step.state}`],
        'aria-current': step.state === 'current' ? 'step' : undefined,
      }, [
        h('span', { class: 'journey__number', 'aria-hidden': 'true' }, step.state === 'complete' ? '✓' : String(index + 1)),
        step.state === 'locked'
          ? h('span', { class: 'journey__label' }, step.label)
          : h('a', { class: 'journey__label', href: fragmentPath(step.path) }, step.label),
        h('span', { class: 'journey__state' }, step.state),
      ]))),
    ])
  },
})
