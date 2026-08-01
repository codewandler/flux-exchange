// The flux-exchange service, as this console reads it.
//
// This is the **only** module that knows a network exists. The fifteen components under
// `src/components/` take everything they render as a prop and resolve paths through an injected
// `PathResolver`; that is what makes them carryable, `test/components.test.mjs` holds it, and nothing
// here is allowed to change it. So the fetching lives at the app layer, and what this file hands the
// app is the same `Catalog` the components were already given — see `adapt` below.
//
// Two properties are the point of the file:
//
//   1. **An unreachable service is never an empty catalogue.** `loadCatalogue` returns either a
//      catalogue or a failure, never a catalogue that happens to be empty because nothing answered.
//      "Zero connectors" and "the service is not there" are different facts and the reader is owed
//      the difference.
//   2. **Nothing is filled in.** The served catalogue is a *thinner* document than the one the
//      carried components were written against: it publishes each operation's declared metadata and
//      no request shape, no credentials, no hosts and no Flux source. Those fields are mapped to the
//      contract's own empty values and the reader is told, once, that an empty field here means
//      unpublished by this source rather than absent from the connector.

import type { Catalog, Issue, Operation, Provider, Service, Status } from './catalog.mts'

// ---------------------------------------------------------------------------------------------
// The endpoints.
//
// Origin-relative, because the console is served by the same host that serves the API. They are
// exported rather than inlined for one reason: a failure has to be able to name the exact endpoint
// that did not answer, and a message naming an endpoint the code does not call would be worse than
// no message at all.
// ---------------------------------------------------------------------------------------------

/** Where the served catalogue lists its connectors. */
export const CONNECTORS_ENDPOINT = '/api/catalogue/connectors'

/** The reserved service name, which every published address elides. Vocabulary, not data. */
const RESERVED_SERVICE = 'default'

/** Where one connector's operations are served, by connector id. */
export function operationsEndpoint(connector: string): string {
  return `${CONNECTORS_ENDPOINT}/${encodeURIComponent(connector)}/operations`
}

// ---------------------------------------------------------------------------------------------
// The served contract.
//
// Typed as the service publishes it and not as this console wishes it were: `risk` and
// `idempotency` are the vocabularies below, but they are typed as `string` where they enter the
// carried contract, which is also `string` — a value from a newer service than this bundle must
// render as itself rather than fail a type guard.
// ---------------------------------------------------------------------------------------------

/** One entry of `GET /api/catalogue/connectors`. */
export interface ServedConnector {
  id: string
  operation_count: number
}

/**
 * One entry of `GET /api/catalogue/connectors/{id}/operations`.
 *
 * `effects_derived` and `admitted` are the two fields with no home in the carried catalogue
 * contract, and both carry a distinction that is destroyed by dropping it:
 *
 *   - `effects_derived: true` means the service **inferred** the effects rather than reading them
 *     from a declaration. An inference shown as a declaration is a claim nobody made.
 *   - `admitted` is three-valued. `null` is not `false`: it means no principal was resolved, so the
 *     catalogue is saying what exists rather than what the reader may call. There is no sign-in yet,
 *     so `null` is what every operation carries today.
 */
export interface ServedOperation {
  id: string
  service: string
  description: string
  risk: string
  idempotency: string
  effects: string[]
  effects_derived: boolean
  admitted: boolean | null
}

// ---------------------------------------------------------------------------------------------
// Failure.
//
// Three kinds, because an operator responds to them differently and the repository's own convention
// is that "rejected" and "unreachable" are not the same event:
//
//   unreachable  nothing answered — no service, wrong port, no network
//   refused      something answered with a status that is not a catalogue
//   unreadable   something answered 2xx with a body this console could not read as the contract
//
// Every one of them carries the endpoint, and every message names it.
// ---------------------------------------------------------------------------------------------

/** How a catalogue load failed. */
export type FailureKind = 'unreachable' | 'refused' | 'unreadable'

/** One failed catalogue load, in enough detail to act on. */
export interface CatalogueFailure {
  kind: FailureKind
  /** The endpoint that did not produce a catalogue. Always exactly one this console actually called. */
  endpoint: string
  /** The HTTP status, or `null` when nothing answered at all. */
  status: number | null
  /** What the transport or the body said, verbatim — never a sentence this console made up. */
  detail: string
}

/** The one-line heading a page shows for this failure. */
export function failureHeadline(failure: CatalogueFailure): string {
  switch (failure.kind) {
    case 'unreachable':
      return 'The flux-exchange service could not be reached'
    case 'refused':
      return 'The flux-exchange service refused the catalogue request'
    case 'unreadable':
      return 'The flux-exchange service answered with something this console could not read'
  }
}

