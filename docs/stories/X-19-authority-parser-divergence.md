---
id: X-19
title: "The cleartext check parses an authority the way the client that sends the secret does"
status: ready
priority: 1
epic: serve
areas: [exchange-server]
note: "found by X-17's reviewer, 2026-08-01: `http://evil.example\\@127.0.0.1/token` passes `carries_a_secret_safely` as loopback, while the `url` crate reqwest actually uses resolves the host to `evil.example` — so the check clears a configuration that sends the client secret in cleartext to a remote host"
---

# The cleartext check parses an authority the way the client that sends the secret does

## Goal
The parser that decides whether a back-channel endpoint is safe agrees with the parser that
actually dials it.

## The divergence

X-17 refuses a cleartext token endpoint or key set unless the host is loopback, and `host_in` is a
hand-rolled authority parser because this workspace carries no URL crate. Its doc makes an absolute
claim:

> anything it does not understand comes out as a string that is not a loopback address … can
> therefore refuse a working configuration … but cannot admit a cleartext one.

**That claim is false for one spelling.** A backslash before the `@`:

```
FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT=http://evil.example\@127.0.0.1/token
```

- `carries_a_secret_safely` reads the authority as `127.0.0.1` — loopback — and admits it.
- `url` 2.5.8, which reqwest 0.13.1 depends on, ends the authority at `\` for special schemes and
  resolves the host to **`evil.example`**.

So the request carrying this host's client secret as HTTP Basic credentials goes **in cleartext to a
remote host**, past a check whose whole purpose is to stop exactly that. Measured by the reviewer,
both parsers in one program:

```
safe=true  url_scheme=http  url_host=evil.example   "http://evil.example\@127.0.0.1/token"
```

## Why this is priority 1 but not an emergency

The input is **operator-supplied configuration**, never caller-reachable, and no caller can induce
it. Before X-17 every `http` endpoint was admitted, so this is strictly tighter than what shipped
last week rather than a regression. What makes it worth doing now is that the code states a
guarantee it does not hold, and a guarantee that is wrong in one spelling is one nobody can rely on
in any spelling.

The reviewer tested the rest of the obvious class and **all of it errs conservative** — refused:
`127.0.0.1.evil.com`, `0x7f.0.0.1`, `0177.0.0.1`, `2130706433`, `127.0.0.1.`,
`[::ffff:127.0.0.1]`, `[::1%eth0]`, `localhost.`, `localhost.evil.com`, `ⓛocalhost`,
`127。0。0。1`, tab and newline variants, `127.0.0.1@evil.example`, leading whitespace, and every
`#`/`?`/`/` placement. The backslash is the one hole found.

## Acceptance
- [ ] **Failing-first test** — `http://evil.example\@127.0.0.1/token` is **refused** at startup. It
      currently passes, so the test fails before the fix.
- [ ] A table of hostile authority spellings is refused, including the fifteen the reviewer already
      confirmed conservative, so the class is pinned rather than the one instance.
- [ ] Genuine loopback spellings still pass — `http://127.0.0.1:8080/token`, `http://localhost/token`,
      `http://[::1]/token` — asserted in the same run, so the fix cannot pass by refusing everything.
- [ ] `host_in`'s doc comment states a guarantee that is **true**. If the implementation cannot make
      the absolute claim hold, the comment says what it actually promises.

## Notes
- Two defensible shapes, and the story does not mandate either:
  1. **Refuse any authority containing a character WHATWG treats as a delimiter** (`\`, and audit
     the rest of that set) before parsing. Cheap, and it fails closed by construction.
  2. **Take the `url` crate.** reqwest already depends on it, so it is in `Cargo.lock` today and
     costs no new third-party code — but adding it to *this* crate's manifest is a dependency
     decision, which is fenced. If that is the right answer, say so and stop; do not add it.
- Whichever shape: the invariant to state and test is that **the deciding parser and the dialing
  parser agree**, not that this one spelling is handled.
- The same `host_in` is used for both `_TOKEN_ENDPOINT` and `_JWKS_URI`. The JWKS URI carries no
  secret, so the consequence there is weaker, but the check should not diverge between them.
