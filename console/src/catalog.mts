// The shape of the generated catalogue, and every question the explorer asks of it.
//
// The types mirror `docs/designs/catalog-json.md`, which is the contract: every key is always
// present, an absent value is `null` or `[]`. Nothing here invents a value — no provider, no
// operation and no issue code is named in this file or in any component. Add a fourth provider to
// `providers/*.toml` and the site covers it with no edit.

/** `catalog` | `provider` | `operation` — how far a condition reaches. */
export type Scope = 'catalog' | 'provider' | 'operation'

export interface Issue {
  code: string
  scope: Scope
  summary: string
  params: string[]
}

/**
 * One published fact about an operation that is **not** a reason it fails (C-206).
 *
 * The sibling of {@link Issue}, and separate from it for the reason `works` exists at all: `works`
 * is `issues.length === 0`, so a fact that is not a defect cannot live in that list without making
 * an operation carry a listed reason and claim to work at the same time.
 *
 * It has no `params` and no `story`: a note is not something anyone is going to close.
 */
export interface Note {
  code: string
  scope: Scope
  summary: string
}

export interface Status {
  works: boolean
  issues: Issue[]
  /** Facts that are not defects. `[]` for every operation shipped today — never a missing key. */
  notes: Note[]
}

export interface Parameter {
  name: string
  in: string
  wire: string | null
  description: string
  required: boolean
  schema: Record<string, unknown>
}

/**
 * One API surface of a provider — the unit that is addressed, versioned, selected and installed.
 *
 * A service owns its own `base_url` and `api_version`, defaulting to its provider's, and `gid` is the
 * address a consumer copies. Both `api_version` and `gid` are `null` for a provider that declares
 * neither, which is most of them.
 */
export interface Service {
  name: string
  description: string
  base_url: string
  hosts: string[]
  api_version: string | null
  gid: string | null
  operation_count: number
}

export interface Operation {
  id: string
  provider: string
  /** The service this operation belongs to — exactly one, and every operation has one. */
  service: string
  description: string
  risk: string
  idempotency: string
  method: string
  path: string
  parameters: Parameter[]
  body_schema: Record<string, unknown> | null
  response_schema: Record<string, unknown> | null
  credentials: string[][]
  hosts: string[]
  flux: string
  status: Status
}

export interface Credential {
  name: string
  /**
   * `prefix` is the literal text in front of the credential in a header value, trailing space
   * included — `'Bearer '`, `'SSWS '`, `'Token token='`, or `''` for a header whose whole value is
   * the secret. It is published because without it `Authorization: SSWS <token>` and a raw
   * `Authorization: <token>` are the same two keys (C-184). Never any part of a credential value.
   */
  scheme: { kind: string; name: string | null; prefix: string }
  description: string
  env: string[]
  user_env: string[]
  user_suffix: string | null
  oauth2: boolean
}

export interface Auth {
  schemes: string[]
  credentials: Credential[]
  default: string[][]
}

/** Where on an inbound request one named value is read from. */
export interface FieldSelector {
  source: string
  name: string
}

/**
 * The parameters of one HMAC webhook signature, in the vendor's own terms.
 *
 * `secret` is a **credential name** — resolve it against `provider.auth.credentials[].name`, where
 * it is declared with `scheme: 'signing'`. No value for it is anywhere in this document.
 */
export interface Hmac {
  algorithm: string
  encoding: string
  header: string
  prefix: string | null
  signed: string
  timestamp: FieldSelector | null
  /**
   * How the signed timestamp is **spelled** — `'unix_seconds'` or `'rfc3339'` — and `null` for a
   * scheme that signs none. A separate axis from `timestamp`, which says only where it is read from.
   */
  timestamp_format: string | null
  secret: string
  tolerance: string | null
}

/**
 * How a delivery on one binding proves it came from the vendor.
 *
 * `kind` is always present and `verified` is `kind !== 'none'` restated, which is the pair that
 * matters: a consumer tells a deliberately-unverifiable surface from a verified one by reading a
 * value, never by noticing that a key is missing.
 */
export interface Verification {
  kind: string
  verified: boolean
  hmac: Hmac | null
}

/** Which operation answers an event on a binding, and how its parameters are filled. */
export interface Reply {
  operation: string
  oip: string | null
  result: string | null
  bind: Record<string, string>
}

