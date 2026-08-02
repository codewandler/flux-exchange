// What this platform is, as the console presents it — and what it is honestly not.
//
// **Why this file exists at all.** `docs/vision.md` says the console has two jobs: people sign in
// *to wire things up* and *to see what happened*. The connector catalogue is neither. It is
// reference material — what this build **could** run — and while it was the console's entire
// surface, the console read as a connector browser rather than as the admin surface of a service
// that holds credentials and executes on a tenant's behalf. So the shape of the platform is stated
// here, once, as data: the shell renders it, and `test/shell.test.mjs` walks it.
//
// **Half of it does not exist yet, and that is the harder half to get right.** `invoke`, `subscribe`
// and execution records are not built. Leaving them out would hide the platform's own shape; giving
// them a screen would be worse. Principle 7 of the vision, restated in `AGENTS.md`:
//
//   > Say what is not built. A page or a type that implies a working service costs more than an
//   > honest gap.
//
// A named, disabled entry with a reason beside it is honest. A screen with a plausible empty state
// is a lie — and it is the *comfortable* lie, because a placeholder looks more finished than a
// disabled entry to anyone who is not asking whether it is real. So the model carries the
// invariant rather than trusting anybody to remember it: **a surface that is not built has no
// path**, and `test/shell.test.mjs` asserts that no route resolves to one and that no source
// mounts a screen for one.
//
// **Two questions, and they stopped having one answer.** "Does this console have a screen for it"
// and "does the service publish this capability" were the same question for every surface here
// until `invoke` shipped a route with no screen behind it — and a single `built` flag standing for
// both then answered the second one wrongly, on a page (X-41) and very nearly in a public
// machine-readable descriptor (X-42). They are `built` and `served` now, and the second is checked
// against the server's own route table by the Rust gate rather than kept true by remembering.
//
// This module is deliberately data and pure functions. It imports nothing — not Vue, not the
// router, and certainly not `service.mts` — so the shell, the router and the tests can all read
// the same statement of what this platform is without any of them owning it.

/**
 * One surface of the platform, as the console presents it.
 *
 * The fields are ordered the way a reader meets them: what it is called, what it is for, whether
 * you can go there, and — when you cannot — why not.
 */
export interface Surface {
  /** Stable id. Also the `data-surface` attribute the shell renders and the tests address it by. */
  id: string
  /** What it is called on the page. */
  label: string
  /** What it is for, in one line, in the vocabulary the vision uses. */
  summary: string
  /**
   * The catalogue-style path this surface is at, resolved through the fragment router.
   *
   * `null` means there is nowhere to go, and it means that for two different reasons which
   * [`built`](Surface#built) tells apart: `identity` is built and is a *control* in the header
   * rather than a page, while `invoke`, `subscribe` and `activity` have no path because there is
   * nothing behind them. **A surface that is not built must carry `null` here** — that is the
   * honesty invariant in its smallest form, and it is asserted.
   */
  path: string | null
  /**
   * Whether **this console** has a screen for it.
   *
   * A question about navigation, and only about navigation: it decides whether the rail renders a
   * link or a disabled entry, and it is what [`path`](Surface#path) has to agree with.
   *
   * **It is not the same question as [`served`](Surface#served), and X-42 is where that stopped
   * being a distinction without a difference.** Until `invoke` shipped, every surface answered both
   * questions the same way, so one flag could stand for both and nobody could tell. Then
   * `POST /api/operations/{operation}/invoke` landed with no screen behind it, and a reader of this
   * file that took `built` for "does the service do this" got a false answer — one that X-41's page
   * published to humans and X-42 came within a commit of publishing to machines.
   */
  built: boolean
  /**
   * Whether **the service** publishes this capability.
   *
   * A question about the API: is there a route on this host that does this thing? That is what an
   * agent is asking, and it is the field `onboarding.mts` derives from — a page and a descriptor
   * that answered it from [`built`](Surface#built) would report the console's gaps as the
   * platform's.
   *
   * **This claim is checked against the route table, in the server, in the gate.**
   * `routes::onboarding::tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`
   * walks `routes::MODULES` and compares; its companion refuses to let a published route go
   * unaccounted for. So this is not a flag somebody keeps true by remembering — a route landing or
   * leaving turns the Rust gate red until this file agrees with it.
   *
   * A surface the service does not serve cannot have a screen either — an unserved surface with a
   * console screen would be a screen over nothing — and
   * `test/onboarding.test.mjs::a_surface_the_service_does_not_serve_has_no_screen_either` asserts
   * that direction, which is what keeps [`absent`](Surface#absent) usable as the reason a
   * capability is withheld.
   */
  served: boolean
  /**
   * Why it does not exist, in this repository's own words. Empty when it does.
   *
   * Rendered on the disabled entry rather than kept as a comment, because a disabled control with
   * no reason beside it reads as a bug in the console rather than as a statement about the service.
   *
   * **It has to say which of the two gaps it is describing**, now that they can differ. A surface
   * that is neither `built` nor `served` has one gap and this states it; `invoke` has a screen
   * missing and an API present, and saying "not built" without saying which would be the same
   * falsehood in a friendlier place.
   */
  absent: string
}

