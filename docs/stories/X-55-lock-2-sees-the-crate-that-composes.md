---
id: X-55
title: "Lock 2 sees the crate that composes, or says why it does not"
status: done
priority: 2
epic: invoke
areas: [exchange-host, exchange-server]
note: "found by X-48 round 2: lock 2 scans crates/exchange-host/src only. exchange-server gets one rule and is otherwise unscanned — including execution.rs, which holds this composition's transport and sandbox posture"
---

# Lock 2 sees the crate that composes, or says why it does not

## Goal
The boundary of what the locks cover is a decision, not the shape of one `sources_under` call.

## What was found

`no_second_request_path.rs` scans `crates/exchange-host/src`. `exchange-server` gets exactly one rule
— it may not name `connector_pack` — and is otherwise unscanned.

That is arguably right: **`exchange-host` is the published crate, and locks 1–2 are the whole of what
ships.** A consumer binds its own `Contexts` and may well pass `System::new` with the sandbox
disabled, so the posture in `execution.rs` is a property of *this repository's composition* and not of
the package. X-48 round 2 wrote that down.

But it leaves a gap the same round created an argument around. `execution.rs` holds this
composition's transport **and** its sandbox posture, and the four-mechanism section presents that
posture as the backstop for lock 2's admitted blind spot — a backstop sitting outside what lock 2 can
see. Meanwhile a second request path added to `exchange-server` is caught by nothing structural.

## The question to settle

Are the locks about **the published crate** or about **the deployed binary**? They are different
claims and the repository currently makes the first while several documents read as the second.

If the answer is "the crate", then the honest fix is a sentence, not a wider scan — and the
four-mechanism section must stop leaning on a composition-level backstop to cover a crate-level
blind spot. If the answer is "the binary", the scan widens and `execution.rs` needs an exception list
with an argument per entry, the way lock 1's allow-list works.

## Acceptance
- [x] The question above is answered in `docs/designs/invoke.md`, with the argument, not in a comment.
      → §2, `#### Where the locks stop`: the question, the answer, the alternative and why it was not
      taken, what the answer costs, and what it forbids.
- [x] **Failing-first test** — whichever boundary is chosen, a violation on the wrong side of it is
      caught. If the boundary stays where it is, the test is that the *claim* narrows: no document may
      present a composition-level control as covering a crate-level gap.
      → `no_document_claims_more_than_the_locks_reach`, with `the_claim_scanner_catches_what_it_claims_to`
      driving the same pure function over prose it must reject and prose it must accept.
- [x] `AGENTS.md`'s statement of the invariant agrees with whichever answer is chosen.
      → the "This host constructs no request of its own" bullet now carries the decision verbatim,
      and the test refuses `AGENTS.md` if it stops doing so.

## Notes
- Do not widen the scan reflexively. `exchange-server` legitimately holds a transport — that is what
  a composition is for — so a naive widening turns lock 2 red on correct code, and the exception list
  it would need is exactly the "one more file on the list" drift the locks exist to avoid.

## Progress 2026-08-02 — the boundary stays, the claim narrows, on `impl/X-55`

**The answer is "the crate".** `exchange-host` is what `cargo publish` uploads; `exchange-server` is
`publish = false` and is one composition among the ones a consumer writes. Lock 2 still walks
`crates/exchange-host/src`, now via `claim::SCANNED` — one value, asserted by
`the_locks_bound_the_published_crate` — so widening it later is an edit that fails a test carrying
the argument rather than a quiet change to a path string.

**The claim was made falsifiable rather than merely rewritten**, which is what the Acceptance asked
for. `no_document_claims_more_than_the_locks_reach` reads `AGENTS.md`, `docs/designs/invoke.md` and
`no_second_request_path.rs` itself and holds them to two rules: each carries `claim::THE_DECISION`
verbatim — *"The locks bound the published crate, not the deployed binary."* — and none names a
control from outside the boundary (`guarded_system`, `SandboxMode::Require`) in a paragraph that
also leans on it. It reads prose only: `prose_of` is the mirror of `code_of`, because `claim`'s own
tables hold a control name and a coverage word as *code* three lines apart, and a scanner reading
code would refuse this file for declaring the rule it enforces.

**Failing-first, at `195a6bc`, with only the test added.** Four violations: all three documents
missing the decision, plus the module doc's four-mechanism bullet naming `guarded_system` and
`backstop` in one paragraph. The four-mechanism section is now three, all three inside the crate
that ships; the composition's posture is described where it lives and is not counted.

**The residual is now written down instead of implied.** A second request path added to
`exchange-server` — an OAuth exchange in a route, a pinger in `main.rs` — is caught by nothing
structural, because the one rule reaching that crate bounds *naming the pack* and not *building a
request*. Named in the design, in `AGENTS.md` and in the test's module doc as a review matter, so
nobody rediscovers it as a finding.

**Known limit of the claim rule, driven as a test rather than left to be found:** it is per
paragraph, so a control named in one paragraph and leaned on two paragraphs later is not caught.
Widening it to whole documents would refuse `docs/designs/invoke.md` for discussing lock 3 and the
composition in the same file, which is most of what that document is for.

**For [[X-56]].** Its fourth Acceptance item — *"whatever X-55 decides about the scan boundary lands
here in the same pass"* — is satisfied: the boundary decision and its argument are in
`docs/designs/invoke.md`. Its other three are **not**: the design still does not enumerate lock 2's
rules, does not state the name-versus-value limit, and the mechanism argument still lives in the
test's module doc rather than in the design with the test pointing at it. X-56 is unblocked and has
work left.
