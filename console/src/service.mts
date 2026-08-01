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
// The one thing this file reads that is not the served document: whether this build's service
// serves the invocation route. It is a statement about the API surface — no tenant, no principal,
// no session — and `adapt` below is where the whole argument for reading it here lives.
import { SURFACES } from './surfaces.mts'

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

/** Where this host says who it resolved the caller to be. */
export const SESSION_ENDPOINT = '/api/session'

/** Where this tenant's connections are listed. */
export const CONNECTIONS_ENDPOINT = '/api/connections'

/**
 * Where a human begins signing in.
 *
 * **Followed, never fetched.** It answers `303` to the identity provider's authorization endpoint,
 * so a `fetch` would chase the redirect inside the page — landing the provider's login document in
 * a response body this console cannot do anything with — while the browser's address bar never
 * moves. The console renders an ordinary anchor and lets the browser navigate, which is what a
 * `303` is for. `routes::signin` in `crates/exchange-server` carries the other half of this.
 */
export const SIGNIN_ENDPOINT = '/api/signin'

/** The reserved service name, which every published address elides. Vocabulary, not data. */
const RESERVED_SERVICE = 'default'

/** Where one connector's operations are served, by connector id. */
export function operationsEndpoint(connector: string): string {
  return `${CONNECTORS_ENDPOINT}/${encodeURIComponent(connector)}/operations`
}

/**
 * Where one connector's **declared** credentials are served, by connector id.
 *
 * On the catalogue and not under `/api/connections` for the reason that decides everything about
 * this read: what a connector declares is a vendor fact, identical for every caller and every
 * deployment of a version, while whether *this tenant holds* one is per-principal state. The first
 * is anonymous; the second is the connections surface and requires a principal. A connect form
 * needs only the first, so it asks only for the first.
 */
export function declarationEndpoint(connector: string): string {
  return `${CONNECTORS_ENDPOINT}/${encodeURIComponent(connector)}/credentials`
}

/**
 * Where one connector's connection is created, read and destroyed.
 *
 * `{connector}` is a catalogue key and never an address: it selects *what* is being connected, and
 * the tenant — the only part of a credential address a caller could want to move — comes from the
 * resolved principal. `routes::connections` carries the other half of this.
 */
export function connectionEndpoint(connector: string): string {
  return `${CONNECTIONS_ENDPOINT}/${encodeURIComponent(connector)}`
}

/**
 * Where one credential of one connection is **replaced**.
 *
 * X-39's route, and the reason this console never offers a delete. Replacing a leaked secret by
 * `DELETE` then `POST` has a window in it where the tenant has no connection at all and everything
 * relying on it fails; a rotation is a single atomic write with no such moment. Nothing here calls
 * it yet — the console names it, so an operator meeting the `409` is sent to the operation that
 * works rather than to the one that breaks their tenant for a few seconds.
 */
