---
id: X-59
title: "A deployment can hold one tenant and stop asking which"
status: ready
priority: 2
epic: local-identity
design: docs/designs/local-identity.md
areas: [exchange-server, exchange-host]
note: "the --dev slice shipped; ready for the remaining provider-independent single-tenant startup declaration"
---

# A deployment can hold one tenant and stop asking which

## Goal
Running this for yourself does not mean inventing a tenant id.

## Why this is separate from [[X-58]], and second

The request that prompted this epic framed static users and "no multi-tenancy" as alternatives. They
are **two axes**, and all four combinations are legitimate — local users on a multi-tenant host is a
self-hosted team; OIDC on a single-tenant host is one company's deployment.

**Authentication is what blocks console use. Tenancy is a convenience.** So this is second, and it is
not a prerequisite for anything.

## What it must not become

`Deployment::SingleTenant` already exists — `admit_runtime` takes it, `runtime.rs` distinguishes it
from `MultiTenant`. This extends that; it does not invent a mode.

**Single-tenant is one tenant, not no tenant.** Every credential address is
`tenants/<tenant>/<authority>/<credential>`, and a mode that omitted the segment would write
credentials where nothing else looks for them — the same stranding upstream's instance elision exists
to avoid, and a one-way door for anybody who later grows a second tenant.

So: one tenant, **named once at startup**, and every principal is of it. The address is unchanged.

## Acceptance
- [x] `flux-exchange --dev` declares the single tenant `dev` and resolves the startup user's
      principal as `user:${USER}@dev`; from Cargo the spelling is `cargo run -- --dev`, because Cargo
      forwards binary arguments only after `--`.
      → `tests::dev_declares_the_startup_user_and_one_dev_tenant` and the process-level smoke run.
- [x] Following **Sign in** under `--dev` offers a real browser POST action for that sole implied
      principal, establishes an HttpOnly session cookie and returns to the console; it never prints
      the handle or puts the token in a readable body. Explicit rosters keep the bearer exchange.
      → `tests::dev_signin_is_a_real_browser_action_and_not_an_instruction_page`.
- [ ] A deployment declares one tenant at startup and every principal it resolves is of that tenant.
- [x] **The credential address is byte-identical** to what a multi-tenant deployment renders for the
      same tenant. Assert it literally, the way `tests/engine_line.rs` asserts the rendered address —
      this is the property that keeps the door open.
      → `tests::dev_credentials_keep_the_multi_tenant_address_layout` pins
      `tenants/dev/com.zendesk.api/api_token` literally.
- [x] **Failing-first test** — nothing in a request can name a tenant, in this mode or any other. This
      already holds; pin it here too, because a single-tenant mode is where somebody would be tempted
      to accept one "since there is only one".
      → `tests::dev_resolves_the_startup_user_to_dev_and_no_request_can_rename_it` sends hostile query,
      header and body claims through the new startup composition and still resolves `dev`.
- [x] Moving a single-tenant deployment to multi-tenant does not strand a stored credential. State how
      in the design, and test it if it is testable.
      → the address-layout test above and `docs/designs/local-identity.md` state why removing the flag
      leaves the same `tenants/dev/...` address.
- [x] The console does not show a tenant column that always says the same word, and the descriptor
      does not publish the tenant's name — it is tenant-specific and `GET /api/onboarding` is
      anonymous.
      → the console has no tenant listing column, and the existing
      `nothing_tenant_specific_can_reach_this_page`/`the_document_is_identical_with_two_tenants_connected`
      tests keep the descriptor tenant-free.

## Notes
- The honest gain is small and worth saying so: it removes a made-up id from the getting-started path.
  It does not remove tenancy from the model, and it must not.

## Progress
- **2026-08-02 — owner chose the concrete local-development shape.** `--dev` is the single-tenant
  declaration and implies `user:${USER}@dev`; an explicit `FLUX_EXCHANGE_DEV_IDENTITY` roster stays
  the multi-tenant development path.
- **The local slice is delivered and walked.** `cargo run -- --dev` announced
  `timo -> User:timo@dev`, selected `Deployment::SingleTenant`, admitted the loopback bind and served
  the API. The full Rust, console and public-site gates pass. What remains before the story itself is
  done is the broader, orthogonal deployment declaration: selecting one tenant independently of how
  that deployment authenticates, including OIDC and X-58's future verified local users.
- **2026-08-02 — the browser dead end is closed in v0.14.1.** The sign-in page used to explain a
  bearer request a browser link could not make. `--dev` now offers a POST button for its sole implied
  principal; explicit rosters remain manual so a form cannot choose between local identities.
- **2026-08-03 — status reconciled.** No implementation is currently in flight. The delivered
  `--dev` slice remains released, while the unchecked provider-independent startup declaration is a
  concrete ready follow-up rather than a permanently active lane.
