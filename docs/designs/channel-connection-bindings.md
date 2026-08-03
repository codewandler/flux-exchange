# Design: immutable connection bindings for generated channels

**Status:** accepted for X-122 · **Epic:** `generated-connector-channels`

## Decision

A channel record persists the host-minted connection UUID directly. It does not persist the
operator's mutable label and it introduces no second binding or connection-existence table.

The label is an authenticated management input only. At channel create or update, Exchange resolves
it through `ConnectionRegistry` inside the principal's tenant and intersects that answer with the
addresses returned by `SecretStore::references`. The record receives only the UUID already owned by
that held connection. A request body has no field for a UUID, authority, host, endpoint or credential
address.

Management views perform the reverse projection: the stored UUID is matched to the registry's
current label. Renaming therefore changes what an operator sees without rewriting the channel or a
credential, and the UUID never enters an anonymous catalogue or descriptor.

## Sole and plural connections

Omitting the label is admitted only when the tenant holds exactly one connection to the connector
and that connection has one registry row. This includes the legacy unqualified credential layout:
the row already gives the sole connection its future immutable UUID even though the address elides
it until a second connection is created.

Several held connections with no label are ambiguous and refuse. A sole legacy connection with no
label also refuses with an instruction to label it first: inventing an unnamed mapping would
contradict the requirement that a human chooses the label and would turn the registry into something
other than a naming overlay.

A UUID-shaped input is still only a label. Exchange never parses it as an address component; it must
resolve as a label within the caller's tenant and then prove that UUID is held there.

## Resolving a channel plan

The channel planner derives the current address layout on every start from credential references:

- when the record's UUID occurs in the instance-qualified inventory, credential and configuration
  ports are both bound with `for_instance`;
- when the inventory contains exactly one legacy connection, both ports use the unqualified
  compatibility constructors while the record retains its immutable UUID;
- a mixed layout, an absent connection, or an inventory containing other instances but not the
  bound UUID refuses before connector-pack prepares a socket plan.

This keeps one record working through first-to-second qualification and two-to-one collapse. It also
makes restoration deterministic: the persisted UUID, not label ordering or a first match, selects
the same connection after restart.

## Deletion refuses; it does not cascade

Deleting a connection refuses while a durable channel record binds its UUID. A cascade is rejected:
the credential, settings, registry and channel stores have no shared transaction, so claiming an
atomic cascade would be false.

Channel create/rebind and connection delete take the existing `(tenant, connector)`
`ConnectionGuard`. Under that claim, deletion checks the durable channel store before mutating any
credential. An instance delete checks the exact UUID; deleting a sole legacy connection refuses when
any channel for that tenant and connector exists. This is the same explicitly single-process
consistency boundary the connection surface already documents.

Removing a channel does not need the claim. A concurrent connection delete may conservatively
refuse after seeing the record, but it cannot delete first and admit a stale channel afterward.

## Persistent compatibility

Pre-X-122 records stored the connector id where the UUID now lives and contain no evidence of which
account they meant once several instances exist. The persistent decoder refuses those records rather
than choosing an account. Recreating the channel with an operator-selected label is the only safe
migration; guessing from the first registry row is the original defect under another name.
