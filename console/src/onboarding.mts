// What this service can tell an agent, derived from what the console already says it is.
//
// **Why this file exists at all.** `docs/vision.md` calls the agent this platform's *primary*
// caller — people sign in to wire things up, agents call operations all day — and until this page
// existed nothing anywhere answered "what is this, and how do I connect to it?" for one. Everything
// built so far serves the other caller.
//
// **Why it is data rather than a paragraph.** The state being described changes weekly. Service
// Account authentication and invocation are live now; Apps and hosted Agents are still target
// architecture. Prose describing a particular build goes stale within a release, and principle 7 of
// the vision — restated in `AGENTS.md` — makes a page implying a working service cost more than an
// honest gap. So the page does not describe this build. It **derives** from `surfaces.mts`, the same
// declaration `ConsoleShell.mts` builds the navigation from, and the derivation is the honesty:
//
//   > **Nothing here may claim to work unless a surface the console declares the service `served`
//   > backs it.**
//
// **That used to say `built`, and saying `built` was wrong.** `built` is a claim about console
// navigation — is there a screen — and this page is answering a question about the API. The two
// coincided for every surface until `POST /api/operations/{operation}/invoke` shipped in v0.7.0
// with no screen behind it, at which point deriving from `built` made this page tell an agent
// author that nothing in the deployment can be called while the route sat in the published surface.
// X-42 caught it by measuring against the route table; `surfaces.mts` now answers the two questions
// separately, and `routes::onboarding::tests` in the server crate is what holds `served` to the
// route table rather than to whoever last edited it.
//
// The guard is still deliberately one-directional. A served surface is not proof that a *particular*
// route exists — `identity` being served does not by itself mean `POST /api/service-accounts` does. What the
// rule buys is the direction that matters: it can only ever take a claim *off* the page, never put
// one on. A surface regressing to unserved silently withdraws every step standing on it, and a step
// no surface backs at all cannot be claimed however confidently it is written.
//
// `authenticate` stands on the identity surface. `GET /api/session` is both the smallest bearer
// probe and the route that returns the resolved principal, so the page can describe authentication
// without inventing a second identity endpoint.
//
// This module is data and pure functions. It imports `surfaces.mts` and **nothing else** — not Vue,
// not `service.mts`, which is the only module in this console that knows a network exists. That is
// what lets `test/onboarding.test.mjs` assert the page reads nothing and could not have.

import { SURFACES, type Surface } from './surfaces.mts'

/**
 * Where the onboarding page lives, as a catalogue-style path the fragment router resolves.
 *
 * `/connect` and not `/service-accounts`: this is a reference for an App, Agent or automation that
 * needs to connect. The Service Account collection is tenant management state; this page holds none.
 */
export const ONBOARDING_PATH = '/connect'

/**
 * Where the service serves the machine rendering of everything on this page (X-42).
 *
 * A route on the **service**, not a path in this console: a page is a human artifact and the
 * charter's primary caller does not read pages. It is named here rather than in `service.mts`
 * because this module is the one both renderings derive from, and because this console never
 * fetches it — the page mentions it, so an agent author reading the page learns their agent does
 * not have to. `crates/exchange-server/src/routes/onboarding.rs` publishes it, and
 * `the_endpoint_the_document_names_is_the_route_that_serves_it` there is what stops the two
 * spellings drifting.
 */
export const DESCRIPTOR_ENDPOINT = '/api/onboarding'

/**
 * What this service is, in the one form both renderings say it in.
 *
 * It lived in `AgentOnboarding.mts` until X-42 needed the same sentence on the wire. Copy that two
 * renderings each hold their own version of is copy that disagrees within a release, which is the
 * failure `docs/designs/agent-onboarding.md` names explicitly — so the layout file renders this
 * and states nothing of its own.
 */
export const SERVICE = {
  /** The one name this service is published under. */
  name: 'flux-exchange',
  /** What it does, in `docs/vision.md`'s own words. A standalone sentence, so it reads as one. */
  summary:
    'Holds credentials, terminates channels, runs operations for many callers and records what ' +
    'happened. Its primary caller is an agent, not a human: people sign in to wire things up, ' +
    'and agents are what call operations all day.',
}

/**
 * How a caller presents a token to this service.
 *
 * **The scheme is a fact about the service; whether presenting one identifies you is not.** The
 * guard reads `Authorization: Bearer <material>` today and has since X-03 — that is true of this
 * host now, and stating it costs a stranger nothing they could not learn by sending a request.
 * What is *not* true is that an agent's token resolves to a principal, and this constant
 * deliberately does not say either way: [`capability`](AUTHENTICATION#capability) names the step
 * that owns that question, so the gap is stated once, where the surface it stands on can withdraw
 * it.
 */