export function credentialEndpoint(connector: string, credential: string): string {
  return `${connectionEndpoint(connector)}/credentials/${encodeURIComponent(credential)}`
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
 *   - `admitted` is three-valued. `null` is not `false`: it means the question was never asked, so
 *     the catalogue is saying what exists rather than what the reader may call. `null` is what every
 *     operation carries today, and **not because nobody can sign in** — that sentence was true when
 *     this file was written and stopped being true when OIDC landed, and X-57 made a locally
 *     provisioned host say so as well. It is `null` because nothing in this build decides what a
 *     principal may run: there is no grant model, so the answer would be the same for a signed-in
 *     reader. X-13 is the story that gives this field its other two values.
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

/** One failed read of one endpoint, in enough detail to act on. */
export interface ServiceFailure {
  kind: FailureKind
  /** The endpoint that did not answer. Always exactly one this console actually called. */
  endpoint: string
  /** The HTTP status, or `null` when nothing answered at all. */
  status: number | null
  /** What the transport or the body said, verbatim — never a sentence this console made up. */
  detail: string
}

/**
 * The catalogue's spelling of [`ServiceFailure`].
 *
 * The name predates the session and connection reads, which fail in exactly the same three ways
 * against exactly the same transport. It is kept because `CatalogueFailure.mts` is the view that
 * renders one, and renaming a view to follow a type alias would buy nothing.
 */
export type CatalogueFailure = ServiceFailure

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

/**
 * A refusal this service worded for itself.
 *
 * Distinct from [`ServiceFailure`], and the difference is who is speaking. A `ServiceFailure` is
 * this console's account of a read that did not happen — it summarises, and [`refusalDetail`]
 * truncates, because there a sentence on a page is better than a dump of a body. A refusal of a
 * *write* is the opposite case: the service has answered in sentences several stories were spent
 * arguing over (X-18, X-20, X-29), and those sentences are what an operator acts on.
 *
 * So `error` is carried **whole and verbatim**. The `409` on an already-connected connector is 206
 * characters long; put through the summariser it would arrive on the page missing its last clause,
 * which is this console re-wording a refusal it was told not to re-word.
 */
export interface ServiceRefusal {
  /** The endpoint that refused. */
  endpoint: string
  /** The HTTP status it refused with. */
  status: number
  /** The service's own `error` sentence, untruncated and unedited. */
  error: string
}

/** A body that was read, or the failure that reading it was. */
type Read =
  | { ok: true; body: unknown }
  // The body is carried on the failure too, because a refusal's body is where the service says what
  // would have worked — the declared credential names on a `422`, the address on a `409` — and a
  // caller that only ever saw the summarised sentence could not read either.
  | { ok: false; failure: ServiceFailure; body: unknown }

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

/**
 * What a body says its `error` is, verbatim, or `null` when it says nothing this can read.
 *
 * Deliberately reads only `error` and deliberately does not truncate: this is the sentence the
 * service composed for a caller, and the two liberties [`refusalDetail`] takes — guessing among
 * spellings, cutting at 200 characters — are both wrong for a refusal that is itself the answer.
 */
function serviceError(body: unknown): string | null {
  if (!isObject(body)) return null
  return typeof body.error === 'string' ? body.error : null
}

/**
 * One endpoint, read as JSON, with every way that can go wrong named as itself.
 *
 * `init` is how a write goes through the same three-kind funnel as a read. It is not a general
 * escape hatch: everything that reaches the network in this console does so here, so there is one
 * place that decides what "unreachable", "refused" and "unreadable" mean, and one place a reviewer
 * checks that no caller put a credential value somewhere other than the body.
 */
async function read(endpoint: string, options: LoadOptions, init?: RequestInit): Promise<Read> {
  const transport = options.fetch ?? globalThis.fetch
  const url = `${options.origin ?? ''}${endpoint}`

  let response: Response
  try {
    response = await transport(url, init)
  } catch (error) {
    return {
      ok: false,
      body: undefined,
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
      body,
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
      body,
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
// The session.
//
// Read, never invented. The console does not decide whether anyone is signed in — it asks
// `/api/session` and renders the answer, including the answer "I could not tell you".
// ---------------------------------------------------------------------------------------------

/**
 * A caller this host resolved, exactly as `GET /api/session` publishes one.
 *
 * `tenant` is the field that matters most on this page and it is not decoration: every credential
 * address the console can show is derived from it, and it comes from the resolved principal rather
 * than from anything a caller controls. See `exchange_host::Principal`.
 */
export interface Principal {
  /** `user`, `agent` or `service` — what kind of caller this is. */
  kind: string
  /** Its stable identifier within the tenant. */
  id: string
  /** The tenant it belongs to. */
  tenant: string
}

/**
 * What the console knows about who is signed in.
 *
 * The same three states as [`CatalogueState`] and for the same reason, with one distinction worth
 * being explicit about: **`ready` with `principal: null` is an answer, and `failed` is not.** A
 * `401` means this host looked and resolved nobody — a fact, and the one that should offer a
 * sign-in link. A read that never completed means the console does not know, and rendering that as
 * "not signed in" would tell a reader they had been signed out by an outage.
 */
export type SessionState =
  | { status: 'loading' }
  | { status: 'ready'; principal: Principal | null }
  | { status: 'failed'; failure: ServiceFailure }

/** A principal in a `GET /api/session` body, or `null` when the body carries none this can read. */
function readPrincipal(body: unknown): Principal | null {
  if (!isObject(body) || !isObject(body.principal)) return null
  const principal = body.principal
  if (typeof principal.id !== 'string' || typeof principal.tenant !== 'string') return null
  return {
    kind: typeof principal.kind === 'string' ? principal.kind : '',
    id: principal.id,
    tenant: principal.tenant,
  }
}

/**
 * Who this host says the caller is.
 *
 * `401` is not a failure here. It is this host answering the question — nobody presented a
 * credential it could resolve, or it has no identity provider bound to resolve one with — and
 * either way there is no principal, which is precisely what the header needs to know in order to
 * offer a way in. Every other refusal *is* a failure: a `503` from an unreachable identity provider
 * and a signed-out browser are different events an operator responds to differently, and this
 * console's whole discipline is not collapsing that pair.
 */
export async function loadSession(options: LoadOptions = {}): Promise<SessionState> {
  const answered = await read(SESSION_ENDPOINT, options)

  if (!answered.ok) {
    if (answered.failure.status === 401) return { status: 'ready', principal: null }
    return { status: 'failed', failure: answered.failure }
  }

  // A `200` whose body carries no principal this console can read is not a session. Rendering it as
  // signed-out would be the same lie in the other direction, so it is `unreadable`.
  const principal = readPrincipal(answered.body)
  if (!principal) {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint: SESSION_ENDPOINT,
        status: 200,
        detail: 'no `principal` with an `id` and a `tenant` in the body',
      },
    }
  }

  return { status: 'ready', principal }
}

/**
 * End the caller's session and report whether the service agreed.
 *
 * `DELETE`, so this one is a fetch and not a link — there is no navigation that means "close my
 * session", and the route answers `204` with a cleared cookie rather than a document. The caller
 * reloads on success; a failure is returned rather than swallowed, because a sign-out that silently
 * did nothing leaves a reader believing they have left.
 */
export async function signOut(options: LoadOptions = {}): Promise<ServiceFailure | null> {
  const transport = options.fetch ?? globalThis.fetch
  const url = `${options.origin ?? ''}${SESSION_ENDPOINT}`

  try {
    const response = await transport(url, { method: 'DELETE' })
    if (response.ok) return null
    return {
      kind: 'refused',
      endpoint: SESSION_ENDPOINT,
      status: response.status,
      detail: '',
    }
  } catch (error) {
    return {
      kind: 'unreachable',
      endpoint: SESSION_ENDPOINT,
      status: null,
      detail: describe(error),
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Connections: the first of the console's two jobs, and the only one that works today.
//
// A connection is **addresses and never values**. `GET /api/connections` answers with the address
// each declared credential would live at and whether anything is held there; the value never leaves
// the store, which is the north star of this whole service. Nothing here reconstructs one, and
// nothing here should ever start.
// ---------------------------------------------------------------------------------------------

/** One declared credential of a connection: where it lives, and whether anything is there. */
export interface HeldCredential {
  /** The flat-namespace name the catalogue publishes, e.g. `zendesk.api_token`. */
  name: string
  /** `tenants/<tenant>/<authority>/<credential>` — an address, never a value. */
  address: string
  /** Whether this tenant has stored something at that address. */
  held: boolean
}

/** One connector this tenant has connected. */
export interface Connection {
  connector: string
  vendor: string
  /** The authority its credentials are addressed under, or `null` when the connector declares none. */
  authority: string | null
  credentials: HeldCredential[]
}

/**
 * What this tenant has wired up.
 *
 * Three states, exactly as the catalogue has. **There is no fourth state for "signed out"** and that
 * is deliberate: the session is read first and the console already knows, so a caller with no
 * principal is never asked this question at all. A `401` arriving here therefore means the session
 * ended between the two reads, which is a real failure and is reported as one.
 */
export type ConnectionsState =
  | { status: 'loading' }
  | { status: 'ready'; connections: Connection[] }
  | { status: 'failed'; failure: ServiceFailure }

/** The credentials in one served connection, or the reason the entry is not one. */
function readCredentials(entry: Record<string, unknown>): HeldCredential[] | string {
  if (!Array.isArray(entry.credentials)) return 'a connection entry has no `credentials` array'
  const credentials: HeldCredential[] = []
  for (const credential of entry.credentials) {
    if (!isObject(credential) || typeof credential.name !== 'string') {
      return 'a credential entry has no `name`'
    }
    credentials.push({
      name: credential.name,
      address: typeof credential.address === 'string' ? credential.address : '',
      held: credential.held === true,
    })
  }
  return credentials
}

/**
 * One connection, or the reason the value is not one.
 *
 * Shared by the listing and by what a create answers with, because they are the same view: `POST`
 * answers `201` with exactly the object `GET` lists. That is worth having in one function rather
 * than two — the property this console is holding is that neither of them can carry a value, and a
 * second reader is a second place for one to appear.
 */
function readConnection(entry: unknown): Connection | string {
  if (!isObject(entry) || typeof entry.connector !== 'string') {
    return 'a connection entry has no `connector`'
  }
  const credentials = readCredentials(entry)
  if (typeof credentials === 'string') return credentials
  return {
    connector: entry.connector,
    vendor: typeof entry.vendor === 'string' ? entry.vendor : entry.connector,
    authority: typeof entry.authority === 'string' ? entry.authority : null,
    credentials,
  }
}

/** The connections in a `GET /api/connections` body, or the reason it is not one. */
function readConnections(body: unknown): Connection[] | string {
  if (!isObject(body) || !Array.isArray(body.connections)) {
    return 'no `connections` array in the body'
  }
  const connections: Connection[] = []
  for (const entry of body.connections) {
    const connection = readConnection(entry)
    if (typeof connection === 'string') return connection
    connections.push(connection)
  }
  return connections
}

/**
 * This tenant's connections, or the reason there are none to show.
 *
 * A single entry that fails the shape check fails the whole load, for the reason `readConnectors`
 * gives: a listing silently missing one connection is a listing that lies about what is wired up,
 * and this is the page an operator would use to check whether a leaked credential is still there.
 */
export async function loadConnections(options: LoadOptions = {}): Promise<ConnectionsState> {
  const answered = await read(CONNECTIONS_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }

  const connections = readConnections(answered.body)
  if (typeof connections === 'string') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint: CONNECTIONS_ENDPOINT,
        status: 200,
        detail: connections,
      },
    }
  }

  return { status: 'ready', connections }
}

// ---------------------------------------------------------------------------------------------
// Connecting: the write, and the one question that has to be asked before it.
//
// A connect form needs to know **what a connector declares**, and this console must not be the
// thing that knows. A list kept here goes stale the day a connector gains a credential, and the
// operator who then cannot enter it has no way to tell that the console is the one that is wrong.
// So the declaration is read from the service, every time, and the inputs are whatever it said.
//
// Since X-46 that question is a `GET` on the catalogue rather than a `POST` the service refuses:
// **rendering a form costs no write.** `loadDeclaration` carries the history.
// ---------------------------------------------------------------------------------------------

/** What a connector declares — the names a connection may carry values for. */
export interface Declaration {
  /** The connector these belong to, echoed so the value stands on its own. */
  connector: string
  /**
   * The flat-namespace names the catalogue publishes, in the order the service published them.
   *
   * Unreordered and unfiltered on purpose: the order is the connector's own, and a console that
   * sorted them would be presenting its own opinion of a declaration as the declaration.
   */
  credentials: string[]
}

/**
 * What this console knows about one connector's declaration.
 *
 * Four states rather than the three the reads above use, and the fourth earns its place. `refused`
 * is the service answering the question — the catalogue does not carry this connector at all — in a
 * sentence the operator acts on, and folding it into `failed` would put that sentence through the
 * summariser that truncates. `failed` stays what it is everywhere else in this file: nothing was
 * read, and the console cannot tell you anything.
 *
 * A connector that declares **no** credential is none of those: it is `ready` with an empty
 * `credentials`, because "nothing to store for this one" is an answer and not an absence of one.
 * Before X-46 it arrived as a `refused`, since the workaround could only learn it from a refusal.
 */
export type DeclarationState =
  | { status: 'loading' }
  | { status: 'ready'; declaration: Declaration }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/**
 * The declared names in a catalogue declaration, or `null` when it carries none this can read.
 *
 * All-or-nothing, for the reason `readConnectors` gives: one entry that fails the shape check fails
 * the whole read. A declaration silently missing a credential is a form silently missing an input,
 * and an operator who fills in the boxes they were given has no way to tell that a box is absent.
 */
function readDeclared(body: unknown): string[] | null {
  if (!isObject(body) || !Array.isArray(body.credentials)) return null
  const declared = body.credentials
    .map((credential) =>
      isObject(credential) && typeof credential.name === 'string' ? credential.name : null
    )
    .filter((name): name is string => name !== null)
  return declared.length === body.credentials.length ? declared : null
}

/**
 * What a connector declares, read from the catalogue.
 *
 * ```text
 * GET /api/catalogue/connectors/slack/credentials
 * 200 {"connector":"slack","authority":"com.slack.api","credentials":[{"name":"slack.bot_token",…}]}
 * ```
 *
 * **A read, and only a read.** X-44 had no route to ask this of: the served catalogue published an
 * operation's metadata and no credentials, so the only place the service stated a declaration was
 * the `422` it gives a `POST /api/connections/{connector}` carrying `{"credentials": {}}`. That
 * workaround wrote nothing and carried no value — both were verified, and the `422` still says
 * exactly what it said — but it made a capability fact something this console inferred from an
 * error body, so rewording the refusal would have rendered every connector unreadable. X-46 made it
 * a field, one layer up from where X-43 made the same call for sign-in availability, and this
 * function is the whole of the console's half of it.
 *
 * Three properties survive the change, and they are the ones worth keeping. The names still come
 * from `connector_catalog`, so a connector that gains a credential gains an input with nobody
 * editing this console. Nothing is sent, so there is no value to leak and nothing an audit of the
 * traffic has to be told to disregard. And the endpoint is anonymous, so a form renders before a
 * principal resolves rather than after.
 */
export async function loadDeclaration(
  connector: string,
  options: LoadOptions = {}
): Promise<DeclarationState> {
  const endpoint = declarationEndpoint(connector)
  const answered = await read(endpoint, options)

  if (answered.ok) {
    const declared = readDeclared(answered.body)
    if (declared) return { status: 'ready', declaration: { connector, credentials: declared } }

    // A `200` whose body this console cannot read is not a connector that declares nothing. An
    // empty `credentials` array is that, and it arrives through the branch above as an empty
    // declaration — which `Connect.mts` renders as "declares no credential", the true sentence.
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint,
        status: 200,
        detail: 'the catalogue answered with no `credentials` array this console could read',
      },
    }
  }

  const error = serviceError(answered.body)
  if (error !== null && answered.failure.status !== null) {
    // The catalogue naming the id it does not carry — a `404` on a connector this console listed
    // is the service and this bundle disagreeing about what exists, and that sentence is the one
    // an operator acts on. Kept whole rather than summarised, for the reason `serviceError` gives.
    return {
      status: 'refused',
      refusal: { endpoint, status: answered.failure.status, error },
    }
  }

  return { status: 'failed', failure: answered.failure }
}

