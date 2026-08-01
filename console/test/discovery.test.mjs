// The alarm on test discovery.
//
// `test/discovery/subdirectory.test.mjs` is the canary: it sits one level down and proves, by
// running, that nested tests run. But a canary is quiet. If the discovery pattern is narrowed again
// the canary simply stops being executed, `npm test` reports a smaller number nobody was counting,
// and the suite is green. That is the exact shape of the defect X-32 was opened about, one level up.
//
// So this file sits at the *top* level, where any pattern that finds tests at all will find it, and
// it fails loudly for the regression the canary can only fall silent for.
//
// It asserts two things, and needs both:
//
//   1. the configured script hands Node a recursive pattern, quoted so the shell cannot touch it;
//   2. that pattern really does recurse on the Node actually running — measured, not assumed.
//
// The second is not ceremony. X-32's own notes recorded that "Node 22's `--test` discovers
// recursively when pointed at a directory", and on Node 22.23.1 that is false: `node --test test/`
// does not enumerate the directory, it tries to load `test` as a module and dies with
// MODULE_NOT_FOUND. Discovery behaviour is a moving target across Node majors and this repository
// pins a major it will eventually raise, so the property is measured against a throwaway fixture
// on every run rather than believed once.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const consoleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** The `test` script exactly as `npm test` would run it. */
function testScript() {
  const manifest = JSON.parse(readFileSync(path.join(consoleRoot, 'package.json'), 'utf-8'))
  return manifest.scripts.test
}

test('the_test_script_hands_node_a_recursive_pattern_the_shell_cannot_eat', () => {
  const script = testScript()

  // `**` is the whole point: `test/*.test.mjs` is one directory level, which is what X-32 fixed.
  assert.match(
    script,
    /\*\*/,
    `the test script is \`${script}\` — without a \`**\` segment it matches one directory level and any test in a subdirectory of test/ is silently skipped`
  )

  // And it has to reach Node *unexpanded*. npm runs scripts through `sh`, and POSIX `sh` has no
  // globstar: unquoted, `test/**/*.test.mjs` is expanded by the shell as `test/*/*.test.mjs`, which
  // matches only the second level — so the top-level tests, including this file, would drop out.
  // Quoting hands the pattern to Node, whose own glob does understand `**`. The failure mode of
  // getting this wrong is once again a smaller green suite.
  assert.match(
    script,
    /'[^']*\*\*[^']*'/,
    `the test script is \`${script}\` — the pattern must be single-quoted or \`sh\` expands \`**\` as \`*\` and only one directory level survives`
  )
})

test('the_configured_pattern_actually_recurses_on_this_node', () => {
  // A throwaway tree with a test at each of three depths, plus a module that is *not* a test file.
  // Run the real script string against it, through `sh`, exactly as npm would.
  const fixture = mkdtempSync(path.join(os.tmpdir(), 'flux-console-discovery-'))
  const marker = path.join(fixture, 'collected.txt')

  // Appended at module scope, not inside a test: what is being measured is whether Node *collected*
  // the file, and a file it imports has already been collected whether or not it declares a test.
  const collector = (name) =>
    `import { appendFileSync } from 'node:fs'\n` +
    `import { test } from 'node:test'\n` +
    `appendFileSync(process.env.CONSOLE_DISCOVERY_MARKER, '${name}\\n')\n` +
    `test('${name}', () => {})\n`

  const files = {
    'test/top.test.mjs': 'top',
    'test/sub/nested.test.mjs': 'nested',
    'test/sub/deeper/deep.test.mjs': 'deep',
    // Not named `*.test.mjs`. Node's *bare* `node --test` treats every `.mjs` under a `test/`
    // directory as a test file, so this one distinguishes "recurses over our pattern" from "sweeps
    // the whole working directory" — two fixes that look alike and are not.
    'test/sub/helper.mjs': 'helper',
  }
  for (const [file, name] of Object.entries(files)) {
    const target = path.join(fixture, file)
    mkdirSync(path.dirname(target), { recursive: true })
    writeFileSync(target, collector(name))
  }

  // `NODE_TEST_CONTEXT` is set by the runner in every test child, and it is inherited. Left in
  // place, the fixture run believes it is itself a test child reporting to a parent, performs no
  // file discovery, and exits 0 having collected nothing — the probe would then fail identically
  // whether discovery works or not, which is the one thing a guard must never do.
  const env = { ...process.env, CONSOLE_DISCOVERY_MARKER: marker }
  delete env.NODE_TEST_CONTEXT

  try {
    execFileSync('sh', ['-c', testScript()], { cwd: fixture, env, stdio: 'ignore' })
  } catch (error) {
    assert.fail(`the test script failed against a fixture of passing tests: ${error.message}`)
  }

  const collected = existsSync(marker)
    ? readFileSync(marker, 'utf-8').split('\n').filter(Boolean).sort()
    : []

  // Sorted, because `node --test` runs files in parallel child processes and the append order is
  // whichever finishes first.
  assert.deepEqual(
    collected,
    ['deep', 'nested', 'top'],
    `\`${testScript()}\` collected [${collected.join(', ')}] from a fixture holding tests at three depths plus one non-test module; it must collect all three tests and only those`
  )
})
