---
id: X-14
title: "Two instances of one connector, told apart by a name the operator chose"
status: blocked
priority: 5
epic: connections
note: "owner-raised 2026-08-01: a tenant with two Zendesk instances collides on tenants/<tenant>/<authority>/<service>/<credential> — the address has no instance dimension, so the second connection silently overwrites the first"
---

# Two instances of one connector, told apart by a name the operator chose

## Goal
One tenant can hold **two connections to the same connector** — two Zendesk subdomains, two Jira
sites, a sandbox and a production account — and name which one an invocation uses, without ever
naming a host or a credential.

## Why this is not a detail

The derived address has no instance dimension. `connector_spec`'s `credential_ref_for`
(`ir.rs:1268`) composes `tenants/<tenant>/<authority>/<service>/<credential>` from the tenant, the
connector's declared `authority`, the service and the credential's leaf name. **Nothing in it varies
per connection.** So a tenant that connects `acme.zendesk.com` and then `acme-eu.zendesk.com`
resolves both to one address: the second write overwrites the first, and every subsequent call goes
to whichever credential survived — with a `200` from the wrong instance rather than a refusal.

That is the same failure flux-connectors' C-226 records from the other side (one credential that
cannot be shared by two connectors); this is one connector that cannot hold two credentials.

## The constraint that makes this hard

`docs/designs/invoke.md` states the confused-deputy answer plainly: **the caller cannot name the
authority**, cannot name the host, and cannot name the credential. An instance selector is a value
the caller *does* supply, so it has to be introduced without reopening that door.

The shape that appears to hold — to be confirmed by the design, not assumed here: an instance name is
an **operator-chosen label scoped to the tenant**, and the caller names only the label. The host,
the authority and the credential address are all resolved *from the connection the label maps to*,
inside the tenant the principal was resolved for. The caller names *which of my connections*, never
*what that connection points at*. A label that does not resolve within the principal's tenant is a
refusal, not a fallback to a default.

## Acceptance
- [ ] A tenant can hold two connections to the same connector, and both work.
- [ ] **Failing-first test** — creating a second connection to a connector the tenant already has
      **does not overwrite the first**. This is the bug that motivates the story; assert it at the
      store, not only at the API.
- [ ] The credential address includes the instance, and two instances of one connector for one
      tenant provably render **different** addresses.
- [ ] An invocation names an instance by its label and nothing else. **Failing-first test** — a
      caller that supplies a host, an authority or a credential address is refused, and the refusal
      names the address rather than the value.
- [ ] **Failing-first test** — tenant A cannot reach tenant B's instance by naming B's label. The
      label is resolved *within* the principal's tenant, never globally.
- [ ] Exactly one connection, or a named default, is used when a caller names no instance — and
      whichever rule is chosen, an **ambiguous** case is refused rather than guessed. A tenant with
      two instances and a call that names neither must not silently pick one.
- [ ] Deleting one instance leaves the other's credential intact.
- [ ] The label's spelling is validated where it is constructed, the way `Tenant::new` already does
      — it becomes an address segment, so a traversing spelling is refused at construction.

## Progress
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
