---
id: X-50
title: "A connector that needs no credential can actually be connected"
status: backlog
epic: connections
areas: [console, exchange-server]
note: "found adjacent to X-49, 2026-08-01: the console disables Connect for a connector that declares no credentials — a state X-46 made reachable and X-49 pinned the *render* of, but nobody can act on it"
---

# A connector that needs no credential can actually be connected

## Goal
An operator can connect a connector that declares no credentials, or is told plainly why not.

## What was found

X-49 pinned the `declares-nothing` render: `freshdesk` publishes operations and declares no
credential, arrives at the console as `ready` with `credentials: []`, and now renders a note saying
so rather than an empty form. That is the *display* half.

The **act** half is unbuilt. `Connect.mts` disables the submit button when `names.length === 0`, so
the one connector in the shipped catalogue that reaches this state cannot be connected at all. The
page explains itself and then refuses to do anything.

This predates X-46 — before it, a connector declaring nothing arrived as `refused` and produced the
same empty `names` — so nothing regressed. It became *visible* when X-46 made the state legible, and
X-49 deliberately did not touch it, because a coverage story that changes behaviour is two stories in
one commit.

## The question this has to answer first

**Is a credential-less connection a thing this platform has?** The whole model is
`tenants/<tenant>/<authority>/<credential>` — a connection *exists* exactly when the store holds a
value at a derived address (`docs/designs/connections.md`). A connector with nothing to store has no
address to occupy, so under the current model it cannot be connected, and the disabled button is
**correct**.

If that is the answer, the fix is one sentence in the note, not a working button — and the note is
currently silent about it, which is the actual defect.

If it is not the answer, then a connection is no longer "a value in the store" and that is a change
to the epic's central claim, not a console tweak.

X-47's per-connection settings sharpen this: `freshdesk` is *also* one of the four connectors whose
templated value is its whole destination authority, so it is refused there too. A connector that can
be neither configured nor connected may simply be one this host does not serve — and saying that is
better than two independent refusals that never mention each other.

## Acceptance
- [ ] The question above is answered in `docs/designs/connections.md`, not in a comment.
- [ ] Whichever way it goes, **the console says it**. A disabled control with no explanation is the
      thing being fixed, and it is fixed either by the control working or by the note saying why it
      cannot.
- [ ] **Failing-first test** — the chosen behaviour is pinned. If connecting stays impossible, a test
      asserts the note names the reason; if it becomes possible, a test drives it end to end.
- [ ] If a credential-less connection is refused, the refusal and X-47's configuration refusal for the
      same connector **agree** — one story about `freshdesk`, not two.

## Notes
- Also found adjacent to X-49 and deliberately left there: `the_declaration_never_says_whether_anyone_holds_it`
  (`crates/exchange-server/src/routes/catalogue/view.rs:641`) walks `credentials` with no non-vacuity
  counter, and that inner loop **is vacuous for `freshdesk` today**. Same class as the hole X-49
  closed one test above it. Cheap; do it here rather than filing a third story.
- The section header in `console/test/connect.test.mjs` reads "4. The three states, kept apart" while
  `DeclarationState` has had four states since X-46.