/**
 * What became of a connect.
 *
 * Three outcomes for the same reason every read here has three states: a refusal and an outage are
 * different events, and a caller must not be able to confuse either with success. There is no
 * `loading` — a write is awaited by whoever started it, and the busy flag belongs to the view.
 */
export type ConnectOutcome =
  | { status: 'connected'; connection: Connection }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/**
 * Connect a connector, with the values an operator typed.
 *
 * **The values go in the body and nowhere else.** Not in the path, not in a query string — a value
 * in a query string is a value in an access log, and in a browser's history and in every referrer
 * header the page then sends. This function is the only place in the console a credential value is
 * ever handled, it holds none after it returns, and what it returns is the service's own view of
 * the connection: addresses and `held`, never a value.
 *
 * A refusal is carried whole. `409` — the connector is already connected — is not special-cased
 * here; it is a refusal like any other, and the *view* is what knows to point an operator at
 * rotation. This function will not delete anything, and there is deliberately no sibling that
 * would: `DELETE` then `POST` is the window X-39 exists to remove.
 */
export async function connect(
  connector: string,
  values: Record<string, string>,
  options: LoadOptions = {}
): Promise<ConnectOutcome> {
  const endpoint = connectionEndpoint(connector)
  const answered = await read(endpoint, options, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ credentials: values }),
  })

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return { status: 'refused', refusal: { endpoint, status: answered.failure.status, error } }
    }
    return { status: 'failed', failure: answered.failure }
  }

  const connection = readConnection(answered.body)
  if (typeof connection === 'string') {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint, status: 201, detail: connection },
    }
  }

  return { status: 'connected', connection }
}

