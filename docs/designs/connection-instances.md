# Design: two instances of one connector

**Status:** accepted · **Epic:** `connections` · **Stories:** X-14

## Why

`docs/designs/connections.md` states the property this design has to extend without breaking:

> A connection exists exactly when the store holds a value at one of the addresses derived for that
> tenant and connector. **There is no second source of truth to disagree with the credentials.**

That is what makes `DELETE` destroying credentials not a step somebody could forget, and it is worth
more than any convenience. It is also why a tenant can hold exactly **one** connection per connector
today: nothing in the derived address varies per connection, so `acme.zendesk.com` and
`acme-eu.zendesk.com` render one address, the second write overwrites the first, and every call
afterwards gets a `200` from whichever account survived. A wrong answer, not a refusal.

## What upstream already settled, and what it left here

The address dimension **has landed** (upstream C-406, published in `connector-address` 0.9):

```text
tenants/<tenant>/<authority>[/@instances/<uuid>][/<service>]/<credential>
```

`CredentialRef::for_instance` spells it, and `TenantInstances::resolve` is the whole rule for when it
may be spelled:

| held | named | address |
|---|---|---|
| one (or the first) | anything consistent | **no instance segment** — byte-identical to today |
| several | one of them | that uuid |
| several | none | **refused**, naming the uuids that would have worked |
| any | one the tenant does not hold | **refused** |

Two consequences worth reading off that table before anything is built.

**The refusal on "several held, none named" is already the behaviour this story's Acceptance asks
for.** There is no default instance and no first match, because either would answer from whichever
account happened to be stored. Nothing here should add one back.

**The elision is not a courtesy, it is what makes this shippable.** Qualifying every address would
strand every credential already stored, everywhere, at once. So the un-instanced form stays the
address while a tenant holds one connection.

What upstream states is the *host's* job, and therefore X-14's: **resolving an operator's label to a
uuid.** `crates/exchange-host/src/connections.rs` says the same from this side and names
`address_of_declared` as the single seam where the level goes.

## The awkward thing this design has to resolve

`validate_instance` requires the canonical lowercase hyphenated 36-character uuid **and nothing
else** — braced, URN, unhyphenated and uppercase spellings are all refused, because they are one
connection at two addresses. So **an operator's label cannot be the instance segment.** The
attractive shape — call the connection `sandbox` and let `sandbox` be the path component — is closed
by the scheme, and closed for a good reason.

That forces an indirection: a `label -> uuid` map. And an indirection is exactly the "second source
of truth" this epic refused.

### The resolution: the record can fail to name a connection, and can never invent or hide one

The invariant is preserved by **splitting what each thing is authoritative for**:

- **Existence stays derived from the store.** The set of instances a tenant holds is enumerated by
  walking the addresses under `tenants/<tenant>/<authority>/@instances/`, exactly as the connections
  listing is derived today. `TenantInstances::held` is fed from *that*, never from the label record.
- **The label record is authoritative only for what an operator called a connection.** It is
  consulted to turn a name into a uuid, and for nothing else.

So the record cannot make a connection appear: a label pointing at a uuid the store does not hold
resolves to a uuid that is not in `held`, and `TenantInstances::resolve` already refuses precisely
that case, with a message naming what would have worked. And it cannot make one disappear: a uuid the
store holds with no label is still in `held`, still listed, and still addressable — it renders
unlabelled rather than becoming invisible.

**The strongest form of that claim is the test to write**: delete the entire label record and assert
that every connection is still listed and still reachable, only unnamed. A naming overlay that
survives its own deletion is not a second source of truth.

### What the indirection buys, which is the reason to accept it at all

**A rename moves no credentials.** The uuid is minted once and never changes; the label points at it.
Without the indirection, renaming a connection would mean rewriting every credential address under
it — a bulk credential move, triggered by a cosmetic act. With it, a rename is one map entry.

That is not a side benefit; it is the argument. An indirection that bought nothing would not be worth
the invariant it complicates.

## Approach

### 1. The caller names the label. It never names anything else.

