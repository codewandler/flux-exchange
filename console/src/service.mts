// The flux-exchange service, as this console reads it.
//
// This is the **only** module that knows a network exists. Catalogue views receive the completed
// document from `App.vue`; they never fetch for themselves, so an unreachable service and an empty
// catalogue remain different states all the way to the page.
//
// Two properties are the point of the file:
//
//   1. **An unreachable service is never an empty catalogue.** `loadCatalogue` returns either a
//      catalogue or a failure, never a catalogue that happens to be empty because nothing answered.
//      "Zero connectors" and "the service is not there" are different facts and the reader is owed
//      the difference.
//   2. **Nothing is filled in.** The exchange-owned catalogue type has fields only for facts this
//      API publishes. An unpublished request path or host is impossible to render rather than a
//      plausible empty string.

import type { Catalog, Connector, Operation } from './catalog.mts'

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

/** The anonymous fact that decides whether the console may offer the sign-in navigation. */
export const SIGNIN_AVAILABILITY_ENDPOINT = '/api/signin/availability'

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

/** Public generated channel declarations for one connector. */
export function channelDeclarationEndpoint(connector: string): string {
  return `${CONNECTORS_ENDPOINT}/${encodeURIComponent(connector)}/channels`
}

/** Tenant-derived persistent connector channels. */
export const CHANNELS_ENDPOINT = '/api/channels'

export function channelEndpoint(channel: string): string {
  return `${CHANNELS_ENDPOINT}/${encodeURIComponent(channel)}`
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

/** The one versioned read/write surface for a complete labelled connection. */
export function connectionPlanEndpoint(connector: string): string {
  return `${connectionEndpoint(connector)}/plan`
}

/** One declared setting's compare-and-set authority lifecycle for an operator-owned label. */
export function connectionAuthorityEndpoint(
  connector: string,
  label: string,
  service: string,
  field: string,
): string {
  return `${connectionEndpoint(connector)}/instances/${encodeURIComponent(label)}` +
    `/settings/${encodeURIComponent(service)}/${encodeURIComponent(field)}/authority`
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

/** Where one catalogue operation is invoked with its declared parameter object. */
export function invocationEndpoint(operation: string): string {
  return `/api/operations/${encodeURIComponent(operation)}/invoke`
}

/** Tenant-derived workflow authoring collection. */
export const WORKFLOWS_ENDPOINT = '/api/workflows'
/** Tenant-derived durable workflow activity. */
export const WORKFLOW_RUNS_ENDPOINT = '/api/workflow-runs'
/** Curated immutable App Packages and tenant installations. */
export const APP_PACKAGES_ENDPOINT = '/api/app-packages'
export const APPS_ENDPOINT = '/api/apps'
export const MODEL_PROFILES_ENDPOINT = '/api/model-profiles'
/** The upstream editor schema and executable palette. */
export const EDITOR_CATALOG_ENDPOINT = `${WORKFLOWS_ENDPOINT}/editor-catalog`

export function workflowEndpoint(workflow: string): string {
  return `${WORKFLOWS_ENDPOINT}/${encodeURIComponent(workflow)}`
}

export function workflowRunsEndpoint(workflow: string): string {
  return `${workflowEndpoint(workflow)}/runs`
}

export function workflowRunEndpoint(run: string): string {
  return `${WORKFLOW_RUNS_ENDPOINT}/${encodeURIComponent(run)}`
}

export function appChatEndpoint(app: string): string {
  return `${APPS_ENDPOINT}/${encodeURIComponent(app)}/chat`
}

export function appActivityEndpoint(app: string): string {
  return `${APPS_ENDPOINT}/${encodeURIComponent(app)}/activity`
}

// ---------------------------------------------------------------------------------------------
// The served contract.
//
// Typed as the service publishes it and not as this console wishes it were: `risk` and
// `idempotency` remain strings at this anonymous boundary, so a value from a newer service than
// this bundle can render as itself rather than fail a type guard.
// ---------------------------------------------------------------------------------------------

/** One entry of `GET /api/catalogue/connectors`. */
export interface ServedConnector {
  id: string
  vendor: string
  description: string
  operation_count: number
  channel_count: number
}

/**
 * One entry of `GET /api/catalogue/connectors/{id}/operations`.
 *
 * `effects_derived` and `admitted` both carry a distinction that is destroyed by dropping it:
 *
 *   - `effects_derived: true` means the service **inferred** the effects rather than reading them
 *     from a declaration. An inference shown as a declaration is a claim nobody made.
 *   - `admitted` is three-valued. `null` is not `false`: it means the question was never asked, so
 *     the catalogue is saying what exists rather than what the reader may call. `null` is what every
 *     operation carries today. The grant model gates invocation, but this route is anonymous and
 *     intentionally does not evaluate a principal's grants.
 */
export interface ServedOperation {
  id: string
  service: string
  description: string
  input_schema: Record<string, unknown>
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
  | { status: 'ready'; catalog: Catalog }
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
  | { ok: true; status: number; body: unknown }
  // The body is carried on the failure too, because a refusal's body is where the service says what
  // would have worked — the declared credential names on a `422`, the address on a `409` — and a
  // caller that only ever saw the summarised sentence could not read either.
  | { ok: false; status: number | null; failure: ServiceFailure; body: unknown }

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
      status: null,
      failure: { kind: 'unreachable', endpoint, status: null, detail: describe(error) },
    }
  }

  // A successful DELETE has deliberately no representation. Treating its empty body as malformed
  // would turn a completed revocation into an indeterminate failure in the console.
  if (response.ok && response.status === 204) return { ok: true, status: response.status, body: null }

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
      status: response.status,
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
      status: response.status,
      body,
      failure: { kind: 'unreadable', endpoint, status: response.status, detail: unreadable },
    }
  }

  return { ok: true, status: response.status, body }
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
    if (typeof entry.vendor !== 'string') return `connector \`${entry.id}\` has no \`vendor\``
    if (typeof entry.description !== 'string') {
      return `connector \`${entry.id}\` has no \`description\``
    }
    connectors.push({
      id: entry.id,
      vendor: entry.vendor,
      description: entry.description,
      operation_count: typeof entry.operation_count === 'number' ? entry.operation_count : 0,
      channel_count: typeof entry.channel_count === 'number' ? entry.channel_count : 0,
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
    if (!isObject(entry.input_schema)) return `operation \`${entry.id}\` has no \`input_schema\``
    operations.push({
      id: entry.id,
      service: typeof entry.service === 'string' ? entry.service : RESERVED_SERVICE,
      description: typeof entry.description === 'string' ? entry.description : '',
      input_schema: entry.input_schema,
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
  for (const answer of answers) {
    if (!('connector' in answer)) return { status: 'failed', failure: answer }
    pairs.push(answer)
  }

  return { status: 'ready', catalog: adapt(pairs) }
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
  /** `user`, `service_account` or `service` — what kind of caller this is. */
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

/** Whether this deployment can turn a human into a principal, kept separate from session state. */
export type SignInAvailabilityState =
  | { status: 'loading' }
  | { status: 'ready'; available: boolean }
  | { status: 'failed'; failure: ServiceFailure }

/** Read the deployment's sign-in fact without guessing it from the session response. */
export async function loadSignInAvailability(
  options: LoadOptions = {}
): Promise<SignInAvailabilityState> {
  const answered = await read(SIGNIN_AVAILABILITY_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }
  if (!isObject(answered.body) || typeof answered.body.sign_in_available !== 'boolean') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint: SIGNIN_AVAILABILITY_ENDPOINT,
        status: 200,
        detail: 'the response carries no boolean sign_in_available field',
      },
    }
  }
  return { status: 'ready', available: answered.body.sign_in_available }
}

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
  /** The operator-owned name of this instance, or `null` for an unnamed legacy connection. */
  label: string | null
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
    label: typeof entry.label === 'string' ? entry.label : null,
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
// Connecting: one value-free projection and its hosted local-management ceremony.
// ---------------------------------------------------------------------------------------------

/** The value-free connection-plan version understood by this console. */
export const CONNECTION_PLAN_VERSION = 'exchange.connection-plan.v2'

/** The sole hosted transport for a connection mutation. */
export const LOCAL_MANAGEMENT_PROTOCOL = 'exchange.local-management.v1'
export const LOCAL_MANAGEMENT_ENDPOINT = '/api/onboarding/frames'

export interface ConnectionPlanChoice {
  value: string
  label: string
}

/** A stable target and its static v2 content identity. */
export interface ConnectionPlanTarget {
  id: string
  revision: string
}

export type ConnectionAuthorityState = 'unset' | 'proposed' | 'approved' | 'revoked'

export type ConnectionAuthorityAction = 'approve' | 'revoke'

/** Value-free authority state for a field whose value can become a request destination. */
export interface ConnectionFieldAuthority {
  state: ConnectionAuthorityState
  revision: string | null
  actions: ConnectionAuthorityAction[]
}

/** One descriptor, retained even when another descriptor shares its submission target. */
export interface ConnectionPlanField {
  identity: string
  provenance: 'exchange' | 'provider.config' | 'provider.auth'
  name: string
  service: string | null
  label: string
  help: string
  required: boolean
  secret: boolean
  input: string
  aliases: string[]
  binds: string | null
  also_binds: string[]
  routable: boolean
  set: boolean | null
  target: ConnectionPlanTarget | null
  authority: ConnectionFieldAuthority | null
  choices: ConnectionPlanChoice[] | null
  reason: string | null
}

/** A value-free projection of every field the connector declares and how complete one label is. */
export interface ConnectionPlan {
  version: typeof CONNECTION_PLAN_VERSION
  connector: string
  credential_revision: string | null
  vendor: string
  labels: string[]
  plan_revision: string
  selection: string | null
  state: 'complete' | 'incomplete'
  fields: ConnectionPlanField[]
}

export type ConnectionPlanState =
  | { status: 'loading' }
  | { status: 'ready'; plan: ConnectionPlan }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

function strings(value: unknown, field: string): string[] | string {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== 'string')) {
    return `\`${field}\` is not an array of strings`
  }
  return [...value]
}