// ---------------------------------------------------------------------------------------------
// Grants: what this tenant may run — and the one read in this file whose answer is a *policy*.
//
// X-13 gated `invoke` on a grant and X-62 gave the grant a surface. Three routes, and the third is
// the point rather than a convenience: `POST /api/grants/preview` answers **which operations a
// proposed grant would admit**, before it exists, from the same `OperationFacts::of` projection the
// gate itself decides on. A grant nobody can evaluate before saving is a grant somebody sets too
// wide.
//
// **Nothing here composes an operation id.** The selector on the wire is three axes — `max_risk`,
// `effects_within`, `idempotency` — and the route refuses `allow_ids`, `deny_ids` and four more
// spellings with a `422`. `selectorBody` below is the whole of what this console can send, and it
// has nowhere to put a name. `test/grants.test.mjs::the_console_never_sends_an_operation_id` walks
// the bodies rather than the statuses, because not tripping a refusal is a weaker claim than being
// unable to.
//
// **And nothing here decides admission.** `admits` is read off the answer, never computed. See
// `granting.mts`, which holds the vocabulary and the whole-set composition and no rule.
// ---------------------------------------------------------------------------------------------

/**
 * Where this tenant's grants are read and replaced.
 *
 * A collection path with no parameter, and there is nowhere a tenant could be spelled into it: the
 * tenant is read from the principal this host resolved. `routes::grants` carries the other half.
 */
export const GRANTS_ENDPOINT = '/api/grants'