/**
 * The sentence a page shows for this failure.
 *
 * It names the endpoint in every branch. That is the whole contract of this function: a reader
 * looking at a console with nothing on it has to be able to tell, without opening a devtools panel,
 * that a request failed and which one.
 */
export function failureMessage(failure: CatalogueFailure): string {
  const detail = failure.detail ? ` ${failure.detail}` : ''
  switch (failure.kind) {
    case 'unreachable':
      return `${failure.endpoint} could not be reached.${detail} Nothing was read, so this page is empty because the request failed — not because the catalogue is.`
    case 'refused':
      return `${failure.endpoint} answered ${failure.status}.${detail} No catalogue was read, so this page is not showing an empty catalogue — it is showing none at all.`
    case 'unreadable':
      return `${failure.endpoint} answered ${failure.status} with a body this console could not read as a catalogue.${detail} Nothing below was fetched.`
  }
}

// ---------------------------------------------------------------------------------------------
// The load.
// ---------------------------------------------------------------------------------------------

/**
 * What the console has, at any moment, from the service.
 *
 * A discriminated union rather than a catalogue plus an optional error, so that "empty" and
 * "failed" cannot be confused by construction: a failed load has no `catalog` field to read, and a
 * caller that forgets to check the status does not silently get an empty document.
 */
export type CatalogueState =
  | { status: 'loading' }
  | {
      status: 'ready'
      catalog: Catalog
      /**
       * The served operations by id, kept beside the adapted catalogue.
       *
       * `effects` and `admitted` have no field in the carried contract and this console will not add
       * one — `catalog.mts` is shared with flux-connectors. Dropping them instead would throw away
       * exactly the metadata a grant is written over, so they are kept here and rendered by the app
       * layer (`OperationFacts.mts`).
       */
      served: Record<string, ServedOperation>
    }
  | { status: 'failed'; failure: CatalogueFailure }

/** How to reach the service. Both have honest defaults; tests supply their own `fetch`. */
export interface LoadOptions {
  /** The transport. Defaults to the platform's. */
  fetch?: typeof globalThis.fetch
  /** A prefix for every endpoint, for a console served from somewhere other than the API's origin. */
  origin?: string
}

/** A body that was read, or the failure that reading it was. */
type Read = { ok: true; body: unknown } | { ok: false; failure: CatalogueFailure }

/** What went wrong, in the transport's own words — never a sentence invented here. */
function describe(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}

/**
 * What a refusal body says, preferring its own words to a dump of it.
 *
 * The contract says an unknown connector answers 404 with a JSON body naming the id, but does not
 * name that body's fields — so the common spellings are tried and anything else is shown verbatim.
 * Truncated, because a refusal is a sentence on a page and not a log line.
 */
function refusalDetail(body: unknown): string {
  if (typeof body === 'string') return body.slice(0, 200)
  if (body && typeof body === 'object') {
    for (const key of ['error', 'message', 'detail']) {
      const value = (body as Record<string, unknown>)[key]
      if (typeof value === 'string') return value.slice(0, 200)
    }
    return JSON.stringify(body).slice(0, 200)
  }
  return ''
}

/** One endpoint, read as JSON, with every way that can go wrong named as itself. */
async function read(endpoint: string, options: LoadOptions): Promise<Read> {
  const transport = options.fetch ?? globalThis.fetch
  const url = `${options.origin ?? ''}${endpoint}`

  let response: Response
  try {
    response = await transport(url)
  } catch (error) {
    return {
      ok: false,
      failure: { kind: 'unreachable', endpoint, status: null, detail: describe(error) },
    }
  }

  // The body is read before the status is judged, because a refusal's body is where the reason is —
  // and a refusal with an unreadable body is still a refusal, never an unreadable success.
  let body: unknown
  let unreadable: string | null = null
  try {
    body = await response.json()
  } catch (error) {
    unreadable = describe(error)
  }

  if (!response.ok) {
    return {
      ok: false,
      failure: {
        kind: 'refused',
        endpoint,
        status: response.status,
        detail: unreadable === null ? refusalDetail(body) : '',
      },
    }
  }

  if (unreadable !== null) {
    return {
      ok: false,
      failure: { kind: 'unreadable', endpoint, status: response.status, detail: unreadable },
    }
  }

  return { ok: true, body }
}

/** Whether a value is a JSON object, which is the only shape either route may answer with. */
function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * The connectors in a `GET /connectors` body, or the reason it is not one.
 *
 * A shape check rather than a schema: the fields this console reads have to be there and be the
 * right kind, and a field it does not read is none of its business. An entry that fails the check
 * fails the whole load, because a catalogue silently missing one connector is a catalogue that lies
 * about what exists.
 */
