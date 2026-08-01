// Print the agent descriptor this build's console derives, so the site can check the committed one.
//
//   node scripts/derive-descriptor.mjs        # from web/
//
// **Why this exists rather than the site just reading the artifact.**
// `crates/exchange-server/src/routes/onboarding.json` is a committed copy of a derivation, written
// by `console/scripts/agent-descriptor.mjs`. Copies rot. `console/test/descriptor.test.mjs` fails
// when it has, but `web/` and `console/` are separate Node trees that can be built and tested apart
// — so a site that derived every badge from that artifact and assumed somebody else had checked it
// would be stale in the exact place it advertises as derived. `.vitepress/descriptor.mts` runs this
// and compares, on every site build.
//
// **It adds no dependency, and that is why it is a subprocess.** `console/src/descriptor.mts` and
// the two modules it imports are pure TypeScript data and functions — no Vue, no bundler, no
// imports outside `console/src/` — so Node's own type stripping runs them directly. Spawning a
// plain `node` keeps that out of the VitePress config's esbuild bundle, where a relative import
// reaching into a sibling tree would be resolved by a bundler with different rules. `web/` still
// shares no dependency and no lockfile with `console/`; it reads a source file, the way the Rust
// crate reads the JSON.
//
// Requires Node 22.18+, where type stripping is on by default. `web/README.md` says Node 22+.

import path from 'node:path'

import { repoRoot } from '../.vitepress/descriptor.mts'

const { descriptorJson } = await import(
  path.join(repoRoot(), 'console', 'src', 'descriptor.mts')
)

process.stdout.write(descriptorJson())
