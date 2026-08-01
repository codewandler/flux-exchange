---
id: X-13
title: "Grants gate invocation"
status: ready
epic: invoke
priority: 2
areas: [exchange-host, exchange-server]
note: "Selector and Grant are already tested types in exchange-host; this is where they become the thing standing between a principal and an effect"
---

# Grants gate invocation

## Goal
An operation runs only if a grant the caller holds admits it — decided from the operation's declared
metadata, not from a list of names.

## Acceptance
- [ ] Invocation consults the caller's grants and refuses with `Error::NotGranted`, naming the
      principal and the operation.
- [ ] **Failing-first test** — a read-only grant (`risk <= low`) admits the reads of a connector and
      refuses its writes, with no operation named anywhere in the grant.
- [ ] **Failing-first test** — a grant for one connector does not reach another.
- [ ] An explicit `deny` beats an explicit `allow`, end to end and not merely in the type.
- [ ] Grants are storable and editable per tenant.

## Progress
- (blocked on X-12)

## Notes
- The unit tests in `crates/exchange-host/src/grant.rs` already pin the semantics, including the
  deny-beats-allow asymmetry and subset-not-intersection on effects. This story wires them to a route.

## Unblocked, 2026-08-01

X-11 removed the engine-line blocker, so this is no longer waiting on upstream. It **is** ordered
behind X-12 — there is nothing to gate until something invokes — and that is an ordering edge inside
the epic rather than a block.

**Two stories have been shipped on the explicit promise that this one closes their gap**, and both
said so rather than pretending otherwise:

- `docs/designs/agent-access.md`: an agent token "authenticates and authorises nothing beyond what
  any principal may do", which X-40 narrowed to *except that it may not create a principal*.
- X-04's design records that `invoke` is gated by identity alone until this lands, and calls it "a
  stated gap rather than a position".

So this is not a new capability bolted on; it is the thing two shipped stories are waiting for.