/**
 * Where a proposed grant is evaluated without being stored.
 *
 * A path of its own rather than a flag on the write, so evaluating a policy is not one typo away
 * from applying it — the service makes that structural and this console spells it the same way.
 */
export const GRANTS_PREVIEW_ENDPOINT = '/api/grants/preview'

/**
 * A predicate over what an operation *declares*. The only thing this console can express.
 *
 * `null` on any axis means the axis bounds nothing, which the service spells by the field being
 * absent — see [`selectorBody`]. Three fields, and there is deliberately no fourth: `Selector` in
 * `exchange-host` also carries `allow_ids` and `deny_ids`, for an operator editing the grant file by
 * hand, and this type is where the console's inability to write one begins.
 */
export interface Selector {
  /** Admit only operations at or below this risk, in the catalogue's own spelling. */
  maxRisk: string | null
  /** Admit only operations whose effects are a subset of this set. */
  effectsWithin: string[] | null
  /** Admit only operations with this idempotency. */
  idempotency: string | null
}

/** One grant as a caller states it: a connector, and a predicate over its operations. */
export interface ProposedGrant {
  /** The catalogue id. Never a wildcard — a grant that reached every connector would re-admit
   * whatever the next connection added, without anyone deciding to. */
  connector: string
  selector: Selector
}

/**
 * One operation a grant admits, with the facts it was admitted **on**.
 *
 * The service's `OperationFacts`, verbatim. They are here rather than left to be joined against the
 * catalogue because the whole value of the preview is that an operator can see *why* — a list of
 * forty ids says nothing about whether the bound was set too wide.
 */
export interface AdmittedOperation {
  id: string
  risk: string
  idempotency: string
  effects: string[]
}

/**
 * One grant this tenant holds, as `GET /api/grants` answers with one.
 *
 * `expressible` is the field that matters most and it is the service's, not this console's: a grant
 * written by hand may name operations explicitly, or name a connector this build does not carry, and
 * this surface can render neither back. It is shown rather than hidden — a read that quietly omitted
 * it would let an operator replace a set they had never been shown — and `PUT` is refused while one
 * is held.
 */
export interface HeldGrant {
  connector: string
  vendor: string
  selector: Selector
  /** Whether this surface could have written this grant, and could write it again. */
  expressible: boolean
  /** Why not, in the service's own words. Empty when it could. */
  reason: string
  /** The operations it names explicitly, when it names any. `null` otherwise. */
  exempt: { always: string[]; never: string[] } | null
  /** How many operations the connector declares — what `admits` is read against. */
  declares: number
  /** What it admits **today**, from the projection the gate decides on. */
  admits: AdmittedOperation[]
}

/**
 * What the console knows about this tenant's grants.
 *
 * The same three states the connections read has, and for the same reason: a tenant that has been
 * granted nothing and a read that never happened must never render alike. The first is the state
 * every deployment starts in since X-13 — and it is the state in which nothing runs — so a console
 * that showed it as a failure would send an operator looking for an outage.
 */
export type GrantsState =
  | { status: 'loading' }
  | {
      status: 'ready'
      grants: HeldGrant[]
      /** Whether a `PUT` here would faithfully replace what is stored. The service's own field. */
      editable: boolean
    }
  | { status: 'failed'; failure: ServiceFailure }

/**
 * What the console knows about a grant that does not exist yet.
 *
 * Four states, and `refused` earns its place for [`DeclarationState`]'s reason: a connector this
 * build does not carry, or a body naming an operation, is the service answering the question in a
 * sentence an operator acts on, and folding it into `failed` would put that sentence through the
 * summariser that truncates.
 */
export type PreviewState =
  | { status: 'loading' }
  | { status: 'ready'; grant: HeldGrant }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** What became of a write. The same three outcomes a connect has, for the same reason. */
export type GrantOutcome =
  | { status: 'saved'; grants: HeldGrant[]; editable: boolean }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** A selector in a served body. Every axis absent-or-null means the axis bounds nothing. */
function readSelector(value: unknown): Selector {
  if (!isObject(value)) return { maxRisk: null, effectsWithin: null, idempotency: null }
  return {
    maxRisk: typeof value.max_risk === 'string' ? value.max_risk : null,
    effectsWithin: Array.isArray(value.effects_within)
      ? value.effects_within.filter((effect): effect is string => typeof effect === 'string')
      : null,
    idempotency: typeof value.idempotency === 'string' ? value.idempotency : null,
  }
}

/**
 * The admitted operations in a served grant, or the reason the entry is not one.
 *
 * All-or-nothing, for `readConnectors`' reason and with more at stake: a preview silently missing an
 * operation is an operator approving a grant they have not seen the whole of.
 */
function readAdmitted(value: unknown): AdmittedOperation[] | string {
  if (!Array.isArray(value)) return 'a grant carries no `admits` array'
  const admitted: AdmittedOperation[] = []
  for (const entry of value) {
    if (!isObject(entry) || typeof entry.id !== 'string') return 'an admitted operation has no `id`'
    admitted.push({
      id: entry.id,
      risk: typeof entry.risk === 'string' ? entry.risk : '',
      idempotency: typeof entry.idempotency === 'string' ? entry.idempotency : '',
      effects: Array.isArray(entry.effects)
        ? entry.effects.filter((effect): effect is string => typeof effect === 'string')
        : [],
    })
  }
  return admitted
}

/** The operations a grant names explicitly, or `null` when it names none. */
function readExempt(value: unknown): HeldGrant['exempt'] {
  if (!isObject(value)) return null
  const names = (of: unknown): string[] =>
    Array.isArray(of) ? of.filter((name): name is string => typeof name === 'string') : []
  return { always: names(value.always), never: names(value.never) }
}

