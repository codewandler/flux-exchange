// The same facts as the onboarding page, in a form something can parse.
//
// **Why this exists.** `docs/vision.md` calls the agent this platform's primary caller, and X-41
// gave that caller a *page*. A page is a human artifact: an agent does not read a hero section, it
// fetches something and acts. So `docs/designs/agent-onboarding.md` §3 asks for a second rendering
// — small, stable, fetchable, naming the endpoints, the auth scheme and the capabilities — and §2
// asks for the thing that makes two renderings worth having: **one truth**.
//
// This module is that truth's second rendering. It reads `onboarding.mts`, which reads
// `surfaces.mts`, which is what the navigation reads. It states no fact of its own — every string
// below comes from the model, and the only work here is shape.
//
// **What this file is not.** It is not the transport. The console never fetches this document and
// this module is not imported by the app; the *service* serves it, from
// `crates/exchange-server/src/routes/onboarding.json`, which is written from here by
// `scripts/agent-descriptor.mjs`. That artifact is a copy of a derivation, so it can rot — which is
// exactly what `test/descriptor.test.mjs` exists to make impossible, in both directions:
//
//   1. the artifact the service serves equals what this function derives today, so editing
//      `surfaces.mts` or `onboarding.mts` without regenerating is a red gate; and
//   2. the artifact and the rendered **page** agree field by field, which is the assertion the
//      design asks for — that the two renderings agree, not that each is separately correct.
//
// **Every field here is a disclosure.** The document is served to strangers on a service that holds
// credentials, so the rule is the page's, one notch stricter because a machine will not skim it:
// nothing tenant-specific, nothing about this deployment's configuration, nothing a value could
// occupy. What comes out of here is identical in every deployment of a given build — the one fact
// that varies between deployments is `sign_in_available`, and it is deliberately **not** derived
// here, because this console cannot know it. The service adds that field and nothing else.

import { SURFACES, type Surface } from './surfaces.mts'
import {
  AUTHENTICATION,
  DESCRIPTOR_ENDPOINT,
  SERVICE,
  STEPS,
  authenticationStep,
  available,
  withheld,
} from './onboarding.mts'

/**
 * What the document is, so a caller that fetched it can tell what it got.
 *
 * A private name rather than a well-known URI, and the reason is written where the route is
 * (`crates/exchange-server/src/routes/onboarding.rs`): no registered `/.well-known/` URI describes
 * "how an agent authenticates to this service", and serving this at a name some other ecosystem's
 * clients already parse would be worse than a private one — they would parse it and be wrong.
 */
export const DESCRIPTOR = 'flux-exchange.agent-onboarding'

/**
 * The document's own version, bumped when a field changes meaning or leaves.
 *
 * It is here because the name above is ours: a caller cannot fall back on a standard's versioning,
 * so this document carries its own.
 */
export const DESCRIPTOR_VERSION = 2

/** One call, as the document publishes it. */
export interface DescriptorCall {
  method: string
  endpoint: string
  caller: string
  note: string
  warn: string
}

/** One capability, and whether this build lets an agent use it. */
export interface DescriptorCapability {
  id: string
  title: string
  summary: string
  /** Whether this build has it. Derived; never written. */
  live: boolean
  /** How to do it, or `null` — `null` exactly when it is not live, as on the page. */
  call: DescriptorCall | null
  /** Why it cannot be done, in the console's own words. Empty exactly when it is live. */
  withheld: string
}

/** The document, minus the one field only the service can fill in. */
export interface Descriptor {
  descriptor: string
  version: number
  endpoint: string
  service: { name: string; summary: string }
  authentication: { scheme: string; header: string; capability: string; live: boolean }
  capabilities: DescriptorCapability[]
}

/**
 * The descriptor this build's service publishes.
 *
 * `surfaces` is a parameter for the reason `onboarding.mts`'s `available` takes one: without it, a
 * document that agrees with the model today is indistinguishable from one that hardcoded today's
 * answers, and `the_descriptor_is_derived_and_not_a_coincidence` drives it against a hypothetical
 * build where `invoke` exists.
 *
 * Keys are written out rather than spread, so the artifact's field order is this file's decision
 * and a reviewer reading the JSON meets the fields in the order a reader would.
 */
export function descriptor(surfaces: readonly Surface[] = SURFACES): Descriptor {
  return {
    descriptor: DESCRIPTOR,
    version: DESCRIPTOR_VERSION,
    endpoint: DESCRIPTOR_ENDPOINT,
    service: { name: SERVICE.name, summary: SERVICE.summary },
    authentication: {
      scheme: AUTHENTICATION.scheme,
      header: AUTHENTICATION.header,
      capability: AUTHENTICATION.capability,
      // Read off the step, so the scheme cannot read as a working one on a build where nothing
      // resolves a token. The reason it does not work is *not* restated here — it is on that
      // capability, once.
      live: available(authenticationStep(), surfaces),
    },
    capabilities: STEPS.map((step) => {
      // Gated on `live` and **not** on `step.call` being set, which is not the same thing and was
      // not caught by reading. `onboarding.mts` holds `call === null` for every step that is
      // withheld *in this build*, so the two spellings agree today — but a surface regressing to
      // unbuilt withdraws a step whose model still carries its call, and the page drops the
      // instruction (`AgentOnboarding.mts` renders it only when `available`) while a document
      // spelled the other way would keep publishing the endpoint. That is the drift this story
      // exists to make impossible, in the one direction that costs a caller something: an endpoint
      // beside a capability that does not exist is an invitation to call nothing.
      const live = available(step, surfaces)

      return {
        id: step.id,
        title: step.title,
        summary: step.summary,
        live,
        call:
          live && step.call
            ? {
                method: step.call.method,
                endpoint: step.call.endpoint,
                caller: step.call.caller,
                note: step.call.note,
                warn: step.call.warn,
              }
            : null,
        withheld: withheld(step, surfaces),
      }
    }),
  }
}

/**
 * The artifact's text, exactly as it is committed.
 *
 * One function so the writer and the test cannot disagree about trailing newlines or indentation —
 * a byte comparison that fails on whitespace teaches everyone to ignore it.
 */
export function descriptorJson(surfaces: readonly Surface[] = SURFACES): string {
  return `${JSON.stringify(descriptor(surfaces), null, 2)}\n`
}
