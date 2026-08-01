---
id: X-48
title: "The invoke composition's safety claims are as strong as its code"
status: in-progress
priority: 1
epic: invoke
areas: [exchange-host, exchange-server]
note: "found by X-12's independent review, 2026-08-01: the sandbox silently takes a permissive default in the one function that writes two other settings longhand to avoid exactly that; a comment claims processes cannot be spawned when they can; and deleting the runtime gate from invoke breaks no test"
---

# The invoke composition's safety claims are as strong as its code

## Goal
Every safety claim the invoke path makes in a comment is either enforced or removed.

## What the review found

X-12 shipped in `v0.7.0` and was reviewed afterwards. The verdict was `PASS` and nothing was
exploitable — but four findings share one shape: **the code says something stronger than it does.**

### 1. The sandbox takes a permissive default, in the function that exists to avoid that

`execution.rs` builds the `System` with `System::new(workspace)`, which sets
`sandbox: Sandbox::disabled()`. Upstream's own doc on that constructor says *"Production entry
points should use `System::from_env`/`System::with_sandbox` instead."*

The same function writes `allowed_secrets: Some(Vec::new())` and `private_net: PrivateNetAllow::None`
out **longhand**, with a comment saying it does so *"because 'the default happened to be strict' is
not a property anybody can rely on"* — and then takes the permissive default one line away. That
inconsistency is the finding: the argument was made and not applied.

### 2. A comment claims a property the code does not have

> `ToolContext`'s spawner is left unbound, so no process can be spawned through it either.

`spawner` is the sub-agent seam. `ToolContext::system()` hands back the `System`, whose `run` and
`run_with_env` spawn processes and whose `read_file`/`write_file` reach the working directory.
**Latent** — nothing in the invoke path calls `ctx.system()` — but a false sentence is exactly what
stops the next reader noticing.

### 3. Lock 2 catches the crate name, not the value

`no_second_request_path.rs` forbids naming `flux_system` in host sources. But `exchange-host`
re-exports `ToolContext`, and `ctx.system()` yields the `System` by inference — so a host source can
reach process spawn and the filesystem **without writing any forbidden string**. No host source does
this today; the reviewer checked.

### 4. Nothing pins that `invoke` consults the runtime gate

`admit_runtime` is called before the store and before `resolve`, and that ordering is correct. But
every invoke-level test drives an `http` connector, and the refusal tests call `admit_runtime`
**directly**. **Deleting the `admit_runtime(...)?` line from `Invoker::invoke` breaks no test in the
workspace.** Acceptance item 6 is met in code with no tripwire.

## Acceptance
- [x] **Failing-first test** — removing the runtime gate from `Invoker::invoke` fails a test. That is
      finding 4, and it is the one with a real hole behind it.
      → `crates/exchange-host/tests/runtime_gate.rs`.
- [x] The sandbox posture is **chosen explicitly and written out**, like the two settings beside it,
      whichever way it is chosen. If disabled is right for this composition, say why in the code; if
      it is not, change it. **Do not leave it inherited.**
      → `SandboxMode::Require`, all three fields longhand, in
      `crates/exchange-server/src/execution.rs`'s `guarded_system`; pinned by
      `execution::tests::the_sandbox_posture_is_chosen_and_not_inherited`.
- [x] The `spawner` comment either becomes true or is replaced by what is actually true.
      → replaced; `guarded_system`'s "What this is **not**" section says what `ctx.system()` reaches
      and that an unbound `spawner` closes only the sub-agent seam.
- [x] Lock 2 closes the `ctx.system()` path, **or** its doc states plainly that it catches names and
      not values, and names what covers the rest. A guard that overstates its reach is worse than one
      that admits its edge — three stories in this repository have now had to correct exactly that.
      → both: `rules::REACHES_THE_SYSTEM` refuses `.system(` in host sources (self-tested), and the
      module doc's new "What lock 2 is, and what it is not" states the name-versus-value limit and
      names the four mechanisms that cover the rest.
- [x] The cosmetic ones, since they are in the same files: the 18-space run inside the startup
      refusal at `bind.rs`, the unused `Field` re-export (permanent public surface on a published
      crate), and `exchange-host`'s crate doc still saying "the service around them is not built".
      → the run and the crate doc are fixed. **`Field` is not unused** — see Progress; the finding is
      wrong and the re-export's doc now records why, so it is not filed a third time.

## Notes
- **Nothing here is exploitable today** and the review said so. This is about the gap between what
  the code claims and what it enforces, which is the failure mode this repository has corrected more
  often than any other.