function readTarget(value: unknown, context: string): ConnectionPlanTarget | null | string {
  if (value === null) return null
  if (!isObject(value) || typeof value.id !== 'string' || value.id === '' ||
      !lowerHex256(value.revision, false)) {
    return `${context} has no target identity`
  }
  if (!hasOnlyKeys(value, ['id', 'revision'])) return `${context} target is not a closed id and revision object`
  return { id: value.id, revision: value.revision }
}

function readChoices(value: unknown, context: string): ConnectionPlanChoice[] | string {
  if (!Array.isArray(value)) return `${context} choices are not an array`
  if (value.length === 0) return `${context} publishes empty choices instead of omitting them`
  const choices: ConnectionPlanChoice[] = []
  const values = new Set<string>()
  for (const choice of value) {
    if (!isObject(choice) || typeof choice.value !== 'string' || typeof choice.label !== 'string') {
      return `${context} has a choice without string value and label`
    }
    if (!hasOnlyKeys(choice, ['value', 'label'])) return `${context} choice is not a closed value and label object`
    if (values.has(choice.value)) return `${context} repeats choice value \`${choice.value}\``
    values.add(choice.value)
    choices.push({ value: choice.value, label: choice.label })
  }
  return choices
}

function relativeAuthorityTarget(value: unknown): value is string {
  return typeof value === 'string' && value.startsWith('/api/') && !value.startsWith('//') &&
    !value.includes('?') && !value.includes('#') && !value.includes('://')
}

const MAX_U64 = '18446744073709551615'

function lowerHex256(value: unknown, nonzero: boolean): value is string {
  return typeof value === 'string' && /^[0-9a-f]{64}$/.test(value) &&
    (!nonzero || value !== '0'.repeat(64))
}

function canonicalAuthorityRevision(value: unknown): value is string {
  if (typeof value !== 'string' || !/^[1-9][0-9]*$/.test(value)) return false
  return value.length < MAX_U64.length || (value.length === MAX_U64.length && value <= MAX_U64)
}

function hasOnlyKeys(value: Record<string, unknown>, keys: string[]): boolean {
  const admitted = new Set(keys)
  return Object.keys(value).every((key) => admitted.has(key)) && keys.every((key) => key in value)
}

function readFieldAuthority(value: unknown, context: string): ConnectionFieldAuthority | string {
  if (!isObject(value) || !['unset', 'proposed', 'approved', 'revoked'].includes(String(value.state))) {
    return `${context} has no valid authority state`
  }
  if (!hasOnlyKeys(value, ['state', 'revision', 'actions'])) {
    return `${context} authority is not a closed state, revision and actions object`
  }
  if (value.state === 'unset') {
    return value.revision === null && Array.isArray(value.actions) && value.actions.length === 0
      ? { state: 'unset', revision: null, actions: [] }
      : `${context} has malformed unset authority metadata`
  }
  if (!canonicalAuthorityRevision(value.revision)) {
    return `${context} authority has no canonical u64 revision`
  }
  if (value.state === 'revoked') {
    return Array.isArray(value.actions) && value.actions.length === 0
      ? { state: 'revoked', revision: value.revision, actions: [] }
      : `${context} revoked authority still advertises actions`
  }
  if (!Array.isArray(value.actions)) return `${context} authority has no state-specific actions`

  if (value.state === 'approved') {
    if (value.actions.length !== 1 || value.actions[0] !== 'revoke') {
      return `${context} approved authority has invalid state-specific actions`
    }
    return { state: 'approved', revision: value.revision, actions: ['revoke'] }
  }

  if (value.actions.length !== 2 || value.actions[0] !== 'approve' || value.actions[1] !== 'revoke') {
    return `${context} proposed authority has invalid state-specific actions`
  }
  return { state: 'proposed', revision: value.revision, actions: ['approve', 'revoke'] }
}

function readPlanField(value: unknown, at: number): ConnectionPlanField | string {
  const context = `field ${at + 1}`
  if (!isObject(value)) return `${context} is not an object`
  const fieldKeys = [
    'identity', 'provenance', 'name', 'service', 'label', 'help', 'required', 'secret', 'input',
    'aliases', 'binds', 'also_binds', 'routable', 'set', 'target', 'authority', 'choices', 'reason',
  ]
  if (!hasOnlyKeys(value, fieldKeys)) {
    return `${context} is not the closed v2 field object`
  }
  for (const key of ['identity', 'name', 'label', 'help', 'input'] as const) {
    if (typeof value[key] !== 'string') return `${context} has no string \`${key}\``
  }
  if (!['exchange', 'provider.config', 'provider.auth'].includes(String(value.provenance))) {
    return `${context} has no valid \`provenance\``
  }
  if (value.service !== null && typeof value.service !== 'string') return `${context} has an invalid \`service\``
  if (value.binds !== null && typeof value.binds !== 'string') return `${context} has invalid \`binds\``
  for (const key of ['required', 'secret', 'routable'] as const) {
    if (typeof value[key] !== 'boolean') return `${context} has no boolean \`${key}\``
  }
  if (value.secret ? value.set !== null : typeof value.set !== 'boolean') {
    return `${context} does not use null only for a secret field's live set fact`
  }
  if (value.secret !== (value.input === 'secret')) return `${context} has inconsistent secret input metadata`

  const aliases = strings(value.aliases, `${context}.aliases`)
  if (typeof aliases === 'string') return aliases
  if (new Set(aliases).size !== aliases.length) return `${context} repeats a CLI alias`
  if (aliases.some((alias) => !/^--[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(alias))) {
    return `${context} has a malformed CLI alias`
  }
  if (value.secret && aliases.length !== 0) return `${context} publishes a secret CLI alias`

  const target = readTarget(value.target, context)
  if (typeof target === 'string') return target
  if (value.routable && target === null) return `${context} is routable but has no target`
  if (!value.routable && target !== null) return `${context} is unroutable but has a target`
  if (!value.routable && typeof value.reason !== 'string') return `${context} is unroutable but has no reason`
  if (value.routable && value.reason !== null) return `${context} is routable but publishes a reason`

  const alsoBinds = strings(value.also_binds, `${context}.also_binds`)
  if (typeof alsoBinds === 'string') return alsoBinds
  if (new Set(alsoBinds).size !== alsoBinds.length) return `${context} repeats an also_binds identity`

  let choices: ConnectionPlanChoice[] | null = null
  if (value.choices !== null) {
    const parsed = readChoices(value.choices, context)
    if (typeof parsed === 'string') return parsed
    choices = parsed
  }

  let authority: ConnectionFieldAuthority | null = null
  if (value.authority !== null) {
    const parsed = readFieldAuthority(value.authority, context)
    if (typeof parsed === 'string') return parsed
    if (value.set !== (parsed.state === 'approved')) {
      return `${context} authority state does not match whether the field is runtime-effective`
    }
    if (value.service === null || value.binds === null || value.provenance !== 'provider.config' || value.secret || !value.routable) {
      return `${context} authority does not belong to a routable non-secret provider setting`
    }
    authority = parsed
  }

  // Credential target identities are the generic routing fact. Reject an inconsistent descriptor
  // rather than ever letting a credential-shaped target degrade into an ordinary text control.
  if (target?.id.startsWith('credential.') && value.secret !== true) {
    return `${context} routes to a credential target but is not secret`
  }
  if (value.provenance === 'provider.auth' &&
      (value.service !== null || value.identity !== value.binds || value.identity !== target?.id || !value.secret)) {
    return `${context} is not a complete synthesized provider.auth credential descriptor`
  }

  return {
    identity: value.identity as string,
    provenance: value.provenance as ConnectionPlanField['provenance'],
    name: value.name as string,
    service: value.service as string | null,
    label: value.label as string,
    help: value.help as string,
    required: value.required as boolean,
    secret: value.secret as boolean,
    input: value.input as string,
    aliases,
    binds: value.binds as string | null,
    also_binds: alsoBinds,
    routable: value.routable as boolean,
    set: value.set as boolean | null,
    target,
    authority,
    choices,
    reason: value.reason as string | null,
  }
}

function readConnectionPlan(body: unknown, expectedConnector: string): ConnectionPlan | string {
  if (!isObject(body)) return 'the plan is not an object'
  if (body.version !== CONNECTION_PLAN_VERSION) return `unsupported or missing plan version`
  if (!hasOnlyKeys(body, [
    'version', 'connector', 'credential_revision', 'vendor', 'labels', 'plan_revision',
    'selection', 'state', 'fields',
  ])) {
    return 'the plan is not a closed v2 object'
  }
  if (typeof body.connector !== 'string') return 'the plan has no connector'
  if (body.connector !== expectedConnector) {
    return `the plan names connector \`${body.connector}\` instead of \`${expectedConnector}\``
  }
  if (typeof body.vendor !== 'string') return 'the plan has no vendor'
  const labels = strings(body.labels, 'labels')
  if (typeof labels === 'string') return labels
  if (new Set(labels).size !== labels.length) return 'the plan repeats an existing label'
  if (labels.some((label) => !/^[A-Za-z0-9_-]{1,64}$/.test(label))) return 'the plan has an invalid label'
  if (labels.some((label, at) => at > 0 && labels[at - 1] >= label)) {
    return 'the plan labels are not in canonical order'
  }
  if (body.selection !== null && typeof body.selection !== 'string') return 'the plan has an invalid selection'
  if (typeof body.selection === 'string' && !labels.includes(body.selection)) {
    return 'the selected label is not present in labels'
  }
  if (!lowerHex256(body.plan_revision, false)) return 'the plan has no canonical plan revision'
  if (body.selection === null ? body.credential_revision !== null : !lowerHex256(body.credential_revision, true)) {
    return 'the plan has a credential revision inconsistent with its selection'
  }
  if (body.state !== 'complete' && body.state !== 'incomplete') return 'the plan has no valid state'
  if (!Array.isArray(body.fields) || body.fields.length === 0) return 'the plan has no fields'

  const fields: ConnectionPlanField[] = []
  const identities = new Set<string>()
  const aliases = new Set<string>()
  const targetRevisions = new Map<string, string>()
  for (const [at, value] of body.fields.entries()) {
    const field = readPlanField(value, at)
    if (typeof field === 'string') return field
    if (identities.has(field.identity)) return `field identity \`${field.identity}\` appears more than once`
    identities.add(field.identity)
    for (const alias of field.aliases) {
      if (aliases.has(alias)) return `CLI alias \`${alias}\` appears more than once`
      aliases.add(alias)
    }
    if (field.target !== null) {
      const revision = targetRevisions.get(field.target.id)
      if (revision !== undefined && revision !== field.target.revision) {
        return `target \`${field.target.id}\` has inconsistent revisions`
      }
      targetRevisions.set(field.target.id, field.target.revision)
    }
    fields.push(field)
  }
  const derivedState = fields.every((field) =>
    !field.required || (field.routable && (field.secret || field.set === true)))
    ? 'complete'
    : 'incomplete'
  if (body.state !== derivedState) {
    return `the plan claims \`${body.state}\` but its required fields derive \`${derivedState}\``
  }
  if (body.selection === null && fields.some((field) => field.authority !== null && field.authority.state !== 'unset')) {
    return 'a proposed authority state has no selected label'
  }
  const name = fields[0]
  if (name.identity !== 'connection.name' || name.provenance !== 'exchange' || name.name !== 'name' || name.service !== null ||
      name.binds !== null || name.target?.id !== 'connection.name' || !name.required || name.secret ||
      name.input !== 'text' || !name.routable || name.aliases.length !== 1 || name.aliases[0] !== '--name' ||
      name.authority !== null || name.choices !== null || name.reason !== null) {
    return 'the first field is not `connection.name`'
  }

  return {
    version: CONNECTION_PLAN_VERSION,
    connector: body.connector,
    credential_revision: body.credential_revision as string | null,
    vendor: body.vendor,
    labels,
    plan_revision: body.plan_revision,
    selection: body.selection as string | null,
    state: body.state,
    fields,
  }
}

/**
 * Read a plan, selecting an existing operator-owned label only in the query. Values have no query
 * representation and this API deliberately accepts no other selection input.
 */
export async function loadConnectionPlan(
  connector: string,
  selection: string | null,
  options: LoadOptions = {}
): Promise<ConnectionPlanState> {
  const endpoint = connectionPlanEndpoint(connector)
  const selectedEndpoint = selection === null
    ? `${endpoint}?version=${encodeURIComponent(CONNECTION_PLAN_VERSION)}`
    : `${endpoint}?version=${encodeURIComponent(CONNECTION_PLAN_VERSION)}&name=${encodeURIComponent(selection)}`
  const answered = await read(selectedEndpoint, options)

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return { status: 'refused', refusal: { endpoint: selectedEndpoint, status: answered.failure.status, error } }
    }
    return { status: 'failed', failure: answered.failure }
  }

  const plan = readConnectionPlan(answered.body, connector)
  if (typeof plan === 'string') {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint: selectedEndpoint, status: answered.status, detail: plan },
    }
  }
  return { status: 'ready', plan }
}

