---
id: X-14
title: "Two instances of one connector, told apart by a name the operator chose"
status: done
priority: 2
epic: connections
design: docs/designs/connection-instances.md
note: "delivered in v0.16: durable tenant-scoped labels, host-minted UUIDs, atomic first-to-second migration, per-instance settings/rotation and explicit invocation selection"
---

# Two instances of one connector, told apart by a name the operator chose

## Goal
One tenant can hold **two connections to the same connector** — two Zendesk subdomains, two Jira
sites, a sandbox and a production account — and name which one an invocation uses, without ever
naming a host or a credential.

## Why this is not a detail

The legacy derived address had no instance dimension. Without X-14, a tenant connecting
`acme.zendesk.com` and then `acme-eu.zendesk.com` resolved both to one address: the second write
would overwrite the first, and every subsequent call would go to whichever credential survived —
with a `200` from the wrong instance rather than a refusal. C-406 added the optional address level;
C-494 and connector v0.18 added the inventory, batch and instance-aware host ports needed to use it
without a partial migration.

That is the same failure flux-connectors' C-226 records from the other side (one credential that
cannot be shared by two connectors); this is one connector that cannot hold two credentials.

## The constraint that makes this hard

`docs/designs/invoke.md` states the confused-deputy answer plainly: **the caller cannot name the
authority**, cannot name the host, and cannot name the credential. An instance selector is a value
the caller *does* supply, so it has to be introduced without reopening that door.

**The design is now written**: `docs/designs/connection-instances.md`. Read it before starting — it
settles the shape below and adds three things this story could not know when it was filed:

1. **The address dimension already exists upstream.** `connector-address` 0.9 publishes
   `CredentialRef::for_instance` and `TenantInstances::resolve`, and `connections.rs` already names
   `address_of_declared` as the seam it goes in. The ambiguous case is *already* refused upstream —
   do not add a default back.
2. **A label cannot be the address segment.** `validate_instance` requires a canonical lowercase
   hyphenated uuid and nothing else, so the label→uuid indirection is forced rather than chosen.
3. **The migration is the sharp edge**, and is the only code here that moves a credential it did not
   create: the day a second connection appears, the first's address gains a segment.

The shape the design confirms: an instance name is
an **operator-chosen label scoped to the tenant**, and the caller names only the label. The host,
the authority and the credential address are all resolved *from the connection the label maps to*,
inside the tenant the principal was resolved for. The caller names *which of my connections*, never
*what that connection points at*. A label that does not resolve within the principal's tenant is a
refusal, not a fallback to a default.

## Acceptance
- [x] A tenant can hold two connections to the same connector, and both work.
- [x] **Failing-first test** — creating a second connection to a connector the tenant already has
      **does not overwrite the first**. This is the bug that motivates the story; assert it at the
      store, not only at the API.
- [x] The credential address includes the instance, and two instances of one connector for one
      tenant provably render **different** addresses.
- [x] An invocation names an instance by its label and nothing else. **Failing-first test** — a
      caller that supplies a host, an authority or a credential address is refused, and the refusal
      names the address rather than the value.
- [x] **Failing-first test** — tenant A cannot reach tenant B's instance by naming B's label. The
      label is resolved *within* the principal's tenant, never globally.
- [x] An invocation selects a connection with `?connection={label}` while its JSON body remains the
      operation's raw parameter object. Omitting the label works only for a sole connection; two
      instances and no label is an **ambiguous** refusal rather than a default or first match.
- [x] Management uses `/api/connections/{connector}/instances/{label}`. A human can label the sole
      legacy connection before creating a second, and renaming a label moves no credential.
- [x] Existence is derived from `SecretStore::references` under the tenant/authority scope. Deleting
      the label record cannot invent or hide a connection; every held UUID remains listed, unnamed.
- [x] The second create holds the tenant/connector lock and uses one checked `SecretBatch` to migrate
      the first connection and write the second. An unsupported or failed batch leaves the first
      byte-identical and refuses the create.
- [x] Deleting one instance leaves the other's credential intact.
- [x] The label's spelling is validated where it is constructed, the way `Tenant::new` already does
      — 1–64 ASCII alphanumeric, `-`, or `_` bytes. It is not the UUID address segment, and the host
      never accepts a caller-supplied UUID, authority, host or credential address.

## Progress

- **Delivered for v0.16, 2026-08-03.** Exchange consumes registry-only connector v0.18 and Flux
  v0.54.4. `ConnectionRegistryStore` persists tenant/connector-scoped label→UUID rows outside the
  worktree; credential inventory remains authoritative for existence. Label-scoped create,
  read/rename/delete, settings and rotation are live. The first-to-second and two-to-one transitions
  are checked atomic `SecretBatch` operations, and a failed transition is retryable with its inert
  pre-written UUID. Invocation resolves `?connection=` inside the principal tenant and rejects every
  caller-supplied address axis. Point-only stores retain the sole legacy surface and explicitly
  refuse plural management.
