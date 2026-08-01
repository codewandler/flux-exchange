---
id: X-19
title: "The cleartext check parses an authority the way the client that sends the secret does"
status: done
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
- [x] **Failing-first test** — `http://evil.example\@127.0.0.1/token` is **refused** at startup. It
      currently passes, so the test fails before the fix.
- [x] A table of hostile authority spellings is refused, including the fifteen the reviewer already
      confirmed conservative, so the class is pinned rather than the one instance.
- [x] Genuine loopback spellings still pass — `http://127.0.0.1:8080/token`, `http://localhost/token`,
      `http://[::1]/token` — asserted in the same run, so the fix cannot pass by refusing everything.
- [x] `host_in`'s doc comment states a guarantee that is **true**. If the implementation cannot make
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

## Progress
- **Done 2026-08-01.** Gate green: 39 + 159 tests, clippy clean, fmt clean. Genuine merge-base
  failure — the hostile URL was admitted at `4395ffa` and the test failed there without any new
  symbol.
- **The class was measured, not assumed.** A throwaway crate outside the workspace ran **475,270**
  generated spellings through the shipped parser, the candidate, and real `url` 2.5.8:
  - shipped parser: **15** endpoints admitted that `url` dials to a remote host over `http` — all
    backslash. The X-17 review found 3; the corpus also turned up `http://.\@…`, `http://%2e\@…`,
    `http://0x7f\@…` and `http://ⓛ\@…`, the same mechanism with a different left-hand side.
  - new parser: **0** admitted, and 0 that it admits while `url` refuses to dial at all.
  - **15,784** remain in the *refusing* direction — `2130706433`, `0x7f.0.0.1`, trailing dot,
    `loc%61lhost`, `ⓛocalhost`, whitespace `url` strips. The operator sees a startup refusal and the
    secret is unsent, and the doc now names these rather than implying they do not exist.
- `host_in` returns `Option` now, so **"this module cannot say" is a value** rather than an empty
  string that happens not to be loopback. It follows WHATWG at the three places WHATWG decides: `\`
  joins the authority terminator set; a bracketed literal must close and parse as an `Ipv6Addr` with
  only a port after it; a port must be ASCII digits fitting a `u16`.
- **The doc no longer makes the absolute claim.** It promises one direction — whenever `host_in`
  returns a host, `url` returns the same host — names the working configurations it refuses, and
  says the agreement is **measured, not proved**. That is the honest statement, and it is the one
  the implementor argued for rather than restoring the stronger sentence.
- **New refusals X-17 did not have**, and the first things to check if an operator insists a config
  is fine: the port rule (`127.0.0.1:8080` fine, `:+80` and `:99999` refused) and the IPv6 rule
  (`[127.0.0.1]` refused, as `url` also refuses it).
- **Still open, deliberately:** `https` short-circuits before `host_in` runs, so
  `https://evil.example\@127.0.0.1/token` is admitted and dialled to `evil.example` over TLS. That
  is correct under X-17's rule, which is about *transport* and not *destination* — the secret is
  protected in transit and the cert is checked. This module vouches for the channel, never for who
  is on the other end. If destination is meant to be a guarantee, it is a separate story.
- The `url` crate remains the only thing that would give back an absolute rather than a measured
  guarantee. Not taken here — the manifest is fenced and the admit-direction hole is closed either
  way — but it is a live follow-up rather than a closed question.
