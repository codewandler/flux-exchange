// An operation detail made only from facts the exchange catalogue publishes.

import { computed, defineComponent, h, type PropType } from 'vue'
import type { Catalog, SearchView } from './catalog.mts'
import { explorerPath, fragmentPath } from './routing.ts'

export default defineComponent({
  name: 'CatalogueOperation',
  props: {
    catalog: { type: Object as PropType<Catalog>, required: true },
    id: { type: String, required: true },
    returnView: { type: Object as PropType<SearchView | undefined>, default: undefined },
  },
  setup(props) {
    const found = computed(() => {
      for (const connector of props.catalog.connectors) {
        const operation = connector.operations.find((candidate) => candidate.id === props.id)
        if (operation) return { connector, operation }
      }
      return null
    })

    return () => {
      if (!found.value) {
        return h('p', { class: 'catalogue-operation__missing' }, [
          'No operation with the id ',
          h('code', null, props.id),
          ' is in this exchange catalogue.',
        ])
      }

      const { connector, operation } = found.value
      const admission =
        operation.admitted === true
          ? 'Admitted for the resolved principal.'
          : operation.admitted === false
            ? 'Refused for the resolved principal.'
            : 'Not evaluated. This anonymous catalogue says what exists, not what a principal may call.'
      const provenance = operation.effectsDerived
        ? 'Inferred by the service from the operation; the connector did not declare these effects.'
        : 'Declared by the connector.'

      return h('article', { class: 'catalogue-operation', 'data-operation': operation.id }, [
        h('nav', { class: 'catalogue-operation__breadcrumbs', 'aria-label': 'Breadcrumb' }, [
          h('a', { href: fragmentPath(explorerPath(props.returnView ?? { kind: 'operations', query: '' })) }, 'Catalogue'),
          ' / ', h('span', { 'aria-current': 'page' }, operation.id),
        ]),
        h('p', { class: 'catalogue-operation__lede' }, operation.description),
        h('p', { class: 'catalogue-operation__chips' }, [
          h(
            'a',
            {
              class: 'finder-chip finder-chip--link',
              href: fragmentPath(explorerPath({ kind: 'connectors', query: connector.id })),
            },
            connector.vendor
          ),
          h('code', { class: 'finder-chip' }, operation.service),
          h('span', { class: 'finder-chip' }, `risk: ${operation.risk}`),
          h('span', { class: 'finder-chip' }, operation.idempotency),
        ]),
        h('section', { class: 'catalogue-operation__section' }, [
          h('h2', null, 'Effects'),
          operation.effects.length
            ? h(
                'p',
                { class: 'catalogue-operation__effects' },
                operation.effects.map((effect) => h('code', { key: effect }, effect))
              )
            : h('p', { class: 'catalogue-operation__note' }, 'No effects were published.'),
          h('p', { class: 'catalogue-operation__note', 'data-derived': String(operation.effectsDerived) }, provenance),
        ]),
        h('section', { class: 'catalogue-operation__section' }, [
          h('h2', null, 'Admission'),
          h('p', { class: 'catalogue-operation__note', 'data-admitted': String(operation.admitted) }, admission),
        ]),
        h('p', { class: 'catalogue-operation__action' }, [
          h('a', { href: fragmentPath(`/invoke?operation=${encodeURIComponent(operation.id)}`) }, 'Invoke this operation'),
        ]),
      ])
    }
  },
})
