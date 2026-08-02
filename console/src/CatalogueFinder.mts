// One exchange-owned finder over the catalogue this service actually publishes.

import {
  computed,
  defineComponent,
  h,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
  type PropType,
} from 'vue'
import {
  SEARCH_KINDS,
  deriveServices,
  operationPath,
  searchCatalogue,
  searchCounts,
  type Catalog,
  type ConnectorResult,
  type OperationResult,
  type SearchKind,
  type SearchResult,
  type SearchView,
  type ServiceResult,
} from './catalog.mts'
import { fragmentPath, replaceExplorerView } from './routing.ts'

const LABELS: Record<SearchKind, string> = {
  connectors: 'Connectors',
  services: 'Services',
  operations: 'Operations',
}

function plural(kind: SearchKind, count: number): string {
  const singular = kind === 'connectors' ? 'connector' : kind === 'services' ? 'service' : 'operation'
  return `${count} ${singular}${count === 1 ? '' : 's'}`
}

function highlighted(value: string, query: string) {
  const terms = query.trim().split(/\s+/).filter(Boolean)
  if (!terms.length) return value
  const escaped = terms.map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  const matcher = new RegExp(`(${escaped.join('|')})`, 'ig')
  return value.split(matcher).map((part, index) =>
    terms.some((term) => term.toLowerCase() === part.toLowerCase())
      ? h('mark', { key: `${index}-${part}` }, part)
      : part
  )
}

