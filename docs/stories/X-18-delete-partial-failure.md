---
id: X-18
title: "A delete that fails half way says what it destroyed"
status: ready
priority: 2
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
- [ ] **Failing-first test** — a `DELETE` whose *n*-th credential deletion fails answers with a
      refusal naming what was destroyed and what is still held. `TestStore` needs only a delete
      counter to drive it; no committed test currently drives `remove` through a mid-loop failure.
- [ ] The refusal uses the **same shape `partly_written` already established** for create, rather
      than a second vocabulary for the same idea.
- [ ] A `DELETE` that succeeds entirely is unchanged — `204`, nothing held — asserted in the same
      run so the reporting cannot pass by breaking delete.
- [ ] The module's stated rule ("a half-written connection is one an operator cannot tell from a
      working one") is stated for the delete direction too, in the code, not only here.
- [ ] Nothing in the refusal carries a credential **value**, and nothing names another tenant's
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