export const AUTHENTICATION = {
  /** The `Authorization` scheme, spelled as a caller sends it. */
  scheme: 'Bearer',
  /** The header it travels in. */
  header: 'Authorization',
  /** The [`Step`] id whose availability decides whether presenting one identifies a caller. */
  capability: 'authenticate',
}

/**
 * The one call a step is done by.
 *
 * `caller` is separate from `note` because it is the field an agent author gets wrong: minting is a
 * call a *human* makes, and reading it as something the agent does leaves them looking for a token
 * they cannot obtain. `warn` is the sentence that must not be skimmed — rendered emphasised, and
 * empty for every step that has nothing irreversible in it.
 */
export interface Call {
  /** The HTTP method, spelled as a caller would send it. */
  method: string
  /**
   * The route, origin-relative, in the route table's own spelling.
   *
   * **A parameter here may only be a catalogue key.** The rule was "parameterless, always", which
   * was the right instinct written one notch too tight: what must never appear is a *value* — a
   * tenant, a principal, a credential, an address — because this page states the shape of the
   * service and never its contents. `{operation}` is none of those; it is the public vocabulary the
   * anonymous catalogue already publishes, and refusing it would mean either omitting the one route
   * that runs an operation or describing it in prose to dodge a test.
   *
   * `nothing_tenant_specific_can_reach_this_page` holds the corrected rule over every entry here,
   * against a written-out list of the keys that are admissible, so admitting a second one is a
   * decision somebody makes rather than a regex that already allows it. The server states the same
   * invariant from the other side and over the whole surface, in
   * `routes::tests::no_published_route_takes_a_tenant_in_its_path`.
   */
  endpoint: string
  /** Who makes this call, and with what standing. */
  caller: string
  /** What to send and what comes back. */
  note: string
  /** The one thing a reader must not skim past, or empty when there is none. */
  warn: string
}

/**
 * One thing an agent author needs to know how to do, and whether this build lets them.
 *
 * The fields are ordered the way a reader meets them: what it is called, what it is for, what backs
 * it, how to do it, and — when nothing backs it — why not.
 */
export interface Step {
  /** Stable id. Also the `data-step` attribute the page renders and the tests address it by. */
  id: string
  /** What it is called on the page. */
  title: string
  /** What it is, in the vocabulary the vision uses. Stated as a capability, never as a promise. */
  summary: string
  /**
   * The [`Surface`] id this step stands on, or `null` when the console declares no surface for it.
   *
   * `null` is not a gap to be filled in later for its own sake — see the module note on
   * `authenticate`. It means this step cannot be claimed, and that is a state the model has to be
   * able to express, because the platform genuinely has capabilities the console has no place for.
   */
  surface: string | null
  /**
   * How to do it. **`null` unless a built surface backs it**, and asserted both ways.
   *
   * Withheld steps carry none because an endpoint printed on this page is an invitation to call it,
   * and there is nothing to call. Available steps must carry one, so the day a surface flips to
   * `built` the tests go red until somebody writes down how — a "you can now" with no instruction
   * under it is the same unhelpful gap in a friendlier shape.
   */
  call: Call | null
  /**
   * Why it cannot be done, **only** for a step no surface backs.
   *
   * A step that has a surface takes the surface's own `absent` instead, so the page and the
   * navigation cannot say different things about the same gap. Empty otherwise, and asserted.
   */
  pending: string
}

/**
 * Everything an agent author has to know, in the order it happens to them.
 *
 * **The order is the argument.** Reading the catalogue first, because it is the one thing that works
 * with no credential at all and it answers "is there anything here I want?" before anything else.
 * Then identity, the human-owned setup surfaces, the two remote-binding verbs and the durable
 * workflow record. Planned Agent and Lease capabilities stay in this same model so the public
 * surface can name the intended shape without turning prose into a promise.
 */
