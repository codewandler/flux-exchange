// Write the agent descriptor the service serves, from the model the console page renders.
//
//   node scripts/agent-descriptor.mjs        # from console/
//
// The artifact lands in the server crate rather than here, because the *service* serves it: an
// agent fetches `/api/onboarding` from the host it is talking to, not from a bundle. It is
// committed rather than generated at build time for the reason every generated artifact in this
// repository is — `crates/exchange-server` is a Cargo crate with no Node in its build, and a build
// step that crosses that line would make `cargo build` depend on npm.
//
// A committed copy of a derivation can rot, so nothing trusts this script having been run:
// `test/descriptor.test.mjs` derives the document again and fails when the committed artifact
// differs. This script is the fix for that failure, not the guard against it.

import { writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { descriptorJson } from '../src/descriptor.mts'

const here = path.dirname(fileURLToPath(import.meta.url))
const artifact = path.resolve(here, '..', '..', 'crates', 'exchange-server', 'src', 'routes', 'onboarding.json')

writeFileSync(artifact, descriptorJson(), 'utf-8')
console.log(`wrote ${artifact}`)
