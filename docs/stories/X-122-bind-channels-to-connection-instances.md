---
id: X-122
title: "Bind a generated channel to one immutable connection instance"
status: ready
priority: 1
epic: generated-connector-channels
areas: [exchange-host, exchange-server, console, docs]
note: "X-14 makes invocation instance-aware; generated channels still store the connector id as their connection and need a rename-safe UUID binding"
---

# Bind a generated channel to one immutable connection instance

## Goal

A persistent generated connector channel uses exactly one of a tenant's connections when several
instances of that connector exist, without accepting a host, authority, credential address or
caller-supplied UUID and without changing identity when an operator renames the connection.

## Why

X-14 makes ordinary invocation resolve `?connection=<label>` to a held, host-minted UUID inside the
principal's tenant. Channel records predate that dimension: their `connection` field is the connector
id, because only one connection could exist when X-101 shipped. Once credentials move below
`@instances/<uuid>`, a channel planned against the sole legacy address can no longer resolve safely.
Choosing the first held instance would send or receive against the wrong vendor account while
looking healthy.

## Acceptance

- [ ] Write a design first. Decide whether a channel record persists the immutable connection UUID
      directly or persists another stable Exchange-owned binding; a mutable label alone is not
      sufficient because renaming must not retarget or break a running channel.
- [ ] A signed-in human chooses an operator label at channel create/update time; the host resolves it
      inside the principal's tenant and persists no caller-supplied UUID, authority, host or
      credential address.
- [ ] **Failing-first test** — with two connections for one connector, two channels resolve distinct
      credential and configuration addresses and neither silently uses the first match.
- [ ] **Failing-first test** — tenant A cannot bind a channel to tenant B's label or UUID.
- [ ] Omitting a connection is valid only for a sole connection. Several held instances and no
      selector refuse as ambiguous; no default or primary instance exists.
- [ ] Renaming a connection moves no credential and does not retarget, stop or orphan a channel
      already bound to its immutable instance.
- [ ] Deleting an instance refuses while a durable channel still binds it, or atomically removes the
      channel binding under an explicitly designed cascade. No stale channel repeatedly retries an
      address that no longer exists.
- [ ] Restored channels after restart resolve the same instance they used before restart.
- [ ] The console and public management docs expose labels only and explain the rename/delete
      consequences without rendering a credential value.

## Notes

- Build on `ConnectionRegistry` and `SecretStore::references`; do not introduce another connection
  existence table.
- Keep `ChannelRecord` value-free. A stable UUID is an address component, not credential material,
  but it remains tenant state and must not enter the anonymous catalogue or descriptor.
- This story is a prerequisite for claiming generated channels support plural connector instances.