export const STEPS: readonly Step[] = [
  {
    id: 'read-the-catalogue',
    title: 'Read what this build could run',
    summary:
      'Every connector this deployment carries and every operation each one declares — what it is, ' +
      'what it costs, and what effects it has.',
    surface: 'catalogue',
    call: {
      method: 'GET',
      endpoint: '/api/catalogue/connectors',
      caller: 'Anyone. This route resolves no principal and takes no token.',
      note:
        'One connector per entry, with the count of operations it declares; the operations ' +
        'themselves are at the same path under the connector id, at ' +
        '/api/catalogue/connectors/{id}/operations. It answers what exists, not what you may ' +
        'call — with no principal resolved every operation comes back admitted: null, which is a ' +
        'third value and not a refusal.',
      warn: '',
    },
    pending: '',
  },
  {
    id: 'create-service-account',
    title: 'Create a Service Account',
    summary:
      'A Service Account is a non-human principal of exactly one tenant. A signed-in human creates ' +
      'it and hands its one-time token to an App, Agent or automation.',
    surface: 'identity',
    call: {
      method: 'POST',
      endpoint: '/api/service-accounts',
      caller:
        'A signed-in human, from this console. Not the App or Agent that will use it — creating ' +
        'non-human identities is an operator action.',
      note:
        'Send id, what to call the Service Account within the tenant, and expires_at, seconds since the Unix ' +
        'epoch. The expiry is never defaulted: a body without one is refused rather than given a ' +
        'lifetime this host picked. There is no tenant field, and there is nowhere one could be ' +
        'put — the tenant is read from the caller this host resolved, and a tenant in the body is ' +
        'ignored rather than honoured.',
      warn:
        'The response carries the token, and it is shown once. This host keeps a verifier rather ' +
        'than the token, so nothing here can show it to you again. Lose it and mint another.',
    },
    pending: '',
  },
  {
    id: 'authenticate',
    title: 'Authenticate with a Service Account',
    summary:
      'Present the one-time token as a bearer credential and have this host resolve the Service ' +
      'Account in its tenant.',
    surface: 'identity',
    call: {
      method: 'GET',
      endpoint: '/api/session',
      caller: 'The App, Agent or automation holding the Service Account token.',
      note:
        'Send Authorization: Bearer <token>. A successful response identifies the principal as ' +
        'kind service_account and carries no credential material.',
      warn:
        'A Service Account receives no authority merely by authenticating. Tenant grants still ' +
        'decide which declared operations and channels it may use.',
    },
    pending: '',
  },
  {
    id: 'discover-effective-catalogue',
    title: 'Discover usable operations',
    summary:
      'Read exactly the connected and granted operation bindings available to this resolved ' +
      'Service Account, with a stable generation for turn-boundary refresh.',
    surface: 'catalogue',
    call: {
      method: 'GET',
      endpoint: '/api/catalogue/effective',
      caller: 'A Service Account presenting its bearer token.',
      note:
        'The response contains only operation declarations and existing tenant-local connection ' +
        'labels that this principal can invoke. Its generation is content-addressed: retain the ' +
        'tool projection while it is unchanged and refresh only between turns when it changes.',
      warn:
        'This is not the anonymous full catalogue or a connection-management surface. It accepts ' +
        'no tenant, credential, endpoint, runtime or authority selector.',
    },
    pending: '',
  },
  {
    id: 'connections',
    title: 'Manage connections and credentials',
    summary:
      'Inspect and wire this tenant’s connector instances while credential values remain ' +
      'write-only and every address is derived from the resolved principal’s tenant.',
    surface: 'connections',
    call: {
      method: 'GET',
      endpoint: '/api/connections',
      caller:
        'An operator authorized for this tenant. Service Accounts and other automation do not ' +
        'receive connection-management authority.',
      note:
        'The collection returns connector instances, labels, declared credential addresses and ' +
        'whether each credential is held, plus value-free supplier evidence. It never returns a ' +
        'credential or setting value.',
      warn:
        'Writing or deleting connection state changes the authority later invocations use. Those ' +
        'verbs are operator-scoped independently of this readable collection entry point.',
    },
    pending: '',
  },
  {
    id: 'grants',
    title: 'Manage grants',
    summary:
      'Inspect and edit the tenant’s fail-closed policy over connector-declared risk, effects and ' +
      'idempotency rather than lists of operation names.',
    surface: 'grants',
    call: {
      method: 'GET',
      endpoint: '/api/grants',
      caller:
        'An operator authorized for this tenant. An agent cannot read policy it could use to plan ' +
        'around a refusal and cannot widen its own authority.',
      note:
        'The collection is the tenant’s current grant document. PUT replaces it and POST ' +
        '/api/grants/preview evaluates a proposed selector without saving it.',
      warn:
        'With no admitted grant, invocation and inbound subscription refuse before credential ' +
        'material is read or vendor traffic is delivered.',
    },
    pending: '',
  },
  {
    id: 'invoke',
    title: 'Call an operation',
    summary:
      'Name an operation and get a result, without naming a host, a credential or a tenant. The ' +
      'credential never crosses the boundary; the authority does.',
    surface: 'invoke',
    call: {
      method: 'POST',
      // The one endpoint on this page that carries a parameter, and `{operation}` is a catalogue
      // key — the same public vocabulary `/api/catalogue/connectors/{id}/operations` uses. It
      // selects *what* runs; the tenant, the host and the credential are all derived from things
      // the caller did not supply, which is why this is not the tenant-shaped path the rule against
      // parameters exists to keep off this page.
      endpoint: '/api/operations/{operation}/invoke',
      caller:
        'Any principal this host resolved, including a signed-in human or a Service Account ' +
        'presenting its bearer token.',
      note:
        '{operation} is the catalogue’s own id, as served at /api/catalogue/connectors, and the ' +
        'body is that operation’s declared parameters verbatim — there is no envelope, no tenant, ' +
        'no connector, no host and no credential, because every one of those is derived from the ' +
        'operation and from the principal this host resolved. The answer carries sent and ' +
        'retryable as fields, because an HTTP status answers neither: a 502 does not say whether ' +
        'the vendor received it. A vendor’s own 4xx comes back as 200 with its response in ' +
        'content, unshaped — the vendor said no and we could not ask are different events. A host ' +
        'with no credential store bound runs nothing and says so.',
      // The last two sentences changed in X-62 and the change is the story. They used to read
      // "Grants are held per tenant and there is no route that edits one, so a tenant nobody has
      // granted anything runs nothing at all: ask whoever operates this deployment" — true when
      // X-13 shipped the gate fail-closed with no surface behind it, and false the moment one
      // landed. A claim about this build has to leave the page in the change that makes it false;
      // `the_invoke_warning_names_the_route_a_grant_is_edited_at` is what holds that.
      warn:
        'This route is gated by identity and by grant. An operation runs only if a grant your ' +
        'tenant holds admits it, decided from what the operation declares — its risk, its effects, ' +
        'its idempotency — rather than from a list of names; anything else is refused with 403 ' +
        'not_granted, before any credential is read and with nothing sent. Grants are held per ' +
        'tenant, and a signed-in human of that tenant edits them at PUT /api/grants, stating a ' +
        'connector and at most a risk level rather than a list of operation ids. An agent cannot ' +
        'write there and cannot read there either: a caller able to enumerate a tenant’s policy ' +
        'can plan around it, which is why a refusal here names the operation and never the rule ' +
        'that refused it. So a tenant nobody has granted anything still runs nothing at all — ask ' +
        'whoever holds the tenant for a grant that admits what you need.',
    },
    pending: '',
  },
  {
    id: 'subscribe',
    title: 'Receive a vendor’s events',
    summary:
      'Have this host terminate a vendor’s channel, verify what arrives against the connector’s ' +
      'declaration, and hand you a typed event.',
    surface: 'subscribe',
    call: {
      method: 'GET',
      endpoint: '/api/subscribe',
      caller: 'A resolved principal presenting its session cookie or bearer token.',
      note:
        'Upgrade to WebSocket, then send {action:"subscribe",request_id,channel_id}. The channel ' +
        'id must belong to the principal’s tenant and its connector, binding and complete selected ' +
        'event set must be admitted by an inbound grant. Acknowledgements and refusals echo ' +
        'request_id; live event messages carry only the declared event envelope.',
      warn:
        'Delivery is live and at-most-once: there is no replay or cursor. A subscriber that cannot ' +
        'keep up with its bounded queue is disconnected without stopping the vendor channel.',
    },
    pending: '',
  },
  {
    id: 'workflows',
    title: 'Store and publish workflows',
    summary:
      'Author, validate and publish immutable tenant-local Flux Programs; triggers, conditions ' +
      'and schedules remain Program semantics rather than a second Exchange execution model.',
    surface: 'workflows',
    call: {
      method: 'GET',
      endpoint: '/api/workflows',
      caller:
        'An operator authorized for this tenant. Agents invoke a published workflow through the ' +
        'ordinary operation and grant boundary; they do not mutate its stored Program.',
      note:
        'The collection lists tenant workflow drafts and immutable published versions. Run ' +
        'records are read separately through /api/workflow-runs.',
      warn:
        'Publishing freezes the Program and its derived requirements. It does not create a ' +
        'parallel trigger or scheduling runtime inside Exchange.',
    },
    pending: '',
  },
  {
    id: 'read-what-happened',
    title: 'Read back what you did',
    summary:
      'Read immutable workflow run status and value-free node events without exposing arguments, ' +
      'results or credentials in the structural trace.',
    surface: 'activity',
    call: {
      method: 'GET',
      endpoint: '/api/workflow-runs',
      caller:
        'A signed-in human. Runs are selected only from the tenant on the resolved principal; ' +
        'there is no tenant field or tenant-shaped path.',
      note:
        'The answer lists immutable workflow version, terminal status and editor node lifecycle ' +
        'events. Each event contains only node identity, source path, occurrence, phase and an ' +
        'optional selected branch. Invocation arguments and node values are never recorded there.',
      warn: '',
    },
    pending: '',
  },
  {
    id: 'agents',
    title: 'Host Managed Agents',
    summary:
      'Run a model, authored loop and bounded operation and datasource capabilities as a Managed ' +
      'Agent inside an installed Flux App.',
    surface: 'managed-agents',
    call: {
      method: 'POST',
      endpoint: '/api/apps/{app}/chat',
      caller: 'A resolved principal. The App and all of its frozen bindings come from that principal’s tenant.',
      note: 'The message wakes the App Package’s declared Managed Agent through a durable Event Delivery and Flux event log.',
      warn: 'The Managed Agent can spend only its installed opaque authority; it cannot read or address a credential.',
    },
    pending: '',
  },
  {
    id: 'leases',
    title: 'Hold runtime leases',
    summary:
      'Hold a caller-grant-scoped pull resource until its holder releases it or its TTL passes.',
    surface: null,
    call: null,
    pending: 'Planned in X-118, “Make leases own rich runtime resources.”',
  },
]