/**
 * One grant, or the reason the value is not one.
 *
 * Shared by the listing, by what a write answers with, and by the preview — because all three are
 * the same view. That is deliberate rather than economical: the preview an operator decides against
 * and the grant they end up holding must be rendered from one reader, or the two could differ in a
 * field and nobody would be able to see it.
 */
function readGrant(entry: unknown): HeldGrant | string {
  if (!isObject(entry) || typeof entry.connector !== 'string') {
    return 'a grant entry has no `connector`'
  }
  const admits = readAdmitted(entry.admits)
  if (typeof admits === 'string') return admits

  return {
    connector: entry.connector,
    vendor: typeof entry.vendor === 'string' ? entry.vendor : entry.connector,
    selector: readSelector(entry.selector),
    // Absent reads as **not** expressible. The safe direction: a console that assumed it could write
    // a grant back would compose a set that silently dropped whatever it could not read.
    expressible: entry.expressible === true,
    reason: typeof entry.reason === 'string' ? entry.reason : '',
    exempt: readExempt(entry.exempt),
    declares: typeof entry.declares === 'number' ? entry.declares : 0,
    admits,
  }
}

/** The grants in a `GET`/`PUT /api/grants` body, or the reason it is not one. */
function readGrants(body: unknown): { grants: HeldGrant[]; editable: boolean } | string {
  if (!isObject(body) || !Array.isArray(body.grants)) return 'no `grants` array in the body'
  const grants: HeldGrant[] = []
  for (const entry of body.grants) {
    const grant = readGrant(entry)
    if (typeof grant === 'string') return grant
    grants.push(grant)
  }
  // `!== false` rather than `=== true`: a body that omitted the flag is one this console cannot
  // read, and the screen refuses to write on `editable: false` — so defaulting it false would turn
  // an unreadable field into a screen that will not save and cannot say why.
  return { grants, editable: body.editable !== false }
}

/**
 * A selector as the service takes it: **omitted means unbounded**, and `null` is not the same word.
 *
 * The route's own documentation says every field of the selector may be omitted, and an omitted
 * `max_risk` admits every level. Sending `null` would work today — `Option<Risk>` reads it as
 * `None` — and it says something weaker: a body that names an axis it is not bounding is a body
 * that has to be read twice.
 *
 * **There is nowhere here for an operation id**, which is this function's other job. Three keys,
 * written out, from three typed fields. `the_console_never_sends_an_operation_id` asserts the shape
 * of what leaves rather than the status of what comes back.
 */
function selectorBody(selector: Selector): Record<string, unknown> {
  const body: Record<string, unknown> = {}
  if (selector.maxRisk !== null) body.max_risk = selector.maxRisk
  if (selector.effectsWithin !== null) body.effects_within = selector.effectsWithin
  if (selector.idempotency !== null) body.idempotency = selector.idempotency
  return body
}

/** One proposed grant as the service takes it. */
function grantBody(proposed: ProposedGrant): Record<string, unknown> {
  return { connector: proposed.connector, selector: selectorBody(proposed.selector) }
}

/**
 * What this tenant may run.
 *
 * ```text
 * GET /api/grants
 * 200 {"grants":[{"connector":"github","selector":{…},"admits":[{…}],"declares":12,…}],"editable":true}
 * ```
 *
 * A single entry that fails the shape check fails the whole read, for `loadConnections`' reason and
 * with more at stake: this is the page an operator uses to check what an agent of their tenant can
 * do, and a listing silently missing a grant is a listing that under-reports authority.
 */
export async function loadGrants(options: LoadOptions = {}): Promise<GrantsState> {
  const answered = await read(GRANTS_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }

  const held = readGrants(answered.body)
  if (typeof held === 'string') {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint: GRANTS_ENDPOINT, status: 200, detail: held },
    }
  }

  return { status: 'ready', ...held }
}

/**
 * What a proposed grant would admit, without storing it.
 *
 * ```text
 * POST /api/grants/preview  {"connector":"github","selector":{"max_risk":"low"}}
 * 200                       {"connector":"github","declares":12,"admits":[{"id":…,"risk":…},…]}
 * ```
 *
 * **A read that happens to be a `POST`**, because the question has a body. It stores nothing — the
 * route is a pure function of the catalogue and of what the caller just typed, and it answers on a
 * host that has bound no grant store at all, which is the composition an operator meets before they
 * have finished configuring anything.
 *
 * The answer is the service's own projection. Nothing here filters it, sorts it or decides any part
 * of it: `granting.mts` holds no admission rule and neither does this.
 */
export async function previewGrant(
  proposed: ProposedGrant,
  options: LoadOptions = {}
): Promise<PreviewState> {
  const answered = await read(GRANTS_PREVIEW_ENDPOINT, options, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(grantBody(proposed)),
  })

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return {
        status: 'refused',
        refusal: { endpoint: GRANTS_PREVIEW_ENDPOINT, status: answered.failure.status, error },
      }
    }
    return { status: 'failed', failure: answered.failure }
  }

  const grant = readGrant(answered.body)
  if (typeof grant === 'string') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint: GRANTS_PREVIEW_ENDPOINT,
        status: 200,
        detail: grant,
      },
    }
  }

  return { status: 'ready', grant }
}

/**
 * Replace what this tenant may run, entire.
 *
 * ```text
 * PUT /api/grants  {"grants":[{"connector":"github","selector":{"max_risk":"low"}}]}
 * 200              {"grants":[…],"editable":true}
 * ```
 *
 * **Whole-set, because the store is.** `exchange_host::Grants::set` takes the entire set on purpose:
 * what an operator needs to be able to state is *what this tenant may do*, entire, and a `grant(one)`
 * beside a `revoke(one)` is a sequence nobody can see the end state of. Composing that set is
 * `granting.mts`'s job — including refusing to compose one that would silently drop what it could
 * not express.
 *
 * A refusal is carried whole. The two this route reserves are worth naming because a status code is
 * not an answer to either: `409` is this tenant holding a grant that names operations explicitly,
 * and `422` is a body this surface cannot express. The *view* is what turns each into something to
 * do; this function does not paraphrase them.
 */