- The reviewer's residual on the `Sent` classification is **a port problem, not a code problem**:
  `Egress` is a public port, so a downstream composition whose transport returns `Error::Config`
  after dispatch would be told "not sent". Nothing in this tree does that. Consider whether a test
  driving a transport error through `classify` and asserting `Sent::Maybe` is worth having — the
  story that shipped it asked for a re-measure on every bump and gave nobody a way to do it.
  **Not done here** — it is a `classify` test, not a safety-claim correction, and it wants its own
  story.
- Also worth knowing, and deliberately **not** in this story's Acceptance: caller path parameters are
  not percent-encoded by the upstream evaluator, so a parameter can reshape the *path* on the
  declared host. The **origin** is unmovable, which is what X-12's Acceptance asked for — but if
  "the caller cannot name the destination" is ever read as covering the path, that needs an upstream
  story rather than a local patch.

## Progress

**2026-08-01 — implemented on `impl/X-48`.** All five Acceptance items are ticked; one of them is
ticked with a correction rather than a change.

**Finding 4 was real and the hole was measured, not assumed.** At the merge base (`b208e53`),
deleting `admit_runtime(self.deployment, &ConnectorSurface::of(provider))?;` from `Invoker::invoke`
left `cargo test --workspace` at **326 passed, 0 failed**. The tripwire is
`crates/exchange-host/tests/runtime_gate.rs`.

**It reads source, and the reason is measured.** `admit_runtime`'s answer is a function of exactly
two values — the bound `Deployment` and the runtime the catalogue declares — and every catalogue
connector declares `http`, which both deployment classes admit. So no value reachable through
`Invoker::invoke`'s parameters makes the gate answer anything but `Ok`, and the refusal is not
behaviourally drivable from this side at all. The test therefore asserts presence **and order**
(gate before `Credentials::new`, gate before `connector_pack::resolve`), is self-tested against four
bodies it must reject and one it must accept, and its module doc states plainly that this is a claim
about source order rather than behaviour, and names what covers the rest. If a connector ever ships
declaring a locally-executing runtime, `invoke.rs`'s `the_whole_catalogue_declares_http` goes red
first, and at that point a real behavioural test becomes possible and should replace this one.

**Lock 2 was closed rather than only documented, and then documented anyway.**
`rules::REACHES_THE_SYSTEM` refuses `.system(` in `exchange-host/src`. It is a rule of its own rather
than a `FORBIDDEN` entry because `FORBIDDEN` structurally could not have caught it: it matches the
crate name `flux_system`, and `ctx.system()` yields the `System` through the re-exported
`ToolContext` without naming it. `ToolContext::workspace` is private and `WorkspaceContext::system`
spells `.system(` too, so the one call syntax is the whole door. No file is on an exception list.

**The sandbox was changed, not merely written out.** `SandboxMode::Require`, `network: false`,
`extra_writable: []`, every field named so an upstream addition is a compile error here. `Require`
means a spawn is confined by a real backend or `Sandbox::ensure_available` refuses it — the
fail-closed reading, matching "refuse; never repair". Costs one cached, process-global backend probe
at composition time.

**`Field` is not unused, and the finding is wrong.** `DeclaredSetting::field()` returns `Field<'_>`,
so without the re-export a composition could call that public method and not name its return type —
and matching on it would force the composition to name `connector-pack`, which is the one thing lock
1 exists to prevent. `tests/connection_settings.rs` already exercises it through the re-export. The
re-export's doc now records this so it is not filed a third time. **It was deliberately not
removed**; a reviewer expecting a deletion here should read that paragraph rather than assume the
item was skipped.

## Progress 2026-08-01 — merged, then sent back by its own review

Merged as `c06f56c`; gate green at 331 tests. **The independent review returned REWORK**, and the
findings are the story's own failure shape reproduced inside the file whose Acceptance says *a guard
that overstates its reach is worse than one that admits its edge*. Rework on `impl/X-48-r2`.

Each was demonstrated with compiling code, not argued:

1. **Lock 2's new doc names a method that does not exist and the door is open.** It claims
   `ToolContext::system` is the only accessor and cites `WorkspaceContext::system`. Against the
   pinned engine line, `ToolContext::workspace_context()` is public
   (`flux-runtime-0.46.0/src/lib.rs:1320`), `WorkspaceContext::active() -> Arc<System>` is public
   (`:1169`), and **there is no `WorkspaceContext::system` at all**. A source file doing
   `ctx.workspace_context().active().run(...)` type-checks, reaches process spawn, names no
   forbidden string, and leaves the whole workspace green.
