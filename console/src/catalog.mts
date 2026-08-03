// The catalogue exactly as flux-exchange serves it, plus the pure questions its finder asks.
//
// X-86 deliberately ended the old arrangement where this file mirrored flux-connectors' public
// documentation document. That document contains request paths, hosts, generated Flux, credentials,
// core entries and inbound bindings; this service publishes none of them. An exchange-owned type
// makes an unpublished fact impossible to render instead of representing it as a plausible blank.

/** One operation in the anonymous exchange catalogue. */
export interface Operation {
  id: string
  service: string
  description: string
  /** The exact declared parameter object the runtime tool validates. */
  inputSchema: Record<string, unknown>
  risk: string
  idempotency: string
  effects: string[]
  /** Whether `effects` were inferred by this host rather than declared by the connector. */
  effectsDerived: boolean
  /** `null` means admission was not evaluated; it is never another spelling of `false`. */
  admitted: boolean | null
}

/** One connector and the operations the exchange fetched for it. */
export interface Connector {
  id: string
  vendor: string
  description: string
  operationCount: number
  channelCount: number
  operations: Operation[]
}

/** The complete anonymous catalogue. */
export interface Catalog {
  connectors: Connector[]
}

/** A service is real catalogue data reconstructed from the service on every operation. */
export interface Service {
  name: string
  operationCount: number
}

export interface ServiceEntry {
  connector: Connector
  service: Service
}

/** The three result kinds this exchange can actually populate. */
export const SEARCH_KINDS = ['connectors', 'services', 'operations'] as const
export type SearchKind = (typeof SEARCH_KINDS)[number]

/** One search field and one selected result kind. */
export interface SearchView {
  kind: SearchKind
  query: string
}

export type ConnectorResult = { kind: 'connectors'; connector: Connector }
export type ServiceResult = { kind: 'services'; connector: Connector; service: Service }
export type OperationResult = {
  kind: 'operations'
  connector: Connector
  operation: Operation
}
export type SearchResult = ConnectorResult | ServiceResult | OperationResult
export type SearchCounts = Record<SearchKind, number>

/** The browse view: the connector directory, unconstrained. */
export function emptySearchView(): SearchView {
  return { kind: 'connectors', query: '' }
}

/** Collapse the only insignificant syntax search accepts. */
export function normalizeQuery(query: string): string {
  return String(query ?? '').trim().replace(/\s+/g, ' ')
}

/** Every service, connector by connector and in first-operation order. */
export function deriveServices(catalog: Catalog): ServiceEntry[] {
  return catalog.connectors.flatMap((connector) => {
    const counts = new Map<string, number>()
    for (const operation of connector.operations) {
      counts.set(operation.service, (counts.get(operation.service) ?? 0) + 1)
    }
    return [...counts].map(([name, operationCount]) => ({
      connector,
      service: { name, operationCount },
    }))
  })
}

interface Candidate {
  result: SearchResult
  primary: string[]
  visible: string[]
  order: number
}

/** Candidates carry exactly the fields their result renders; an invisible fact cannot match. */
function candidates(catalog: Catalog, kind: SearchKind): Candidate[] {
  if (kind === 'connectors') {
    return catalog.connectors.map((connector, order) => ({
      result: { kind, connector },
      primary: [connector.id, connector.vendor],
      visible: [connector.id, connector.vendor, connector.description],
      order,
    }))
  }

  if (kind === 'services') {
    return deriveServices(catalog).map(({ connector, service }, order) => ({
      result: { kind, connector, service },
      primary: [service.name],
      visible: [service.name, connector.id, connector.vendor],
      order,
    }))
  }

  let order = 0
  return catalog.connectors.flatMap((connector) =>
    connector.operations.map((operation) => ({
      result: { kind, connector, operation },
      primary: [operation.id],
      visible: [
        operation.id,
        operation.description,
        connector.id,
        connector.vendor,
        operation.service,
        operation.risk,
        operation.idempotency,
        ...operation.effects,
      ],
      order: order++,
    }))
  )
}

/**
 * Relevance without an index worth maintaining for a catalogue of hundreds.
 *
 * Every term must occur somewhere visible. The full normalized query then decides the useful
 * ordering: exact primary name, prefix, primary substring, metadata. Source order is the stable
 * tiebreaker and is the whole ordering when the query is empty.
 */
function relevance(candidate: Candidate, query: string): number | null {
  if (!query) return 0

  const whole = query.toLowerCase()
  const terms = whole.split(' ')
  const primary = candidate.primary.map((field) => field.toLowerCase())
  const visible = candidate.visible.map((field) => field.toLowerCase())
  if (!terms.every((term) => visible.some((field) => field.includes(term)))) return null

  if (primary.some((field) => field === whole)) return 0
  if (primary.some((field) => field.startsWith(whole))) return 1
  if (terms.every((term) => primary.some((field) => field.includes(term)))) return 2
  return 3
}

/** Results for one selected tab. */
export function searchCatalogue(catalog: Catalog, view: SearchView): SearchResult[] {
  const query = normalizeQuery(view.query)
  return candidates(catalog, view.kind)
    .map((candidate) => ({ candidate, score: relevance(candidate, query) }))
    .filter(
      (entry): entry is { candidate: Candidate; score: number } => entry.score !== null
    )
    .sort((a, b) => a.score - b.score || a.candidate.order - b.candidate.order)
    .map(({ candidate }) => candidate.result)
}

/** Match counts shown on all three tabs for the current query. */
export function searchCounts(catalog: Catalog, query: string): SearchCounts {
  return Object.fromEntries(
    SEARCH_KINDS.map((kind) => [kind, searchCatalogue(catalog, { kind, query }).length])
  ) as SearchCounts
}

/** Canonical state after `/explorer?`, with defaults omitted. */
export function encodeSearchView(view: SearchView): string {
  const query = normalizeQuery(view.query)
  const params = new URLSearchParams()
  if (view.kind !== 'connectors') params.set('kind', view.kind)
  if (query) params.set('q', query)
  return params.toString()
}

/** Unknown state widens to Connectors rather than manufacturing an empty result kind. */
export function decodeSearchView(search: string): SearchView {
  const params = new URLSearchParams(search.replace(/^\?/, ''))
  const candidate = params.get('kind') ?? ''
  const kind = SEARCH_KINDS.includes(candidate as SearchKind)
    ? (candidate as SearchKind)
    : 'connectors'
  return { kind, query: normalizeQuery(params.get('q') ?? '') }
}

/** The stable detail route of one operation. */
export function operationPath(operation: Operation, returnView?: SearchView): string {
  const path = `/operations/${encodeURIComponent(operation.id)}`
  if (!returnView) return path
  const params = new URLSearchParams({
    return_kind: returnView.kind,
    return_q: returnView.query,
  })
  return `${path}?${params}`
}
