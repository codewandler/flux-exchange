// The mint screen's model: where it lives, who may use it, what a token from here can do today, and
// what a "copy" button owes an operator when the copy silently did not happen.
//
// **Why this is a module and not a few consts in the screen.** Two of the four are claims about the
// platform rather than about the layout, and this repository's rule for those is X-41's: a claim is
// **derived** from `surfaces.mts` — the same declaration the navigation reads — so it cannot rot
// into a page describing a build that no longer exists. `onboarding.mts` establishes that
// derivation and states the rule it holds:
//
//   > Nothing here may claim to work unless a surface the console declares the service `served`
//   > backs it.
//
// and, crucially, that the rule is **one-directional**: it can take a claim *off* a page, never put
// one on. Everything below obeys that. [`tokenStanding`] reads availability through
// `onboarding.mts`'s own `available`, so it cannot disagree with the onboarding page about the same
// step; [`authorisation`] is a sentence about what a token admits its holder to, and it is
// withdrawn the moment a token can actually be **presented**, rather than being edited into
// something narrower by whoever happens to notice.
//
// **X-42 answered the trip-wire this module left, and the answer changed the sentence rather than
// deleting it.** The withdrawal used to fire when *anything a token could be presented for* became
// available, on the reasoning that "nothing is gated" is only safe while there is nothing to gate.
// Measuring the document against the route table found that `invoke` had been available since
// v0.7.0 and that three renderings — the nav, the onboarding page and this screen — were each
// reporting otherwise, because all three read a flag that answers whether the *console* has a
// screen. Correcting that fired this trip-wire, as designed. But withdrawing here would have left
// an operator minting a token with nothing on the page about what it admits to, at exactly the
// moment the answer stopped being "nothing at all" — so the sentence now states the exposure, and
// the withdrawal is keyed on the event that genuinely makes it a grant question: a token that can
// be presented.
//
// This module imports `onboarding.mts` and, for a type only, `service.mts`. It reads nothing over
// the network — the screen does that — and it holds no state at all, which is the property that
// matters most here: nothing in this file could be where a minted token ends up living.

import type { Principal } from './service.mts'
import { STEPS, authenticationStep, available, withheld, type Step } from './onboarding.mts'
import { SURFACES, type Surface } from './surfaces.mts'

/**
 * Where the mint screen lives, as a catalogue-style path the fragment router resolves.
 *
 * `/agents` and not `/connect`: `onboarding.mts` reserved this name for the operator-facing page
 * about a tenant's agents, and this is the first of it. That page's own argument for taking
 * `/connect` instead was that a path reading like a collection of tenant records is a poor name for
 * a page holding none — which is exactly the reason it is the right name for this one, which does.
 *
 * **The path carries nothing and must not learn how.** A route that can hold a value is a value in
 * the address bar, in every history entry after it, and in the referrer of every link the page then
 * offers. `test/agents.test.mjs` pins `parseRoute` to a bare `{ name: 'agents' }`.
 */
export const AGENTS_PATH = '/agents'

/**
 * The onboarding step that *is* this screen.
 *
 * `onboarding.mts` lists its steps in "the order it happens to them", so everything after this one
 * is something done while **holding** the token — which is what makes [`tokenStanding`] a slice of
 * that list rather than a second list kept here. Reading the catalogue comes before it and needs no
 * token at all; minting is what the operator is doing rather than something the token can do.
 */
export const MINT_STEP = 'be-minted'

/**
 * The kinds of principal that may mint, as this console understands X-40.
 *
 * **This is a courtesy, not the rule.** The rule is `routes::agents::MAY_MINT`, enforced by the
 * route's `Access::PrincipalOfKind` guard and again inside `AgentStore::mint`. What this list buys
 * is that an operator who cannot mint is told so instead of being offered a button and discovering
 * the `403` by pressing it — and when the two ever disagree, the service wins and its own sentence
 * is what the screen shows, which is why `Agents.mts` renders a refusal whole.
 *
 * Spelled in the vocabulary `GET /api/session` publishes a principal's `kind` in, lowercase, which
 * is `PrincipalKind`'s own `Display`.
 */