/** How a webhook is registered with the vendor through its own API. */
export interface Subscription {
  subscribe: string
  unsubscribe: string | null
  list: string | null
  callback_param: string
}

/** What a human does in the vendor's dashboard, for vendors with no subscription API. */
export interface ManualSetup {
  docs_url: string | null
  steps: string[]
}

/** One event a vendor sends, in the vendor's own spelling. */
export interface InboundEvent {
  name: string
  service: string
  oip: string | null
  description: string
  default: boolean
  group: string
  when: Record<string, unknown>
  schema: Record<string, unknown> | null
}

/**
 * One ingress surface a connector describes: a transport, the events it carries, how a delivery is
 * proven, and what answers it.
 *
 * Note what is *not* here, and cannot be: no URL, no secret, no schedule. The endpoint is the
 * operator's deployment detail, the secret is a credential name a host resolves, and the loop a poll
 * runs on belongs to an operator's own program.
 */
export interface Channel {
  name: string
  service: string
  oip: string | null
  description: string
  transport: string
  events: string[]
  verification: Verification
  discriminator: FieldSelector | null
  delivery_id: FieldSelector | null
  payload: Record<string, string>
  reply: Reply | null
  cursor: string | null
  interval: string | null
  subscription: Subscription | null
  setup: ManualSetup | null
}

export interface Provider {
  id: string
  authority: string | null
  vendor: string
  description: string
  base_url: string
  api_version: string | null
  hosts: string[]
  services: Service[]
  auth: Auth
  operation_count: number
  operations: Operation[]
  /** The events this connector receives — the inbound half of its surface. `[]` for most. */
  events: InboundEvent[]
  /** The ingress surfaces it describes. `[]` for a connector that declares none. */
  channels: Channel[]
}

export type CoreAvailability = 'available' | 'planned'

export interface ToolSpec {
  name: string
  description: string
  input_schema: Record<string, unknown>
  effects: string[]
  risk: string
  idempotency: string
  access: string[]
  group?: string
}

export interface CoreEntryBase {
  $schema: string
  $id: string
  schema_version: number
  name: string
  title: string
  description: string
  category: string[]
  availability: CoreAvailability
}

export interface CoreOperation extends CoreEntryBase {
  kind: 'operation'
  tool_spec: ToolSpec
}

export interface CoreNode extends CoreEntryBase {
  kind: 'node'
  schema_ref: string
}

export interface CoreCapability extends CoreEntryBase {
  kind: 'capability'
  callable: boolean
  operation_ids: string[]
}

export type CoreEntry = CoreOperation | CoreNode | CoreCapability

export interface CoreSchemas {
  catalog: Record<string, unknown>
  entry: Record<string, unknown>
  flux_ast: Record<string, unknown>
}

export interface CoreCatalog {
  $schema: string
  $id: string
  schema_version: number
  generator: string
  operations: CoreOperation[]
  nodes: CoreNode[]
  capabilities: CoreCapability[]
  schemas: CoreSchemas
}

export interface Catalog {
  schema_version: number
  generator: string
  providers: Provider[]
  core: CoreCatalog | null
}

/** Every Flux-owned core entry in its declared kind order. */
export function allCoreEntries(core: CoreCatalog): CoreEntry[] {
  return [...core.operations, ...core.nodes, ...core.capabilities]
}

/** The stable explorer page for a Flux-owned core entry. */
export function coreEntryHref(entry: CoreEntry): string {
  const section = entry.kind === 'capability' ? 'capabilities' : `${entry.kind}s`
  return `/core/${section}/${encodeURIComponent(entry.name)}`
}

/** Resolve a canonical core specification id to the entry that owns it. */
export function coreEntryById(core: CoreCatalog, id: string): CoreEntry | undefined {
  return allCoreEntries(core).find((entry) => entry.$id === id)
}

/**
 * **The distinction the whole explorer turns on.**
 *
 * `works` is `false` for all 25 operations today and that is correct — no provider can make a live
 * call yet, because the auth seam has not landed in flux. Rendering that as "0 of 25 working" would
 * be accurate and useless, and it would tar 20 operations that are working exactly as designed.
 *
 * `scope` is what separates the two: an issue an operation *owns* is a defect in that operation, and
 * an issue scoped to its provider or to the catalogue is a condition it merely inherits along with
 * everything else. The first is a badge on the operation; the second is a banner over the set.
 */
