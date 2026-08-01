---
id: X-14
title: "Two instances of one connector, told apart by a name the operator chose"
status: ready
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
- (not started — design first)

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
