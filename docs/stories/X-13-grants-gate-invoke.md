---
id: X-13
title: "Grants gate invocation"
status: blocked
epic: invoke
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