export interface ConnectionPlanSubmission {
  begin: {
    authorities: { revision: string | null; target: string }[]
    connector: string
    label: string
    plan_revision: string
    settings: { target: string; value: string }[]
    targets: { revision: string; target: string }[]
  }
  /** Raw UTF-8 bytes sent only in ordinal FXLM SECRET frames, never JSON. */
  secrets: { target: string; value: Uint8Array }[]
}

export interface ConnectReceipt {
  schema: 'exchange.connect-receipt.v1'
  connector: string
  label: string
  operation: 'connect'
  receipt_id: string
  replayed: boolean
  commit: { audit: 'committed'; resource: 'committed' }
}

export type ConnectionPlanOutcome =
  | { status: 'answered'; result: ConnectReceipt }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

export interface ConnectionAuthorityTransition {
  connector: string
  label: string
  service: string
  field: string
  revision: string
  action: ConnectionAuthorityAction
}

export interface ConnectionAuthorityTransitionResult {
  version: typeof CONNECTION_PLAN_VERSION
  connector: string
  label: string
  service: string
  field: string
  action: 'approved' | 'revoked'
  authority: {
    state: 'approved' | 'revoked'
    revision: string
  }
}

export interface ConnectionAuthorityPartialResult extends ConnectionAuthorityTransitionResult {
  outcome: 'partial'
  may_have_happened: true
}

export type ConnectionAuthorityResult = ConnectionAuthorityTransitionResult | ConnectionAuthorityPartialResult

export type ConnectionAuthorityOutcome =
  | { status: 'answered'; result: ConnectionAuthorityResult }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

export interface ConnectionAuthorityInspectionRequest {
  connector: string
  label: string
  service: string
  field: string
  state: Exclude<ConnectionAuthorityState, 'unset'>
  revision: string
}

export interface ConnectionAuthorityInspectionResult {
  version: typeof CONNECTION_PLAN_VERSION
  connector: string
  label: string
  service: string
  field: string
  authority: {
    state: Exclude<ConnectionAuthorityState, 'unset'>
    revision: string
    origin: string
  }
}

export type ConnectionAuthorityInspectionOutcome =
  | { status: 'answered'; result: ConnectionAuthorityInspectionResult }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

function readAuthorityTransition(
  body: unknown,
  transition: ConnectionAuthorityTransition
): ConnectionAuthorityResult | string {
  if (!isObject(body)) return 'the authority transition response is not an object'
  if (body.version !== CONNECTION_PLAN_VERSION) return 'unsupported authority transition response version'
  const partial = body.outcome === 'partial' || 'may_have_happened' in body
  const expectedKeys = partial
    ? ['version', 'connector', 'label', 'service', 'field', 'action', 'authority', 'outcome', 'may_have_happened']
    : ['version', 'connector', 'label', 'service', 'field', 'action', 'authority']
  if (!hasOnlyKeys(body, expectedKeys)) {
    return 'the authority transition response is not a closed value-free object'
  }
  for (const key of ['connector', 'label', 'service', 'field'] as const) {
    if (body[key] !== transition[key]) return `the authority transition response has a mismatched ${key}`
  }
  if (!isObject(body.authority) || !canonicalAuthorityRevision(body.authority.revision) ||
      body.authority.revision !== transition.revision) {
    return 'the authority transition response has a mismatched revision'
  }
  if (!hasOnlyKeys(body.authority, ['state', 'revision'])) {
    return 'the authority transition response carries unexpected authority data'
  }
  const expected = transition.action === 'approve' ? 'approved' : 'revoked'
  if (body.action !== expected || body.authority.state !== expected) {
    return `the ${transition.action} action did not return ${expected}`
  }
  const result: ConnectionAuthorityTransitionResult = {
    version: CONNECTION_PLAN_VERSION,
    connector: transition.connector,
    label: transition.label,
    service: transition.service,
    field: transition.field,
    action: expected,
    authority: { state: expected, revision: transition.revision },
  }
  if (!partial) return result
  if (body.outcome !== 'partial' || body.may_have_happened !== true) {
    return 'the partial authority transition response does not say it may have happened'
  }
  return { ...result, outcome: 'partial', may_have_happened: true }
}

function readAuthorityInspection(
  body: unknown,
  request: ConnectionAuthorityInspectionRequest,
): ConnectionAuthorityInspectionResult | string {
  if (!isObject(body) || body.version !== CONNECTION_PLAN_VERSION) {
    return 'unsupported authority inspection response version'
  }
  if (!hasOnlyKeys(body, ['version', 'connector', 'label', 'service', 'field', 'authority'])) {
    return 'the authority inspection response is not a closed object'
  }
  for (const key of ['connector', 'label', 'service', 'field'] as const) {
    if (body[key] !== request[key]) return `the authority inspection response has a mismatched ${key}`
  }
  if (!isObject(body.authority) || !hasOnlyKeys(body.authority, ['state', 'revision', 'origin']) ||
      body.authority.state !== request.state || body.authority.revision !== request.revision ||
      !canonicalAuthorityRevision(body.authority.revision) ||
      typeof body.authority.origin !== 'string' || body.authority.origin === '') {
    return 'the authority inspection response has invalid state, revision or origin'
  }
  return {
    version: CONNECTION_PLAN_VERSION,
    connector: request.connector,
    label: request.label,
    service: request.service,
    field: request.field,
    authority: {
      state: request.state,
      revision: request.revision,
      origin: body.authority.origin,
    },
  }
}

/** Inspect the exact normalized proposal through the configured-operator-only same-origin read. */
export async function inspectConnectionAuthority(
  request: ConnectionAuthorityInspectionRequest,
  options: LoadOptions = {},
): Promise<ConnectionAuthorityInspectionOutcome> {
  const endpoint = connectionAuthorityEndpoint(
    request.connector, request.label, request.service, request.field,
  )
  if (!canonicalAuthorityRevision(request.revision)) {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint, status: null, detail: 'invalid authority inspection request' },
    }
  }
  const answered = await read(endpoint, options)
  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.status !== null) {
      return { status: 'refused', refusal: { endpoint, status: answered.status, error } }
    }
    return { status: 'failed', failure: answered.failure }
  }
  const result = readAuthorityInspection(answered.body, request)
  if (answered.status !== 200 || typeof result === 'string') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable', endpoint, status: answered.status,
        detail: typeof result === 'string' ? result : `HTTP ${answered.status} cannot carry an authority inspection`,
      },
    }
  }
  return { status: 'answered', result }
}

/** Execute one compare-and-set authority action without carrying the proposed origin. */
export async function transitionConnectionAuthority(
  transition: ConnectionAuthorityTransition,
  options: LoadOptions = {}
): Promise<ConnectionAuthorityOutcome> {
  const endpoint = connectionAuthorityEndpoint(
    transition.connector, transition.label, transition.service, transition.field,
  )
  const expectedMethod = transition.action === 'approve' ? 'PUT' : 'DELETE'
  if (!relativeAuthorityTarget(endpoint) ||
      (transition.action !== 'approve' && transition.action !== 'revoke') ||
      !canonicalAuthorityRevision(transition.revision)) {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint, status: null, detail: 'invalid authority transition request' },
    }
  }
  const answered = await read(endpoint, options, {
    method: expectedMethod,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ version: CONNECTION_PLAN_VERSION, revision: transition.revision }),
  })

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.status !== null) {
      return { status: 'refused', refusal: { endpoint, status: answered.status, error } }
    }
    return { status: 'failed', failure: answered.failure }
  }
  const result = readAuthorityTransition(answered.body, transition)
  const expectedStatus = typeof result !== 'string' && 'outcome' in result ? 207 : 200
  if (answered.status !== expectedStatus || typeof result === 'string') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable', endpoint, status: answered.status,
        detail: typeof result === 'string' ? result : `HTTP ${answered.status} cannot carry an authority transition`,
      },
    }
  }
  return { status: 'answered', result }
}