2. **The four-mechanism section overstates lock 1**, and that overstatement is load-bearing —
   it is what covers lock 2's admitted blind spot. `dependencies_of` matches the literal line
   `[dependencies]`, so a `[dependencies.reqwest]` table escapes it entirely and lock 1 says nothing.
3. **`runtime_gate.rs` asserts a substring, not a call.** Three mutations leave the gate dead and all
   four tests green — a discarded result, a `if false` branch, and **no call at all**, just a string
   literal that mentions it. The file already classifies comments for exactly this reason and does
   not classify string literals.

The review independently confirmed the non-drivability claim rather than accepting it — 53
`Provider` literals in `connector-catalog-0.9.0`, every one `Runtime::Http`, that version pinned at
`Cargo.lock:310-311`, `Deployment::admits` returning `Ok` for `Http` under both classes. So there is
no behavioural backstop under this test, which makes finding 3 the serious one.

Confirmed sound and not to be re-done: the failing-first proof reproduces; the brace extractor fails
loudly rather than silently at 1–4 stray braces; `Require` is the strictest mode; `network: false`
matches upstream; `SandboxSettings` is not `#[non_exhaustive]`; nothing on the invoke path calls
`ctx.system()` today.

**Open for whoever owns the boundary:** lock 2 scans `crates/exchange-host/src` only, and
`guarded_system` — presented as the backstop for lock 2's blind spot — lives in the unscanned
`exchange-server`.

## Progress 2026-08-01 — rework round 1, on `impl/X-48-r2`

All three findings reproduced first, at `0d7c1f7`, before anything was changed. Finding 3's three
mutations each left `--test runtime_gate` at `4 passed`. Finding 1's proof-of-concept —
`crates/exchange-host/src/pinger.rs` calling `ctx.workspace_context().active().run(…)` — reached
process spawn with the **whole workspace green, lock 2 included**. Finding 2 reproduced with a
table-form dependency the ALLOWED list has no entry for: `5 passed`.

**Finding 3 — the claim moved out of the scanner and into the type system.** The three mutations are
not patchable in a text scanner; the third put the marker in a string literal, and classifying
string literals only moves the goalposts to the next thing text cannot see. So `admit_runtime` now
returns `Admitted` (`runtime.rs`), a struct with a private field, no public constructor, no
`Default` and no `Clone`, and `Admitted::resolve` is the **only** route from `invoke` to
`connector_pack::resolve`. All three mutations, plus forging the witness inside `invoke.rs`, are now
compile errors. The chain and — importantly — the one link nothing covers are written out on
`Admitted` itself: a future edit could still call `Admitted::of(deployment, Runtime::Http)` with a
hardcoded runtime, and no test would see it, because `Http` is the honest answer for every shipped
connector.

`runtime_gate.rs` kept the job it can actually do — **ordering**, which the compiler does not hold —
and its doc now opens by quoting the three mutations that defeated the previous version of itself.

**Finding 2 — the parser was made true and given the test it never had.** `header_of` classifies
`[dependencies]`, `[dependencies.name]` and both under `[target.…]`; `dev-`/`build-dependencies`
stay out of scope with reasons. The real defect was that **lock 1's parser had no self-test** while
lock 2's rules have had one since X-12, so nothing measured the gap between what it read and what
Cargo accepts. `the_manifest_parser_reads_every_shape_cargo_allows` drives it in both directions.

**Finding 1 — the instrument changed rather than the string list growing.** Chasing accessor
spellings is unwinnable: `.system(` was written believing it was the only door and
`.workspace_context().active()` walked past it. `HOLDS_A_TOOL_CONTEXT` bounds **possession** instead
— only the seam and the crate root may name `ToolContext`, the same two files that may name
`Egress` — so a file that cannot name the handle has nothing to call any accessor on. `.system(`
stays underneath as a cheap second net, explicitly demoted from "closes the door".

**The boundary question, answered.** `guarded_system` is in `exchange-server` and lock 2 never reads
it. That is now stated where it matters: the sandbox posture is a property of **this repository's
composition, not of the published crate**. A downstream binary implements `Contexts` itself and
supplies whatever `System` it built — quite possibly `System::new`, whose sandbox is disabled. For a
consumer of `codewandler-flux-exchange-host` that backstop does not exist, and locks 1–2 are the
whole of what ships. It is also narrower than "a spawn": the `Exempt` paths skip
`ensure_available` entirely.

**Both halves of the `guarded_system` claim were measured rather than asserted.** Reverting
`invoker` to `System::new(workspace)` leaves `the_sandbox_posture_is_chosen_and_not_inherited`
**green** and fails clippy with `function `guarded_system` is never used`. The doc now names
`dead_code` as the mechanism instead of implying the test is.