`docs/designs/invoke.md` is unmoved: the caller cannot name the authority, the host, or the
credential address. An instance selector is a value the caller *does* supply, so it is bounded to the
one question it is allowed to ask — **which of my connections** — and never *what that connection
points at*.

The label is resolved **inside the tenant the principal was resolved for**. A label that does not
resolve there is a refusal, never a fallback.

Cross-tenant reach is refused **twice, independently**, and that redundancy is deliberate: the label
lookup is scoped to the principal's tenant so tenant B's label is simply absent in A; and even a
caller that somehow named B's raw uuid gets it checked against `held`, which was derived from A's
slice of the store. Neither check relies on the other being correct.

### 2. Where the label record lives

Beside the connection settings, not beside the credentials — same reasoning as `connection-settings`:
a label is not a credential, it is not stored as one, and its bounds are never summed with the
credential allowance. It is not secret, and unlike a setting value it **is** read back out: an
operator that cannot see what it named its connections cannot name one.

### 3. The migration is loud, and it is the risky part

The address is a function of how many connections the tenant holds, so **the day a second connection
appears, the first credential's address gains a segment** and the stored value has to move.

This is the only part of this story that writes credentials it did not create, so it gets the
tightest treatment:

- It happens **inside the same lock** the create decides and writes under — the same read-decide-write
  discipline `SettingsStore::set` uses. A migration that raced a rotation would move a stale value
  over a fresh one.
- It **fails closed**. If the move cannot be completed, the create fails and the first connection is
  left exactly where it was. A half-migrated tenant is worse than a refused create.
- The old address is deleted only after the new one reads back. The intermediate state — value at
  both addresses — is safe (the un-instanced address is what `resolve` elides to only when the tenant
  holds one, and by then it holds two, so nothing reads the old one); the reverse order is not.

### 4. What is deliberately not built

- **No default instance, and no "primary".** Upstream refuses the ambiguous case and this repository
  should not soften that. An operator with two connections that wants one of them to be the implicit
  answer is asking for the `200` from the wrong account.
- **No instance dimension in the catalogue or the descriptor.** How many connections a tenant holds is
  tenant-specific, and X-42's anonymous surface must not learn it.

## Alternatives considered

- **Make the label the address segment.** Closed by `validate_instance`, and rightly: two spellings of
  one name would be one connection at two addresses. Fighting that from here would mean a local
  address scheme, which is the thing `connections.rs` exists not to be.
- **Store the label as a credential-shaped value at a well-known leaf.** Keeps one store, and is
  wrong: a label is not a credential, would be counted against the credential allowance, and would be
  write-only like every other value there — so nobody could read back what they named.
- **Derive instances from the settings store instead of the credential store.** Rejected: settings are
  optional and most connectors declare none, so a tenant could hold a connection that the instance
  enumeration cannot see. Existence must be derived from the thing that always exists — the
  credential.
- **Version the address and qualify everything.** Rejected upstream and for the same reason here: it
  strands every stored credential at once, to buy uniformity nobody asked for.

## Risks & open questions

- **The migration is the sharp edge.** It is the first code in this repository that moves a credential
  it did not create. Its failure mode must be "the create was refused", never "the credential is
  somewhere else now".
- **A label collision within a tenant** must be refused at create, not resolved by suffixing. Two
  connections called `prod` is an operator error and guessing which is meant is the whole class of bug
  this story exists to close.
- **Label spelling** is validated where it is constructed, the way `Tenant::new` does. It is not an
  address segment, but it does reach a URL path and a stored map key, and the cost of validating is
  one function.
- **An agent configured with a label survives a rename only if the operator updates it.** The
  indirection makes the rename cheap for credentials, not for whoever was told the old name. Worth
  saying in the listing rather than pretending otherwise.

## Acceptance / done

X-14's Acceptance, unchanged. The shape it defers to this document: the caller names an
**operator-chosen label scoped to its own tenant**, the uuid is minted by the host and never named by
a caller, existence stays derived from the credential store, and the ambiguous case is refused rather
than defaulted.