interface HostedSocket {
  binaryType: BinaryType
  protocol: string
  send(data: ArrayBufferView | ArrayBuffer | Blob | string): void
  close(code?: number, reason?: string): void
  addEventListener(type: 'open', listener: () => void): void
  addEventListener(type: 'message', listener: (event: MessageEvent) => void): void
  addEventListener(type: 'error', listener: () => void): void
  addEventListener(type: 'close', listener: () => void): void
}

/** Injectable only so the byte contract can be tested without a network. */
export interface WebSocketConstructor {
  new(url: string, protocols: string | string[]): HostedSocket
}

export interface HostedConnectionOptions {
  webSocket?: WebSocketConstructor
  href?: string
}

const textEncoder = new TextEncoder()
const textDecoder = new TextDecoder('utf-8', { fatal: true })

function canonicalJson(value: unknown): string {
  if (value === null || typeof value === 'boolean' || typeof value === 'string') return JSON.stringify(value)
  if (typeof value === 'number' && Number.isSafeInteger(value)) return JSON.stringify(value)
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`
  if (isObject(value)) {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`
  }
  throw new TypeError('FXLM control JSON contains an unsupported value')
}

function frame(opcode: number, payload: Uint8Array): Uint8Array {
  const encoded = new Uint8Array(12 + payload.length)
  encoded.set([0x46, 0x58, 0x4c, 0x4d, 1, 1], 0)
  new DataView(encoded.buffer).setUint16(6, opcode)
  new DataView(encoded.buffer).setUint32(8, payload.length)
  encoded.set(payload, 12)
  return encoded
}

function controlFrame(opcode: number, value: unknown): Uint8Array {
  return frame(opcode, textEncoder.encode(canonicalJson(value)))
}

function secretFrame(ordinal: number, value: Uint8Array): Uint8Array {
  const payload = new Uint8Array(2 + value.length)
  new DataView(payload.buffer).setUint16(0, ordinal)
  payload.set(value, 2)
  return frame(0x0003, payload)
}

function parseServerFrame(bytes: Uint8Array): { opcode: number; body: unknown } | string {
  if (bytes.length < 12 || bytes[0] !== 0x46 || bytes[1] !== 0x58 || bytes[2] !== 0x4c || bytes[3] !== 0x4d) {
    return 'the hosted response has no FXLM header'
  }
  if (bytes[4] !== 1 || bytes[5] !== 2) return 'the hosted response has the wrong version or direction'
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const length = view.getUint32(8)
  if (length > 65_536 || bytes.length !== 12 + length) return 'the hosted response is truncated or has surplus bytes'
  try {
    const source = textDecoder.decode(bytes.subarray(12))
    const body: unknown = JSON.parse(source)
    if (canonicalJson(body) !== source) return 'the hosted response control JSON is not canonical'
    return { opcode: view.getUint16(6), body }
  } catch {
    return 'the hosted response control JSON is unreadable'
  }
}

function readNeed(body: unknown, expected: ConnectionPlanSubmission['secrets']): {
  proposal_digest: string; transaction_id: string
} | string {
  if (!isObject(body) || !hasOnlyKeys(body, ['proposal_digest', 'secrets', 'transaction_id']) ||
      !lowerHex256(body.proposal_digest, false) || !lowerHex256(body.transaction_id, false) ||
      body.transaction_id.slice(0, 16) === '0'.repeat(16) ||
      body.transaction_id.slice(16) === '0'.repeat(48) || !Array.isArray(body.secrets) ||
      body.secrets.length !== expected.length) {
    return 'the hosted NEED_SECRETS response is not the closed transaction object'
  }
  for (const [at, need] of body.secrets.entries()) {
    if (!isObject(need) || !hasOnlyKeys(need, ['ordinal', 'target']) || need.ordinal !== at + 1 ||
        need.target !== expected[at]?.target) {
      return 'the hosted NEED_SECRETS response changed the requested ordinal or target order'
    }
  }
  return { proposal_digest: body.proposal_digest, transaction_id: body.transaction_id }
}

function readReceipt(body: unknown, connector: string, label: string): ConnectReceipt | string {
  if (!isObject(body) || !hasOnlyKeys(body, [
    'commit', 'connector', 'label', 'operation', 'receipt_id', 'replayed', 'schema',
  ]) || body.schema !== 'exchange.connect-receipt.v1' || body.connector !== connector ||
      body.label !== label || body.operation !== 'connect' || !lowerHex256(body.receipt_id, true) ||
      typeof body.replayed !== 'boolean' || !isObject(body.commit) ||
      !hasOnlyKeys(body.commit, ['audit', 'resource']) || body.commit.audit !== 'committed' ||
      body.commit.resource !== 'committed') {
    return 'the hosted receipt is not the closed connect receipt'
  }
  return body as unknown as ConnectReceipt
}

function readLocalRefusal(body: unknown): { status: number; code: string } | string {
  if (!isObject(body) || body.schema !== 'exchange.local-management-error.v1' ||
      typeof body.code !== 'string' || !Number.isInteger(body.status)) {
    return 'the hosted refusal is not the closed local-management error'
  }
  const keys = body.commit === 'none'
    ? ['code', 'commit', 'retry', 'schema', 'status']
    : ['code', 'commit', 'receipt_id', 'retry', 'schema', 'status']
  if (!hasOnlyKeys(body, keys)) {
    return 'the hosted refusal is not the closed local-management error'
  }
  const preDecision: Record<string, readonly [number, string]> = {
    invalid_frame: [400, 'never'], unsupported_version: [426, 'never'], wrong_direction: [400, 'never'],
    unexpected_frame: [409, 'never'], frame_too_large: [413, 'never'], truncated_frame: [400, 'never'],
    surplus_data: [400, 'never'], deadline_exceeded: [408, 'refresh'], peer_unverified: [403, 'never'],
    unsafe_root: [503, 'operator'], local_management_unavailable: [503, 'operator'],
    invalid_request: [400, 'never'], unknown_connector: [404, 'refresh'], unknown_label: [404, 'refresh'],
    invalid_label: [422, 'never'], secret_json_forbidden: [415, 'never'], unknown_target: [422, 'refresh'],
    stale_plan: [409, 'refresh'], stale_credential_revision: [409, 'refresh'],
    credential_state_conflict: [409, 'refresh'], proposal_conflict: [409, 'refresh'],
    connect_busy: [409, 'refresh'], grant_unexpressible: [409, 'operator'], grant_stale: [409, 'refresh'],
    grant_digest_mismatch: [409, 'refresh'], service_account_conflict: [409, 'refresh'],
    writer_invalid: [400, 'never'], writer_closed: [409, 'operator'], store_unavailable: [503, 'operator'],
    audit_unavailable: [503, 'operator'], internal_refusal: [500, 'operator'],
  }
  if (body.commit === 'none') {
    const expected = preDecision[body.code]
    if (expected === undefined || body.status !== expected[0] || body.retry !== expected[1]) {
      return 'the hosted refusal has a tuple outside the closed pre-decision table'
    }
  } else if (body.commit !== 'query_receipt' || body.retry !== 'same_proposal' ||
      !lowerHex256(body.receipt_id, true) ||
      !((body.code === 'store_unavailable' || body.code === 'audit_unavailable') && body.status === 503 ||
        body.code === 'internal_refusal' && body.status === 500)) {
    return 'the hosted refusal has a tuple outside the closed post-decision table'
  }
  return { status: body.status as number, code: body.code }
}

function hostedFramesUrl(href?: string): string | null {
  const base = href ?? globalThis.location?.href
  if (base === undefined) return null
  const url = new URL(LOCAL_MANAGEMENT_ENDPOINT, base)
  if (url.protocol === 'https:') url.protocol = 'wss:'
  else if (url.protocol === 'http:') url.protocol = 'ws:'
  else return null
  return url.href
}

async function messageBytes(data: unknown): Promise<Uint8Array | null> {
  if (data instanceof ArrayBuffer) return new Uint8Array(data)
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
  if (typeof Blob !== 'undefined' && data instanceof Blob) return new Uint8Array(await data.arrayBuffer())
  return null
}

/**
 * Run one hosted FXLM connect ceremony. Control JSON is value-free; each credential is sent only
 * as the raw payload of its one requested ordinal frame.
 */
