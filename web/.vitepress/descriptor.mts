// Where every status badge on this site comes from (X-64).
//
// **The problem this solves.** This repository corrected five separate renderings of one false claim
// in a single week — that `invoke` was not built — across a page, a screen, an inventory, a
// descriptor and an explorer, plus a sixth in a README. Each was written honestly. Each went stale.
// Each was caught by a review rather than by a mechanism. A documentation site is a factory for that
// failure: `docs/designs/public-docs-site.md` §1 is the argument, and the conclusion is that no page
// here states its own liveness.
//
// So the site does not describe what this build can do. It **derives** it, from the one honesty
// mechanism in this repository that has survived a review: `GET /api/onboarding`, whose `live` flags
// are held to `routes::MODULES` in both directions by
// `routes::onboarding::tests::a_capability_is_live_exactly_when_a_route_on_this_surface_serves_it`
// (X-42, tightened by X-52 so a capability cannot even name a route that does not serve it). A route
// landing or leaving turns the Rust gate red until `console/src/surfaces.mts` agrees with it, and
// this file is what carries that agreement onto a public page.
//
// **What it derives is the status, and only the status.** The descriptor names capabilities and
// endpoints. Most of what this site is for — *why the credential never crosses the boundary* — is
// not in it and must not be forced through it. Pages are written; badges are derived.
//
// **What it refuses.** A page under `capabilities/` that names no capability, and a page that names
// one the descriptor does not publish, both stop the build. That second case is the one absence
// hides in: a page whose subject has left the descriptor rendering with a blank status tells a
// reader nothing is wrong, and "nothing is wrong" is the claim this whole mechanism exists to stop
// anybody making by accident.

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'

/** One capability, as `console/src/descriptor.mts` publishes it. */
export interface Capability {
  id: string
  title: string
  summary: string
  live: boolean
  call: { method: string; endpoint: string } | null
  withheld: string
}

/** The document the service serves, in the shape this site reads. */
export interface Descriptor {
  capabilities: Capability[]
}

/**
 * What the page chrome renders, stamped onto a page's frontmatter at build time.
 *
 * Derived here and passed through frontmatter rather than read by the component, so the descriptor
 * never reaches the client bundle: the badge is a build-time fact about a build-time artifact, and
 * shipping the document to a browser would invite somebody to make it a runtime fetch — at which
 * point the site would be describing whichever deployment answered, which §5 of the design forbids.
 */
export interface CapabilityStatus {
  id: string
  title: string
  live: boolean
  /** `METHOD /endpoint`, when the descriptor names one. Live does not imply a call — see below. */
  call: string | null
  /** Why it cannot be done, in the descriptor's own words. Empty exactly when it is live. */
  withheld: string
}

/**
 * The repository root, found by walking up for `Cargo.toml`.
 *
 * Walked rather than counted in `..`s because VitePress bundles this module into a temporary config
 * beside `.vitepress/config.mts`, and a relative path that happens to work today is a path that
 * breaks the day that temporary file moves. `Cargo.toml` is the marker because the workspace root is
 * the one thing about this repository's layout that cannot move without everything else moving too.
 */
export function repoRoot(): string {
  let here = import.meta.dirname
  while (!existsSync(path.join(here, 'Cargo.toml'))) {
    const up = path.dirname(here)
    if (up === here) {
      throw new Error(
        `no Cargo.toml above ${import.meta.dirname}: the site cannot find the repository it documents, ` +
          'and therefore cannot find the descriptor its status badges derive from'
      )
    }
    here = up
  }
  return here
}

/** Where the *service* keeps the document it serves, compiled in with `include_str!`. */
export function artifactPath(): string {
  return path.join(repoRoot(), 'crates', 'exchange-server', 'src', 'routes', 'onboarding.json')
}

/** The directory whose pages are each about one capability, and must each carry a derived status. */
export const CAPABILITIES_DIR = 'capabilities'

/**
 * Whether this Node runs TypeScript without a flag, which is what the currency check needs.
 *
 * VitePress wants 22+. This wants type stripping on by default, so that
 * `console/src/descriptor.mts` runs with no compiler and no dependency — the whole reason the site
 * can re-derive the descriptor without `web/` taking on a build of its own. Where that is available
 * is **not** a single floor, and treating it as one is the bug review caught: it landed in 22.18,
 * but the 23 line forked before that and only picked it up in 23.6, so 23.0–23.5 are *above* a
 * naive `>= 22.18` and still cannot do it. Those runners would get the syntax error this check
 * exists to replace.
 *
 * Stated as the two ranges that actually work. Node 23 is end-of-life, which makes this narrow
 * rather than unimportant: a wrong version check is worst exactly when it is nearly right.
 */
function stripsTypesByDefault(version: string): boolean {
  const [major, minor] = version.split('.').map(Number)
  if (major > 23) return true
  if (major === 23) return minor >= 6
  if (major === 22) return minor >= 18
  return false
}

/** What to tell somebody whose Node cannot, in the terms `web/package.json` and the README use. */
const NODE_REQUIREMENT = '22.18+ (or 23.6+ on the 23 line, which is end-of-life)'

/**
 * Fail the build unless the committed artifact is what `console/src/descriptor.mts` derives today.
 *
 * The design names this as the seam: *"if the site reads it and nobody regenerates it, the site is
 * stale in the one place it claimed not to be. The console suite already fails on drift; the site's
 * build must depend on the same check rather than assuming it ran."* `web/` and `console/` are
 * separate trees with separate lockfiles that CI can run apart, so assuming is exactly what this
 * would otherwise be doing.
 */