export function ownIssues(operation: Operation): Issue[] {
  return operation.status.issues.filter((issue) => issue.scope === 'operation')
}

/**
 * The facts about an operation that are not defects.
 *
 * **The distinction this exists for.** An operation with no credentials is in one of two opposite
 * situations, and until C-206 the catalogue published one sentence for both: a credential is being
 * withheld because it cannot be held safely yet — the reader has to wait — or the vendor requires
 * none at all, and the reader has nothing to do. The first is an issue and the second is a note, so
 * a page can finally tell a visitor which one they are looking at.
 *
 * Nothing here names a code. A note carries the sentence it should be shown as, exactly as an issue
 * does, and this file has never known what any of them are called.
 *
 * **The `??` is a build-order tolerance, not a contract.** The catalogue always publishes the key —
 * that is the guarantee this file is typed against, and `notes: Note[]` above is not optional. But
 * `public/catalog.json` is a *committed* whole-catalogue artifact regenerated by a full build, so
 * between an emitter change and that build the site is reading a document older than itself. Every
 * such field is in the same position; falling back here keeps the site buildable for that window
 * instead of failing on 242 operations at once, and it is the only place the absence is considered.
 */
export function notes(operation: Operation): Note[] {
  return operation.status.notes ?? []
}

/** The conditions an operation inherits from its provider or from the catalogue as a whole. */
export function inheritedIssues(operation: Operation): Issue[] {
  return operation.status.issues.filter((issue) => issue.scope !== 'operation')
}

/** Whether this operation has a problem nothing else in the catalogue has. */
export function ownsDefect(operation: Operation): boolean {
  return ownIssues(operation).length > 0
}

/** Every operation, flattened across providers, in the order the catalogue declares them. */
export function allOperations(catalog: Catalog): Operation[] {
  return catalog.providers.flatMap((provider) => provider.operations)
}

/** One issue per `code`, keeping the first occurrence — a shared condition is stated once. */
export function distinct(issues: Issue[]): Issue[] {
  const seen = new Map<string, Issue>()
  for (const issue of issues) if (!seen.has(issue.code)) seen.set(issue.code, issue)
  return [...seen.values()]
}

/** The conditions that reach the whole catalogue, stated once. */
export function catalogIssues(catalog: Catalog): Issue[] {
  return distinct(
    allOperations(catalog)
      .flatMap((operation) => operation.status.issues)
      .filter((issue) => issue.scope === 'catalog')
  )
}

/** The conditions that reach every operation of one provider, stated once. */
export function providerIssues(provider: Provider): Issue[] {
  return distinct(
    provider.operations
      .flatMap((operation) => operation.status.issues)
      .filter((issue) => issue.scope === 'provider')
  )
}

/** How many operations own a defect — the only headline count worth showing. */
export function defectCount(operations: Operation[]): number {
  return operations.filter(ownsDefect).length
}

/**
 * The operation's signature, taken from the first line of its generated Flux.
 *
 * Reassembling `name(param: Type, …) -> Any` from the parameter list would be a second, subtly
 * different renderer for something the emitter already decided. This is the emitter's own answer.
 */
export function signature(operation: Operation): string {
  return operation.flux.split('\n', 1)[0]
}

/** The stable, deep-linkable URL of one operation, relative to the site root. */
export function operationHref(operation: Operation): string {
  return `/operations/${operation.id}`
}

// ---------------------------------------------------------------------------------------------
// Where a path becomes an href.
//
// The two functions above answer *which page*, and that answer is the catalogue's — it is the same
// wherever these components are mounted. Turning `/operations/<id>` into an href a browser can follow
// is the **host's** answer, and it differs: this site is served under a base path, so VitePress has
// `withBase`; another host has its own router, or none.
//
// Importing `withBase` made five components answer a question they do not own, and it is the whole of
// what tied them to VitePress (C-142). So it is a port: injected, with an identity default, because
// the honest behaviour for a host that says nothing is to leave the path alone.
//
// The key is a plain string rather than a Vue `InjectionKey`, deliberately. This file has no imports
// and that is a property worth keeping — it is what makes the data contract portable, and it is what
// lets the build-time `paths.mts` loaders read it under plain Node. Callers type the injection at the
// `inject` site instead.
// ---------------------------------------------------------------------------------------------

/** How a host turns a site-root-relative path into an href. */
export type PathResolver = (path: string) => string