export async function applyConnectionPlan(
  connector: string,
  submission: ConnectionPlanSubmission,
  options: HostedConnectionOptions = {},
): Promise<ConnectionPlanOutcome> {
  const endpoint = LOCAL_MANAGEMENT_ENDPOINT
  const url = hostedFramesUrl(options.href)
  const Constructor = options.webSocket ?? globalThis.WebSocket as unknown as WebSocketConstructor | undefined
  if (url === null || Constructor === undefined || connector !== submission.begin.connector) {
    return {
      status: 'failed',
      failure: { kind: 'unreachable', endpoint, status: null, detail: 'the hosted local-management transport is unavailable' },
    }
  }
  if (submission.begin.label === '' || submission.secrets.some(({ value }) => value.length < 1 || value.length > 8192)) {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint, status: null, detail: 'the connection proposal has an invalid label or secret bound' },
    }
  }

  return await new Promise((resolve) => {
    let terminal = false
    let phase: 'opening' | 'need' | 'receipt' = 'opening'
    const socket = new Constructor(url, LOCAL_MANAGEMENT_PROTOCOL)
    socket.binaryType = 'arraybuffer'

    const finish = (outcome: ConnectionPlanOutcome): void => {
      if (terminal) return
      terminal = true
      socket.close()
      resolve(outcome)
    }
    const unreadable = (detail: string): void => finish({
      status: 'failed', failure: { kind: 'unreadable', endpoint, status: null, detail },
    })

    socket.addEventListener('open', () => {
      if (socket.protocol !== LOCAL_MANAGEMENT_PROTOCOL) {
        unreadable('the hosted endpoint did not select the exact local-management subprotocol')
        return
      }
      phase = 'need'
      socket.send(controlFrame(0x0001, submission.begin))
    })
    socket.addEventListener('message', (event) => {
      void (async () => {
        const bytes = await messageBytes(event.data)
        if (bytes === null) {
          unreadable('the hosted endpoint returned a non-binary message')
          return
        }
        const parsed = parseServerFrame(bytes)
        if (typeof parsed === 'string') {
          unreadable(parsed)
          return
        }
        if (parsed.opcode === 0x7fff) {
          const refusal = readLocalRefusal(parsed.body)
          if (typeof refusal === 'string') unreadable(refusal)
          else finish({ status: 'refused', refusal: { endpoint, status: refusal.status, error: refusal.code } })
          return
        }
        if (phase === 'need' && parsed.opcode === 0x0002) {
          const need = readNeed(parsed.body, submission.secrets)
          if (typeof need === 'string') {
            unreadable(need)
            return
          }
          for (const [at, secret] of submission.secrets.entries()) socket.send(secretFrame(at + 1, secret.value))
          socket.send(controlFrame(0x0004, need))
          phase = 'receipt'
          return
        }
        if ((phase === 'need' || phase === 'receipt') && parsed.opcode === 0x0006) {
          const receipt = readReceipt(parsed.body, connector, submission.begin.label)
          if (typeof receipt === 'string') unreadable(receipt)
          else finish({ status: 'answered', result: receipt })
          return
        }
        unreadable('the hosted endpoint returned an opcode outside the connect state machine')
      })()
    })
    socket.addEventListener('error', () => finish({
      status: 'failed', failure: { kind: 'unreachable', endpoint, status: null, detail: 'the hosted local-management connection failed' },
    }))
    socket.addEventListener('close', () => {
      if (!terminal) finish({
        status: 'failed', failure: { kind: 'unreachable', endpoint, status: null, detail: 'the hosted local-management connection closed before a receipt' },
      })
    })
  })
}

/** What became of an atomic credential replacement. */
export type RotationOutcome =
  | { status: 'rotated'; connection: Connection }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** Replace one held credential without ever removing the connection around it. */
export async function rotateCredential(
  connector: string,
  credential: string,
  value: string,
  options: LoadOptions = {}
): Promise<RotationOutcome> {
  const endpoint = credentialEndpoint(connector, credential)
  const answered = await read(endpoint, options, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ value }),
  })
  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return { status: 'refused', refusal: { endpoint, status: answered.failure.status, error } }
    }
    return { status: 'failed', failure: answered.failure }
  }
  const connection = readConnection(answered.body)
  return typeof connection === 'string'
    ? { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: connection } }
    : { status: 'rotated', connection }
}

// ---------------------------------------------------------------------------------------------
// Invocation: the body is the operation's parameter object and nothing else.
// ---------------------------------------------------------------------------------------------

export interface InvocationResult {
  operation: string
  content: string
  view: string | null
  isError: boolean
}

export interface InvocationRefusal {
  endpoint: string
  status: number
  refusal: string
  operation: string
  sent: 'no' | 'maybe' | 'unknown'
  retryable: boolean
  message: string
  supplyAt: string | null
}

export type InvokeOutcome =
  | { status: 'invoked'; result: InvocationResult }
  | { status: 'refused'; refusal: InvocationRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** Invoke one catalogue operation, preserving the service's sent/retryable distinction. */
export async function invokeOperation(
  operation: string,
  params: Record<string, unknown>,
  options: LoadOptions = {}
): Promise<InvokeOutcome> {
  const endpoint = invocationEndpoint(operation)
  const answered = await read(endpoint, options, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(params),
  })
  if (!answered.ok) {
    if (isObject(answered.body) && answered.failure.status !== null) {
      return {
        status: 'refused',
        refusal: {
          endpoint,
          status: answered.failure.status,
          refusal: typeof answered.body.refusal === 'string' ? answered.body.refusal : 'refused',
          operation: typeof answered.body.operation === 'string' ? answered.body.operation : operation,
          sent: answered.body.sent === 'no' || answered.body.sent === 'maybe' ? answered.body.sent : 'unknown',
          retryable: answered.body.retryable === true,
          message: typeof answered.body.message === 'string'
            ? answered.body.message
            : serviceError(answered.body) ?? 'the service refused this invocation',
          supplyAt: typeof answered.body.supply_at === 'string' ? answered.body.supply_at : null,
        },
      }
    }
    return { status: 'failed', failure: answered.failure }
  }
  if (
    !isObject(answered.body) ||
    typeof answered.body.operation !== 'string' ||
    typeof answered.body.content !== 'string'
  ) {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint, status: 200, detail: 'the invocation result has no operation or content' },
    }
  }
  return {
    status: 'invoked',
    result: {
      operation: answered.body.operation,
      content: answered.body.content,
      view: typeof answered.body.view === 'string' ? answered.body.view : null,
      isError: answered.body.is_error === true,
    },
  }
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
  /** Closed declared event subsets admitted on the connector's inbound bindings. */
  inbound?: InboundGrant[]
}

export interface InboundGrant {
  binding: string
  events: string[]
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
  /** Closed declared inbound authority, empty for every pre-channel grant document. */
  inbound: InboundGrant[]
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

// ---------------------------------------------------------------------------------------------
// Generated connector channels. Declarations are anonymous vendor facts; records are tenant data.
// ---------------------------------------------------------------------------------------------

export interface ChannelEventDeclaration {
  name: string
  description: string
  group: string
  default: boolean
}

export interface ChannelDeclaration {
  connector: string
  name: string
  service: string
  description: string
  transport: 'socket' | 'webhook' | 'poll'
  events: ChannelEventDeclaration[]
}

export interface HeldChannel {
  id: string
  connector: string
  connection: string
  binding: string
  events: string[]
  status: 'starting' | 'running' | 'retrying' | 'refused' | 'stopped'
}

/** The only connection projection a channel form receives. */
export interface ChannelConnectionLabel {
  connector: string
  label: string
}

export type ChannelDeclarationsState =
  | { status: 'loading' }
  | { status: 'ready'; declarations: ChannelDeclaration[] }
  | { status: 'failed'; failure: ServiceFailure }

export type ChannelsState =
  | { status: 'loading' }
  | { status: 'ready'; channels: HeldChannel[] }
  | { status: 'failed'; failure: ServiceFailure }

export type ChannelMutation =
  | { status: 'saved'; channel: HeldChannel }
  | { status: 'removed' }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** Reduce connection management state to the operator labels accepted by channel writes. */
export function channelConnectionLabels(state: ConnectionsState): ChannelConnectionLabel[] {
  if (state.status !== 'ready') return []
  return state.connections.flatMap((connection) =>
    connection.label === null
      ? []
      : [{ connector: connection.connector, label: connection.label }]
  )
}

function readChannelDeclaration(connector: string, value: unknown): ChannelDeclaration[] | string {
  if (!isObject(value) || !Array.isArray(value.channels)) return 'no `channels` array in the body'
  const declarations: ChannelDeclaration[] = []
  for (const entry of value.channels) {
    if (!isObject(entry) || typeof entry.name !== 'string') return 'a channel declaration has no `name`'
    if (!['socket', 'webhook', 'poll'].includes(String(entry.transport))) {
      return `channel \`${entry.name}\` has an unknown transport`
    }
    if (!Array.isArray(entry.events)) return `channel \`${entry.name}\` has no events array`
    const events: ChannelEventDeclaration[] = []
    for (const event of entry.events) {
      if (!isObject(event) || typeof event.name !== 'string') return `channel \`${entry.name}\` has an unnamed event`
      events.push({
        name: event.name,
        description: typeof event.description === 'string' ? event.description : '',
        group: typeof event.group === 'string' ? event.group : '',
        default: event.default === true,
      })
    }
    declarations.push({
      connector,
      name: entry.name,
      service: typeof entry.service === 'string' ? entry.service : RESERVED_SERVICE,
      description: typeof entry.description === 'string' ? entry.description : '',
      transport: entry.transport as ChannelDeclaration['transport'],
      events,
    })
  }
  return declarations
}

function readHeldChannel(value: unknown): HeldChannel | string {
  if (
    !isObject(value) ||
    typeof value.id !== 'string' ||
    typeof value.connector !== 'string' ||
    typeof value.connection !== 'string'
  ) {
    return 'a channel has no `id`, `connector` or connection label'
  }
  const statuses = ['starting', 'running', 'retrying', 'refused', 'stopped'] as const
  if (!statuses.includes(value.status as typeof statuses[number])) return `channel \`${value.id}\` has an unknown status`
  if (!Array.isArray(value.events) || value.events.some((event) => typeof event !== 'string')) {
    return `channel \`${value.id}\` has no event list`
  }
  return {
    id: value.id,
    connector: value.connector,
    connection: value.connection,
    binding: typeof value.binding === 'string' ? value.binding : '',
    events: value.events as string[],
    status: value.status as HeldChannel['status'],
  }
}

export async function loadChannelDeclarations(catalog: Catalog, options: LoadOptions = {}): Promise<ChannelDeclarationsState> {
  const connectors = catalog.connectors.filter((connector) => connector.channelCount > 0)
  const answers = await Promise.all(connectors.map(async (connector) => {
    const endpoint = channelDeclarationEndpoint(connector.id)
    const answered = await read(endpoint, options)
    if (!answered.ok) return answered.failure
    const declarations = readChannelDeclaration(connector.id, answered.body)
    return typeof declarations === 'string'
      ? { kind: 'unreadable' as const, endpoint, status: 200, detail: declarations }
      : declarations
  }))
  const declarations: ChannelDeclaration[] = []
  for (const answer of answers) {
    if (!Array.isArray(answer)) return { status: 'failed', failure: answer }
    declarations.push(...answer)
  }
  return { status: 'ready', declarations }
}

export async function loadChannels(options: LoadOptions = {}): Promise<ChannelsState> {
  const answered = await read(CHANNELS_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }
  if (!Array.isArray(answered.body)) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: CHANNELS_ENDPOINT, status: 200, detail: 'body is not a channel array' } }
  }
  const channels: HeldChannel[] = []
  for (const value of answered.body) {
    const channel = readHeldChannel(value)
    if (typeof channel === 'string') {
      return { status: 'failed', failure: { kind: 'unreadable', endpoint: CHANNELS_ENDPOINT, status: 200, detail: channel } }
    }
    channels.push(channel)
  }
  return { status: 'ready', channels }
}