/**
 * Every surface, in the order the shell presents them.
 *
 * **The order is the argument.** Connections first because wiring things up is the first of the
 * console's two jobs and the one that works today; Activity second because seeing what happened is
 * the other one, even though it is not built; then the two execution verbs; and the catalogue
 * **last**, because it is reference material and this story exists to stop it being the front door.
 *
 * `identity` is not in that sequence — it is the header's own affordance rather than a place — but
 * it is a surface of the platform and it is listed, so a walk over this array is a walk over
 * everything the console claims this service has.
 */
export const SURFACES: readonly Surface[] = [
  {
    id: 'connections',
    label: 'Connections',
    summary: "What this tenant has wired up, and which of each connector's credentials it holds.",
    path: '/connections',
    built: true,
    served: true,
    absent: '',
  },
  {
    id: 'grants',
    label: 'Grants',
    summary: 'What this tenant may run: a connector, at most a risk level, and the effects allowed.',
    // Directly after Connections, because the two are one job in two steps rather than two jobs. A
    // tenant with a connection and no grant runs **nothing** — X-13 closed the gate fail-closed —
    // so an operator who has just wired a connector up and stopped has not finished, and the entry
    // that tells them so belongs where they will read it next.
    path: '/grants',
    built: true,
    served: true,
    absent: '',
  },
  {
    id: 'activity',
    label: 'Activity',
    summary: 'Who asked, which grant admitted it, what was called and what came back.',
    path: null,
    built: false,
    served: false,
    absent:
      'This service records nothing yet. There is no execution record to read, so there is no ' +
      'Activity screen — an empty one would say that nothing has happened, which is a different ' +
      'claim entirely.',
  },
  {
    id: 'invoke',
    label: 'Invoke',
    summary: 'Call a connector operation as this tenant, without ever holding its credential.',
    path: '/invoke',
    built: true,
    served: true,
    absent: '',
  },
  {
    id: 'subscribe',
    label: 'Subscribe',
    summary: "Terminate a vendor's channel and receive typed, declared events.",
    path: null,
    built: false,
    served: false,
    absent:
      '`subscribe`, the websocket and channels are not built. Nothing here terminates a channel, ' +
      'so there is no subscription to show and no route to call.',
  },
  {
    id: 'catalogue',
    label: 'Catalogue',
    summary: 'Every operation this build could run, what each one costs, and what it declares.',
    // `/explorer` and not `/`: the catalogue is reference material rather than the front door, and
    // `/` is where a reader lands. Search state lives on this route, so links remain shareable.
    path: '/explorer',
    built: true,
    served: true,
    absent: '',
  },
  {
    id: 'identity',
    label: 'Identity',
    summary: 'Who you are signed in as, and the tenant every credential address is derived from.',
    // The header's affordance, not a page. Built, and deliberately pathless — see `path` above.
    path: null,
    built: true,
    served: true,
    absent: '',
  },
]

/**
 * The surface a route belongs to, or `null` for a route that belongs to none.
 *
 * Several routes are one surface: an operation page and a core page are both the catalogue, and a
 * reader who has followed a link into an operation should still see where they are. `unknown` maps
 * to `null` on purpose — a path that resolves to nothing must not light up a surface, because that
 * would tell the reader they had arrived somewhere.
 *
 * Every name this function knows is a route [`parseRoute`](./routing.ts) can actually produce.
 * Nothing here may name a surface that is not built; `test/shell.test.mjs` asserts it.
 */
export function surfaceOfRoute(name: string): string | null {
  switch (name) {
    case 'connections':
      return 'connections'
    case 'grants':
      return 'grants'
    case 'invoke':
      return 'invoke'
    case 'explorer':
    case 'operation':
    case 'core':
      return 'catalogue'
    default:
      return null
  }
}