/** The injection key a host provides its own {@link PathResolver} under. */
export const PATH_RESOLVER = 'flux-connectors:path-resolver'

/** The default resolver: a host that says nothing leaves the path exactly as the catalogue gave it. */
export const identityPath: PathResolver = (path) => path

/**
 * A short label for a parameter's type, from the vendor's JSON Schema.
 *
 * A label only — the schema itself is always shown verbatim alongside it, because the constraint
 * keywords a label drops (`format`, `enum`, `minimum`) are often the ones a caller needs.
 */
export function schemaType(schema: Record<string, unknown>): string {
  const alternatives = (schema.oneOf ?? schema.anyOf) as Record<string, unknown>[] | undefined
  if (Array.isArray(alternatives)) return alternatives.map(schemaType).join(' | ')

  const type = schema.type
  if (Array.isArray(type)) return type.join(' | ')
  if (typeof type === 'string') {
    if (type === 'array') {
      const items = schema.items as Record<string, unknown> | undefined
      return items ? `array<${schemaType(items)}>` : 'array'
    }
    return type
  }
  return 'any'
}

/** The distinct values of one operation facet, in the order the catalogue declares them. */
export function facet(operations: Operation[], pick: (operation: Operation) => string): string[] {
  return [...new Set(operations.map(pick))]
}

/**
 * The reserved service name — the only name in this file that is not read out of the catalogue,
 * because it is not catalogue data.
 *
 * It is vocabulary from the address grammar: an operation naming no service belongs to it, no
 * provider may declare it, and it is elided from every published address and every file name. The
 * consequence this site has to honour is that it is never rendered — a card listing it, or a filter
 * offering it, would name something no address contains.
 */
const RESERVED_SERVICE = 'default'

/** The services a provider publishes under a name of their own, in catalogue order. */
export function namedServices(provider: Provider): Service[] {
  return provider.services.filter((service) => service.name !== RESERVED_SERVICE)
}

/**
 * The service options a visitor can choose from, narrowed to one connector when one is chosen.
 *
 * Dependent in one direction only, which is the obvious one: choosing a connector narrows the
 * services to that connector's, and choosing a service with no connector chosen stays valid. A
 * connector that addresses a single surface offers nothing here — its one service is the reserved
 * one, and it names no address to filter on.
 */
export function serviceFacet(providers: Provider[], provider = ''): string[] {
  const scope = provider ? providers.filter((owner) => owner.id === provider) : providers
  return [...new Set(scope.flatMap((owner) => namedServices(owner).map((service) => service.name)))]
}

/**
 * The service an operation should state, or `null` when its connector addresses a single surface.
 *
 * Naming the service of a connector that has only one would repeat what its card already says, and
 * for the reserved one it would name something the address elides.
 */
export function operationService(provider: Provider, operation: Operation): string | null {
  return namedServices(provider).length > 1 ? operation.service : null
}

/**
 * The `api_version` worth showing for a service: `null` when it is its connector's own.
 *
 * A service inherits its connector's version unless it overrides it, so repeating the inherited
 * value on every service would bury the one that actually differs.
 */
export function serviceApiVersion(provider: Provider, service: Service): string | null {
  return service.api_version === provider.api_version ? null : service.api_version
}

/**
 * The address a consumer copies for a connector that addresses a single surface, or `null`.
 *
 * `gid` is a property of a service, and for such a connector that service is the reserved one —
 * whose name the address elides, which is exactly why the value reads as the connector's own rather
 * than as a service's. It is `null` for every connector that declares no authority, and a null
 * address renders as nothing at all: a placeholder would put a value on the page that the catalogue
 * does not publish and no consumer could copy.
 */
export function providerAddress(provider: Provider): string | null {
  const reserved = provider.services.find((service) => service.name === RESERVED_SERVICE)
  return reserved?.gid ?? null
}

// ---------------------------------------------------------------------------------------------
// The inbound surface (C-83).
//
// An operation is flux calling the vendor; an event is the vendor calling flux, and a channel
// binding is the composition of the two. All three are members of a service, and until C-83 only the
// first reached an artifact — so the site could describe half of what a connector does.
//
// A binding is not an operation and is deliberately not rendered as one. It declares and never
// installs: it names no URL, no schedule and no secret, and it is emitted into no Flux module at all.
// What a visitor needs from it is what it listens for, how a delivery is proven, and what answers.
// ---------------------------------------------------------------------------------------------