function channelFailure(answered: Exclude<Read, { ok: true }>, endpoint: string): ChannelMutation {
  const error = serviceError(answered.body)
  return answered.failure.kind === 'refused' && error
    ? { status: 'refused', refusal: { endpoint, status: answered.failure.status ?? 0, error } }
    : { status: 'failed', failure: answered.failure }
}

async function writeChannel(endpoint: string, method: string, body: unknown, options: LoadOptions): Promise<ChannelMutation> {
  const answered = await read(endpoint, options, {
    method,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!answered.ok) return channelFailure(answered, endpoint)
  const channel = readHeldChannel(answered.body)
  return typeof channel === 'string'
    ? { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: channel } }
    : { status: 'saved', channel }
}

export async function createChannel(connector: string, connection: string, binding: string, events: string[], options: LoadOptions = {}): Promise<ChannelMutation> {
  return writeChannel(CHANNELS_ENDPOINT, 'POST', { connector, connection, binding, events }, options)
}

export async function updateChannel(channel: HeldChannel, connection: string, events: string[], options: LoadOptions = {}): Promise<ChannelMutation> {
  return writeChannel(channelEndpoint(channel.id), 'PUT', { connection, events }, options)
}

export async function removeChannel(channel: HeldChannel, options: LoadOptions = {}): Promise<ChannelMutation> {
  const endpoint = channelEndpoint(channel.id)
  const transport = options.fetch ?? globalThis.fetch
  try {
    const response = await transport(`${options.origin ?? ''}${endpoint}`, { method: 'DELETE' })
    if (response.ok) return { status: 'removed' }
    let body: unknown
    try { body = await response.json() } catch { body = undefined }
    const error = serviceError(body)
    return error
      ? { status: 'refused', refusal: { endpoint, status: response.status, error } }
      : { status: 'failed', failure: { kind: 'refused', endpoint, status: response.status, detail: refusalDetail(body) } }
  } catch (error) {
    return { status: 'failed', failure: { kind: 'unreachable', endpoint, status: null, detail: describe(error) } }
  }
}

// ---------------------------------------------------------------------------------------------
// Workflows and activity. App.vue owns every read; the visual/editor views receive data as props.
// ---------------------------------------------------------------------------------------------

export interface EditorDiagnostic {
  code: string
  message: string
  node_id?: string
  path?: string
  range?: { start: number; end: number }
}

export interface EditorGraph {
  schema_version: number
  name?: string
  params: unknown[]
  returns?: unknown
  body: Array<Record<string, unknown>>
}

export interface WorkflowDraft {
  id: string
  title: string
  revision: number
  published_version: number | null
  source: string
  graph: EditorGraph | null
  diagnostics: EditorDiagnostic[]
  input_schema: Record<string, unknown>
}

export interface EditorOperation {
  id: string
  kind: 'connector' | 'cognition'
  group: string
  description: string
  input_schema: Record<string, unknown>
  risk: string
  idempotency: string
  effects: string[]
  access: unknown[]
}

export interface EditorCatalog {
  schemaVersion: number
  schema: Record<string, unknown>
  operations: EditorOperation[]
}

export interface WorkflowRunEvent {
  sequence: number
  event: {
    node_id: string
    source_path: string
    occurrence: number
    phase: 'entered' | 'branch_selected' | 'succeeded' | 'failed'
    branch?: string
  }
}

export interface WorkflowRun {
  id: string
  workflow_id: string
  version: number
  status: 'running' | 'succeeded' | 'failed' | 'cancelled'
  result: string | null
  error: string | null
  created_at_ms: number
  events: WorkflowRunEvent[]
}

export type WorkflowsState =
  | { status: 'loading' }
  | { status: 'ready'; workflows: WorkflowDraft[] }
  | { status: 'failed'; failure: ServiceFailure }

export type EditorCatalogState =
  | { status: 'loading' }
  | { status: 'ready'; catalog: EditorCatalog }
  | { status: 'failed'; failure: ServiceFailure }

export type ActivityState =
  | { status: 'loading' }
  | { status: 'ready'; runs: WorkflowRun[] }
  | { status: 'failed'; failure: ServiceFailure }

export type WorkflowMutation =
  | { status: 'saved'; workflow: WorkflowDraft }
  | { status: 'published'; version: number }
  | { status: 'started'; run: WorkflowRun }
  | { status: 'validated'; id: string; workflow: Omit<WorkflowDraft, 'id' | 'title' | 'revision' | 'published_version'> }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

function readWorkflow(value: unknown): WorkflowDraft | string {
  if (!isObject(value) || typeof value.id !== 'string' || typeof value.title !== 'string') {
    return 'a workflow has no `id` or `title`'
  }
  if (typeof value.revision !== 'number' || typeof value.source !== 'string') {
    return `workflow ${value.id} has no revision or source`
  }
  return {
    id: value.id,
    title: value.title,
    revision: value.revision,
    published_version: typeof value.published_version === 'number' ? value.published_version : null,
    source: value.source,
    graph: isObject(value.graph) ? value.graph as unknown as EditorGraph : null,
    diagnostics: Array.isArray(value.diagnostics)
      ? value.diagnostics.filter((item): item is EditorDiagnostic => isObject(item) && typeof item.code === 'string' && typeof item.message === 'string')
      : [],
    input_schema: isObject(value.input_schema) ? value.input_schema : {},
  }
}

function readRun(value: unknown): WorkflowRun | string {
  if (!isObject(value) || typeof value.id !== 'string' || typeof value.workflow_id !== 'string') {
    return 'a workflow run has no `id` or `workflow_id`'
  }
  if (typeof value.version !== 'number' || typeof value.status !== 'string') {
    return `workflow run ${value.id} has no version or status`
  }
  const statuses = ['running', 'succeeded', 'failed', 'cancelled'] as const
  if (!statuses.includes(value.status as typeof statuses[number])) return `workflow run ${value.id} has an unknown status`
  const phases = ['entered', 'branch_selected', 'succeeded', 'failed'] as const
  const events: WorkflowRunEvent[] = []
  if (Array.isArray(value.events)) {
    for (const item of value.events) {
      if (!isObject(item) || typeof item.sequence !== 'number' || !isObject(item.event)) continue
      const event = item.event
      if (
        typeof event.node_id !== 'string' ||
        typeof event.source_path !== 'string' ||
        typeof event.occurrence !== 'number' ||
        typeof event.phase !== 'string' ||
        !phases.includes(event.phase as typeof phases[number])
      ) continue
      events.push({
        sequence: item.sequence,
        event: {
          node_id: event.node_id,
          source_path: event.source_path,
          occurrence: event.occurrence,
          phase: event.phase as WorkflowRunEvent['event']['phase'],
          ...(typeof event.branch === 'string' ? { branch: event.branch } : {}),
        },
      })
    }
  }
  return {
    id: value.id,
    workflow_id: value.workflow_id,
    version: value.version,
    status: value.status as WorkflowRun['status'],
    result: typeof value.result === 'string' ? value.result : null,
    error: typeof value.error === 'string' ? value.error : null,
    created_at_ms: typeof value.created_at_ms === 'number' ? value.created_at_ms : 0,
    events,
  }
}

export async function loadWorkflows(options: LoadOptions = {}): Promise<WorkflowsState> {
  const answered = await read(WORKFLOWS_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }
  if (!isObject(answered.body) || !Array.isArray(answered.body.workflows)) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: WORKFLOWS_ENDPOINT, status: 200, detail: 'no `workflows` array' } }
  }
  const workflows: WorkflowDraft[] = []
  for (const value of answered.body.workflows) {
    const workflow = readWorkflow(value)
    if (typeof workflow === 'string') return { status: 'failed', failure: { kind: 'unreadable', endpoint: WORKFLOWS_ENDPOINT, status: 200, detail: workflow } }
    workflows.push(workflow)
  }
  return { status: 'ready', workflows }
}

export async function loadEditorCatalog(options: LoadOptions = {}): Promise<EditorCatalogState> {
  const answered = await read(EDITOR_CATALOG_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }
  if (!isObject(answered.body) || typeof answered.body.schema_version !== 'number' || !Array.isArray(answered.body.operations)) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: EDITOR_CATALOG_ENDPOINT, status: 200, detail: 'no editor schema version or operations' } }
  }
  const operations = answered.body.operations.filter((operation): operation is EditorOperation =>
    isObject(operation) && typeof operation.id === 'string' && (operation.kind === 'connector' || operation.kind === 'cognition'))
  if (operations.length !== answered.body.operations.length) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: EDITOR_CATALOG_ENDPOINT, status: 200, detail: 'an editor operation is unreadable' } }
  }
  return { status: 'ready', catalog: { schemaVersion: answered.body.schema_version, schema: isObject(answered.body.schema) ? answered.body.schema : {}, operations } }
}

export async function loadActivity(options: LoadOptions = {}): Promise<ActivityState> {
  const answered = await read(WORKFLOW_RUNS_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }
  if (!isObject(answered.body) || !Array.isArray(answered.body.runs)) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: WORKFLOW_RUNS_ENDPOINT, status: 200, detail: 'no `runs` array' } }
  }
  const runs: WorkflowRun[] = []
  for (const value of answered.body.runs) {
    const run = readRun(value)
    if (typeof run === 'string') return { status: 'failed', failure: { kind: 'unreadable', endpoint: WORKFLOW_RUNS_ENDPOINT, status: 200, detail: run } }
    runs.push(run)
  }
  return { status: 'ready', runs }
}

