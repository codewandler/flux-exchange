---
id: X-48
title: "The invoke composition's safety claims are as strong as its code"
status: ready
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
- [ ] **Failing-first test** — removing the runtime gate from `Invoker::invoke` fails a test. That is
      finding 4, and it is the one with a real hole behind it.
- [ ] The sandbox posture is **chosen explicitly and written out**, like the two settings beside it,
      whichever way it is chosen. If disabled is right for this composition, say why in the code; if
      it is not, change it. **Do not leave it inherited.**
- [ ] The `spawner` comment either becomes true or is replaced by what is actually true.
- [ ] Lock 2 closes the `ctx.system()` path, **or** its doc states plainly that it catches names and
      not values, and names what covers the rest. A guard that overstates its reach is worse than one
      that admits its edge — three stories in this repository have now had to correct exactly that.
- [ ] The cosmetic ones, since they are in the same files: the 18-space run inside the startup
      refusal at `bind.rs`, the unused `Field` re-export (permanent public surface on a published
      crate), and `exchange-host`'s crate doc still saying "the service around them is not built".

## Notes
- **Nothing here is exploitable today** and the review said so. This is about the gap between what
  the code claims and what it enforces, which is the failure mode this repository has corrected more
  often than any other.
- The reviewer's residual on the `Sent` classification is **a port problem, not a code problem**:
  `Egress` is a public port, so a downstream composition whose transport returns `Error::Config`
  after dispatch would be told "not sent". Nothing in this tree does that. Consider whether a test
  driving a transport error through `classify` and asserting `Sent::Maybe` is worth having — the
  story that shipped it asked for a re-measure on every bump and gave nobody a way to do it.
- Also worth knowing, and deliberately **not** in this story's Acceptance: caller path parameters are
  not percent-encoded by the upstream evaluator, so a parameter can reshape the *path* on the
  declared host. The **origin** is unmovable, which is what X-12's Acceptance asked for — but if
  "the caller cannot name the destination" is ever read as covering the path, that needs an upstream
  story rather than a local patch.
