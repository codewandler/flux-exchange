// Where the dev server sends `/api`, resolved from the same setting the service reads.
//
// A module of its own rather than three lines inside `vite.config.ts`, because the thing worth
// asserting is the resolution and the config object cannot be asked about it without a dev server
// standing behind it. See `test/proxy.test.mjs` for what that buys.
//
// The environment arrives as an argument. That is what makes this testable without touching the
// process, and it is also why this file needs no `@types/node`: nothing here reaches for a global
// the console's type-check does not have.

/**
 * Where the service listens when nothing says otherwise.
 *
 * The same address as `DEFAULT_BIND` in `crates/exchange-server/src/bind.rs`, which is the module
 * that decides it — loopback, because a service holding other people's credentials that is
 * reachable by default is reachable before anybody decided it should be.
 */
export const DEFAULT_BIND = '127.0.0.1:8080'

/** The setting that overrides {@link DEFAULT_BIND}, spelled as the service spells it. */
export const BIND_ENV = 'FLUX_EXCHANGE_BIND'

/** Just enough of an environment to read one setting out of. */
export type Environment = Readonly<Record<string, string | undefined>>

/**
 * The environment this process was started with, or an empty one where there is no `process`.
 *
 * Read off `globalThis` rather than through a declared `process` global: the console's `tsconfig`
 * admits `vite/client` and nothing else, and a dependency on `@types/node` is far too much to pay
 * for one lookup.
 */
export function processEnvironment(): Environment {
  const host = globalThis as { process?: { env?: Environment } }
  return host.process?.env ?? {}
}

/**
 * The origin `vite dev` proxies `/api` to.
 *
 * The address is used as it was written. An unspecified bind (`0.0.0.0`) is not turned into
 * loopback and a malformed one is not corrected: this is a configured value, and repairing it here
 * would mean the dev server reaches a service the operator did not ask for — or, worse, silently
 * agrees with a bind the service itself refused to start on.
 *
 * A blank value is the one exception, and it is not a repair: `FLUX_EXCHANGE_BIND=` is how a shell
 * clears a variable rather than how it names a host, so it is read as "not set".
 */
export function apiProxyTarget(env: Environment = processEnvironment()): string {
  const bind = env[BIND_ENV]?.trim()
  return `http://${bind ? bind : DEFAULT_BIND}`
}