- **Blocked on upstream, 2026-08-01.** The instance dimension belongs in `connector_spec`'s
  `CredentialRef`, filed as flux-connectors **C-406** (an optional uuid, required when a tenant holds
  more than one integration of the same kind — owner-directed). It cannot be added here: this story's
  own Notes forbid forking the address scheme locally, because two spellings of an address is how two
  components stop agreeing where a credential lives.
- C-406 is itself sequenced behind flux-connectors **C-407**, which extracts the credential address
  vocabulary into its own crate — i.e. it moves the very types C-406 adds a component to. That work
  is in flight in another session.
- **UPDATE — C-406 has landed upstream and is merged to flux-connectors `main`** (`14b5dc7`,
  merge `9c740a1`). The blocker narrows from *unimplemented* to **unpublished**: crates.io still
  serves `codewandler-connector-spec` 0.8.0 and this repository pins `"0.8"` from the registry.
- **The address grammar, as landed** — build against this, do not re-derive it:
  `tenants/<tenant>/<authority>[/@instances/<uuid>][/<service>]/<credential>`
  - `InstanceId` is a validated newtype: canonical lowercase hyphenated uuid only, and the nil uuid
    is refused because "no instance" is already spelled by omitting the level.
  - The marker is `@instances` and the `@` is load-bearing — it is unspellable in every component
    grammar, so the level cannot be forged and no service or credential name is reserved away. A bare
    uuid segment would have been ambiguous with a service, since a uuid is a well-formed service name.
  - `TenantInstances` carries the fact the crate cannot derive — how many connections a tenant holds
    and which is named — and states the whole rule: elide at one, the named one at several, **refuse
    when several and none is named**, refuse a uuid the tenant does not hold. That refusal is
    Acceptance item 6 of this story, already enforced upstream.
  - `credential_ref_for` now takes `TenantInstances`; every existing call site passes
    `TenantInstances::sole()`, so no shipped address moved (`connector-cli diff`: 557 up to date).
- **The label → uuid mapping is explicitly ours.** Upstream recorded the split in
  `docs/designs/credential-addressing.md`: the label is tenant-scoped, mutable and renameable, and a
  compiled artifact must hold none of those. So this story is exactly that resolution plus threading
  a connection through — the caller names *which of my connections*, the host resolves it to the uuid.
- **What unblocks this now:** flux-connectors publishes (likely 0.9.0 — `credential_ref_for`'s
  signature changed, so it is breaking), and this repository moves its `connector-spec` pin. Note the
  **same release also closes X-11**, since C-403 already moved the engine line and `connector-pack`
  must be republished against it — one upstream release turns four blocked stories here into ready
  ones.

## Notes
- **Design first.** This changes an address scheme that `docs/designs/invoke.md` and X-10 both build
  on, and it touches the confused-deputy argument, which is the repository's north star. Write the
  design under `docs/designs/` before implementing.
- **X-10 must not land its address scheme without this.** X-10's Acceptance currently specifies
  `tenants/<tenant>/<authority>/<credential>` as *the* derived address; that spelling is what
  collides. Either X-14 lands first, or X-10 lands the instance dimension as part of its own work.
- The address is composed inside `connector-pack`, upstream of this repository. Check whether the
  instance dimension belongs in `connector_spec`'s `CredentialRef` (a change to file upstream, in
  flux-connectors) or whether this host can compose it into the leaf without upstream agreeing.
  **Do not fork the address scheme locally to get around an upstream gap** — a second spelling of an
  address is how two components stop agreeing about where a credential lives.
- `Tenant::new` (`crates/exchange-host/src/principal.rs`) is the precedent for validating a segment
  at construction. Build on it; do not re-validate ad hoc.

## Release checkpoint, 2026-08-03

flux-connectors C-494 now implements `CredentialScope`, address-only enumeration, checked atomic
secret batches and instance-aware credential/configuration host ports on its v0.18 candidate branch.
This story deliberately does not consume that working tree through a path or Git dependency. Start
the Exchange registry and migration only after the four connector crates are published together at
v0.18 and this repository can move both connector and Flux engine pin sets in one commit.

The public contract is now pinned before implementation: management resources are label-scoped,
invoke uses `?connection=` without wrapping the operation body, omission is sole-only, the host mints
the UUID, and the first-to-second transition is one C-494 `SecretBatch` under the connection lock.

## Unblocked, 2026-08-01

**The instance dimension is published and already in this repository's lockfile.** X-11 upgraded to
`connector-address` 0.9, which carries C-406: `CredentialRef` gained an optional
`@instances/<uuid>` level.

Two facts X-11 verified, which this story starts from rather than re-establishing:

- **`CredentialRef::new` still elides the instance level**, so today's addresses are unchanged and
  every existing connection keeps its address. That was asserted literally in
  `tests/engine_line.rs`, not assumed.
- **`TenantLayout::parse` now accepts 6- and 7-segment paths** — the instanced forms — where 0.8
  refused them as "has N segments". This repository never calls `parse`, only `render` via
  `address_path`, so nothing here changed. It would matter to anything that starts parsing
  operator-supplied paths, which is a thing this repository deliberately does not do.

The `409` refusal that names this story was corrected by X-11: it used to tell operators the
instance level "is not published yet; this host pins connector-spec 0.8", which is now false.
