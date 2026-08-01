// What this service can tell an agent, derived from what the console already says it is.
//
// **Why this file exists at all.** `docs/vision.md` calls the agent this platform's *primary*
// caller — people sign in to wire things up, agents call operations all day — and until this page
// existed nothing anywhere answered "what is this, and how do I connect to it?" for one. Everything
// built so far serves the other caller.
//
// **Why it is data rather than a paragraph.** The state being described changes weekly. An agent can
// be minted (X-36) and cannot yet authenticate (X-37); it can invoke nothing at all, and that one is
// blocked upstream (X-11, X-12). Prose describing that is false within a release, and principle 7 of
// the vision — restated in `AGENTS.md` — makes a page implying a working service cost more than an
// honest gap. So the page does not describe this build. It **derives** from `surfaces.mts`, the same
// declaration `ConsoleShell.mts` builds the navigation from, and the derivation is the honesty:
//
//   > **Nothing here may claim to work unless a surface the console already marks `built` backs it.**
//
// That guard is deliberately one-directional. A built surface is not proof that a particular route
// exists — `identity` being built does not by itself mean `POST /api/agents` does. What the rule
// buys is the direction that matters: it can only ever take a claim *off* the page, never put one
// on. A surface regressing to unbuilt silently withdraws every step standing on it, and a step no
// surface backs at all cannot be claimed however confidently it is written.
//
// **`authenticate` is the step with no surface, and that is the point.** The console's surfaces are
// places an operator goes; authenticating is not one, so there is nothing in `surfaces.mts` to hang
// it on — and by the rule above, a step with no backing surface is withheld. Which is exactly right
// today: nothing on this host resolves an agent token to a principal. When X-37 lands, making this
// step claimable takes a surface the navigation also shows, which is a visible edit rather than a
// quiet one.
//
// This module is data and pure functions. It imports `surfaces.mts` and **nothing else** — not Vue,
// not `service.mts`, which is the only module in this console that knows a network exists. That is
// what lets `test/onboarding.test.mjs` assert the page reads nothing and could not have.

import { SURFACES, type Surface } from './surfaces.mts'

/**
 * Where the onboarding page lives, as a catalogue-style path the fragment router resolves.
 *
 * `/connect` and not `/agents`: this is a reference about how to connect one, and `/agents` is the
 * name a *listing* of a tenant's agents would want when revocation (X-38) needs one. A path that
 * reads like a collection of tenant records is a poor name for a page whose whole discipline is
 * holding no tenant records.
 */
export const ONBOARDING_PATH = '/connect'

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
   * The route, origin-relative.
   *
   * **Parameterless, always.** A path with a segment in it would be a connector, a credential or a
   * tenant — this page states the shape of the service and never its contents, and
   * `nothing_tenant_specific_can_reach_this_page` holds that over every entry here.
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
 * Then the identity, which is the concrete step that works today. Then authenticating, calling,
 * subscribing and reading back — the four that do not, in the order they would.
 *
 * It is short on purpose. A long tutorial for a platform this young would be describing something
 * that does not exist.
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
    id: 'be-minted',
    title: 'Be issued an identity',
    summary:
      'An agent is a principal of exactly one tenant. It does not sign in: a human who already is ' +
      'mints it and hands over the token.',
    surface: 'identity',
    call: {
      method: 'POST',
      endpoint: '/api/agents',
      caller:
        'A signed-in human, from this console. Not the agent — the route requires a principal, and ' +
        'an agent has none until this call has been made for it.',
      note:
        'Send id, what to call the agent within the tenant, and expires_at, seconds since the Unix ' +
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
    title: 'Authenticate as that identity',
    summary:
      'Present the token on a request and have this host resolve you to the agent principal it ' +
      'minted, in that agent’s tenant.',
    surface: null,
    call: null,
    pending:
      'No route on this host accepts an agent token. Minting stores a verifier and hands you the ' +
      'token, and nothing yet resolves one back to a principal — so an agent holding a token is ' +
      'not a caller this service can identify. The only principals a deployment resolves today are ' +
      'humans who signed in through its identity provider.',
  },
  {
    id: 'invoke',
    title: 'Call an operation',
    summary:
      'Name an operation and get a result, without naming a host, a credential or a tenant. The ' +
      'credential never crosses the boundary; the authority does.',
    surface: 'invoke',
    call: null,
    pending: '',
  },
  {
    id: 'subscribe',
    title: 'Receive a vendor’s events',
    summary:
      'Have this host terminate a vendor’s channel, verify what arrives against the connector’s ' +
      'declaration, and hand you a typed event.',
    surface: 'subscribe',
    call: null,
    pending: '',
  },
  {
    id: 'read-what-happened',
    title: 'Read back what you did',
    summary: 'Who asked, which grant admitted it, what was called and what came back.',
    surface: 'activity',
    call: null,
    pending: '',
  },
]

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
 * The whole honesty rule in one expression: a surface the console marks built, or nothing.
 */
export function available(step: Step, surfaces: readonly Surface[] = SURFACES): boolean {
  const surface = backing(step, surfaces)
  return surface !== null && surface.built
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