// ---------------------------------------------------------------------------------------------
// Installed Apps. App.vue owns these reads and passes completed state into the view.
// ---------------------------------------------------------------------------------------------

export interface AppPackage {
  id: string
  version: string
  integrity: string
  requirements: {
    connections: Array<{ name: string; connector: string }>
    access_layers: Array<{ name: string; connector: string; required: boolean }>
  }
}

export interface InstalledApp {
  id: string
  package: string
  version: string
  activation: string
  review_fingerprint: string
  connections: Record<string, string>
  model_profile: { id: string; provider: string; model: string }
  risk_ceiling: string
  scopes: string[]
}

export interface AppActivity {
  id: string
  session: string
  kind: string
  delivery: string
  outcome: string
  at_ms: number
}

export type AppsState =
  | { status: 'loading' }
  | { status: 'ready'; packages: AppPackage[]; apps: InstalledApp[] }
  | { status: 'failed'; failure: ServiceFailure }

export type AppMutation =
  | { status: 'installed'; app: InstalledApp }
  | { status: 'answered'; reply: string; session: string; activation: string }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

function appFailure(answered: Extract<Read, { ok: false }>): AppMutation {
  return answered.failure.kind === 'refused'
    ? { status: 'refused', refusal: { endpoint: answered.failure.endpoint, status: answered.failure.status ?? 0, error: serviceError(answered.body) ?? answered.failure.detail } }
    : { status: 'failed', failure: answered.failure }
}

export async function loadApps(options: LoadOptions = {}): Promise<AppsState> {
  const [packageAnswer, appAnswer] = await Promise.all([
    read(APP_PACKAGES_ENDPOINT, options),
    read(APPS_ENDPOINT, options),
  ])
  if (!packageAnswer.ok) return { status: 'failed', failure: packageAnswer.failure }
  if (!appAnswer.ok) return { status: 'failed', failure: appAnswer.failure }
  if (!isObject(packageAnswer.body) || !Array.isArray(packageAnswer.body.packages)) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: APP_PACKAGES_ENDPOINT, status: 200, detail: 'no `packages` array' } }
  }
  if (!isObject(appAnswer.body) || !Array.isArray(appAnswer.body.apps)) {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint: APPS_ENDPOINT, status: 200, detail: 'no `apps` array' } }
  }
  return {
    status: 'ready',
    packages: packageAnswer.body.packages.filter((item): item is AppPackage => isObject(item) && typeof item.id === 'string' && typeof item.version === 'string') as AppPackage[],
    apps: appAnswer.body.apps.filter((item): item is InstalledApp => isObject(item) && typeof item.id === 'string' && typeof item.activation === 'string') as InstalledApp[],
  }
}

export interface InstallAppChoice {
  id: string
  package: string
  version: string
  connection: string
  access_layers: string[]
  risk_ceiling: string
  scopes: string[]
  profile: string
  static_reply: string
}

export async function installApp(choice: InstallAppChoice, options: LoadOptions = {}): Promise<AppMutation> {
  const profile = await read(MODEL_PROFILES_ENDPOINT, options, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ id: choice.profile, provider: 'static', model: 'static', revision: 1, static_reply: choice.static_reply }),
  })
  if (!profile.ok) return appFailure(profile)
  const answered = await read(APPS_ENDPOINT, options, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      id: choice.id, package: choice.package, version: choice.version,
      connections: { slack: choice.connection }, model_profile: choice.profile,
      access_layers: choice.access_layers, datasources: {}, risk_ceiling: choice.risk_ceiling,
      scopes: choice.scopes, review: null,
    }),
  })
  if (!answered.ok) return appFailure(answered)
  return { status: 'installed', app: answered.body as InstalledApp }
}

export async function sendAppMessage(app: string, message: string, session: string | null, options: LoadOptions = {}): Promise<AppMutation> {
  const endpoint = appChatEndpoint(app)
  const answered = await read(endpoint, options, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ message, ...(session ? { session } : {}) }),
  })
  if (!answered.ok) return appFailure(answered)
  if (!isObject(answered.body) || typeof answered.body.reply !== 'string' || typeof answered.body.session !== 'string') {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: 'no App reply or session' } }
  }
  return { status: 'answered', reply: answered.body.reply, session: answered.body.session, activation: typeof answered.body.activation === 'string' ? answered.body.activation : '' }
}

export async function loadAppActivity(app: string, options: LoadOptions = {}): Promise<AppActivity[]> {
  const endpoint = appActivityEndpoint(app)
  const answered = await read(endpoint, options)
  if (!answered.ok || !isObject(answered.body) || !Array.isArray(answered.body.activity)) return []
  return answered.body.activity.filter((item): item is AppActivity => isObject(item) && typeof item.id === 'string' && typeof item.outcome === 'string') as AppActivity[]
}

function mutationFailure(answered: Extract<Read, { ok: false }>): WorkflowMutation {
  if (answered.failure.kind === 'refused') {
    return { status: 'refused', refusal: { endpoint: answered.failure.endpoint, status: answered.failure.status ?? 0, error: serviceError(answered.body) ?? answered.failure.detail } }
  }
  return { status: 'failed', failure: answered.failure }
}

async function workflowWrite(endpoint: string, method: string, body: unknown, options: LoadOptions): Promise<Read> {
  return read(endpoint, options, { method, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) })
}

export async function createWorkflow(id: string, title: string, source: string, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const answered = await workflowWrite(WORKFLOWS_ENDPOINT, 'POST', { id, title, edit: { mode: 'source', source } }, options)
  if (!answered.ok) return mutationFailure(answered)
  const workflow = readWorkflow(answered.body)
  return typeof workflow === 'string'
    ? { status: 'failed', failure: { kind: 'unreadable', endpoint: WORKFLOWS_ENDPOINT, status: 201, detail: workflow } }
    : { status: 'saved', workflow }
}

export async function saveWorkflow(workflow: WorkflowDraft, source: string, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const endpoint = workflowEndpoint(workflow.id)
  const answered = await workflowWrite(endpoint, 'PUT', { revision: workflow.revision, title: workflow.title, edit: { mode: 'source', source } }, options)
  if (!answered.ok) return mutationFailure(answered)
  const saved = readWorkflow(answered.body)
  return typeof saved === 'string'
    ? { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: saved } }
    : { status: 'saved', workflow: saved }
}

export async function saveWorkflowGraph(workflow: WorkflowDraft, graph: EditorGraph, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const endpoint = workflowEndpoint(workflow.id)
  const answered = await workflowWrite(endpoint, 'PUT', { revision: workflow.revision, title: workflow.title, edit: { mode: 'graph', graph } }, options)
  if (!answered.ok) return mutationFailure(answered)
  const saved = readWorkflow(answered.body)
  return typeof saved === 'string'
    ? { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: saved } }
    : { status: 'saved', workflow: saved }
}

export async function validateWorkflowEdit(workflow: WorkflowDraft, source: string, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const endpoint = `${workflowEndpoint(workflow.id)}/validate`
  const answered = await workflowWrite(endpoint, 'POST', { edit: { mode: 'source', source } }, options)
  if (!answered.ok) return mutationFailure(answered)
  if (!isObject(answered.body) || typeof answered.body.source !== 'string') {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: 'validation returned no source' } }
  }
  return { status: 'validated', id: workflow.id, workflow: {
    source: answered.body.source,
    graph: isObject(answered.body.graph) ? answered.body.graph as unknown as EditorGraph : null,
    diagnostics: Array.isArray(answered.body.diagnostics) ? answered.body.diagnostics as unknown as EditorDiagnostic[] : [],
    input_schema: isObject(answered.body.input_schema) ? answered.body.input_schema : {},
  } }
}

export async function validateWorkflowGraph(workflow: WorkflowDraft, graph: EditorGraph, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const endpoint = `${workflowEndpoint(workflow.id)}/validate`
  const answered = await workflowWrite(endpoint, 'POST', { edit: { mode: 'graph', graph } }, options)
  if (!answered.ok) return mutationFailure(answered)
  if (!isObject(answered.body) || typeof answered.body.source !== 'string') {
    return { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 200, detail: 'validation returned no source' } }
  }
  return { status: 'validated', id: workflow.id, workflow: {
    source: answered.body.source,
    graph: isObject(answered.body.graph) ? answered.body.graph as unknown as EditorGraph : null,
    diagnostics: Array.isArray(answered.body.diagnostics) ? answered.body.diagnostics as unknown as EditorDiagnostic[] : [],
    input_schema: isObject(answered.body.input_schema) ? answered.body.input_schema : {},
  } }
}

export async function publishWorkflow(workflow: WorkflowDraft, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const endpoint = `${workflowEndpoint(workflow.id)}/publish`
  const answered = await workflowWrite(endpoint, 'POST', { revision: workflow.revision }, options)
  if (!answered.ok) return mutationFailure(answered)
  return isObject(answered.body) && typeof answered.body.version === 'number'
    ? { status: 'published', version: answered.body.version }
    : { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 201, detail: 'publication returned no version' } }
}

export async function startWorkflowRun(workflow: WorkflowDraft, params: unknown, options: LoadOptions = {}): Promise<WorkflowMutation> {
  const endpoint = workflowRunsEndpoint(workflow.id)
  const answered = await workflowWrite(endpoint, 'POST', { params }, options)
  if (!answered.ok) return mutationFailure(answered)
  const run = readRun(answered.body)
  return typeof run === 'string'
    ? { status: 'failed', failure: { kind: 'unreadable', endpoint, status: 202, detail: run } }
    : { status: 'started', run }
}

export async function cancelWorkflowRun(run: string, options: LoadOptions = {}): Promise<ServiceFailure | null> {
  const endpoint = `${workflowRunEndpoint(run)}/cancel`
  const answered = await read(endpoint, options, { method: 'POST' })
  return answered.ok ? null : answered.failure
}