export const MAY_MINT: readonly string[] = ['user']

/**
 * Whether this principal may create a principal.
 *
 * A `User`, and nothing else. An `Agent` may not, because a leaked token that mints successors
 * makes revocation an incomplete remedy in a way no operator can see; a `Service` may not, because
 * nothing in this repository mints, verifies, lists or revokes a service credential, so admitting
 * it would put the same defect one level further out of sight. The full argument is in
 * `docs/designs/agent-access.md` and in `routes::agents`.
 */
export function mayMint(principal: Principal | null): boolean {
  return principal !== null && MAY_MINT.includes(principal.kind)
}

/** One thing a token from here would be presented for, and whether this build lets it. */
export interface Standing {
  /** The onboarding step, so the screen shows the same title and summary that page does. */
  step: Step
  /** Whether this build lets a token holder do it. Read through `onboarding.mts`, never decided here. */
  can: boolean
  /** Why not, in the backing surface's own words. Empty when it can. */
  reason: string
}

/**
 * Everything a token from here would be presented for, in the order it happens.
 *
 * A slice of `onboarding.mts`'s own list rather than a list of ids kept here, so a step added to
 * that page after minting arrives on this screen with nobody editing it — and a step removed leaves.
 * An empty slice would mean the onboarding model has been reordered out from under this screen, and
 * the tests say so rather than quietly rendering nothing.
 */
export function tokenSteps(): readonly Step[] {
  const at = STEPS.findIndex((step) => step.id === MINT_STEP)
  return at === -1 ? [] : STEPS.slice(at + 1)
}

/**
 * What a token from here can and cannot do today, derived from `surfaces.mts`.
 *
 * `surfaces` is a parameter for the reason `onboarding.mts`'s `backing` takes one: without being
 * able to exercise the derivation against a hypothetical build, a screen that agrees with the model
 * today is indistinguishable from one that hardcoded today's answers.
 */
export function tokenStanding(surfaces: readonly Surface[] = SURFACES): Standing[] {
  return tokenSteps().map((step) => ({
    step,
    can: available(step, surfaces),
    reason: withheld(step, surfaces),
  }))
}

/**
 * What a token minted here authorises, today — or `''` when this build has outgrown the sentence.
 *
 * # What it claims, and when it must stop
 *
 * Two claims, with two different expiries. The first is that a token minted here is **presented
 * nowhere**, which stops being true the day anything on this host resolves an agent token — and
 * that is the `authenticate` step, which is what `presentable` watches.
 *
 * The second used to be that **nothing is gated by a grant**, stated as plainly as possible and
 * marked as the sentence a grant story would have to come and rewrite. X-13 is that story, and this
 * is the rewrite: invocation is gated by a grant, so what a token would authorise is bounded by
 * what its tenant has been granted rather than by the whole catalogue. The claim is *narrower* than
 * the old one and it expires the same way — the day a grant becomes something this console can
 * edit, this paragraph is describing a screen the operator is looking at rather than a file they
 * cannot see.
 *
 * It is withdrawn rather than narrowed at the first of those, because once a token can be
 * presented, *what may this principal do* is a live grant question and this screen has no business
 * answering it from a surface list. Returning `''` takes the claim off the screen and turns
 * `test/agents.test.mjs` red until whoever landed that step says what is true instead.
 *
 * # Why this no longer fires on `invoke`
 *
 * It used to withdraw as soon as *any* token-holder step became available, on the reasoning that a
 * claim about gating is vacuous — and therefore safe — only while there is nothing to gate. X-42
 * measured the descriptor against the server's route table and found `invoke` had been live since
 * v0.7.0, so that trigger was already overdue; firing it would have blanked this paragraph exactly
 * when it acquired teeth. An operator minting a token when the answer is "nothing at all" is owed a
 * sentence; an operator minting one when the answer is "whatever this tenant has been granted, and
 * nothing if it has been granted nothing" is owed it much more.
 *
 * `presentable` is a parameter only so the withdrawal branch can be exercised — nothing in the app
 * passes it, and its default is the derivation. `test/agents.test.mjs` drives both sides.
 */