function readConnectors(body: unknown): ServedConnector[] | string {
  if (!isObject(body) || !Array.isArray(body.connectors)) return 'no `connectors` array in the body'
  const connectors: ServedConnector[] = []
  for (const entry of body.connectors) {
    if (!isObject(entry) || typeof entry.id !== 'string') return 'a connector entry has no `id`'
    connectors.push({
      id: entry.id,
      operation_count: typeof entry.operation_count === 'number' ? entry.operation_count : 0,
    })
  }
  return connectors
}

/** The operations in a `GET /connectors/{id}/operations` body, or the reason it is not one. */
function readOperations(body: unknown): ServedOperation[] | string {
  if (!isObject(body) || !Array.isArray(body.operations)) return 'no `operations` array in the body'
  const operations: ServedOperation[] = []
  for (const entry of body.operations) {
    if (!isObject(entry) || typeof entry.id !== 'string') return 'an operation entry has no `id`'
    operations.push({
      id: entry.id,
      service: typeof entry.service === 'string' ? entry.service : RESERVED_SERVICE,
      description: typeof entry.description === 'string' ? entry.description : '',
      risk: typeof entry.risk === 'string' ? entry.risk : '',
      idempotency: typeof entry.idempotency === 'string' ? entry.idempotency : '',
      effects: Array.isArray(entry.effects) ? entry.effects.filter((e) => typeof e === 'string') : [],
      effects_derived: entry.effects_derived === true,
      // `!== true && !== false` rather than `?? null`, so an absent field and an explicit null both
      // land on the third state instead of one of them becoming a refusal.
      admitted: entry.admitted === true ? true : entry.admitted === false ? false : null,
    })
  }
  return operations
}

/** One connector as it was served, with the operations its own endpoint answered with. */
interface ServedPair {
  connector: ServedConnector
  operations: ServedOperation[]
}

/**
 * The whole catalogue, or the first reason there is not one.
 *
 * Every connector's operations are fetched, and any single failure fails the load. A catalogue
 * assembled from the connectors that happened to answer would be rendered as complete, which is the
 * same class of lie as rendering an outage as an empty catalogue.
 */
export async function loadCatalogue(options: LoadOptions = {}): Promise<CatalogueState> {
  const listed = await read(CONNECTORS_ENDPOINT, options)
  if (!listed.ok) return { status: 'failed', failure: listed.failure }

  const connectors = readConnectors(listed.body)
  if (typeof connectors === 'string') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint: CONNECTORS_ENDPOINT,
        status: 200,
        detail: connectors,
      },
    }
  }

  // In parallel — the catalogue is one screen and a connector per round trip in series is a page that
  // fills in visibly. Reported in list order regardless of which answered first, so which failure a
  // reader is shown does not depend on the network's timing.
  const answers = await Promise.all(
    connectors.map(async (connector): Promise<ServedPair | CatalogueFailure> => {
      const endpoint = operationsEndpoint(connector.id)
      const answered = await read(endpoint, options)
      if (!answered.ok) return answered.failure

      const operations = readOperations(answered.body)
      if (typeof operations === 'string') {
        return { kind: 'unreadable', endpoint, status: 200, detail: operations }
      }
      return { connector, operations }
    })
  )

  const pairs: ServedPair[] = []
  const served: Record<string, ServedOperation> = {}

  for (const answer of answers) {
    if (!('connector' in answer)) return { status: 'failed', failure: answer }
    for (const operation of answer.operations) served[operation.id] = operation
    pairs.push(answer)
  }

  return { status: 'ready', catalog: adapt(pairs), served }
}

// ---------------------------------------------------------------------------------------------
// The adapter: the served document, in the shape the carried components read.
//
// Every unpublished field becomes the contract's own empty value — `''`, `null`, `[]` — and never a
// plausible-looking stand-in. That is the whole discipline here: a base URL of `https://example.com`
// would render as a fact, and an empty one renders as a blank the banner below explains.
// ---------------------------------------------------------------------------------------------

/**
 * What this source publishes, and what it does not — stated once, over the whole catalogue.
 *
 * This is the console's own statement about the document it read, not a condition the service
 * reported, and the `CONSOLE-` code is there so it can never be mistaken for one of the catalogue's
 * own. It is carried on `status.issues` at catalogue scope because that is the channel the carried
 * components already render a set-wide condition through: a banner above the explorer, and a neutral
 * block on each operation page. Saying it in a hand-rolled element instead would put it on one page
 * and not the other.
 */
