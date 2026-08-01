---
id: X-76
title: "A vendor behaviour no document declares is a named quirk of one endpoint, never a field on the vocabulary"
status: ready
priority: 3
epic: credential-acquisition
design: docs/designs/credential-acquisition.md
areas: [exchange-server]
note: "owner-decided 2026-08-02: babelforce's token endpoint DOES take expires_in, with different semantics per grant, and account_id switches the account on refresh — neither is in the vendored spec. So it is a quirk of that endpoint, not a TTL field every acquisition inherits"
---

# A vendor behaviour no document declares is a named quirk of one endpoint, never a field on the vocabulary

## Goal
babelforce's undeclared token-endpoint behaviours are usable and written down as **quirks of that
endpoint**, so no general "requested lifetime" field enters the acquisition vocabulary that every
other connector would then be assumed to honour.

## What was measured, and it overturns this story's first version

Filed 2026-08-01 as *"the owner says a TTL parameter exists and the vendored specification has no such
field — ask the API owners."* Half of that was right. The specification has no such field. **The
parameter is real**, and it is read straight out of `params` in babelforce's own `AuthController.token()`,
which is why nothing in the OpenAPI document could show it.

Measured against the babelforce auth service, 2026-08-02:

| Grant | `expires_in` on the **request** | Semantics |
|---|---|---|
| `client_credentials` | read, **defaulting to `-1`** | `-1` means *never expires* |
| `password` | read when present | otherwise the service's own default |
| `refresh_token` | read, and passed into the refresh | — |
| `link` | read, then **clamped to at most 60s** | a fifth grant type, not in the document either |
| `authorization_code` | **not read at all** | only `access_type`, defaulting to `offline` |

And the one the owner named as the precedent: on the **`refresh_token`** grant, **`account_id`
switches the account** the new token belongs to. The vendor's own source comments it exactly that
way. There is no reading of RFC 6749 under which a refresh changes whose token it is; that is the
definition of a quirk.

Two more, recorded because they are the same category and cost nothing to write down now: the
response carries **`expire_time`** (absolute UTC milliseconds) beside the standard `expires_in`, and
`GET /oauth/tokeninfo` exists. Neither is in the vendored document.

## The rule this story exists to enforce

**Owner-decided 2026-08-02: if it is not in the specification, it does not become a general thing.**

That is the correct call and the table above is the argument for it. A general `requested_ttl` on the
acquisition vocabulary would be wrong in five different ways at once against a single vendor — it
would be a hard cap on `link`, silently ignored on `authorization_code`, the difference between a
one-hour token and an immortal one on `client_credentials`, and it would invite every other connector
to be assumed to honour a field none of them declares. Worse, it is the failure mode this repository
already has a name for: *a marking nothing reads is worse than none, because it reads as safety while
changing nothing.*

**A quirk is confined, named, and attributed.** flux-connectors already carries the word and the
discipline for it — `quirks.pagination` and `quirks.rate_limit`, described there as *declarations, not
behavior*. This is the same shape one seam over: a quirk of one connector's **auth surface** rather
than of one operation.

## Acceptance
- [ ] No lifetime field, TTL or expiry-request enters `AuthHazard`, the acquisition vocabulary, or any
      type [[X-75]] adds. A test or a design note states the absence, so it is a decision rather than
      an omission.
- [ ] The behaviours above are declared as **quirks of babelforce's token endpoint**, per grant, with
      the grant that ignores the field recorded as ignoring it — an empty cell in that table is a fact,
      not a gap.
- [ ] **Failing-first test** — a quirk declared for one connector's auth surface is not applied to
      another connector's. Write the leak first: declare the quirk on babelforce, acquire against a
      second connector, and assert the parameter is absent from the request that goes out.
- [ ] Whatever this host sends is **attributed** where a reader will find it: the vendored document
      does not declare it, the vendor's implementation does, and the date of the measurement is on the
      record. A parameter sent on the strength of nobody-remembers-who-said-so is how the
      `X-Auth-Access-Id` question upstream became unanswerable.
- [ ] Raised with the babelforce API owners as a **documentation** gap rather than a question: five
      request behaviours and two response fields their own OpenAPI documents do not declare. A quirk
      that becomes spec should stop being a quirk.

## Progress
- 2026-08-01 — filed as an open question against the vendored specification.
- 2026-08-02 — **answered from the vendor's implementation, and the story rescoped.** The parameter
  exists; the document is what is incomplete. Owner-decided the same day: quirk of the endpoint, not a
  field on the vocabulary.

## Notes
- Upstream counterpart: **C-440** in flux-connectors, which is where the declaration lands — its
  `[[auth]]` block is the only place that can say *this connector's token endpoint behaves this way*
  without every other connector inheriting the claim.
- The `link` grant and `/oauth/tokeninfo` are **out of scope here** and recorded only so the next
  reader does not re-measure. [[X-75]] uses `password`, and refresh uses `refresh_token`.
- Deliberately **not** in this repository's docs: the vendor's internal client names and their scope
  assignments, which sit beside this code and are nobody's business here.
