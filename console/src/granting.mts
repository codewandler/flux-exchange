// What a tenant may run, as this console models it — and, above everything, what it refuses to model.
//
// **The one rule this file exists to keep.** X-13's Goal is that a grant is decided from an
// operation's *declared metadata* and never from a list of names, and X-62's route refuses a request
// that names an operation. The console's obligation is the stronger one: there must be nowhere in
// anything it composes for an operation id to go. So the draft below is three axes and a connector,
// and there is deliberately no field, no helper and no branch here that could carry an id.
//
// **And there is no admission rule here.** Whether a selector admits an operation is
// `Selector::admits` over `OperationFacts::of`, in `exchange-host`, and `POST /api/grants/preview`
// answers it from that same projection. A TypeScript reimplementation would be a second answer to
// one question, and the one an operator read before saving would be the one that is *not* deciding —
// which is precisely the mistake this whole model was built to prevent, arriving through the screen
// that renders it. `test/grants.test.mjs::the_console_decides_no_admission_of_its_own` scans for it.
//
// This module is data and pure functions, following `minting.mts`: it imports the served types and
// nothing else — not Vue, not `service.mts`'s loaders — so the screen, the router and the tests can
// all read the same statement of what a grant is without any of them owning it.

import type { HeldGrant, Principal, ProposedGrant, Selector } from './service.mts'

/**
 * Where the grants screen lives, as a catalogue-style path the fragment router resolves.
 *
 * `/grants` and not `/authorization`: the noun the service, the store, the gate and the refusal all
 * use is *grant*, and a console that renamed it would make the `403` an operator meets
 * (`not_granted`) and the screen that fixes it two different vocabularies.
 */
export const GRANTS_PATH = '/grants'

/**
 * Which kinds of principal this host admits at `/api/grants`, in the wire spelling.
 *
 * A **courtesy, not the rule.** `routes::grants::MAY_GRANT` is the rule and it is enforced in the
 * guard; this exists so the screen can say *why* there is no form rather than letting an operator
 * find the `403` themselves — and when the service refuses anyway, its own sentence is what gets
 * rendered, unedited. Whoever may edit a grant decides what the tenant can run, which is strictly
 * more authority than creating one Service Account through the owner-local helper.
 */
export const MAY_GRANT: readonly string[] = ['user']

/** Whether this host would admit this principal at the grants routes. `null` is nobody. */
export function mayGrant(principal: Principal | null): boolean {
  return principal !== null && MAY_GRANT.includes(principal.kind)
}

/**
 * The risk levels a bound may be set to, **in the order the bound is read against**.
 *
 * Written out rather than derived, and the reason is the ordering rather than the names: `max_risk`
 * means *at or below*, `exchange_host::Risk` derives `Ord` and documents that ordering as
 * load-bearing, and an order cannot be recovered from a set of strings the catalogue happens to
 * publish. A chooser that offered these in the wrong order would let an operator pick a bound
 * meaning something other than what they read.
 *
 * That makes it a list this console maintains, which is the shape this whole story is against — so
 * it is not left to be noticed. [`unknownRisks`] compares it against what the catalogue actually
 * publishes and the screen says so out loud, which turns "this list went stale" from a silent
 * narrowing into a sentence on the page.
 */
export const RISK_LEVELS: readonly string[] = ['low', 'medium', 'high', 'destructive']

/**
 * The effects a grant may be bounded to, in `exchange_host::Effect`'s own spelling.
 *
 * Unordered — `effects_within` is a subset test and not a bound — so unlike [`RISK_LEVELS`] the
 * order here is presentational. Two of the three are never emitted by this build:
 * `OperationFacts::of` derives effects from whether the catalogue gave an operation a host to reach,
 * so every operation carries `network` and nothing carries the other two. They are offered anyway,
 * because a selector written `effects_within: ["network"]` is a statement an operator is making
 * about the future — it stays exact for this build and refuses the first operation that ever
 * reports another effect — and a chooser that hid the other two would make that statement
 * unwritable.
 */
export const EFFECTS: readonly string[] = ['network', 'workspace_write', 'process']

