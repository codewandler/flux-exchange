---
id: X-18
title: "A delete that fails half way says what it destroyed"
status: done
epic: connections
areas: [exchange-server]
note: "found by a standing audit of the credential surface, 2026-08-01: DELETE has no rollback and no partial-failure report, so a failed delete can leave a live vendor credential on disk while the operator is told only 'retrying may work'"
---

# A delete that fails half way says what it destroyed

## Goal
A `DELETE /api/connections/{connector}` that fails part way through names what it already destroyed
and what is still held, the way a failed create already does.

## What is wrong

`routes/connections.rs` deletes a connection's credentials in a loop and returns a generic
`store_failed` on the first error:

```rust
for (_, reference) in &addresses {
    if let Err(error) = store.delete(reference).await {
        return store_failed(&error);
    }
}
```

`create` goes to real lengths to make exactly this shape impossible for itself — it rolls back what
it wrote and reports whether the rollback succeeded, through `partly_written`, with the argument
written out beside it. `remove` has neither half.

**Reproduced deterministically over HTTP against the real `FileStore`**, no test double. `FileStore`
names its temporary `.<store>.<pid>.<counter>.tmp` and opens it `create_new`, so planting that exact
name makes one write — and only one — fail. With a connector holding two credentials and the second
delete made to fail:

```
DELETE /api/connections/slack
  → 503 "the credential store did not answer, so this host cannot say what this tenant has
     connected. Retrying may work"
on disk after:  tenants/acme/com.slack.api/signing_secret <hex>   ← the other one is destroyed
GET  /api/connections/slack  → 200, bot_token held:false, signing_secret held:true
POST /api/connections/slack  → 409 "already connected … delete the existing connection first"
```

Why it matters, in the order an operator meets it:

- **X-10's Acceptance sentence is false under partial failure.** "Deleting a connection destroys its
  credential." Here a *live* vendor credential survives. In the case a delete exists for — revoking a
  leaked secret — "Retrying may work" is the wrong thing to read while half the secret set is still
  on disk.
- **The refusal misinforms by omission.** `create`'s failure names `left_behind`; `remove`'s names
  nothing, so two failure paths on one surface tell an operator different amounts about the same
  class of event.
- **`GET` then answers `200` for a half connection**, which reads as "connected", and the retry is
  refused `409` pointing back at `DELETE`.
- **A crash mid-loop leaves the same state**, and nothing reconciles at startup.

A retry does converge — the second `DELETE` returned `204` and emptied the store — so this is an
honesty and revocation-guarantee defect, not a lost-data one.

## Acceptance
- [x] **Failing-first test** — a `DELETE` whose *n*-th credential deletion fails answers with a
      refusal naming what was destroyed and what is still held. `TestStore` needs only a delete
      counter to drive it; no committed test currently drives `remove` through a mid-loop failure.
- [x] The refusal uses the **same shape `partly_written` already established** for create, rather
      than a second vocabulary for the same idea.
- [x] A `DELETE` that succeeds entirely is unchanged — `204`, nothing held — asserted in the same
      run so the reporting cannot pass by breaking delete.
- [x] The module's stated rule ("a half-written connection is one an operator cannot tell from a
      working one") is stated for the delete direction too, in the code, not only here.
- [x] Nothing in the refusal carries a credential **value**, and nothing names another tenant's
      address — the existing disclosure guarantees hold unchanged.

## Notes
- Rollback is not available in this direction: a destroyed credential cannot be put back, because
  this host never held the plaintext to restore. So the answer is **honest reporting**, not
  restoration — which is what makes this different from `create`'s rollback and worth its own
  thinking rather than a copy of it.
- Consider whether `GET` should report a half connection as something other than a plain `200`. That
  may belong here or may be its own story; decide and say which.
- Audit note: the same pass ruled out address derivation, tenant confinement and refusal disclosure
  with live evidence — 18 hostile connector ids × 3 methods and 11 hostile credential names, all
  refused with the store untouched. This was the one real defect found.

## Progress
- **Done 2026-08-01.** Gate green: 39 + 158 tests, clippy clean, fmt clean. Genuine merge-base
  failure: the production code at the base was byte-identical, the four diff hunks were all inside
  `mod tests`, and the named test failed there with the story's reproduction verbatim.
- **The loop is now best-effort rather than stopping at the first failure.** The story asked for
  honest reporting and did not ask for this; the implementor argued it and is right — a `DELETE` is
  a revocation, so destroying two of three beats destroying one, and it is what makes the two lists
  complete rather than "one destroyed, two unknown". `rollback` is already best-effort for the same
  reason, so this is the module's existing posture.
- **The store failure's kind survives into the partial-delete refusal** instead of being flattened
  to 503, because answering a `Denied` with "retrying may work" would be a fresh instance of the
  exact misinformation this story is about. `store_failed`'s match was factored into
  `store_failure`; the three caller-facing sentences are unchanged and their guard tests untouched.
- `destroyed` and `left_behind` are computed **asymmetrically** and deliberately: `destroyed` is
  narrowed to addresses the pre-delete probe saw a value at, since calling an empty address
  "destroyed" would overstate what happened to someone counting revoked secrets; `left_behind` lists
  every failed delete regardless, because a failed delete is exactly the case where this host cannot
  say the address is empty.
- **Follow-on filed rather than folded in:** [X-20](X-20-create-failure-kinds.md), the same defect
  on the create side, which `store_failure` is now factored to make cheap; and
  [X-21](X-21-half-connection-visibility.md), whether `GET` can distinguish a damaged connection
  from a deliberately partial one — which needs a record beside the store that this module
  deliberately does not keep, so it is a design question and sits in `backlog`.
