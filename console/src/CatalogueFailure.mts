// What the console shows when there is no catalogue.
//
// **The distinction this element exists for.** A console whose catalogue request failed and a
// console whose catalogue is genuinely empty look identical if both simply render nothing: the
// reader sees no connectors and concludes there are none. So a failed load renders *this* instead of
// an explorer — a block that names the endpoint that did not answer, says what kind of not-answering
// it was, and states outright that the page is empty because the request failed. `role="alert"` and
// `data-catalogue="failed"` are the same statement for a screen reader and for a test.
//
// **Why a render function rather than a single-file component.** The property above is asserted by
// `test/service.test.mjs`, which runs under `node --test` with no bundler — the same choice
// `test/components.test.mjs` explains at length. An SFC would need the Vue compiler wired into the
// test runner to be rendered at all, so the assertion would degrade into grepping a template for a
// string, which is not evidence that anything renders. Written this way it is server-rendered by
// `vue/server-renderer`, which ships inside `vue` itself, and the test reads the real HTML.
//
// This is app-layer and deliberately not under `src/components/`: that directory is the fifteen
// components shared with flux-connectors, and a sixteenth that imported this console's service
// client would break the invariant they are carried by.

import { defineComponent, h, type PropType } from 'vue'
import { failureHeadline, failureMessage, type CatalogueFailure } from './service.mts'

export default defineComponent({
  name: 'CatalogueFailure',
  props: {
    failure: { type: Object as PropType<CatalogueFailure>, required: true },
  },
  setup(props) {
    return () =>
      h(
        'section',
        { class: 'failure', role: 'alert', 'data-catalogue': 'failed', 'data-failure': props.failure.kind },
        [
          h('h1', { class: 'failure__title' }, failureHeadline(props.failure)),
          h('p', { class: 'failure__endpoint' }, [
            'Endpoint: ',
            h('code', null, props.failure.endpoint),
          ]),
          h('p', { class: 'failure__message' }, failureMessage(props.failure)),
        ]
      )
  },
})