export async function replaceGrants(
  grants: readonly ProposedGrant[],
  options: LoadOptions = {}
): Promise<GrantOutcome> {
  const answered = await read(GRANTS_ENDPOINT, options, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ grants: grants.map(grantBody) }),
  })

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return {
        status: 'refused',
        refusal: { endpoint: GRANTS_ENDPOINT, status: answered.failure.status, error },
      }
    }
    return { status: 'failed', failure: answered.failure }
  }

  const held = readGrants(answered.body)
  if (typeof held === 'string') {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint: GRANTS_ENDPOINT, status: 200, detail: held },
    }
  }

  return { status: 'saved', ...held }
}

// ---------------------------------------------------------------------------------------------
// Agents: the one answer in this console that is readable, valuable and unrepeatable.
//
// `POST /api/agents` mints an agent principal for the caller's tenant and hands back its token
// **once**. That is not a convention this console could relax — `crate::agent` stores a verifier,
// so the host is genuinely unable to say it a second time — and it is why this section is shaped
// differently from everything above it. Every other call here returns something the console may
// keep and re-render: addresses, counts, ids, `held`. A minted token is the one value in this file
// that must not outlive the view that shows it.
//
// So this section is exactly one function. There is deliberately **no** sibling that reads a token
// back, no cache of the last mint, and no listing — partly because there would be nothing for them
// to read, and partly because a second way to reach a token is a second place it can be shown
// twice. `Agents.mts` holds the result in the view's own state and nothing above it ever sees one.
// ---------------------------------------------------------------------------------------------

/**
 * Where an agent principal is minted for the caller's tenant.
 *
 * A collection path with **no parameter**, and there is nowhere a tenant could be spelled into it:
 * the tenant is read from the principal this host resolved and from nothing a caller controls.
 * `routes::agents` carries the other half of this.
 */
export const AGENTS_ENDPOINT = '/api/agents'

/** What an operator states when minting. Two fields, and there is no third. */
export interface NewAgent {
  /** What to call the agent within this tenant. A name, never an address and never a tenant. */
  id: string
  /**
   * When its token stops resolving, as seconds since the Unix epoch.
   *
   * **Never defaulted, at either end.** `routes::agents` refuses a body that omits this rather than
   * choosing a lifetime for the caller, and a console that quietly filled one in would undo that
   * decision on every mint an operator makes from a browser — which, after this story, is all of
   * them. `Agents.mts` therefore ships an empty box rather than a sensible number.
   */
  expiresAt: number
}

/**
 * An agent this host has just minted, as `POST /api/agents` answers with one.
 *
 * `shown` is in the body and is deliberately **not** read. The service says `"shown": "once"`, and a
 * page that only stated the one-shot property when the service remembered to say so would fall
 * silent exactly when the service changed — which is the moment an operator most needs telling.
 * Being shown once is a consequence of what the store holds, so the screen states it from that.
 */
export interface MintedAgent {
  /** The principal that now exists: an agent, in the minting caller's tenant. */
  principal: Principal
  /** When its token stops resolving, echoed by the service. Seconds since the Unix epoch. */
  expiresAt: number
  /**
   * The bearer token, readable exactly once.
   *
   * The only credential value this console ever receives. It is not written to any store, does not
   * reach a URL, and is held by the view that renders it and by nothing else — see `Agents.mts`,
   * where that is enforced rather than intended, and `test/agents.test.mjs`, which drives it.
   */
  token: string
}

/**
 * What became of a mint.
 *
 * The same three outcomes a connect has, for the same reason: a refusal and an outage are different
 * events, and neither may be confused with success. The `403` matters more here than anywhere else
 * in this file — X-40 admits only a `User`, so it is the answer an agent or a service gets, and it
 * is the service's own sentence that says so.
 */
export type MintOutcome =
  | { status: 'minted'; minted: MintedAgent }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** A minted agent in a `201` body, or the reason the body is not one. */
function readMinted(body: unknown): MintedAgent | string {
  if (!isObject(body)) return 'the body is not an object'
  const principal = readPrincipal(body)
  if (!principal) return 'no `principal` with an `id` and a `tenant` in the body'
  if (typeof body.token !== 'string' || body.token === '') {
    // Not a mint that produced no token: a `201` this console could not read. Rendering it as a
    // successful mint with an empty token would leave an agent existing on this host that the
    // operator has no credential for and no way to discover.
    return 'no `token` in the body'
  }
  if (typeof body.expires_at !== 'number') return 'no `expires_at` in the body'
  return { principal, expiresAt: body.expires_at, token: body.token }
}

/**
 * Mint an agent principal for the caller's tenant, and hand back its token once.
 *
 * ```text
 * POST /api/agents   {"id":"ci-runner","expires_at":1793491200}
 * 201                {"principal":{…},"expires_at":1793491200,"token":"…","shown":"once"}
 * ```
 *
 * **The body carries two fields and there is nowhere to put a third.** No tenant — the tenant comes
 * from the principal this host resolved, and `routes::agents` ignores rather than refuses one in
 * the body precisely so that nobody can believe it was honoured. No name for the token, no scope,
 * no grant: none of those exist yet, and inventing a field the service would drop is how a console
 * starts describing a service that is not there.
 *
 * **What this function does not do is the interesting half.** It does not log the token, does not
 * put it in a URL, does not keep it, and has no counterpart that could fetch it later. What it
 * returns is the only copy the console will ever have, and its caller is a view that dies when the
 * reader navigates away.
 */
