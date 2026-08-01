---
id: X-39
title: "A credential can be rotated without a window where the connection is gone"
status: done
epic: connections
areas: [exchange-server]
note: "the surface can create and destroy a connection but not replace a credential: a second create is refused 409, so rotating a leaked secret today means DELETE then POST, and everything using it is broken in between"
---

# A credential can be rotated without a window where the connection is gone

## Goal
An operator can replace a credential value in place.

## What is missing

The connection surface has `POST` (create, refused `409` if one exists), `GET` and `DELETE`. There is
no way to **replace** a value. So rotating a credential — the single most common credential-lifecycle
operation, and the remedy for a leak — means `DELETE` then `POST`, with a window in between where the
tenant has no connection at all and anything relying on it fails.

The `409` is correct and must stay: `docs/designs/connections.md` argues at length that an upsert is
the silent overwrite the whole story exists to prevent. **Rotation is not an upsert.** An upsert is a
create that does not know whether it is replacing something; a rotation is an operator saying *replace
this, I know it is there*. The design doc's own note says rotation "needs a" — and stops.

X-18 sharpened the stakes: a partially-failed delete can leave a live vendor credential on disk while
the operator is told to retry. An operator with a leaked secret should not have to reach for delete at
all.

## Acceptance
- [x] **Failing-first test** — a rotation replaces a credential's value, and the connection is
      readable and complete **throughout**: assert there is no observable state in which the tenant
      has no connection.
- [x] Rotation is **explicit** — it names the connection it expects to exist, and is refused if it
      does not. It must not be reachable by accident from the create path, and `POST`'s `409` stays
      exactly as it is.
- [x] A rotation that fails part way reports what it did, in the shape X-18 and X-20 established. It
      must not leave a connection half-old and half-new without saying so.
- [x] The tenant's occupancy bound (X-22) is respected: a rotation to a larger value that would put
      the tenant over its allowance is refused, and the **old value survives** — a refused rotation
      must not destroy what it failed to replace.
- [x] No refusal carries a credential value, and none names another tenant's address.

## Notes
- Decide whether rotation replaces one credential or the whole declared set, and argue it. A
  connector holding several credentials rotated one at a time and a connector rotated wholesale are
  different operations with different failure modes.
- Read `docs/designs/connections.md` first, especially the `409`/upsert argument — this story lives
  next to it and must not undo it.
- The store's write is an atomic whole-file replace. That is a property worth using rather than
  working around.

## Progress
- **Done 2026-08-01.** Gate green: 44 + 188, clippy clean, fmt clean. Genuine merge-base failure.
  **Independent review dispatched** — this touches the published crate's public API and relaxes a
  structural guard.
- **The decision: rotation replaces ONE credential, not the declared set** — and the argument is the
  north star rather than convenience. The host never hands a credential value back out (`GET` answers
  addresses, never values), so a wholesale `PUT` would require a caller to re-send every value it
  wants to **keep**. An operator rotating one of `slack`'s two credentials has no way to obtain the
  other, so a body carrying only what they hold would destroy the rest. *A surface whose safe use
  depends on reading values back out cannot exist on the host whose claim is that the credential never
  crosses the boundary.* Per-credential also matches the failure it exists for: one secret leaks.
- **Separated from create structurally, not by discipline:** different path, different method, and an
  incompatible body type — all three must be deliberate. A test drives all five crossings and asserts
  the stored value is unchanged after each, then re-asserts `POST`'s `409` verbatim.
- **"Never gone" is asserted twice, one structurally:** a concurrent reader with the store's window
  widened never sees the connection incomplete, **and** `store.deletes() == 0` — a delete is the only
  operation that could empty the address, so counting to zero is the property.
- **No third `partly_*` refusal, deliberately:** one atomic `put` has no half.
- **Flagged for review, and the reason this got one:** `no_route_here_accepts_an_address` was relaxed
  by one name. That is a structural guard on the central invariant, and the implementor paid for it
  with a new adversarial test rather than an argument — which is right, and is exactly the claim a
  second context should try to break.
- **Carried forward:** the allowance arithmetic (`occupied() - old + new`, `saturating_sub`, plus a
  `store.get` sizing the old value whose not-found arm counts zero) is the only new numeric path.
  First place to look at a wrongly-refused rotation.
- **Filed as adjacent, not fixed:** a credential cannot be **added** to an existing connection.
  `POST` answers `409` and rotation correctly refuses when the value is absent, so an operator
  wanting `slack.signing_secret` on a connection holding only `bot_token` must `DELETE` and re-`POST`,
  destroying the credential they had. `nothing_to_rotate` says so plainly rather than naming a remedy
  that answers `409`. Worth a story.
- **Reviewed PASS**, independently rather than on the report: the reviewer built its own base proof,
  mutation-tested every guard (five mutations, all caught), and swept **18 hostile credential names**
  beyond the four in the diff — including another connector's declared credential at this connector's
  path. All refused `422` with the store byte-identical. `declares()` is exact byte equality against
  catalogue data and only `declared.leaf` reaches `CredentialRef::new`, so the path segment is a
  lookup key and never an address component. **The relaxation of
  `no_route_here_accepts_an_address` is genuinely paid for.**
- **Correction from the review, and worth knowing:** the concurrent-reader half of the never-gone
  test is **probabilistic**. Under a delete-then-put mutation it *passed* — only `store.deletes() == 0`
  caught the regression. The structural half carries the Acceptance item on its own, and the reader
  half should not be cited as the guard.
- Two stale claims the review found, both corrected at integration: `connections.rs`'s module doc
  said `writes` was "the **only** way supplied values become writes" while `write_of` is now an
  equally public entry point (the invariant held — `writes` delegates — but the sentence was
  load-bearing), and `README.md` still described connections as create/list/delete only.