/**
 * The step [`AUTHENTICATION`] defers the "does it work yet" question to.
 *
 * It throws rather than returning `null` when nothing matches, which is the one thing this module
 * does that could be called repair otherwise: a missing step would silently make the auth scheme
 * look like a claim standing on nothing, and both renderings would publish it. Refuse; never
 * repair.
 */
export function authenticationStep(): Step {
  const step = STEPS.find((candidate) => candidate.id === AUTHENTICATION.capability)
  if (!step) {
    throw new Error(
      `AUTHENTICATION names the \`${AUTHENTICATION.capability}\` step and no step declares that id`
    )
  }
  return step
}

/**
 * The surface a step stands on, or `null` when the console declares none for it.
 *
 * `surfaces` is a parameter rather than a closed-over constant so the derivation can be exercised
 * against a hypothetical build — `the_derivation_is_live_and_not_a_coincidence` marks `invoke` built
 * and asserts the step follows. Without that, a page agreeing with the model today is
 * indistinguishable from one that hardcoded today's answers.
 */
export function backing(step: Step, surfaces: readonly Surface[] = SURFACES): Surface | null {
  if (step.surface === null) return null
  return surfaces.find((surface) => surface.id === step.surface) ?? null
}

/**
 * Whether this build lets an agent author do this, at all.
 *
 * The whole honesty rule in one expression: a surface the console declares the **service** serves,
 * or nothing. Not `built` — see the module note; that spelling reported the console's missing
 * screens as the platform's missing capabilities, and got `invoke` wrong for two releases.
 */
export function available(step: Step, surfaces: readonly Surface[] = SURFACES): boolean {
  const surface = backing(step, surfaces)
  return surface !== null && surface.served
}

/**
 * Why this build does not let an agent author do this, in this repository's own words. Empty when
 * it does.
 *
 * Read off the backing surface wherever there is one, so the page and the navigation state one gap
 * once. A step with no surface has nowhere to read it from and carries its own.
 */
export function withheld(step: Step, surfaces: readonly Surface[] = SURFACES): string {
  if (available(step, surfaces)) return ''
  const surface = backing(step, surfaces)
  return surface ? surface.absent : step.pending
}

/**
 * The one sentence saying what an agent cannot do here, built from the model.
 *
 * Derived rather than written out, for the reason `ConsoleShell.mts`'s `inventory` is: the day a
 * surface lands, its step leaves this sentence in the same edit that makes it a step you can follow.
 */
export function pendingSummary(surfaces: readonly Surface[] = SURFACES): string {
  const names = STEPS.filter((step) => !available(step, surfaces)).map((step) => step.title)
  if (names.length === 0) return ''
  return `Not yet, in this build: ${names.join('; ')}.`
}