/** A selector that bounds nothing: every operation the connector declares. */
export function anySelector(): Selector {
  return { maxRisk: null, effectsWithin: null, idempotency: null }
}

/**
 * One held grant as a proposal — the three axes, and nothing else it carried.
 *
 * **Lossy on purpose, and never used on a grant that would lose something.** A grant written by hand
 * may carry `allow_ids`/`deny_ids`, which this surface does not express; [`replacing`] and
 * [`without`] refuse a set containing one rather than calling this on it. See their note.
 */
export function proposedOf(grant: HeldGrant): ProposedGrant {
  const inbound = grant.inbound ?? []
  return {
    connector: grant.connector,
    selector: grant.selector,
    ...(inbound.length
      ? { inbound: inbound.map((entry) => ({ binding: entry.binding, events: [...entry.events] })) }
      : {}),
  }
}

/**
 * Every held grant this surface could not write back, in the order they were read.
 *
 * `expressible: false` is the service's own field — a grant naming operations explicitly, or naming
 * a connector this build does not carry — and it is what makes `PUT /api/grants` answer `409`
 * instead of dropping the exception silently.
 */
export function blocking(grants: readonly HeldGrant[]): HeldGrant[] {
  return grants.filter((grant) => !grant.expressible)
}

/**
 * The whole set to send when one connector's grant is added or replaced, or `null` when it cannot
 * be composed faithfully.
 *
 * **Whole-set, because `PUT /api/grants` is.** `exchange_host::Grants::set` takes the entire set
 * deliberately — what an operator needs to be able to state is *what this tenant may do*, entire —
 * so editing one grant is reading the set, changing it, and sending it back. Replacing by connector
 * rather than appending is what makes it impossible for this console to send the two-grants-for-one-
 * connector body the route refuses with `422`.
 *
 * **`null` is the refusal, and it is here rather than in the screen.** If any held grant is not
 * expressible, a set composed from [`proposedOf`] would silently drop what it carried, and the only
 * evidence would be an operation running that used to be refused. The service refuses that with
 * `409`; this makes the console unable to ask. *Refuse; never repair* — the caller renders
 * [`blocking`] and tells the operator what is in the way.
 */
export function replacing(
  held: readonly HeldGrant[],
  proposed: ProposedGrant
): ProposedGrant[] | null {
  if (blocking(held).length > 0) return null

  const kept = held
    .filter((grant) => grant.connector !== proposed.connector)
    .map(proposedOf)

  return [...kept, proposed]
}

/**
 * The whole set to send when one connector's grant is revoked, or `null` for [`replacing`]'s reason.
 *
 * Revoking is the same whole-set write with one entry gone. There is deliberately no `DELETE
 * /api/grants/{connector}`: a route that removed one would be a sequence nobody can see the end
 * state of, which is the argument `Grants::set` makes for taking the whole set in the first place.
 */
export function without(
  held: readonly HeldGrant[],
  connector: string
): ProposedGrant[] | null {
  if (blocking(held).length > 0) return null
  return held.filter((grant) => grant.connector !== connector).map(proposedOf)
}

/**
 * Any risk level the catalogue publishes that this console does not offer as a bound.
 *
 * The check that keeps [`RISK_LEVELS`] from going stale in silence. A level added upstream would
 * otherwise simply be missing from the chooser: an operator would set the widest bound this console
 * offers, read a preview that agreed with it, and never learn that operations above it exist. This
 * turns that into something the screen states.
 *
 * Deduplicated and in the catalogue's own order of first appearance, so the sentence names each
 * unknown level once.
 */
export function unknownRisks(risks: readonly string[]): string[] {
  return distinct(risks).filter((risk) => !RISK_LEVELS.includes(risk))
}

/** Any effect the catalogue publishes that this console does not offer. [`unknownRisks`]' reason. */
export function unknownEffects(effects: readonly string[]): string[] {
  return distinct(effects).filter((effect) => !EFFECTS.includes(effect))
}

/** The distinct values, in order of first appearance. */
function distinct(values: readonly string[]): string[] {
  return [...new Set(values)]
}