/**
 * What the console knows about a grant that does not exist yet.
 *
 * Four states, and `refused` earns its place for [`ConnectionPlanState`]'s reason: a connector this
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
  const inbound = readInbound(entry.inbound)
  if (typeof inbound === 'string') return inbound

  return {
    connector: entry.connector,
    vendor: typeof entry.vendor === 'string' ? entry.vendor : entry.connector,
    selector: readSelector(entry.selector),
    inbound,
    // Absent reads as **not** expressible. The safe direction: a console that assumed it could write
    // a grant back would compose a set that silently dropped whatever it could not read.
    expressible: entry.expressible === true,
    reason: typeof entry.reason === 'string' ? entry.reason : '',
    exempt: readExempt(entry.exempt),
    declares: typeof entry.declares === 'number' ? entry.declares : 0,
    admits,
  }
}

function readInbound(value: unknown): InboundGrant[] | string {
  // Older hosts omitted this field, and omission means no inbound authority. Once a host sends it,
  // though, every entry must be legible: dropping a malformed binding or event would let the next
  // whole-set write silently revoke authority the operator was never shown.
  if (value === undefined) return []
  if (!Array.isArray(value)) return 'a grant entry has no `inbound` array'
  const inbound: InboundGrant[] = []
  for (const entry of value) {
    if (!isObject(entry) || typeof entry.binding !== 'string' || !Array.isArray(entry.events)) {
      return 'an inbound grant has no `binding` or `events` array'
    }
    if (entry.events.some((event) => typeof event !== 'string')) {
      return `inbound binding \`${entry.binding}\` has a non-string event`
    }
    inbound.push({ binding: entry.binding, events: entry.events as string[] })
  }
  return inbound
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
  const body: Record<string, unknown> = {
    connector: proposed.connector,
    selector: selectorBody(proposed.selector),
  }
  if (proposed.inbound?.length) {
    body.inbound = proposed.inbound.map((grant) => ({
      binding: grant.binding,
      events: grant.events,
    }))
  }
  return body
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
// Service Accounts: the one answer in this console that is readable, valuable and unrepeatable.
//
// `POST /api/service-accounts` mints a Service Account principal for the caller's tenant and hands back its token
// **once**. That is not a convention this console could relax — the server stores a verifier,
// so the host is genuinely unable to say it a second time — and it is why this section is shaped
// differently from everything above it. Every other call here returns something the console may
// keep and re-render: addresses, counts, ids, `held`. A minted token is the one value in this file
// that must not outlive the view that shows it.
//
// There is deliberately no function that reads a token back and no cache of the last mint. Listing
// exposes ids and expiry only; a second way to reach the token would be a second place it could be
// shown twice. `Agents.mts` holds the result in the view's own state and nothing above it sees one.
// ---------------------------------------------------------------------------------------------

/**
 * Where a Service Account principal is minted for the caller's tenant.
 *
 * A collection path with **no parameter**, and there is nowhere a tenant could be spelled into it:
 * the tenant is read from the principal this host resolved and from nothing a caller controls.
 * `routes::service_accounts` carries the other half of this.
 */
export const SERVICE_ACCOUNTS_ENDPOINT = '/api/service-accounts'

/** A Service Account as the management API lists it: identity and expiry, never credential data. */
export interface ServiceAccountSummary {
  id: string
  expiresAt: number
}

export type ServiceAccountsState =
  | { status: 'ready'; accounts: readonly ServiceAccountSummary[] }
  | { status: 'failed'; failure: ServiceFailure }

export type RevokeServiceAccountOutcome =
  | { status: 'revoked' }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

function readServiceAccounts(body: unknown): readonly ServiceAccountSummary[] | string {
  if (!isObject(body) || !Array.isArray(body.service_accounts)) {
    return 'no `service_accounts` array in the body'
  }

  const accounts: ServiceAccountSummary[] = []
  for (const entry of body.service_accounts) {
    if (!isObject(entry) || typeof entry.id !== 'string' || typeof entry.expires_at !== 'number') {
      return 'a Service Account has no string `id` and numeric `expires_at`'
    }
    accounts.push({ id: entry.id, expiresAt: entry.expires_at })
  }
  return accounts
}

/** List this tenant's live Service Accounts without exposing tokens or verifiers. */
export async function loadServiceAccounts(options: LoadOptions = {}): Promise<ServiceAccountsState> {
  const answered = await read(SERVICE_ACCOUNTS_ENDPOINT, options)
  if (!answered.ok) return { status: 'failed', failure: answered.failure }

  const accounts = readServiceAccounts(answered.body)
  if (typeof accounts === 'string') {
    return {
      status: 'failed',
      failure: {
        kind: 'unreadable',
        endpoint: SERVICE_ACCOUNTS_ENDPOINT,
        status: 200,
        detail: accounts,
      },
    }
  }
  return { status: 'ready', accounts }
}

/** Revoke one Service Account in this tenant. The id is a path segment, never a token. */
export async function revokeServiceAccount(
  id: string,
  options: LoadOptions = {}
): Promise<RevokeServiceAccountOutcome> {
  const endpoint = `${SERVICE_ACCOUNTS_ENDPOINT}/${encodeURIComponent(id)}`
  const answered = await read(endpoint, options, { method: 'DELETE' })
  if (answered.ok) return { status: 'revoked' }

  const error = serviceError(answered.body)
  if (error !== null && answered.failure.status !== null) {
    return {
      status: 'refused',
      refusal: { endpoint, status: answered.failure.status, error },
    }
  }
  return { status: 'failed', failure: answered.failure }
}

/** What an operator states when minting. Two fields, and there is no third. */
export interface NewServiceAccount {
  /** What to call the Service Account within this tenant. A name, never an address or tenant. */
  id: string
  /**
   * When its token stops resolving, as seconds since the Unix epoch.
   *
   * **Never defaulted, at either end.** `routes::service_accounts` refuses an omitted value rather than
   * choosing a lifetime for the caller, and a console that quietly filled one in would undo that
   * decision on every mint an operator makes from a browser — which, after this story, is all of
   * them. `Agents.mts` therefore ships an empty box rather than a sensible number.
   */
  expiresAt: number
}

/**
 * A Service Account this host has just minted, as `POST /api/service-accounts` answers with one.
 *
 * `shown` is in the body and is deliberately **not** read. The service says `"shown": "once"`, and a
 * page that only stated the one-shot property when the service remembered to say so would fall
 * silent exactly when the service changed — which is the moment an operator most needs telling.
 * Being shown once is a consequence of what the store holds, so the screen states it from that.
 */
export interface MintedServiceAccount {
  /** The principal that now exists: a Service Account in the minting caller's tenant. */
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
  | { status: 'minted'; minted: MintedServiceAccount }
  | { status: 'refused'; refusal: ServiceRefusal }
  | { status: 'failed'; failure: ServiceFailure }

/** A minted Service Account in a `201` body, or the reason the body is not one. */
function readMinted(body: unknown): MintedServiceAccount | string {
  if (!isObject(body)) return 'the body is not an object'
  const principal = readPrincipal(body)
  if (!principal) return 'no `principal` with an `id` and a `tenant` in the body'
  if (typeof body.token !== 'string' || body.token === '') {
    // Not a mint that produced no token: a `201` this console could not read. Rendering it as a
    // successful mint with an empty token would leave a Service Account existing on this host that the
    // operator has no credential for and no way to discover.
    return 'no `token` in the body'
  }
  if (typeof body.expires_at !== 'number') return 'no `expires_at` in the body'
  return { principal, expiresAt: body.expires_at, token: body.token }
}

/**
 * Mint a Service Account principal for the caller's tenant, and hand back its token once.
 *
 * ```text
 * POST /api/service-accounts   {"id":"ci-runner","expires_at":1793491200}
 * 201                {"principal":{…},"expires_at":1793491200,"token":"…","shown":"once"}
 * ```
 *
 * **The body carries two fields and there is nowhere to put a third.** No tenant — the tenant comes
 * from the principal this host resolved, and `routes::service_accounts` ignores rather than refuses one in
 * the body precisely so that nobody can believe it was honoured. No name for the token, no scope,
 * no grant: none of those exist yet, and inventing a field the service would drop is how a console
 * starts describing a service that is not there.
 *
 * **What this function does not do is the interesting half.** It does not log the token, does not
 * put it in a URL, does not keep it, and has no counterpart that could fetch it later. What it
 * returns is the only copy the console will ever have, and its caller is a view that dies when the
 * reader navigates away.
 */
export async function mintServiceAccount(
  account: NewServiceAccount,
  options: LoadOptions = {}
): Promise<MintOutcome> {
  const answered = await read(SERVICE_ACCOUNTS_ENDPOINT, options, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ id: account.id, expires_at: account.expiresAt }),
  })

  if (!answered.ok) {
    const error = serviceError(answered.body)
    if (error !== null && answered.failure.status !== null) {
      return {
        status: 'refused',
        refusal: { endpoint: SERVICE_ACCOUNTS_ENDPOINT, status: answered.failure.status, error },
      }
    }
    return { status: 'failed', failure: answered.failure }
  }

  const minted = readMinted(answered.body)
  if (typeof minted === 'string') {
    return {
      status: 'failed',
      failure: { kind: 'unreadable', endpoint: SERVICE_ACCOUNTS_ENDPOINT, status: 201, detail: minted },
    }
  }

  return { status: 'minted', minted }
}

// ---------------------------------------------------------------------------------------------
// The adapter: wire spelling to the exchange-owned console spelling.
// ---------------------------------------------------------------------------------------------

/** One wire operation, preserving every distinction the service publishes. */
function adaptOperation(served: ServedOperation): Operation {
  return {
    id: served.id,
    service: served.service,
    description: served.description,
    inputSchema: served.input_schema,
    risk: served.risk,
    idempotency: served.idempotency,
    effects: served.effects,
    effectsDerived: served.effects_derived,
    admitted: served.admitted,
  }
}

/** The complete document, with no placeholder field for facts this endpoint does not carry. */
function adapt(pairs: ServedPair[]): Catalog {
  return {
    connectors: pairs.map(
      (pair): Connector => ({
        id: pair.connector.id,
        vendor: pair.connector.vendor,
        description: pair.connector.description,
        operationCount: pair.connector.operation_count,
        channelCount: pair.connector.channel_count,
        operations: pair.operations.map(adaptOperation),
      })
    ),
  }
}