export default defineComponent({
  name: 'CatalogueFinder',
  props: {
    catalog: { type: Object as PropType<Catalog>, required: true },
    initialView: { type: Object as PropType<SearchView>, required: true },
  },
  setup(props) {
    const view = ref<SearchView>({ ...props.initialView })
    const searchInput = ref<HTMLInputElement | null>(null)
    const counts = computed(() => searchCounts(props.catalog, view.value.query))
    const shown = computed(() => searchCatalogue(props.catalog, view.value))
    const totalOperations = computed(() =>
      props.catalog.connectors.reduce((sum, connector) => sum + connector.operations.length, 0)
    )

    // Hash navigation can replace one finder URL with another without remounting this component.
    // Keep browser back/forward authoritative instead of leaving the controls on stale local state.
    watch(
      () => props.initialView,
      (next) => {
        view.value = { ...next }
      },
      { deep: true }
    )

    function replace(next: SearchView) {
      view.value = next
      replaceExplorerView(next)
    }

    function choose(kind: SearchKind) {
      replace({ kind, query: view.value.query })
    }

    function focusShortcut(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null
      if (event.key !== '/' || target?.matches('input, textarea, select, [contenteditable="true"]')) return
      event.preventDefault()
      searchInput.value?.focus()
    }
    onMounted(() => { if (typeof window !== 'undefined') window.addEventListener('keydown', focusShortcut) })
    onBeforeUnmount(() => { if (typeof window !== 'undefined') window.removeEventListener('keydown', focusShortcut) })

    function keydown(event: KeyboardEvent, kind: SearchKind) {
      const current = SEARCH_KINDS.indexOf(kind)
      let next = current
      if (event.key === 'ArrowRight') next = (current + 1) % SEARCH_KINDS.length
      else if (event.key === 'ArrowLeft') next = (current - 1 + SEARCH_KINDS.length) % SEARCH_KINDS.length
      else if (event.key === 'Home') next = 0
      else if (event.key === 'End') next = SEARCH_KINDS.length - 1
      else return

      event.preventDefault()
      const selected = SEARCH_KINDS[next]
      choose(selected)
      const tabs = (event.currentTarget as HTMLElement).parentElement
      ;(tabs?.querySelector(`[data-kind="${selected}"]`) as HTMLElement | null)?.focus()
    }

    function showConnector(result: ConnectorResult) {
      replace({ kind: 'operations', query: result.connector.id })
    }

    function showService(result: ServiceResult) {
      replace({
        kind: 'operations',
        query: `${result.connector.id} ${result.service.name}`,
      })
    }

    function connectorCard(result: ConnectorResult) {
      const serviceCount = deriveServices({ connectors: [result.connector] }).length
      return h('li', { class: 'finder-card', key: result.connector.id }, [
        h('div', { class: 'finder-card__head' }, [
          h('h2', { class: 'finder-card__title' }, highlighted(result.connector.vendor, view.value.query)),
          h('code', { class: 'finder-card__id' }, highlighted(result.connector.id, view.value.query)),
        ]),
        h('p', { class: 'finder-card__description' }, highlighted(result.connector.description, view.value.query)),
        h('p', { class: 'finder-card__meta' }, [
          plural('operations', result.connector.operationCount),
          ' · ',
          plural('services', serviceCount),
        ]),
        h(
          'button',
          { type: 'button', class: 'finder-card__action', onClick: () => showConnector(result) },
          'Show operations'
        ),
      ])
    }

    function serviceRow(result: ServiceResult) {
      return h(
        'li',
        { class: 'finder-row', key: `${result.connector.id}/${result.service.name}` },
        [
          h('div', { class: 'finder-row__body' }, [
            h('div', { class: 'finder-row__title-line' }, [
              h('code', { class: 'finder-row__title' }, highlighted(result.service.name, view.value.query)),
              h('span', { class: 'finder-row__owner' }, result.connector.vendor),
            ]),
            h('p', { class: 'finder-row__description' }, [
              plural('operations', result.service.operationCount),
              ` · connector ${result.connector.id}`,
            ]),
          ]),
          h(
            'button',
            { type: 'button', class: 'finder-row__action', onClick: () => showService(result) },
            'Show operations'
          ),
        ]
      )
    }

    function operationRow(result: OperationResult) {
      const operation = result.operation
      return h('li', { class: 'finder-row', key: operation.id }, [
        h('div', { class: 'finder-row__body' }, [
          h(
            'a',
            { class: 'finder-row__title finder-row__link', href: fragmentPath(operationPath(operation, view.value)) },
            operation.id
          ),
          h('p', { class: 'finder-row__description' }, highlighted(operation.description, view.value.query)),
          h('p', { class: 'finder-row__chips' }, [
            h('span', { class: 'finder-chip' }, result.connector.vendor),
            h('code', { class: 'finder-chip' }, operation.service),
            h('span', { class: 'finder-chip' }, `risk: ${operation.risk}`),
            h('span', { class: 'finder-chip' }, operation.idempotency),
            ...operation.effects.map((effect) =>
              h('code', { class: 'finder-chip finder-chip--quiet', key: effect }, effect)
            ),
          ]),
        ]),
      ])
    }

    function result(result: SearchResult) {
      if (result.kind === 'connectors') return connectorCard(result)
      if (result.kind === 'services') return serviceRow(result)
      return operationRow(result)
    }

    function groupedOperations(results: SearchResult[]) {
      const groups = new Map<string, OperationResult[]>()
      for (const candidate of results) {
        if (candidate.kind !== 'operations') continue
        const key = `${candidate.connector.id}/${candidate.operation.service}`
        groups.set(key, [...(groups.get(key) ?? []), candidate])
      }
      return h('div', { class: 'finder__groups' }, [...groups.entries()].map(([key, operations]) => h('section', { key, class: 'finder__group' }, [
        h('h2', { class: 'finder__group-title' }, [
          h('code', null, operations[0].operation.service),
          ` · ${operations[0].connector.vendor}`,
          h('span', null, String(operations.length)),
        ]),
        h('ul', { class: 'finder__results finder__results--operations' }, operations.map(operationRow)),
      ])))
    }

    return () => {
      const kind = view.value.kind
      const count = shown.value.length
      const query = view.value.query.trim()
      return h('section', { class: 'finder', 'data-kind': kind }, [
        h('p', { class: 'finder__summary' }, [
          h('strong', null, String(props.catalog.connectors.length)),
          ' connectors · ',
          h('strong', null, String(deriveServices(props.catalog).length)),
          ' services · ',
          h('strong', null, String(totalOperations.value)),
          ' operations this build can run.',
        ]),
        h('label', { class: 'finder__search' }, [
          h('span', { class: 'finder__search-label' }, 'Search catalogue'),
          h('span', { class: 'finder__search-control' }, [
            h(
              'svg',
              {
                class: 'finder__search-icon',
                viewBox: '0 0 24 24',
                width: '20',
                height: '20',
                fill: 'none',
                stroke: 'currentColor',
                'stroke-width': '2',
                'aria-hidden': 'true',
              },
              [
                h('circle', { cx: '11', cy: '11', r: '7' }),
                h('path', { d: 'm20 20-3.5-3.5' }),
              ]
            ),
            h('input', {
              ref: searchInput,
              value: view.value.query,
              type: 'search',
              placeholder: 'Search connectors, services, and operations  /',
              'aria-describedby': 'finder-count',
              onInput: (event: Event) =>
                replace({ kind, query: (event.target as HTMLInputElement).value }),
            }),
            query
              ? h(
                  'button',
                  {
                    type: 'button',
                    class: 'finder__clear',
                    onClick: () => replace({ kind, query: '' }),
                  },
                  'Clear'
                )
              : null,
          ]),
        ]),
        h(
          'div',
          { class: 'finder__tabs', role: 'tablist', 'aria-label': 'Catalogue result types' },
          SEARCH_KINDS.map((candidate) =>
            h(
              'button',
              {
                key: candidate,
                type: 'button',
                role: 'tab',
                id: `finder-tab-${candidate}`,
                'aria-controls': 'finder-results',
                'aria-selected': String(candidate === kind),
                tabindex: candidate === kind ? 0 : -1,
                'data-kind': candidate,
                class: ['finder__tab', candidate === kind ? 'finder__tab--active' : ''],
                onClick: () => choose(candidate),
                onKeydown: (event: KeyboardEvent) => keydown(event, candidate),
              },
              [LABELS[candidate], h('span', { class: 'finder__tab-count' }, String(counts.value[candidate]))]
            )
          )
        ),
        h(
          'p',
          { id: 'finder-count', class: 'finder__count', 'aria-live': 'polite' },
          query
            ? `${plural(kind, count)} matching “${query}”.`
            : `Browsing ${plural(kind, count)}.`
        ),
        h(
          'div',
          {
            id: 'finder-results',
            role: 'tabpanel',
            'aria-labelledby': `finder-tab-${kind}`,
          },
          count
            ? kind === 'operations' && !query
              ? groupedOperations(shown.value)
              : h(
                'ul',
                { class: ['finder__results', `finder__results--${kind}`] },
                shown.value.map(result)
              )
            : h('div', { class: 'finder__empty' }, [
                h('h2', null, `No ${kind} match${query ? ` “${query}”` : ''}.`),
                query
                  ? h(
                      'button',
                      { type: 'button', class: 'finder__empty-action', onClick: () => replace({ kind, query: '' }) },
                      'Clear search'
                    )
                  : null,
              ])
        ),
      ])
    }
  },
})