const SOURCE_SCOPE: Issue = {
  code: 'CONSOLE-SOURCE-SCOPE',
  scope: 'catalog',
  summary:
    'This catalogue was read from the flux-exchange service, which publishes what each operation is ' +
    'and what it costs — its connector, service, description, risk, idempotency and effects — and ' +
    'nothing more. A request method and path, parameters, credentials, hosts and Flux source are ' +
    'not published by this source, so where this page shows one of them empty it means unpublished ' +
    'here, not absent from the connector.',
  params: [],
}

/**
 * Why nothing in this catalogue is admitted or refused.
 *
 * Attached only when every served operation carries `admitted: null`, so it is read off the document
 * rather than asserted: the day a principal resolves, some operation answers `true` or `false` and
 * this condition disappears on its own with no edit here.
 */
const NO_PRINCIPAL: Issue = {
  code: 'CONSOLE-NO-PRINCIPAL',
  scope: 'catalog',
  summary:
    'No principal is resolved — this console has no sign-in yet — so the service answered `admitted: ' +
    'null` for every operation. What follows is therefore what exists, not what you may call. It is ' +
    'not a statement that anything here is closed to you.',
  params: [],
}

/**
 * One operation, in the carried contract's shape.
 *
 * `works` is `false` and that is not pessimism: nothing in flux-exchange can be invoked yet, and the
 * catalogue-wide condition above is on every operation, so an operation claiming to work while
 * carrying a listed reason it does not would be self-contradictory. `ownIssues` sees none of these —
 * they are catalogue-scoped — so no operation is badged with a defect it does not own.
 */
function adaptOperation(connector: string, served: ServedOperation, status: Status): Operation {
  return {
    id: served.id,
    provider: connector,
    service: served.service,
    description: served.description,
    risk: served.risk,
    idempotency: served.idempotency,
    // Everything from here down is unpublished by this source. Empty, never invented.
    method: '',
    path: '',
    parameters: [],
    body_schema: null,
    response_schema: null,
    credentials: [],
    hosts: [],
    flux: '',
    status,
  }
}

/**
 * The services one connector publishes, reconstructed from its operations.
 *
 * This is real data rather than a placeholder: every served operation names its service, so the set
 * of services and the count in each are exactly what the service said. Everything a service
 * otherwise carries — its own base URL, hosts, version and address — is unpublished and stays empty.
 */
function adaptServices(operations: ServedOperation[]): Service[] {
  const counts = new Map<string, number>()
  for (const operation of operations) {
    counts.set(operation.service, (counts.get(operation.service) ?? 0) + 1)
  }
  return [...counts].map(([name, operation_count]) => ({
    name,
    description: '',
    base_url: '',
    hosts: [],
    api_version: null,
    gid: null,
    operation_count,
  }))
}

/** One connector, in the carried contract's shape. */
function adaptProvider(pair: ServedPair, status: Status): Provider {
  return {
    id: pair.connector.id,
    authority: null,
    // The id is the only name this source publishes for a connector, so it is the only name shown.
    // A vendor's proper name would have to be invented, and an invented one would read as published.
    vendor: pair.connector.id,
    description: '',
    base_url: '',
    api_version: null,
    hosts: [],
    services: adaptServices(pair.operations),
    auth: { schemes: [], credentials: [], default: [] },
    operation_count: pair.connector.operation_count,
    operations: pair.operations.map((served) => adaptOperation(pair.connector.id, served, status)),
    events: [],
    channels: [],
  }
}

/**
 * The catalogue the components render, with the catalogue-wide conditions applied.
 *
 * The conditions are decided over the whole document and then shared by every operation, which is
 * how `catalogIssues` finds them: it collects catalogue-scoped issues off operations and states each
 * once. One `Status` object is shared by every operation rather than copied — it is the same
 * condition, and `distinct` would collapse the copies anyway.
 */
function adapt(pairs: ServedPair[]): Catalog {
  const operations = pairs.flatMap((pair) => pair.operations)
  const issues: Issue[] = [SOURCE_SCOPE]
  if (operations.length > 0 && operations.every((operation) => operation.admitted === null)) {
    issues.push(NO_PRINCIPAL)
  }
  const status: Status = { works: false, issues, notes: [] }

  return {
    // Neither is published by the served document, and nothing renders either — the console's footer
    // names the endpoint it read instead, which is the fact a reader can act on.
    schema_version: 0,
    generator: CONNECTORS_ENDPOINT,
    providers: pairs.map((pair) => adaptProvider(pair, status)),
    // The service publishes no Flux core catalogue. `null` is the contract's own way to say so, and
    // every component that renders core checks for it.
    core: null,
  }
}
