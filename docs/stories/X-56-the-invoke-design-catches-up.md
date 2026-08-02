---
id: X-56
title: "The invoke design says what the locks now do"
status: done
priority: 3
epic: invoke
areas: [exchange-host]
note: "found by X-48 round 2: docs/designs/invoke.md §2 still describes lock 2 as X-12 shipped it. The design and the test have drifted, and the test is the accurate one"
---

# The invoke design says what the locks now do

## Goal
`docs/designs/invoke.md` §2 describes the locks that exist.

## What drifted

§2 describes lock 2 as X-12 shipped it: three bullets about `connector_pack::pack`, `Egress` counted
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
- [x] §2 lists the rules that exist, and says for each what it catches and what it cannot.
- [x] The name-versus-value limit is stated in the design, not only in the test's module doc.
- [x] The four-mechanism argument (lock 1's allow-list, lock 2's names, lock 3's counting transport,
      the composition's posture) appears once, in the design, with the test's doc pointing at it
      rather than restating it — two copies is how this drifted.
- [x] Whatever [[X-55]] decides about the scan boundary lands here in the same pass, or this story
      waits for it.

## Notes
- No behavioural change. The value is entirely in what the next reviewer does not have to rediscover.

## Progress

**Done, on `impl/X-56`.** The section is **§2**, not §3 — `## 2. The dispatch path` is where the
locks live and §3 is credential resolution and redaction ordering. The story text said §3
throughout, and so did seven citation sites; all are corrected. `docs/stories/README.md` mirrors the
frontmatter note and is regenerated at integration, so it is deliberately untouched.

**Acceptance 4 was already satisfied when this started.** X-55 landed (`3ee3698`) and wrote its
decision — the locks bound the published crate, `crates/exchange-host/src`, widening rejected — into
the design under "Where the locks stop". Nothing was redone; that subsection is unchanged.

**Acceptance 1 and 2 are checked rather than reviewed.**
`no_second_request_path::the_design_says_what_every_lock_2_rule_is` builds the rule list out of
`mod rules` and fails unless the design's "What lock 2 is, and what it is not" subsection names
every one of them and carries `claim::THE_NAME_LIMIT` verbatim. It is a presence check over one
section and says so; what it buys is that a rule added to the scanner is a red test at the moment
somebody knows what it catches. `the_design_check_catches_what_it_claims_to` drives it over
fixtures, including the direction that decides it: markers present in the file but absent from the
section must fail, because `ToolContext`, `reqwest` and `Egress` all appear elsewhere in the design
already.

**The argument is three mechanisms, not four** — this story's Acceptance 3 says four, and X-55
struck the composition's sandbox posture from the count before this ran. The design says so in as
many words under "Three mechanisms, and they fail differently", which is now its only copy; the
test's module doc points at that subsection by name and keeps only the one sentence a reader of the
rules needs at the point of use.

**One small code change came with it:** `rules::TRANSPORT_PORT` replaces the `"Egress"` literal in
`violations`, so the rule list is data the design check can read.

**Section names are load-bearing now.** `crates/exchange-server/src/execution.rs` cites the test's
"What lock 2 is, and what it is not" by name, and `claim::THE_LOCK_2_SECTION` requires the design to
carry a subsection spelled the same. Rename one and rename the constant with it.
