---
id: X-156
title: "Adopt the engine-free plan API and own the Tool projection"
status: ready
priority: 2
epic: catalog-artifact
areas: [exchange-host]
note: "The amended Decision 0022 point 3, resolved from X-151's open question into its own story: Exchange consumes connector-resolve's plan-as-data, owns the Tool/ToolSpec projection, and retires the ENGINE_LINE machinery in the same change — the end of the X-146 situation. Reviewed hardest against no_second_request_path"
---

# Adopt the engine-free plan API and own the Tool projection

## Goal

End the engine coupling: Exchange consumes `codewandler-connector-resolve`'s `resolve` — the
request plan as data — and owns the `Tool`/`ToolSpec` projection itself, so adopting a newer flux
stops requiring a `connector-pack` release compiled against it. X-151's Notes state the shape and
the stakes; this story is that section made dispatchable.

## Acceptance

- [ ] `exchange-host` depends on `connector-resolve` and derives every invoked operation's request
      from its `RequestPlan`; the `Tool`/`ToolSpec` projection is Exchange's own, and it
      **projects — never composes**: the plan's method, URL, headers, body, permission subjects
      and redaction set are carried, not reassembled. `no_second_request_path.rs`'s allow-list
      gains `connector-resolve` with a written sentence saying why a plan deriver is not a
      transport; that test is the one this story is reviewed hardest against.
- [ ] Behaviour preservation is proved, not trusted: a characterization test compares, for every
      catalogued operation, the request the wrapper-`Tool` path produced against the request the
      plan-projection path produces — byte-identical, the upstream differential's shape applied at
      Exchange's seam — written failing-first against a seeded divergence.
- [ ] Redaction survives the seam: every secret the plan carries as `SensitiveText` registers with
      Exchange's redaction exactly as the wrapper path registered it; no plan, log or `Debug` path
      prints a value. A test proves the registration, not the intention.
- [ ] The `ENGINE_LINE` marker, the three `engine_line.rs` tests and the "both pin sets move
      together" rule are retired **in this change**, with the history recorded in the CHANGELOG —
      they describe the `Arc<dyn flux_runtime::Tool>` coupling this story removes, and leaving
      them standing is folklore. The flux pins do not move here; what changes is that they *may*
      move independently afterwards.
- [ ] `connector_pack`'s `Tool`-returning wrapper is no longer called from any Exchange path
      (upstream C-541 retires the wrapper once this lands; this story is what un-gates it —
      note it in the story so the cross-repo edge is visible).

## Progress

- 2026-08-12: Filed by the cross-repo coordinator, resolving X-151's open question ("fifth child
  or a criterion inside X-152") in favour of its own story: the projection is the change
  `no_second_request_path` exists to guard, and burying it inside a settings migration would
  dilute exactly the review it needs most.

## Notes

- Depends on X-155 (the 0.23 pins). Adds a dependency, so it never shares a wave.
- Write set: root `Cargo.toml`/`Cargo.lock`, `crates/exchange-host` invoke/registry paths,
  `no_second_request_path.rs`, `engine_line.rs` (deleted), tests.
- One boundary from upstream's design, restated because it is the failure mode: *projecting a plan
  into a Tool is not composing a request* — a consumer that edits a plan has become the second
  request path this family already rejected.
