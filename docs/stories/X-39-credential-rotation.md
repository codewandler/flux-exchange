---
id: X-39
title: "A credential can be rotated without a window where the connection is gone"
status: ready
priority: 2
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
- [ ] **Failing-first test** — a rotation replaces a credential's value, and the connection is
      readable and complete **throughout**: assert there is no observable state in which the tenant
      has no connection.
- [ ] Rotation is **explicit** — it names the connection it expects to exist, and is refused if it
      does not. It must not be reachable by accident from the create path, and `POST`'s `409` stays
      exactly as it is.
- [ ] A rotation that fails part way reports what it did, in the shape X-18 and X-20 established. It
      must not leave a connection half-old and half-new without saying so.
- [ ] The tenant's occupancy bound (X-22) is respected: a rotation to a larger value that would put
      the tenant over its allowance is refused, and the **old value survives** — a refused rotation
      must not destroy what it failed to replace.
- [ ] No refusal carries a credential value, and none names another tenant's address.

## Notes
- Decide whether rotation replaces one credential or the whole declared set, and argue it. A
  connector holding several credentials rotated one at a time and a connector rotated wholesale are
  different operations with different failure modes.
- Read `docs/designs/connections.md` first, especially the `409`/upsert argument — this story lives
  next to it and must not undo it.
- The store's write is an atomic whole-file replace. That is a property worth using rather than
  working around.