export async function mintAgent(agent: NewAgent, options: LoadOptions = {}): Promise<MintOutcome> {
  const answered = await read(AGENTS_ENDPOINT, options, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ id: agent.id, expires_at: agent.expiresAt }),
  })

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return {
        status: 'refused',
        refusal: { endpoint: AGENTS_ENDPOINT, status: answered.failure.status, error },
      }
    }
    return { status: 'failed', failure: answered.failure }
  }

  const minted = readMinted(answered.body)
  if (typeof minted === 'string') {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint: AGENTS_ENDPOINT, status: 201, detail: minted },
    }
  }

  return { status: 'minted', minted }
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
 * rather than asserted: the day the service answers `true` or `false` for one, this condition
 * disappears on its own with no edit here.
 *
 * **The summary used to say "this console has no sign-in yet", and that stopped being true when OIDC
 * landed.** It was doubly wrong after X-57, which made a host provisioned with a local identity
 * report sign-in as available too — so a reader who *was* signed in read a banner telling them the
 * console could not sign anyone in. The correction is not to swap one cause for another, either:
 * `admitted` is `null` whoever is reading, because nothing in this build decides what a principal
 * may run. That is X-13, and it is the event this condition should key on. The code keeps its name
 * so the string a component may already be matching on does not move; what it *says* is the fact.
 */
const NO_PRINCIPAL: Issue = {
  code: 'CONSOLE-NO-PRINCIPAL',
  scope: 'catalog',
  summary:
    'The service answered `admitted: null` for every operation, so what follows is what exists, not ' +
    'what you may call. That is not about being signed out: this host decides nothing about what a ' +
    'principal may run, so the answer is the same whoever is reading. It is not a statement that ' +
    'anything here is closed to you.',
  params: [],
}

/**
 * One operation, in the carried contract's shape.
 *
 * `works` arrives on the shared `Status` that [`adapt`] computes once over the whole document, and
 * what it means is written down there. `ownIssues` sees none of the catalogue-wide conditions above
 * — they are catalogue-scoped — so no operation is badged with a defect it does not own.
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
 * **What `works` means in this console** — set out here because the name does not say it, and
 * because there were four defensible readings and three of them are wrong on this page.
 *
 * It means: **this service runs this operation.** `POST /api/operations/{operation}/invoke` is in
 * the published surface, it is compiled against the same catalogue this document was read from, and
 * dispatch ends in the operation's own compiled Flux. That is the whole claim. It is not a claim
 * that the reader may call it, that anyone holds the credential it needs, or that a call would
 * succeed.
 *
 * The three readings not taken (`docs/stories/X-53-…`), each for one reason:
 *
 *   - **the tenant has a connection**, and **the tenant has supplied every setting the connector
 *     needs** (X-47) — both are per-tenant state, and this explorer is reachable *anonymously*. A
 *     badge derived from either would turn a public page into a report on somebody's connections,
 *     and it would need `GET /api/connections`, which the catalogue path deliberately never calls.
 *     `test/service.test.mjs::the_explorer_is_the_same_document_for_two_tenants` holds that shut
 *     adversarially rather than by reading, the way `routes::onboarding::tests` does for the
 *     descriptor.
 *   - **the caller's principal could invoke it** — that is `admitted`, it is three-valued, and
 *     `null` is not `false`. Collapsing it into a boolean destroys the distinction `OperationFacts`
 *     exists to render, and makes a badge on a public page move with who is looking at it.
 *
 * What is left is a fact about **this build and its route table**, identical for every caller, and
 * it is *derived* rather than asserted: `surfaces.mts` says whether the service serves `invoke`, and
 * the server's own gate
 * (`routes::onboarding::tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`)
 * measures that field against `routes::MODULES`. A route leaving the surface turns the Rust gate red
 * until `surfaces.mts` agrees, and this badge follows with no edit here. That is X-42's correction
 * in `docs/designs/agent-onboarding.md` applied a fourth time: derive, and derive from the construct
 * that answers the question being asked. If that surface is ever renamed away this reads `false`,
 * which under-claims — the direction this repository prefers to fail in.
 *
 * **Every operation gets the same answer, and that is honest rather than lazy.** The catalogue route
 * filters nothing — it serves what this binary was compiled with — and every connector in the
 * shipped catalogue declares `Runtime::Http`, which
 * `exchange_host::invoke::tests::the_whole_catalogue_declares_http` keeps true, so the deployment
 * gate refuses none of them. The day one declares a locally-executing runtime, this console will not
 * be able to see it: the served document publishes no runtime. That is a gap in what the service
 * publishes, recorded in the story, and not something to guess at here.
 *
 * **Why this does not contradict the two conditions on `status.issues`.** The carried contract
 * describes `works` as `issues.length === 0` (`catalog.mts`), from an emitter where every issue is a
 * defect of the operation carrying it. Neither condition here is one: `CONSOLE-SOURCE-SCOPE` is a
 * statement about what this *source* publishes, and `CONSOLE-NO-PRINCIPAL` says in its own words
 * that it "is not a statement that anything here is closed to you". Both are catalogue-scoped,
 * `ownIssues` filters them out, and no operation owns a defect. The banner and the badge answer
 * different questions — *may you call it* and *does this service run it* — and the first is exactly
 * what keeps the second from reading as a promise to the visitor.
 */
const SERVICE_RUNS_OPERATIONS: boolean = SURFACES.some(
  (surface) => surface.id === 'invoke' && surface.served
)

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
  const status: Status = { works: SERVICE_RUNS_OPERATIONS, issues, notes: [] }

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