/**
 * What each verification kind is called on the page.
 *
 * Site vocabulary rather than catalogue data, the same way `RISK_ORDER` and the defect filter's
 * choices are: the catalogue publishes a stable machine token and has no opinion about the English
 * for it. A kind this build has not heard of falls through to the token itself — showing the raw
 * value is honest, whereas a generic label would present an unknown scheme as though it were
 * understood.
 */
const VERIFICATION_LABELS: Record<string, string> = {
  hmac: 'Signed',
  none: 'Unverified',
  connection: 'Authenticated by connection',
}

/** How this binding's verification reads on the page. */
export function verificationLabel(channel: Channel): string {
  return VERIFICATION_LABELS[channel.verification.kind] ?? channel.verification.kind
}

/** Whether this connector describes anything at all on the inbound side. */
export function hasInboundSurface(provider: Provider): boolean {
  return provider.events.length > 0 || provider.channels.length > 0
}

/**
 * The declared events one binding carries, in the binding's own order.
 *
 * A binding names its events and the connector declares them, so this is the join — and it is here
 * rather than in a component because resolving a name against a list is exactly the kind of thing a
 * test should be able to pin. A name with no declaration is dropped rather than rendered as a stub:
 * the loader refuses one, so its appearance would mean the document is malformed, and inventing a
 * placeholder event would put a capability on the page that the connector does not have.
 */
export function channelEvents(provider: Provider, channel: Channel): InboundEvent[] {
  return channel.events
    .map((name) => provider.events.find((event) => event.name === name))
    .filter((event): event is InboundEvent => event !== undefined)
}

/**
 * The bindings a visitor has to be told about: the ones a delivery cannot be attributed on.
 *
 * Read off `verified`, which the catalogue publishes as a value on every binding — never off whether
 * an `hmac` block happens to be absent. That distinction is the whole point of the field: a consumer
 * that tested for absence would read "nothing arrives unsolicited over this socket" and "anyone can
 * POST to this endpoint" as the same thing.
 */
export function unverifiedChannels(provider: Provider): Channel[] {
  return provider.channels.filter((channel) => !channel.verification.verified)
}

/**
 * How a binding's reply is worth showing: its rendered address when the connector publishes one,
 * else the local operation id.
 *
 * The oip is the form that survives leaving the repository, so it is preferred; the id is what a host
 * resolves against the connector's own operations and is the honest fallback for the connectors that
 * declare no authority.
 */
export function replyAddress(channel: Channel): string | null {
  return channel.reply ? (channel.reply.oip ?? channel.reply.operation) : null
}

// ---------------------------------------------------------------------------------------------
// The shareable view.
//
// The explorer promises a stable page per operation. A *view* — "every destructive operation of one
// connector" — had no address at all, because filtering was component state. Putting that state in
// the query string is what makes the promise true of a view, and it is here rather than in the
// component because a serialiser is exactly the kind of thing that should be provable by a test.
// ---------------------------------------------------------------------------------------------

/**
 * The complete filter and sort state of the operation list.
 *
 * Every field is a string, and the empty string means *unset* — not "any" as a magic value, but the
 * absence of a constraint, which is what a missing query parameter means too. That correspondence is
 * why the pair below is as short as it is.
 */
export interface View {
  query: string
  provider: string
  service: string
  risk: string
  idempotency: string
  defect: string
  sort: string
}

/**
 * The orders the list can be shown in.
 *
 * `catalog` is the default and it is not a fallback: it is the order the emitter writes operations
 * into the generated module, so it is the order a reader of the Flux sees. The other two are the
 * re-orderings worth offering over a list this long.
 */
export const SORTS = ['catalog', 'id', 'risk']

/**
 * Risk tiers from least to most consequential.
 *
 * The one ordering the catalogue cannot supply — JSON carries the tier of each operation and no
 * sense of which tier is worse. It has to be declared, and declaring it is the whole point:
 * alphabetically the most consequential tier sorts second of four, which is wrong without ever
 * looking wrong.
 */
export const RISK_ORDER = ['low', 'medium', 'high', 'destructive']

/** The choices the defect filter offers, which are vocabulary of this site and not catalogue data. */
const DEFECTS = ['own', 'none']

/**
 * The query-string key for each field, in the order they are written.
 *
 * Both directions read this one table, so a key cannot be spelled one way when written and another
 * way when read, and the order here is what makes the encoding canonical. The keys are what a
 * visitor sees on the controls rather than what the code calls them — a URL is read and edited by
 * people, and the control is called *Connector*.
 */
