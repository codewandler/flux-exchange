---
id: X-56
title: "The invoke design says what the locks now do"
status: ready
priority: 3
epic: invoke
areas: [exchange-host]
note: "found by X-48 round 2: docs/designs/invoke.md §3 still describes lock 2 as X-12 shipped it. The design and the test have drifted, and the test is the accurate one"
---

# The invoke design says what the locks now do

## Goal
`docs/designs/invoke.md` §3 describes the locks that exist.

## What drifted

§3 describes lock 2 as X-12 shipped it: three bullets about `connector_pack::pack`, `Egress` counted
twice, and `flux_system::net`. Since then it has gained `Rehearsal` (X-47), `REACHES_THE_SYSTEM` and
`HOLDS_A_TOOL_CONTEXT` (X-48), and — the part that matters most — **the name-versus-value limit that
has now been the subject of two review rounds.**

The test is the accurate one. The design is what a reviewer reads first.

## Why this is worth a story rather than a drive-by edit

Two independent reviews spent effort rediscovering that lock 2 checks names and not values, because
the design does not say so. That is the cost being paid, and it is paid per review. It is also the
exact shape this repository keeps correcting: **a document claiming more than the code enforces**,
except here the document claims something *different* rather than something stronger.

## Acceptance
- [ ] §3 lists the rules that exist, and says for each what it catches and what it cannot.
- [ ] The name-versus-value limit is stated in the design, not only in the test's module doc.
- [ ] The four-mechanism argument (lock 1's allow-list, lock 2's names, lock 3's counting transport,
      the composition's posture) appears once, in the design, with the test's doc pointing at it
      rather than restating it — two copies is how this drifted.
- [ ] Whatever [[X-55]] decides about the scan boundary lands here in the same pass, or this story
      waits for it.

## Notes
- No behavioural change. The value is entirely in what the next reviewer does not have to rediscover.