export function assertDescriptorIsCurrent(): void {
  const artifact = artifactPath()
  if (!existsSync(artifact)) {
    throw new Error(`${artifact} is missing: this site derives every capability status from it`)
  }

  if (!stripsTypesByDefault(process.versions.node)) {
    throw new Error(
      `this site needs Node ${NODE_REQUIREMENT} and is running ${process.versions.node}.\n` +
        'The status badges are checked against `console/src/descriptor.mts`, which is TypeScript ' +
        "run by Node's own type stripping — unavailable on this version, where this would instead " +
        'fail as a syntax error from a subprocess.'
    )
  }

  let derived: string
  try {
    derived = execFileSync(
      process.execPath,
      [path.join(repoRoot(), 'web', 'scripts', 'derive-descriptor.mjs')],
      { encoding: 'utf-8' }
    )
  } catch (failure) {
    throw new Error(
      'could not re-derive the agent descriptor from `console/src/descriptor.mts`, so this build ' +
        'cannot tell whether the status badges it is about to publish are current:\n' +
        `${(failure as { stderr?: string }).stderr ?? failure}`
    )
  }

  if (readFileSync(artifact, 'utf-8') !== derived) {
    throw new Error(
      'the committed agent descriptor is not what `console/src/descriptor.mts` derives today.\n' +
        'Run `node scripts/agent-descriptor.mjs` from `console/`.\n' +
        'Until then every status badge on this site is derived from a description of a build that ' +
        'no longer exists — which is the failure this site derives them to avoid.'
    )
  }
}

/**
 * The document the badges derive from: the committed artifact, and nothing else.
 *
 * **There is deliberately no override here.** An earlier draft let an environment variable name a
 * different file, so that `web/test/status.test.mjs` could render the site against a hypothetical
 * build. Review found what that actually was: a build with the variable set publishes badges
 * derived from arbitrary JSON while [`assertDescriptorIsCurrent`] goes on passing, because the
 * guard checks the committed artifact rather than what the badges read. The story's whole
 * proposition is that a page cannot claim a capability is live without the route table agreeing,
 * and that was a documented-as-safe path around it living in production code.
 *
 * The hypothetical builds those tests need are injected in-process instead, through VitePress's
 * `onAfterConfigResolve` hook — see `buildWith` in `web/test/status.test.mjs`. That hook is a
 * parameter of the Node API's `build()` and is not part of `UserConfig`, so a config file cannot
 * set it and the CLI does not expose it: `npm run build` and `pages.yml` have no way to reach it.
 * In a real build the badges read this artifact, and this artifact is checked current.
 */
export function readDescriptor(): Descriptor {
  const artifact = artifactPath()
  if (!existsSync(artifact)) {
    throw new Error(`${artifact} is missing: this site derives every capability status from it`)
  }
  return JSON.parse(readFileSync(artifact, 'utf-8')) as Descriptor
}

/**
 * The status a page carries, or `null` for a page that is not about a capability.
 *
 * Throws — and so fails the build — in the two cases where rendering something would be worse than
 * rendering nothing:
 *
 *   a page under `capabilities/` that declares no `capability:`. Its status would be absent, and
 *   absence on a page about a capability reads as "fine"; and
 *
 *   a page naming a capability the descriptor does not publish. Either the page is about something
 *   this service does not have, or the capability has left the descriptor and the page is now the
 *   only thing on the site still claiming it exists.
 *
 * `relativePath` is the page's path from the site root, as VitePress supplies it.
 */
export function statusFor(
  relativePath: string,
  frontmatter: Record<string, unknown>,
  descriptor: Descriptor = readDescriptor()
): CapabilityStatus | null {
  const declared = frontmatter.capability
  const inDirectory = relativePath.split('/')[0] === CAPABILITIES_DIR

  if (declared === undefined) {
    if (!inDirectory) return null
    throw new Error(
      `${relativePath} declares no \`capability:\` in its frontmatter.\n` +
        `Every page under \`${CAPABILITIES_DIR}/\` is about one capability and takes its status from ` +
        'the agent descriptor; a page here with no capability would publish no status, and a missing ' +
        'status reads as "fine".\n' +
        `Known capabilities: ${descriptor.capabilities.map((entry) => entry.id).join(', ')}`
    )
  }

  if (typeof declared !== 'string') {
    throw new Error(`${relativePath} declares \`capability: ${String(declared)}\`, which is not a capability id`)
  }

  const capability = descriptor.capabilities.find((entry) => entry.id === declared)
  if (!capability) {
    throw new Error(
      `${relativePath} is about the \`${declared}\` capability, which the agent descriptor does not name.\n` +
        'Either this page describes something this service does not have, or `' +
        declared +
        '` has left the descriptor and this page is the last thing on the site still claiming it.\n' +
        `Known capabilities: ${descriptor.capabilities.map((entry) => entry.id).join(', ')}`
    )
  }

  return {
    id: capability.id,
    title: capability.title,
    live: capability.live,
    // Live does not imply a call, and the two must not be conflated. `subscribe` on a build that
    // served it would be live with `call: null`, because `console/src/onboarding.mts` has no route
    // to name for it yet — a badge that read `live && call.endpoint` would crash that build rather
    // than describe it.
    call: capability.call ? `${capability.call.method} ${capability.call.endpoint}` : null,
    withheld: capability.withheld,
  }
}