const VIEW_KEYS: [keyof View, string][] = [
  ['query', 'q'],
  ['provider', 'connector'],
  ['service', 'service'],
  ['risk', 'risk'],
  ['idempotency', 'idempotency'],
  ['defect', 'defect'],
  ['sort', 'sort'],
]

/** The unfiltered view: no constraint anywhere, and the catalogue's own order. */
export function emptyView(): View {
  return {
    query: '',
    provider: '',
    service: '',
    risk: '',
    idempotency: '',
    defect: '',
    sort: SORTS[0],
  }
}

/**
 * A view as a query string, without the leading `?`.
 *
 * An unset field contributes no parameter and the default sort is an unset field, so the unfiltered
 * view is the empty string and the unfiltered URL is clean. Two routes to the same view produce the
 * same string, because the key order is the table's and not the caller's.
 */
export function encodeView(view: View): string {
  const unset = emptyView()
  const params = new URLSearchParams()
  for (const [field, key] of VIEW_KEYS) {
    const value = String(view[field] ?? '').trim()
    if (value && value !== unset[field]) params.set(key, value)
  }
  return params.toString()
}

/**
 * The view a query string names, with anything unrecognised ignored rather than fatal.
 *
 * A link outlives the page it was copied from, so a parameter this build has never heard of is
 * dropped and a field is only accepted when its value is one this file owns the vocabulary for —
 * the sort and the defect filter. The catalogue's own vocabularies (connector, service, risk,
 * idempotency) cannot be checked here, because a pure parse has no catalogue; `narrowView` is the
 * other half.
 */
export function decodeView(search: string): View {
  const params = new URLSearchParams(search)
  const view = emptyView()

  for (const [field, key] of VIEW_KEYS) {
    const value = (params.get(key) ?? '').trim()
    if (value) view[field] = value
  }

  if (!SORTS.includes(view.sort)) view.sort = SORTS[0]
  if (view.defect && !DEFECTS.includes(view.defect)) view.defect = ''

  return view
}

/**
 * The view narrowed to what this catalogue actually offers.
 *
 * The second half of *ignored, not fatal*. A shared link naming a connector that has since been
 * renamed, or a service its connector no longer publishes, must degrade to a **wider** view — a
 * value nothing can match would render an empty catalogue and read as an outage. Dropping the
 * constraint shows more than was asked for, which is the honest failure of the two.
 *
 * It also carries the service filter's one dependency: a service is only kept while the chosen
 * connector still publishes it, so changing connector drops a service that connector never had.
 */
export function narrowView(view: View, providers: Provider[]): View {
  const operations = providers.flatMap((owner) => owner.operations)
  const offered = (value: string, options: string[]) => (options.includes(value) ? value : '')

  const provider = offered(view.provider, providers.map((owner) => owner.id))

  return {
    ...view,
    provider,
    service: offered(view.service, serviceFacet(providers, provider)),
    risk: offered(view.risk, facet(operations, (operation) => operation.risk)),
    idempotency: offered(view.idempotency, facet(operations, (operation) => operation.idempotency)),
  }
}

/** Where a risk tier ranks, with a tier this build has not heard of ranked after every one it has. */
function riskRank(operation: Operation): number {
  const rank = RISK_ORDER.indexOf(operation.risk)
  return rank === -1 ? RISK_ORDER.length : rank
}

/**
 * How two operations compare under one sort.
 *
 * The default compares everything as equal, which is not a stub: `Array.prototype.sort` is stable,
 * so a comparator that separates nothing leaves the catalogue's own order exactly as it was. The
 * same stability is what makes catalogue order the tiebreaker inside a risk tier, so choosing risk
 * groups the list without discarding the order underneath it.
 *
 * Ids are compared by code point rather than by locale: the site is one document and the order it
 * shows must not depend on the reader's browser.
 */
export function compareOperations(sort: string): (a: Operation, b: Operation) => number {
  if (sort === 'id') return (a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
  if (sort === 'risk') return (a, b) => riskRank(a) - riskRank(b)
  return () => 0
}

/** The operations in one sort's order, as a new list — the caller's own is left alone. */
export function sortOperations(operations: Operation[], sort: string): Operation[] {
  return [...operations].sort(compareOperations(sort))
}