export function authorisation(
  surfaces: readonly Surface[] = SURFACES,
  presentable: boolean = available(authenticationStep(), surfaces)
): string {
  const standing = tokenStanding(surfaces)
  if (standing.length === 0 || presentable) return ''

  const live = standing.filter((entry) => entry.can)

  return (
    'Nothing on this host resolves an agent token to a principal yet, so a token minted here is ' +
    'presented nowhere and authorises nothing at all — including the ' +
    `${live.length === 1 ? 'capability' : 'capabilities'} listed here as available, which this ` +
    'service runs for the principals it can resolve and not for a token holder. When a token can ' +
    'be presented it will authorise what any principal of this tenant may do, and since X-13 that ' +
    'is bounded by a grant: an operation runs only if a grant this tenant holds admits it, decided ' +
    'from what the operation declares — its risk, its effects, its idempotency — rather than from ' +
    'a list of names. A tenant holding no grant runs nothing at all. Grants cannot be edited from ' +
    'this console yet: they live in the host’s grant store, and an operator writes them. Two limits ' +
    'hold whatever is granted — an agent may not create a principal, so a token that leaks cannot ' +
    'mint successors and revoking it ends the whole of the access it gave; and an agent’s token ' +
    'grants access to an operation, never to a credential.'
  )
}

/**
 * When a token minted now should stop resolving, as seconds since the Unix epoch.
 *
 * The console converts a lifetime an operator can reason about into the instant the service wants,
 * and shows them the instant before they send it. It does **not** supply the lifetime: the box
 * starts empty, because `routes::agents` refuses a body with no expiry rather than picking one, and
 * a console with a helpful default in it would quietly become the thing that picks.
 *
 * Read against this browser's clock, which is not the host's. A few seconds of skew is immaterial
 * for a lifetime measured in days, and a clock wrong by more than that produces a refusal naming
 * the expiry rather than a token with a lifetime nobody intended.
 */
export function expiryFromNow(days: number, now: number = Date.now()): number {
  return Math.floor(now / 1000) + Math.round(days * 86_400)
}

/** What became of a copy: it happened, or it did not and here is what to tell the operator. */
export type Copied = { ok: true } | { ok: false; reason: string }

/**
 * Put text on the clipboard, and report what actually happened.
 *
 * **The whole point of this function is the failure path.** `navigator.clipboard` exists only on a
 * secure origin, so on `http://` to anything but localhost — a bench deployment, a host behind a
 * plain reverse proxy, the first thing anybody stands up — it is simply `undefined`, and the usual
 * shape of this button (`navigator.clipboard.writeText(value)`, unawaited, uncaught) does nothing
 * at all and says nothing about it. An operator who believes they copied a token they did not is
 * strictly worse off than one who selected it by hand: they navigate away, and the token this host
 * cannot show again is gone.
 *
 * A rejected write is the same event by a different route — a permissions policy, a document that
 * is not focused — so it is reported the same way, in the browser's own words rather than in a
 * sentence this console made up about what it thinks went wrong.
 *
 * `clipboard` is a parameter so the two failures can be driven in a test. It is read at call time
 * rather than at module load, because a page can be loaded before its origin is known to be secure.
 */
export async function writeClipboard(
  text: string,
  clipboard: Clipboard | undefined = globalThis.navigator?.clipboard
): Promise<Copied> {
  if (!clipboard || typeof clipboard.writeText !== 'function') {
    return {
      ok: false,
      reason:
        'This page has no clipboard to write to. The browser exposes one only on a secure origin — ' +
        'https, or localhost — so on a plain http deployment there is nothing here to copy with.',
    }
  }

  try {
    await clipboard.writeText(text)
    return { ok: true }
  } catch (error) {
    return { ok: false, reason: error instanceof Error ? error.message : String(error) }
  }
}
