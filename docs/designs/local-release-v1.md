# Local Exchange release protocol v2 (trust v1)

This filename is retained so existing story links remain stable. This document is the provider-owned
wire contract for X-126, X-128 and X-134. Exchange owns these names,
fields, bounds and conformance cases. Flux C-510 consumes them verbatim; a Flux document or type may
describe how it responds, but cannot define another shape under the same or a competing versioned
name. The long-lived root metadata remains `exchange.release-trust.v1`; channel, manifest,
compatibility and readiness use v2.

## Canonical JSON

Every document below is UTF-8 JSON, without a BOM, and its complete response body is byte-for-byte
the RFC 8785 serialization of one object. There is no leading or trailing whitespace, including no
final newline. Parsers refuse duplicate or unknown members before mapping into a type, numbers that
are not integers in the stated domain, invalid UTF-8 and a body whose reserialization differs.
The examples are pretty-printed for review; RFC 8785, not their visual member order, defines wire
object ordering.

Arrays are ordered as stated; changing their order changes the signed bytes. All SHA-256 values are
64 lowercase hexadecimal characters. Times are UTC RFC 3339 seconds (`YYYY-MM-DDTHH:MM:SSZ`), with
no fractional seconds. `origin` is always exactly
`https://github.com/codewandler/flux-exchange`.

Every JSON integer in this release contract is `0..=9007199254740991`, the largest integer interoperable across RFC
8785 implementations. A narrower field keeps its narrower bound. A value that may need all of a
platform `u64` is instead a canonical decimal string: ASCII digits only, no sign or leading zero,
1..=20 bytes, and numerically within the stated bound. `now` is read from an injected/testable UTC
clock. An interval is valid exactly while `not_before <= now < not_after` (or `issued_at <= now <
expires_at` for documents issued at or before `now`). A document issued in the allowed future skew
is valid exactly when `issued_at <= checked_add(now, 300 seconds) && now < expires_at`; failure of
the checked clock addition refuses the document. Equality with an expiry is expired. Delegated-key
intervals receive no issuance-skew allowance.

Strings which reach filenames or selection have closed grammars:

- a key id is 1..=64 bytes, matches ASCII
  `^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$`, and contains no `--`;
- a protocol id is 1..=128 bytes. Each dot-separated non-version token matches
  `^[a-z](?:[a-z0-9-]*[a-z0-9])?$` with no `--`; there is at least one such token and the final
  token is exactly `v[1-9][0-9]{0,8}`;
- a stable version matches exactly
  `^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$`; there is no prerelease or
  build suffix and the tag is exactly `refs/tags/v<version>`;
- a derived basename matches ASCII
  `^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,126}[A-Za-z0-9])?$`, contains no `..`, is 1..=128 bytes, and is
  unique under ASCII case-folding.

The exact eight protocol ids in this v2 release contract are:

| Field | Required id | Provider wire contract |
|---|---|---|
| `exchange_api` | `exchange.api.v1` | Service Account bearer authentication and the two HTTP routes below |
| `effective_catalogue_response` | `exchange.effective-catalogue-response.v1` | `GET /api/catalogue/effective` and `routes::catalogue::view::EffectiveCatalogue` |
| `invoke_request` | `exchange.invoke-request.v1` | `POST /api/operations/{operation}/invoke`, optional sole `connection` query, raw operation JSON body |
| `invoke_response` | `exchange.invoke-response.v1` | the success `exchange_host::Invocation` and closed HTTP refusal variants on that route |
| `connection_plan` | `exchange.connection-plan.v2` | X-134's non-secret plan and owner-bound submission fixture |
| `local_management` | `exchange.local-management.v1` | X-134's FXLM owner-management fixture |
| `service_account_handoff` | `exchange.service-account-handoff.v1` | X-134's exact one-frame FXSA handoff fixture |
| `supervisor` | `exchange.supervisor-ready.v2` | X-128's unchanged one-shot transport with the v2 inventory below |

X-129 binds the four already-delivered HTTP identities to those exact routes/types and bidirectional
wire tests. X-134 supersedes X-125's secret-bearing submission and binds the v2 plan plus the two
owner-bound protocols. An implementation cannot substitute a package version or advertise an id
whose provider fixture/type test is absent.

This exact eight-field object is the only publishable first-production inventory. The merged
six-field v1 object remains unpublished implementation evidence. Decision 0007 requires X-134 to
implement and revalidate these owner-bound local-management, direct-secret and one-shot Service
Account handoff bytes before X-126 publishes. The first public release cannot omit a field, add an
unknown ninth field or reuse an existing identity for changed semantics.

### Closed FXLM JSON vocabulary

`exchange.local-management.v1` has no implementation-selected JSON spellings. Every non-secret
control payload is one RFC 8785 object from the table below; every listed member is required, no
other member is admitted, and `null` is admitted only where the table says so. A fixture serializes
each positive object to its exact canonical bytes and then changes every member name, JSON type,
bound, enum value, omission and added member one at a time. Production parsing must fail those
mutations before coordinator, store, audit or writer mutation.

The closed scalar and collection types used by those objects are:

| Name | Exact JSON representation and bound |
|---|---|
| `Connector` | string, 1..=128 UTF-8 bytes, byte-equal to one released catalogue connector id |
| `Label` | string, 1..=64 ASCII bytes, each byte alphanumeric, `-` or `_` |
| `ServiceAccountId` | string with the same 1..=64-byte ASCII grammar as `Label` |
| `PlanRevision`, `TargetRevision` | string of exactly 64 lowercase hexadecimal characters generated by the domain-separated algorithms below; never parsed or ordered |
| `CredentialRevision` | opaque string of exactly 64 lowercase hexadecimal characters encoding one Exchange-coordinator-generated nonzero 256-bit value; it is unique within one held label's revision history, never parsed or ordered, and is not derived from credential presence, count, value, generation, time or another digest |
| `ProposalDigest` | SHA-256 encoded as exactly 64 lowercase hexadecimal characters |
| `TransactionId` | exactly 64 lowercase hexadecimal characters encoding 32 bytes allocated by the Exchange transaction coordinator; characters 1..16 decode as its big-endian `u64` generation in `1..=18446744073709551615`, and characters 17..64 encode its unique 192-bit nonce; `connector-secrets` validates/encodes the identity and owns its provider state but generates neither component; a zero first word refuses even when the nonce is nonzero |
| `ReceiptId` | opaque 256-bit value encoded as exactly 64 lowercase hexadecimal characters; all-zero refuses |
| `StoreRevision` | string containing the canonical unsigned decimal representation of `1..=18446744073709551615`: no sign or leading zero, 1..=20 bytes |
| `ExpiresAt` | string containing the canonical unsigned decimal representation of `1..=9223372036854775807`: no sign or leading zero, 1..=19 bytes |
| `Target` | string, 1..=512 UTF-8 bytes, byte-equal to a routable target id in the named plan revision |
| `SettingValue` | string, 0..=1024 UTF-8 bytes; it must also satisfy the named plan target's declared closed choices/normalizer |
| `Ordinal` | JSON integer `1..=64`; the raw `SECRET` prefix carries the same number as a big-endian `u16` |

A `PlanTarget` is exactly `{"revision":TargetRevision,"target":Target}`. A `Setting` is exactly
`{"target":Target,"value":SettingValue}`. An `AuthorityRevision` is exactly
`{"revision":StoreRevision|null,"target":Target}`; `null` means that the plan reported no
existing authority revision and therefore permits only create, never replacement. A `SecretNeed`
is exactly `{"ordinal":Ordinal,"target":Target}`. `targets`, `settings`, `authorities` and
`secrets` are plan order, never lexical or caller-selected order; targets are unique in each array.
`targets` has 1..=64 entries, while `settings`, `authorities` and `secrets` each have 0..=64.
Every `settings`/`authorities` target occurs exactly once in `targets`; every `secrets` target occurs
exactly once in the initiating `targets`. Secret ordinals are exactly `1, 2, ..., secrets.len()` in
array order. There is no ordinal zero and no sparse or caller-chosen ordinal.

For `secrets=[]`, `secrets.len()` is zero and there are no ordinals or `SECRET` frames; the raw
secret payload grammar remains nonempty and cannot encode a synthetic empty secret.

The sole prefix outside FXLM framing is Windows Service Account writer attachment `FXHA`. It is
exactly 16 bytes: ASCII `FXHA`, version byte `1`, direction byte `1` (helper to server), kind byte
`1` (Service Account writer), one zero reserved byte, then the helper-process source HANDLE as a
big-endian nonzero `u64`. It is admitted only once, immediately before one MINT frame on the same
already owner-authenticated named-pipe connection. It is not an FXLM frame, payload member, opcode
or ninth release protocol; every other native operation begins with the 12-byte `FXLM` header.

One outbound grant selector is the exact object
`{"effects_within":null|Effects,"idempotency":null|Idempotency,"max_risk":null|Risk}`.
`Risk` is one of `low`, `medium`, `high`, `destructive`; `Idempotency` is one of `idempotent`,
`conditional`, `not_idempotent`; `Effects` is a unique lexically sorted array of 0..=3 values from
`network`, `process`, `workspace_write`. `null` means that axis is unconstrained; omission does not.
No operation id exception is expressible. One preserved inbound grant is exactly
`{"binding":string,"events":[string...]}`: `binding` is byte-equal to a released binding id and
is 1..=128 UTF-8 bytes; `events` has 1..=256 unique lexically sorted entries, each byte-equal to a
declared event and 1..=128 UTF-8 bytes. A `GrantCandidate` is exactly
`{"connector":Connector,"inbound":[InboundGrant...],"selector":Selector}` with 0..=64 unique
inbound bindings in the selected stored `Grant.inbound` Vec order. That Vec is never lexically
reordered. Each entry's `events` array retains the canonical lexical iteration order of its typed
`BTreeSet`; changing either order changes the candidate and digest.

That candidate is a lossless projection of every *expressible* current grant. Current `Grant` has
exactly `connector`, `selector` and `inbound`; current `Selector` has the three candidate axes plus
`allow_ids` and `deny_ids`. PREVIEW scans the entire decoded grant vector before selecting. Zero
grants for the selected connector means a new candidate with empty inbound authority; exactly one
is eligible for projection; two or more are `grant_unexpressible/409/operator/none` and are never
selected, merged or deduplicated. For that one selected grant, either nonempty id set, an inbound
connector/binding/event that no longer matches the released catalogue, an empty `events` BTreeSet,
two inbound Vec entries repeating one binding, 65 or more inbound entries, 257 or more events in one
entry, or any future stored field without a v1 representation is also
`grant_unexpressible/409/operator/none`, with no CANDIDATE.
Refreshing cannot make manual operation-id or inbound authority expressible, so
`proposal_conflict/409/refresh/none` is not used for these cases. Otherwise CANDIDATE copies all
three proposed selector axes and every held inbound binding/event exactly in the selected inbound
Vec order; the redundant stored `InboundGrant.connector` is reconstructed from
`GrantCandidate.connector`. APPLY revalidates that reconstruction, order and declaration before
CAS. Unrelated connector grants never enter the candidate. The typed whole-file `GrantStore` may
canonicalize their representation when it writes;
every unrelated decoded vector entry remains in the same position with identical connector,
selector (including `allow_ids`/`deny_ids`) and inbound Vec multiplicity/order, and each inbound
entry retains identical connector, binding and event-set values. This explicitly preserves
unrelated duplicate-connector rows, duplicate inbound bindings, empty event sets, over-bound sets
and legacy omitted-`inbound` documents as their decoded `inbound=[]` value. They are neither
projected nor normalized merely because another connector is edited. Original whitespace, JSON
member order and omission spelling are not promised. A future stored `Grant` field is included
unchanged by a new protocol identity or makes binding/preview refuse before any whole-store write;
it is never silently defaulted.

Adversarial expressibility fixtures deserialize real typed store documents, not only HTTP-produced
ones. They require `grant_unexpressible/409/operator/none` with no CANDIDATE for selected-connector
duplicates, empty selected events, duplicate selected inbound bindings, nonempty selected id sets,
65 selected inbound entries and 257 events. Separate successful fixtures place each same shape on
unrelated connectors and assert decoded vector position/multiplicity plus every typed value remains
identical after the selected connector's canonical whole-store CAS. A positive selected grant uses
unique bindings in deliberately nonlexical Vec order and proves PREVIEW, APPLY, replay and QUERY
preserve that order while each typed event set remains lexically canonical.

Each tenant grant set and its `StoreRevision` are one durable atomic record under the store's
exclusive mutation authority. The checked monotonic u64 high-water mark starts at 1, survives
restart, never resets and increments exactly once with every successful whole-set mutation through
FXLM or another retained writer; same-proposal replay and QUERY are not mutations and do not
increment it. Zero, wrap, a missing revision in a migrated record or a noncanonical/corrupt revision
refuses as `store_unavailable/503/operator/none` without resetting or rewriting grants.

The legacy unversioned typed store is distinguished only while its durable store-format marker is
absent. Before any CANDIDATE or mutation is served, Exchange holds the same exclusive mutation
authority, decodes the complete legacy store, assigns revision 1 to every existing tenant without
changing any decoded grant, and atomically publishes the versioned whole-store image plus its format
marker. The first read of an absent tenant similarly atomically creates its empty set at revision 1.
A crash before that atomic replacement leaves only the legacy/absent form and retries the same
revision-1 initialization; a crash after replacement observes the complete migrated record. Once
the marker exists, a missing tenant revision is corruption, never another legacy initialization.

CANDIDATE carries the exact precondition revision read before preview. A successful APPLY atomically
publishes the whole set, the incremented post-commit revision and its terminal proposal/receipt
record. A crash before that atomic commit leaves the old set/revision and permits the same APPLY to
retry; a crash after it makes replay/QUERY return the recorded receipt without a second increment.
The receipt's required `revision` is the post-commit revision; first delivery, same-proposal replay
and QUERY carry the same value. The grant digest has this one byte preimage and no implied separator,
length, newline, schema wrapper or decoded-integer substitution:

```text
GrantProposalInput = {"candidate":GrantCandidate,"revision":StoreRevision}
GrantProposalPreimage =
  UTF8("exchange.local-management.v1.grant-proposal") || 0x00 ||
  UTF8(RFC8785(GrantProposalInput))
ProposalDigest = lowerhex(SHA-256(GrantProposalPreimage))
```

`StoreRevision` is the JSON string containing the canonical decimal bytes in the control object,
not a JSON number. APPLY must carry the byte-identical canonical candidate, decimal precondition
revision and digest. A changed candidate is `grant_digest_mismatch`; a changed high-water mark is
`grant_stale`. Fixtures cover legacy initialization, crash on both sides of its atomic replacement,
restart stability, missing/corrupt post-migration refusal, stale concurrent APPLY, crash on both
sides of commit, and exact precondition-versus-post-commit receipt revisions.

### Reproducible plan and target revisions

X-125 v1 has no revision field. V2 therefore derives revisions from the current provider projection
rather than assigning an implementation-local counter. Literal domains are UTF-8, `||` means byte
concatenation and `0x00` is one zero byte:

```text
TargetRevisionPreimage =
  UTF8("exchange.connection-plan.v2.target-revision") || 0x00 ||
  UTF8(RFC8785(TargetRevisionInput))
TargetRevision = lowerhex(SHA-256(TargetRevisionPreimage))

PlanRevisionPreimage =
  UTF8("exchange.connection-plan.v2.plan-revision") || 0x00 ||
  UTF8(RFC8785(PlanRevisionInput))
PlanRevision = lowerhex(SHA-256(PlanRevisionPreimage))
```

There is no implicit separator, length, newline or schema wrapper beyond the shown single zero
byte. `RFC8785` returns UTF-8 canonical JSON bytes; hashes are over those bytes, not a language
object, pretty JSON or the digest of either component.

`TargetRevisionInput` is exactly
`{"authority":null|"custom_origin","choices":null|[SettingValue...],"destination":Destination,"target":Target}`.
`choices` is the unique provider order of accepted values, not the human labels. `Destination` is
exactly one of `{"kind":"connection_label"}`,
`{"credential":PlanAtom,"kind":"credential"}`, or
`{"kind":"settings","settings":[{"binds":PlanAtom,"service":PlanAtom}...]}`. The settings array is
1..=64 unique declared addresses in the exact order the current X-125 `TargetSpec` writes;
`choices` is `null` or 1..=256 unique values in provider order. These are precisely the current
`submission_targets` equality facts—destination, choices and custom-origin policy—plus the public
target id. Two projected fields sharing a target id must produce byte-identical input or plan
generation refuses before emitting either revision.

`PlanRevisionInput` is exactly
`{"connector":Connector,"fields":[PlanRevisionField...],"schema":"exchange.connection-plan.v2","vendor":PlanDisplay}`.
One `PlanRevisionField` is exactly
`{"aliases":[Alias...],"also_binds":[PlanAtom...],"authority":null|"custom_origin","binds":null|PlanAtom,"choices":null|[PlanChoice...],"help":PlanText,"identity":PlanAtom,"input":PlanAtom,"label":PlanDisplay,"name":PlanAtom,"provenance":"exchange"|"provider.auth"|"provider.config","reason":PlanDisplay|null,"required":boolean,"secret":boolean,"service":null|PlanAtom,"target":null|{"id":Target,"revision":TargetRevision}}`.
The array order is exactly current X-125 projection order: `connection.name`, released config
declaration order, then released auth declaration order for credentials not already bound by a
config field. Alias, `also_binds` and choice order are the provider projection's published order.

The plan preimage includes every static `PlanField` fact a TTY/browser renders. In particular it
includes the deterministic user-visible `reason` that current X-125 `describe_config` derives from
the released declaration/policy for an unroutable field. It does not repeat `routable`: that boolean
is exactly `target != null`, an invariant of both the response and preimage, so `target` determines
it without a second spelling. Comparing the full response against `PlanRevisionField` leaves only
the live field facts out: `set`, plus authority lifecycle `state`, decimal `revision` and `actions`;
the static authority kind remains included as `null|"custom_origin"`. At top level only tenant
`credential_revision`, `labels`, `selection` and their derived complete/incomplete `state` are live
exclusions. Stored values, authority/apply URLs and other endpoint facts never occur in v2 at all.
Thus two hosts running the same released catalogue and policy produce the same revisions, while any
rendered form text (including `reason`), validation, destination, choice, authority-kind or routing
change produces a new target and/or plan revision. Changing only a credential head never changes a
static plan or target revision.

`PlanRevision` and `TargetRevision` are content identities. They are never converted to integers,
compared by magnitude or reused as CAS counters. `StoreRevision` is the separate mutable u64 grant
or authority high-water mark rendered in canonical decimal. An authority/store revision therefore
cannot be mistaken for either 64-lowerhex content identity even when its decimal digits happen to
be hexadecimal characters. `CredentialRevision` is a third category: an opaque live per-label CAS
token, not a static content identity or decimal authority/store counter.

### Complete connection-plan v2 wire object

The plan response is exactly this closed object. Every shown member is required, including members
whose value is `null` or an empty array, and no other member is admitted:

```text
ConnectionPlanV2 =
{"connector":Connector,
 "credential_revision":CredentialRevision|null,
 "fields":[PlanField...],
 "labels":[Label...],
 "plan_revision":PlanRevision,
 "selection":Label|null,
 "state":"complete"|"incomplete",
 "vendor":PlanDisplay,
 "version":"exchange.connection-plan.v2"}

PlanField =
{"aliases":[Alias...],
 "also_binds":[PlanAtom...],
 "authority":PlanAuthority|null,
 "binds":PlanAtom|null,
 "choices":null|[PlanChoice...],
 "help":PlanText,
 "identity":PlanAtom,
 "input":PlanAtom,
 "label":PlanDisplay,
 "name":PlanAtom,
 "provenance":"exchange"|"provider.auth"|"provider.config",
 "reason":PlanDisplay|null,
 "required":boolean,
 "routable":boolean,
 "secret":boolean,
 "service":PlanAtom|null,
 "set":boolean|null,
 "target":null|{"id":Target,"revision":TargetRevision}}

PlanChoice = {"label":PlanDisplay,"value":SettingValue}
PlanAuthority =
  {"actions":[],"revision":null,"state":"unset"} |
  {"actions":["approve","revoke"],"revision":StoreRevision,"state":"proposed"} |
  {"actions":["revoke"],"revision":StoreRevision,"state":"approved"} |
  {"actions":[],"revision":StoreRevision,"state":"revoked"}
```

`PlanAtom` is 1..=512 UTF-8 bytes. `PlanText` is 0..=2048 UTF-8 bytes and `PlanDisplay` is
1..=2048 UTF-8 bytes. An `Alias` is 3..=66 ASCII bytes: `--` followed by 1..=64 lowercase ASCII
letters/digits split into nonempty words by single `-` bytes, beginning with a letter. A plan has
1..=128 fields and 0..=256 unique labels. One field has 0..=64 unique aliases and 0..=64 unique
`also_binds`; `choices` is `null` or 1..=256 entries with unique `value` members. All arrays retain
provider projection order except `labels`, which is lexically sorted by UTF-8 bytes. Field
identities are unique; aliases are unique across the whole plan. Shared non-null target ids have
the same target revision. A secret field has no alias. These collection limits and the 65,536-byte
complete control-payload limit are simultaneous. A provider projection that exceeds any of them
returns `internal_refusal/500/operator/none` without a partial plan; `frame_too_large` remains an
inbound-frame error and is not used to blame the reader for an oversized server projection.

The field order is `connection.name`, released config declaration order, then released auth
declaration order for credentials not already bound by a config field. `target` is non-null exactly
when `routable` is true; `reason` is non-null exactly when `routable` is false. `authority` is
non-null exactly for a released typed custom-origin target. With no selection it is the `unset`
form. With a selection it reports the current value-free lifecycle state; its revision is the
existing canonical decimal authority-store revision, and a non-secret authority field's `set` is
true only for `approved`. For every field with `secret:true`, `set` is the required JSON `null`
regardless of selection, routability or stored credential presence. It is never `true` or `false`.
For every field with `secret:false`, `set` is a required boolean: false when selection is null or the
field is unroutable, otherwise it reports only the selected label's non-secret setting/authority
fact; `connection.name` is true exactly for a selected label. A non-secret field may never carry
`null`.

`selection` is `null` or is byte-equal to one member of `labels`. `credential_revision` is null
exactly when `selection` is null. For every selected held label it is that label's required opaque
head even when every credential target is absent; omission, null, uppercase, short/long, all-zero,
or a value not equal to the snapshotted head refuses. Plan snapshotting reads it independently of
credential presence. `state` is `complete` exactly when every required field is routable and every
required **non-secret** field has `set:true`; a required secret field contributes only its static
routability and its `set:null` never participates. Changing only stored secret presence cannot
change any field `set` or aggregate `state`. No field contains a stored value, normalized origin,
secret fact beyond the declaration's static `secret` boolean, instance id, tenant, principal,
authority URL, HTTP method, apply URL, endpoint or compensation prose. In particular v2 has no
`apply` or `submission` member.

The native `PLAN_QUERY` object is exactly
`{"connector":Connector,"selection":Label|null}`; absence of a selection is represented by the
required JSON `null`, never by omitting the member. `selection:null` requests the unselected plan
used to create a label; a non-null selection is valid only for a label already in `labels` and never
acts as a proposed create name. The owner-authenticated native endpoint returns
`ConnectionPlanV2` as the complete payload of `PLAN_RESPONSE`. The ordinary hosted read remains
authenticated `User` HTTP at
`GET /api/connections/{connector}/plan?version=exchange.connection-plan.v2` with optional
`&name=<Label>`; a Service Account and the onboarding browser capability cannot read it. For the
same resolved owner, connector, selection and state snapshot, the HTTP body and the native
`PLAN_RESPONSE` payload are the same RFC 8785 UTF-8 bytes, with no leading/trailing whitespace or
newline. HTTP adds transport headers only. Plan generation snapshots labels, the selected
credential head, non-secret field facts and authority states once, then serializes that one snapshot
for either transport. It does not read a credential value merely to render a plan.

A connector outside the released catalogue is `unknown_connector/404/refresh/none`; a selection
that violates `Label` grammar is `invalid_label/422/never/none`; and a grammar-valid selection not
held by the resolved owner is `unknown_label/404/refresh/none`. Unavailable registry, settings or
credential state is `store_unavailable/503/operator/none`. None returns a partial plan.

The positive fixture fixes one complete, one incomplete, one unselected and all four authority
states. Every secret field has `set:null`; selected positives carry a 64-lowerhex
`credential_revision` even when the fixture credential set is empty, and the unselected positive
carries `credential_revision:null`. A paired selected fixture holds every static/non-secret fact and
credential head fixed while changing only stored secret presence, and requires byte-identical plan
JSON and aggregate state. Each records `target-revision-input.rfc8785`, `target-revision-preimage.bin`,
`plan-revision-input.rfc8785`, `plan-revision-preimage.bin`, the expected lowerhex digests,
`plan-response.rfc8785.json`, the full `0x0008` FXLM frame and a hosted HTTP body asserted
byte-identical to its payload. Adversarial fixtures independently change every member name, type,
null/omission rule, enum, string/collection/control bound and array order; duplicate every unique
identity; mismatch selection/labels/credential revision, state/required/set, routable/target/reason,
authority/state/revision/actions and shared target revisions; use a zero/decimal/uppercase/short
digest; change an unroutable `reason` and require a different plan revision; change or truncate
either hash domain/preimage; add the v1 `apply` object, endpoint, tenant,
instance or stored value; and place raw, escaped, percent-encoded and base64 secret sentinels in
every response surface. The production HTTP serializer, native serializer and Flux parser consume
the same positives and reject the same adversarial object corpus before rendering, prompting or
mutation. Contradiction fixtures replace a secret field's null with each boolean, replace a
non-secret boolean with null, toggle state because of secret presence, omit a selected head, attach
a head to an unselected plan or derive a head from presence; each refuses.

The 65,536-byte control bound is measured over the complete canonical JSON payload. The cumulative
1,048,576-byte ceremony bound is the sum of every FXLM payload length in the one logical operation,
including control and raw-secret payloads but excluding the 12-byte headers. The limit does not
replace the per-control, per-secret or 64-secret limits; the first limit crossed refuses.

### Closed X-125 target universe and operation projections

The target universe is derived from the actual current X-125 projection, not from caller strings.
X-125's `TargetSpec` is exactly `id`, `destination`, `choices` and `custom_origin`, where destination
is `Credential(name)` or `Settings(Vec<DeclaredSetting>)`. Current `describe_config` writes exactly
one primary declared address into that settings Vec; published `also_binds` remains a static display
fact and does not silently add a destination. `submission_targets` walks current field projection
order and keeps the first non-null target id; a later field sharing the id is deduplicated only when
destination (including settings Vec order), choices and custom-origin policy are byte-identical.
Any mismatch refuses plan generation. V2 prepends the synthetic `connection.name` target whose
destination is `{"kind":"connection_label"}`. The resulting ordered target universe is therefore:

1. `connection.name`;
2. each first non-null target from released config declaration order; then
3. each first non-null `credential.<name>` target from released auth declaration order for a
   credential not already bound by a config field.

Each universe member is classified once and only once. `connection.name` is **connection-name**;
`Destination::Credential` is **credential**; `Destination::Settings` with
`custom_origin=false` is **setting**; and `Destination::Settings` with `custom_origin=true` is
**authority**. An authority target still contributes its submitted non-secret `Setting` value plus
its authority CAS entry; it is not also classified as setting. A target is required exactly when at
least one projected field sharing it has `required:true`; otherwise it is optional. An unroutable
field has no target, and a required unroutable field makes connect `invalid_request/400/never/none`.
More than 64 unique universe targets refuses plan generation as
`internal_refusal/500/operator/none`, so no closed operation needs a larger `targets` array.

For connect `BEGIN 0x0001`, the caller selects optional targets only by including their exact
`PlanTarget` in `targets`; there is no second selection flag. The valid array is the universe order
filtered by this exact rule: `connection.name` and every required target are present, and each
optional routable target is present exactly when selected. No target occurs twice. For every
selected setting or authority target there is exactly one `Setting` with the same target in that
same relative universe order; no Setting exists for connection-name or credential targets. For
every selected authority target there is exactly one `AuthorityRevision`, in the same relative
order, and a create proposal validated against the unselected plan carries its exact `revision:null`.
There is no AuthorityRevision for another partition. The `label` member supplies the
connection-name value, so `connection.name` never appears in `settings`, `authorities` or
`NEED_SECRETS`. `NEED_SECRETS.secrets` is exactly the selected credential targets in relative
universe order with contiguous ordinals. This permits a credential-free settings-only connect to
produce `secrets:[]` without weakening required-target closure.

For credential `BEGIN 0x0030`, the exact `targets` array is the complete credential partition of
the selected plan in universe order: every routable credential target, required or optional, once,
and no connection-name, setting or authority target. The caller cannot select a subset, add a
setting target or use a Setting/AuthorityRevision object on this opcode. An empty credential
partition makes 0x0030 `invalid_request/400/never/none`. `action:"acquire"` is admitted exactly
when every credential address represented by that complete set is absent; `action:"rotate"` is
admitted exactly when every one is present. Mixed, opposite or otherwise nonconforming state is the
single value-free `credential_state_conflict/409/refresh/none`; it identifies no target, count or
presence fact.

A caller-invented target is `unknown_target/422/refresh/none`. Omission of a required connect
target, inclusion of a known optional setting/authority target without its exact Setting, an extra
projection object, duplicate shared target, wrong target order, or cross-partition target is
`invalid_request/400/never/none`. A changed plan revision or any one changed/missing target revision
is `stale_plan/409/refresh/none`. All of these checks, plus credential action-state and head checks
below, finish before transaction allocation, C-515 prepare or prompting. Canonical positives freeze
one connect with required and selected optional targets, one credential-free connect, one full
acquire and one full rotate. One-fact adversarial fixtures independently omit each required target,
add each unselected optional projection, invent a target, duplicate a shared target, exchange two
targets, cross every partition, change one target revision and place a setting target in a credential
operation.

### Exact FXLM opcode payloads

The opcode determines the control type, so control objects carry no `schema`, `type`, `operation`,
tenant or principal member. These are the only payloads:

| Opcode and direction | Exact payload object or bytes |
|---|---|
| connect `BEGIN` `0x0001`, client | `{"authorities":[AuthorityRevision...],"connector":Connector,"label":Label,"plan_revision":PlanRevision,"settings":[Setting...],"targets":[PlanTarget...]}` |
| shared `NEED_SECRETS` `0x0002`, server | `{"proposal_digest":ProposalDigest,"secrets":[SecretNeed...],"transaction_id":TransactionId}` |
| shared `SECRET` `0x0003`, client | non-JSON: big-endian `u16` ordinal followed by 1..=8192 raw secret bytes; payload length is therefore 3..=8194 |
| connect `COMMIT` `0x0004`, client | `{"proposal_digest":ProposalDigest,"transaction_id":TransactionId}` |
| connect `QUERY` `0x0005`, client | `{"receipt_id":ReceiptId}` |
| connect `RECEIPT` `0x0006`, server | the exact connect receipt below |
| plan `PLAN_QUERY` `0x0007`, client | `{"connector":Connector,"selection":Label|null}` |
| plan `PLAN_RESPONSE` `0x0008`, server | the complete canonical `ConnectionPlanV2` object above |
| grant `PREVIEW` `0x0010`, client | `{"connector":Connector,"selector":Selector}` |
| grant `CANDIDATE` `0x0011`, server | `{"candidate":GrantCandidate,"proposal_digest":ProposalDigest,"revision":StoreRevision}` |
| grant `APPLY` `0x0012`, client | `{"candidate":GrantCandidate,"proposal_digest":ProposalDigest,"revision":StoreRevision}` |
| grant `QUERY` `0x0013`, client | `{"receipt_id":ReceiptId}` |
| grant `RECEIPT` `0x0014`, server | the exact grant receipt below |
| Service Account `MINT` `0x0020`, client | `{"expires_at":ExpiresAt,"id":ServiceAccountId}` plus the separately transferred writer capability; no descriptor/HANDLE number occurs in JSON |
| Service Account `QUERY` `0x0021`, client | `{"receipt_id":ReceiptId}` |
| Service Account `RECEIPT` `0x0022`, server | the exact Service Account receipt below |
| credential `BEGIN` `0x0030`, client | `{"action":"acquire"|"rotate","connector":Connector,"credential_revision":CredentialRevision,"label":Label,"plan_revision":PlanRevision,"targets":[PlanTarget...]}` |
| credential `COMMIT` `0x0031`, client | `{"proposal_digest":ProposalDigest,"transaction_id":TransactionId}` |
| credential `RECEIPT` `0x0032`, server | the exact connect receipt below, whose `operation` agrees with `action` |
| credential `QUERY` `0x0033`, client | `{"receipt_id":ReceiptId}` |
| `ERROR` `0x7fff`, server | one exact error object below |

Credential ceremonies reuse shared `NEED_SECRETS` `0x0002` and `SECRET` `0x0003`; there are no
hidden `0x0034`/`0x0035` aliases. The proposal digests have exactly these byte preimages:

```text
ConnectProposalInput =
  {"authorities":[AuthorityRevision...],"connector":Connector,"label":Label,
   "plan_revision":PlanRevision,"settings":[Setting...],"targets":[PlanTarget...]}
ConnectProposalPreimage =
  UTF8("exchange.local-management.v1.connect-proposal") || 0x00 ||
  UTF8(RFC8785(ConnectProposalInput))

CredentialProposalInput =
  {"action":"acquire"|"rotate","connector":Connector,
   "credential_revision":CredentialRevision,"label":Label,"plan_revision":PlanRevision,
   "targets":[PlanTarget...]}
CredentialProposalPreimage =
  UTF8("exchange.local-management.v1.credential-proposal") || 0x00 ||
  UTF8(RFC8785(CredentialProposalInput))

ProposalDigest = lowerhex(SHA-256(the applicable ProposalPreimage))
```

Each input is the exact opcode-specific `BEGIN` object—not a wrapper, semantic projection or parsed
field set—and there is no implicit separator, length or newline beyond the shown `0x00`. The arrays
and their objects retain the exact control-object order and JSON string representations. The
`NEED_SECRETS` digest uses the applicable formula. `COMMIT` must echo the byte-identical digest and
transaction id. Receipt query always names a receipt id and never a transaction, connector, label,
Service Account id or proposal. A response-loss caller that never received a receipt id replays the
byte-identical initiating proposal on a new connection/WebSocket; it does not manufacture a query.

`BEGIN 0x0001` is connection-create, not edit. After validating its static plan/target facts against
the unselected plan, Exchange resolves its proposed `label`. An unheld label may start create. A
held label succeeds only when the canonical BEGIN digest is byte-identical to that label's durable
recorded proposal: it returns the existing `0x0006` receipt with `replayed:true` before any prompt or
write. A held label with a changed digest, or with no recorded X-134 proposal to match, is exactly
`proposal_conflict/409/refresh/none`; it is never treated as update, acquire or rotation. An active
same-label transaction with the same digest and no durable outcome is the separate
`connect_busy/409/refresh/none` row; a different active digest remains `proposal_conflict`. Thus the
ordering is terminal same-digest replay, changed-digest conflict, active same-digest busy, then new
create. Validation and replay/conflict lookup precede transaction-id
allocation and secret prompting. Credential `BEGIN 0x0030` instead requires an existing held label;
its plan-validation selection and server lookup return `unknown_label/404/refresh/none` when absent.

Every held label has one durable credential-head record independently of whether any credential
address is present. Exchange initializes a new label's nonzero `CredentialRevision` as part of the
same committed connect metadata image, including a zero-secret create. For an existing label, it
allocates the next unique nonzero 256-bit revision before credential prepare, journals that
value-free next head, and publishes it only while rolling forward the committed credential
mutation. Abort never advances it. The value is generated from the coordinator's cryptographic
random source with all-zero and every value already recorded for that label rejected; it is not a
hash or encoding of a credential fact. The terminal ledger retains each committed proposal digest,
receipt and post-commit head for replay/query even after later heads advance.

Credential BEGIN processing computes its canonical digest and checks for a terminal same-digest
record before selected-plan revision, target, head or action-state validation. A hit returns the
existing 0x0032 receipt with `replayed:true`, even when the BEGIN carries an older head than the
current plan. Without that terminal hit, Exchange validates the exact selected plan and target set,
then requires `BEGIN.credential_revision` to equal the current durable head. A mismatch is
`stale_credential_revision/409/refresh/none`; it exposes neither head. Only then does it enforce the
single acquire/rotate state rule above. Thus rotation one formed with head `R0` commits and publishes
`R1`; rotation two formed with `R1` has a different BEGIN and digest and commits `R2`; replaying the
first byte-identical `R0` proposal still returns its first receipt without prompting or mutation.

Legacy heads migrate before a selected plan or credential operation is served. Under the same
exclusive credential-mutation authority, Exchange distinguishes an unmarked legacy connection-head
store, allocates a revision for every held label without reading credential presence, and atomically
publishes the complete marked head image. A crash before atomic replacement leaves the legacy image
and retries initialization; a crash after replacement observes all heads. Once the marker exists,
a missing label head, zero/reset sentinel, duplicate, non-lowerhex or otherwise corrupt head is
`store_unavailable/503/operator/none` and is never regenerated. Fixtures cover both migration crash
boundaries, absent and fully populated legacy labels receiving indistinguishable revisions, restart
stability, missing/reset/corrupt refusal, two successive rotations, and old-proposal replay after
the head advances.

The state table is exhaustive. `S` is the initial state and `T` is terminal:

| Operation | Valid frames and states |
|---|---|
| connect/replay | `S --client 0x0001--> B`; `B --server 0x0006--> T` for same-proposal replay, `B --server 0x0002 secrets=[]--> N0`, `B --server 0x0002 secrets=[1..N]--> N` for `N>0`, or `B --server 0x7fff--> T`; `N0 --client 0x0004--> D`; `N --client 0x0003 ordinals 1..N in order--> C --client 0x0004--> D`; `D --server 0x0006|0x7fff--> T` |
| connect query | `S --client 0x0005--> Q --server 0x0006|0x7fff--> T` |
| plan query | `S --client 0x0007--> Q --server 0x0008|0x7fff--> T` |
| grant preview | `S --client 0x0010--> P --server 0x0011|0x7fff--> T` |
| grant apply | `S --client 0x0012--> D --server 0x0014|0x7fff--> T` |
| grant query | `S --client 0x0013--> Q --server 0x0014|0x7fff--> T` |
| Service Account mint | Windows only: `S --client FXHA--> A --client 0x0020--> D`; Unix transfers one writer with the MINT read; `D --server 0x0022|0x7fff--> T` |
| Service Account query | `S --client 0x0021--> Q --server 0x0022|0x7fff--> T` |
| credential/replay | `S --client 0x0030--> B`; `B --server 0x0032--> T` for same-proposal replay, `B --server 0x0002 secrets=[]--> N0`, `B --server 0x0002 secrets=[1..N]--> N` for `N>0`, or `B --server 0x7fff--> T`; `N0 --client 0x0031--> D`; `N --client 0x0003 ordinals 1..N in order--> C --client 0x0031--> D`; `D --server 0x0032|0x7fff--> T` |
| credential query | `S --client 0x0033--> Q --server 0x0032|0x7fff--> T` |

For a non-replay BEGIN with zero secret targets, the Exchange transaction coordinator still
allocates both components of the transaction id, constructs the valid empty `SecretBatch`,
successfully calls C-515 `prepare`, and
then sends `NEED_SECRETS` with `secrets:[]`, that transaction id and the canonical proposal digest.
The client/helper sends `COMMIT` directly; Exchange records the same one durable decision and
coordinates connection label, non-secret settings, authority metadata and audit before committing
the empty provider batch and publishing the receipt. It never synthesizes a secret, accepts an
empty `SECRET`, or skips from BEGIN to a server-side decision/receipt. In `N0`, any `SECRET` is
`unexpected_frame`; for `N>0`, COMMIT before every ordinal is `unexpected_frame`. These errors occur
before decision and follow the normal abort/tombstone rules.

A known opcode with the wrong header direction is `wrong_direction`; a known opcode in any other
state, a skipped/repeated/out-of-order ordinal, omitted writer or second logical operation is
`unexpected_frame`; an unknown opcode is `invalid_frame`. Native EOF before the declared frame or
before `T` is `truncated_frame`; bytes after one complete JSON/raw payload are `surplus_data`.
Hosted close-code mapping remains the transport mapping below and does not change these FXLM codes.

Opcode admissibility is also closed by transport. “Yes” still requires the opcode direction and
state above. After header version and direction have passed, a dash means the known opcode on that
transport is `unexpected_frame`, not an extension point; an invalid direction still takes the
`wrong_direction` row first:

| Opcode family | Owner-authenticated native FXLM | Hosted operator WebSocket | Flux-to-helper request pipe | Helper-to-Flux result pipe |
|---|---|---|---|---|
| connect `0x0001..0x0006` | yes | yes | `0x0001` only | `0x0006` only |
| plan `0x0007..0x0008` | yes | — | — | — |
| grant `0x0010..0x0014` | yes | yes | — | — |
| Service Account `0x0020..0x0022` | yes | — | — | — |
| credential `0x0030..0x0033` plus shared `0x0002..0x0003` | yes | yes | `0x0030` only | `0x0032` only |
| `ERROR 0x7fff` | server response | server response | — | terminal response |

The helper request column admits exactly one complete initiating frame, so shared secret and commit
frames never cross that pipe. The result column admits exactly one receipt or error and therefore
never carries `NEED_SECRETS`, a transaction id or an ordinal. `PLAN_QUERY` is native-only; the
hosted equivalent remains the authenticated human HTTP GET, not WebSocket. Connect, grant and
credential receipt `QUERY` operations are admitted both natively and on a separate hosted
WebSocket. Their native form is non-secret and may be sent directly rather than through the vendor
helper. Service Account `MINT`, `QUERY` and `RECEIPT` are local native FXLM only because they bind a
local writer capability. Hosted mint remains the distinct HTTP FXSA response and has no FXLM query.
A WebSocket never has a generic descriptor, HANDLE or writer capability, so `0x0020..0x0022` on it
is always `unexpected_frame` followed by the protocol close defined below.

### Exact receipts, errors and provider mapping

The four provider fixture identities serialize exactly these closed objects. There are no omitted
or nullable receipt members:

| Fixture identity | Exact object |
|---|---|
| `exchange.connect-receipt.v1` | `{"commit":{"audit":"committed","resource":"committed"},"connector":Connector,"label":Label,"operation":"acquire"|"connect"|"rotate","receipt_id":ReceiptId,"replayed":boolean,"schema":"exchange.connect-receipt.v1"}` |
| `exchange.grant-apply-receipt.v1` | `{"commit":{"audit":"committed","resource":"committed"},"connector":Connector,"receipt_id":ReceiptId,"replayed":boolean,"revision":StoreRevision,"schema":"exchange.grant-apply-receipt.v1"}` |
| `exchange.service-account-mint-receipt.v1` | `{"commit":{"frame_written":true,"verifier":"committed"},"id":ServiceAccountId,"receipt_id":ReceiptId,"replayed":boolean,"schema":"exchange.service-account-mint-receipt.v1"}` |
| `exchange.local-management-error.v1`, before decision | `{"code":ErrorCode,"commit":"none","retry":PreDecisionRetry,"schema":"exchange.local-management-error.v1","status":Status}` |
| `exchange.local-management-error.v1`, after decision | `{"code":PostDecisionCode,"commit":"query_receipt","receipt_id":ReceiptId,"retry":"same_proposal","schema":"exchange.local-management-error.v1","status":Status}` |

The connect receipt's operation is `connect` on opcode `0x0006` and is the initiating `action` on
opcode `0x0032`. `replayed` is `false` only on the first terminal receipt delivery and `true` on
query or same-proposal replay. Receipts never contain a proposal digest, setting, expiry, target,
secret count/presence/length/digest/fingerprint, token fact, credential address, tenant or principal.

`Status` is a JSON integer carrying the closed HTTP-equivalent classification below; it does not
turn FXLM into HTTP. `PreDecisionRetry` is exactly `never`, `refresh` or `operator`. This is the
complete pre-decision error table; no other status/retry/commit tuple is valid:

| `code` | `status` | `retry` | `commit` |
|---|---:|---|---|
| `invalid_frame` | 400 | `never` | `none` |
| `unsupported_version` | 426 | `never` | `none` |
| `wrong_direction` | 400 | `never` | `none` |
| `unexpected_frame` | 409 | `never` | `none` |
| `frame_too_large` | 413 | `never` | `none` |
| `truncated_frame` | 400 | `never` | `none` |
| `surplus_data` | 400 | `never` | `none` |
| `deadline_exceeded` | 408 | `refresh` | `none` |
| `peer_unverified` | 403 | `never` | `none` |
| `unsafe_root` | 503 | `operator` | `none` |
| `local_management_unavailable` | 503 | `operator` | `none` |
| `invalid_request` | 400 | `never` | `none` |
| `unknown_connector` | 404 | `refresh` | `none` |
| `unknown_label` | 404 | `refresh` | `none` |
| `invalid_label` | 422 | `never` | `none` |
| `secret_json_forbidden` | 415 | `never` | `none` |
| `unknown_target` | 422 | `refresh` | `none` |
| `stale_plan` | 409 | `refresh` | `none` |
| `stale_credential_revision` | 409 | `refresh` | `none` |
| `credential_state_conflict` | 409 | `refresh` | `none` |
| `proposal_conflict` | 409 | `refresh` | `none` |
| `connect_busy` | 409 | `refresh` | `none` |
| `grant_unexpressible` | 409 | `operator` | `none` |
| `grant_stale` | 409 | `refresh` | `none` |
| `grant_digest_mismatch` | 409 | `refresh` | `none` |
| `service_account_conflict` | 409 | `refresh` | `none` |
| `writer_invalid` | 400 | `never` | `none` |
| `writer_closed` | 409 | `operator` | `none` |
| `store_unavailable` | 503 | `operator` | `none` |
| `audit_unavailable` | 503 | `operator` | `none` |
| `internal_refusal` | 500 | `operator` | `none` |

Only `store_unavailable`, `audit_unavailable` and `internal_refusal` may be emitted after the durable
decision. They retain respectively status 503, 503 and 500, must carry the allocated receipt id,
and have exactly `retry=same_proposal,commit=query_receipt`. No protocol/frame/caller-validation,
writer, conflict, stale, busy or capacity error is a post-decision tuple.

The released C-515 port maps provider outcomes without implementation choice:

| Provider result | Coordinator phase | FXLM result |
|---|---|---|
| `Absent` | before prepare, or after successful abort | internal success; prepare may start or pre-decision cleanup completes; never a receipt |
| `Prepared` | after prepare and before decision | internal success; Exchange may record the durable decision only after its value-free journal is durable |
| `Prepared` | after decision/recovery | internal success; repeat `commit` |
| `Committed` | after decision/recovery | internal success; roll forward metadata/audit and return or replay the receipt |
| `Committed` | before Exchange's durable decision | `internal_refusal/500/operator/none`; never synthesize a decision or receipt |
| `Absent` | after Exchange's durable decision | `internal_refusal/500/same_proposal/query_receipt` |
| `Unsupported` | before decision | `local_management_unavailable/503/operator/none` |
| `Busy` | before decision | `connect_busy/409/refresh/none` |
| `DigestMismatch` | before decision | `proposal_conflict/409/refresh/none` |
| `TransactionIdReused` | before decision | `internal_refusal/500/operator/none` |
| `NotPrepared` | before decision | `internal_refusal/500/operator/none` |
| `NotPrepared` | after decision | `internal_refusal/500/same_proposal/query_receipt` |
| `AlreadyCommitted` from pre-decision abort | before decision | `internal_refusal/500/operator/none` |
| `Retired` | before decision | `internal_refusal/500/operator/none` |
| `Retired` | after decision | `internal_refusal/500/same_proposal/query_receipt` |
| `Capacity` | before decision | `store_unavailable/503/operator/none` |
| `InvalidBatch` | before decision | `internal_refusal/500/operator/none` |
| `Backend` or unresolved provider I/O | before decision | resolve with `state`; `Absent` retries the same prepare, `Prepared` continues, `Committed` takes the pre-decision invariant-refusal row; if state/cleanup remains unavailable, `store_unavailable/503/operator/none` |
| `Backend` or unresolved provider I/O | after decision | `store_unavailable/503/same_proposal/query_receipt`; recovery queries state and repeats commit |

Repeated commit returning `Committed`, repeated same-id/same-digest prepare returning `Prepared` or
`Committed`, and repeated abort returning `Absent` are success rows, not errors. `reclaim` is never
on a ceremony response path: Exchange either acknowledges one safe generation internally or logs a
value-free operator refusal; it cannot turn reclamation pressure into another client tuple. The
provider fixture enumerates the Cartesian product of operation, state, provider result, decision
bit and expected tuple and marks every row not listed here invalid.

### Hosted canonical origin and bounded ceremony constants

The canonical origin serialization is ASCII, has no trailing slash, and is exactly
`scheme://host` when the effective port is the scheme default (`443` for `https`, `80` for `http`),
or `scheme://host:<port>` otherwise. A non-default port is canonical decimal `1..=65535` with no
leading zero. Scheme and DNS host are lowercase; a DNS host is an already-ASCII IDNA A-label with no
trailing dot; IPv4 uses canonical dotted decimal and IPv6 uses lowercase RFC 5952 inside `[...]`.
The explicit setting is already required to be canonical: `https://example.com:443`,
`http://127.0.0.1:80`, uppercase host/scheme, a leading-zero port or a trailing `/` fails startup
rather than being normalized. Examples admitted by serialization are `https://example.com`,
`https://example.com:8443`, `http://127.0.0.1` and `http://[::1]:3000`; the last two remain limited
to `--dev` and a literal loopback listener.

The request `Origin` value is compared byte-for-byte to that startup-bound canonical string. It is
not normalized first: an explicit default port, alternate IPv6 spelling, uppercase host, trailing
slash or any other byte difference is a 403 mismatch even if a URL library would assign the same
effective port. Absent-setting `--dev` derivation applies the identical serializer to the explicit
listener's literal loopback address and numeric port, including omission of port 80.

Hosted admission has exactly 32 live ceremony slots process-wide and 4 per resolved tenant. A
WebSocket consumes both counters from immediately before its `101` response until its transport is
closed; query, preview and replay consume slots just like mutating ceremonies. There is no queue,
reservation, per-principal override or environment/configuration override. If either counter is
full, the upgrade returns 429 with the exact single delta-seconds header `Retry-After: 5`,
`Cache-Control: no-store`, and a value-free body. Other 429 producers do not define this ceremony
header. Native FXLM does not consume hosted slots; contention at C-515's one prepared slot is the
`connect_busy` tuple above.

Every admitted native or hosted logical operation has an absolute 300-second pre-decision deadline.
For hosted transport it starts when the slot is reserved immediately before `101`; for native it
starts after peer authentication immediately before reading the first FXLM header. It includes
prompt time, is measured by a monotonic clock, is not reset by traffic and ends when the durable
decision is fsynced or a pre-decision terminal response is selected. Expiry zeroizes/aborts as
already specified; it is the distinct `deadline_exceeded/408/refresh/none` outcome because every
frame received before expiry may have been valid for the current state. Hosted closes 1008 with an
empty reason after that FXLM error when safe, while native returns that exact error then EOF when
safe. `unexpected_frame` is never used merely because a valid state remained idle.

After the durable decision, the connection gets a separate 30-second monotonic response budget to
complete roll-forward and canonical audit delivery. Its expiry never aborts, rolls back or edits the
proposal: it returns the applicable post-decision `query_receipt/same_proposal` error when safe,
closes/releases the hosted slot, and leaves recovery to roll forward. The code is
`store_unavailable` while provider/store outcome is unresolved, `audit_unavailable` while canonical
audit delivery is unresolved, and `internal_refusal` for any other incomplete roll-forward; their
fixed post-decision status/retry/commit rows do not change to `deadline_exceeded`. Neither deadline
appears in a JSON member, close reason, header, argv, environment or log value, and neither is
configurable in v1.

### Verified native vendor helper seam

Flux never owns a secret-bearing FXLM session. Its verified-release launch capability can start only
the already selected `flux-exchange` executable in the fixed `local vendor-secret` mode. The exact
Unix command is `flux-exchange local vendor-secret` with no option or positional argument. In that
process, inherited FD 6 is the read end of an anonymous request pipe and inherited FD 7 is the write
end of a distinct anonymous terminal-response pipe. FD 5 is closed: it remains reserved exclusively
for the different `local service-account-mint --writer-fd 5` FXSA mode. The child inherits no other
descriptor at or above 3; after entry it sets `FD_CLOEXEC` on 6 and 7. Stdin, stdout and stderr are
opened on `/dev/null`; the helper opens `/dev/tty` itself only while collecting vendor input.

The exact Windows command is
`flux-exchange local vendor-secret --request-handle <REQUEST> --response-handle <RESPONSE>`, in that
order and with no other option or positional argument. Each replacement is the canonical unsigned
decimal spelling of a nonzero pointer-width HANDLE, with no sign or leading zero. They are distinct;
`REQUEST` is a readable anonymous-pipe handle and `RESPONSE` is a writable handle for another
anonymous pipe. The launcher uses `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` containing exactly those two
handles and no other inheritable handle, even when a planted inheritable-handle canary exists. The
helper clears `HANDLE_FLAG_INHERIT` on both immediately after parsing and revalidates their type,
direction and distinct pipe identity. Standard input/output/error are `NUL`; the helper opens the
current Windows console itself. Zero, duplicate, unlisted, non-pipe, reversed-direction,
noncanonical-decimal or reserved-field discovery refuses.

On both platforms the launch capability fixes the verified executable, helper mode and owned
instance. The helper derives and revalidates the OS-owner private native root and endpoint metadata
through the same X-128 rules as the server; it does not read an endpoint, tenant, connector address,
credential address, executable, program, working directory or extra argument from Flux, argv,
environment or either frame. The launcher selects the provider-owned private native root as the
working directory, but the helper does not resolve a security-relevant relative path from it. No
`FLUX_EXCHANGE_*` environment value selects the endpoint or identity.

Each inherited pipe carries exactly one complete FXLM frame followed immediately by EOF. The
request is a direction-1 `BEGIN 0x0001` or `BEGIN 0x0030` frame and nothing else. The result is a
direction-2 `RECEIPT 0x0006`, `RECEIPT 0x0032` or `ERROR 0x7fff` frame and nothing else. Each frame
is at most 65,548 bytes including its 12-byte header. EOF before a complete frame is
`truncated_frame`; bytes after the declared payload or a second frame are `surplus_data`; a known
but inadmissible opcode is `unexpected_frame`; wrong direction is `wrong_direction`; and an
oversize declaration is `frame_too_large`. The helper writes the applicable canonical value-free
error to the result pipe when that pipe is usable, then closes it. A result-pipe capability failure
cannot be represented on that same pipe and is instead the one value-free helper transport failure
described below.

After parsing the initiating frame, the helper resolves and pins one exact owner endpoint: Unix
`<native-root>/run/local-management-v1.sock`, or the Windows SID-derived
`\\.\pipe\flux-exchange-local-management-v1-<sid-hash>` named above. It refuses an endpoint metadata
or identity change between connections. It opens and owner-authenticates native connection 1, sends
one opcode-specific query as its sole logical operation, receives exactly `PLAN_RESPONSE|ERROR`,
reaches `T` and closes that connection:

- for connection-create `BEGIN 0x0001`, the query is exactly
  `{"connector":BEGIN.connector,"selection":null}`. The proposed new label remains only in BEGIN;
  the helper validates its `Label` grammar but never sends it as plan selection. A label already
  listed by the unselected plan is not rejected by the helper, because only connection 2 can decide
  exact same-proposal replay versus changed-proposal conflict;
- for credential acquire/rotate `BEGIN 0x0030`, the query is exactly
  `{"connector":BEGIN.connector,"selection":BEGIN.label}`. That label must be held; the native
  plan error `unknown_label/404/refresh/none` is terminal.

An `ERROR` is copied as the helper's sole terminal result and connection 2 is never opened. For
`PLAN_RESPONSE`, the helper requires the exact v2 object and the opcode-specific response selection
(`null` for `0x0001`, exact BEGIN label for `0x0030`). The create response must carry
`credential_revision:null`; the credential response must carry a well-formed non-null current head.
The helper validates `BEGIN.credential_revision` as a well-formed nonzero `CredentialRevision` but
does not require equality with that current head: it cannot distinguish a stale new proposal from a
byte-identical replay whose terminal lookup must precede server head validation. It revalidates the
BEGIN connector and label, plan revision, the exact
opcode-specific target closure/order/partition above, every target id/revision, every non-secret
setting target and value against choices/normalization, every authority revision, every secret
field's `set:null`, and the absence of any setting for a field marked secret. A helper plan request
with the opposite selection form, wrong credential-head null/grammar form, any
unknown/non-routable/cross-partition target, changed revision or noncanonical frame is
`invalid_request/400/never/none` before connection 2. A well-formed unequal credential head reaches
connection 2; the server must return either its terminal same-proposal receipt or
`stale_credential_revision` before `NEED_SECRETS`, so it never causes a prompt.

Only after connection 1 is closed and revalidation succeeds does the helper open and
owner-authenticate distinct native connection 2. It writes the byte-identical initiating BEGIN
frame from the request pipe as connection 2's sole logical operation. The helper alone receives
`NEED_SECRETS`, holds the transaction id and ordinal/target list, reads each value from its TTY or
provider-owned loopback browser ceremony, sends ordered `SECRET` frames and `COMMIT`, and owns
connection 2 through its terminal receipt/error. It forwards only that terminal frame to Flux.
Neither intermediate transaction id,
secret ordinal/target list, secret byte nor secret-derived fact reaches the request pipe, result
pipe, argv, environment, stdout, stderr or exit status. A same-proposal replay that immediately
returns a receipt is still one terminal result and opens no prompt.

When `NEED_SECRETS.secrets` is empty, the helper opens neither TTY nor browser, emits no `SECRET`
frame and sends the applicable `COMMIT` immediately with the received transaction id and proposal
digest. For a nonempty list it must collect and send every ordinal before COMMIT. The request/result
pipe contract is identical in both cases, so Flux cannot learn whether prompting occurred.

The request frame plus EOF must complete before 5 monotonic seconds have elapsed from helper spawn;
expiry produces `deadline_exceeded/408/refresh/none` on the result pipe when possible. After request
EOF, endpoint discovery/pinning, both native connect/auth handshakes, the complete connection-1
`PLAN_QUERY -> PLAN_RESPONSE` exchange and revalidation, connection-1 close, and readiness to write
BEGIN on connection 2 share one absolute 5-second pre-ceremony budget. Failure produces
`local_management_unavailable/503/operator/none`. Connection 1 also has the native server's generic
300-second pre-decision read deadline starting after its own authentication, but the helper's
5-second encompassing budget always expires first; a plan read makes no durable decision and never
owns a 30-second post-decision budget. Connection 2 owns the interactive server budgets: its
300-second pre-decision deadline starts after connection-2 authentication immediately before the
server reads BEGIN, and only its durable decision starts the separate 30-second post-decision
budget. Flux's result reader therefore has one absolute 335-second deadline from request EOF:
5 seconds of pre-ceremony work plus connection 2's 300 pre-decision plus its 30 post-decision.
Traffic resets none of these budgets. If that outer deadline expires or the helper
exits/closes without a terminal frame, Flux reports one value-free helper transport refusal and may
only replay the byte-identical initiating frame through a new verified helper; it never invents a
receipt query or assumes whether a decision occurred.

Helper stdout and stderr are always empty. Exit status is exactly 0 after one complete terminal
receipt/error has been written and the response pipe closed, including when the terminal frame is an
application refusal; it is exactly 1 when capability, native transport or response-write failure
prevents that contract. No other status is emitted. Fixtures hold the 4/5-second helper-read;
4/5-second complete discovery, connection-1 handshake/query/response/close/revalidation and
connection-2 handshake/readiness boundary; 334/335-second outer-response boundary; connection 1's
stricter helper cap; and connection 2's 299/300 pre-decision and 29/30 post-decision boundaries with
an injected monotonic clock. Unix fixtures plant FDs 3, 4, 5 and 8; Windows fixtures
plant unrelated inheritable HANDLEs. They prove only 6/7 or the exact two listed HANDLEs arrive,
prove FD 5 remains the distinct FXSA writer ABI, split both sole frames at every byte boundary, and
cover early EOF, trailing bytes, second frames, swapped capabilities, every disallowed opcode,
helper crash before/after the durable decision and failure to write/close the terminal result. A
new-create positive sends plan selection null; a held-label byte-identical create positive also
uses null and receives replay only from connection 2; rotate and acquire positives select their
held BEGIN label and copy its exact credential revision. A committed old-proposal replay retains its
old head, crosses connection 1 despite the now-different selected head, and receives its receipt from
connection 2 without a prompt. An unknown proposal with that stale head receives
`stale_credential_revision` without a prompt. Adversarial helpers select the proposed new
label for create, attach a credential revision to the unselected create plan, use null for a
credential BEGIN, use a malformed head or send any non-credential target on 0x0030 and prove
connection 2 never opens (`unknown_label` from the first plan query and `invalid_request` from helper
revalidation for the other seam violations). A changed create at an occupied label proves the exact
`proposal_conflict` row rather than update or replay. A
credential-free settings-only positive proves empty-batch prepare, `NEED_SECRETS.secrets=[]`, no
TTY/browser open, direct COMMIT and ordinary metadata/audit receipt. Adversarial cases send a
`SECRET` for `N=0`, COMMIT before all ordinals for `N>0`, skip an ordinal, send an empty raw secret
and attempt a direct server receipt/decision for a new non-replay BEGIN before COMMIT; each takes
its exact state/error row.

### Windows Service Account capability attachment

Flux launches `flux-exchange local service-account-mint --id <ID> --expires-at <DECIMAL>
--writer-handle <DECIMAL>` with a closed `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` containing only the
declared writer. The helper validates the local argv HANDLE as a nonzero writable pipe endpoint,
clears `HANDLE_FLAG_INHERIT`, opens the owner-SID named pipe, authenticates and pins its server
process, then writes exactly one `FXHA` attachment immediately followed by canonical MINT. The
attachment's source HANDLE is a non-secret transport capability reference and never enters MINT
JSON.

After named-pipe impersonation and unconditional `RevertToSelf`, the server calls
`GetNamedPipeClientProcessId` on that same connection, opens and pins that client process together
with its creation identity from `GetProcessTimes`, and revalidates its TokenUser SID and
TokenSessionId. It then calls `DuplicateHandle` from that exact pinned client process into itself
using the source value in `FXHA`. The target duplicate is made non-inheritable and must be distinct
from every endpoint/control handle and a writable `FILE_TYPE_PIPE`; only then may the dispatcher
associate it with the immediately following MINT. Neither side reads remote argv or a PEB, uses an
undocumented process-information API, enumerates the process handle table, or trusts process-image
path text as capability identity.

Attachment state is closed. A truncated 16-byte attachment is
`truncated_frame/400/never/none`; a second attachment or an attachment followed by any opcode other
than MINT is `unexpected_frame/409/never/none`; any byte between the exact attachment and the
`FXLM` MINT header is `invalid_frame/400/never/none`. Wrong magic, version, direction, kind or
reserved byte; zero or unrepresentable source HANDLE; client PID/creation-identity change; a source
from another process; duplication failure; a non-pipe duplicate; or an unusable direction is
`writer_invalid/400/never/none`. Every refusal closes every acquired process/duplicate handle and
mutates no verifier. After MINT begins, the existing writer-closed and durable mint result rows
apply unchanged. The attachment source value is excluded from diagnostics, audit, journals,
persistence, receipts and release inventory.

Native adversarial fixtures split `FXHA` at every byte, mutate every field independently, add one
byte, repeat the attachment, substitute another-process and planted pipe HANDLEs, race process exit
and PID reuse against creation-time pinning, and plant an unrelated inheritable capability. The
positive proves the closed launch list admits only the intended writer, server-side duplication
delivers one complete FXSA frame, MINT returns one canonical receipt, and all handles close across
success and refusal. This Windows transport prefix remains part of
`exchange.local-management.v1`; it adds no ninth protocol identity.

### Prepared credential transaction ownership

`connector-secrets` owns the prepared credential representation, terminal ledger, inclusive
retired-through fence and native lifetime lease. Exchange owns its value-free coordinator journal,
metadata/audit roll-forward and durable allocation of both the non-zero generation and unique
192-bit nonce. The Exchange transaction coordinator constructs the opaque 256-bit transaction id
through the released provider API; `connector-secrets` validates and encodes it and owns the
provider transaction state, but generates neither component. Flux never chooses, parses, orders,
generates or logs either component.

Exchange composes only the released object-safe five-method `PreparedSecretStore` port: `prepare`,
`state`, `commit`, `abort` and acknowledged-generation `reclaim`. Public
`Absent|Prepared|Committed` state, same-digest replay, abort-before-prepare tombstones, one prepared
slot, cross-id abort fencing, 4096-terminal/1-MiB capacity without eviction, retired generations and
outcome-uncertain I/O are exactly the C-515 provider contract. Exchange acknowledges `reclaim(G)`
only after every transaction through `G` is terminal and no journal recovery, receipt recovery or
same-proposal replay can query its provider outcome. Reclaim removes no credential; a timer, count
threshold or opaque-id ordering is not acknowledgement.

Exchange opens the registry-resolved `codewandler-connector-secrets` 0.20.0 FileStore before its own
recovery/readiness and retains that same object for the server lifetime. Its exclusive native
writer/recovery lease covers open-time read, recovery and cleanup; a second 0.20 opener refuses,
abrupt exit releases the lease and no process repairs, replaces or reaps it. All 0.19 writers are
quiesced before the first 0.20 open. C-515 proves child-crash recovery and lease contention/release
natively on all five release targets before X-134 consumes its checksummed crates.io artifact.

The provider conformance fixture exhaustively maps successful public states and every payload-free
`Unsupported`, `Busy`, `DigestMismatch`, `TransactionIdReused`, `NotPrepared`, `AlreadyCommitted`,
`Retired`, `Capacity`, `InvalidBatch` and `Backend` outcome to one closed onboarding
code/status/retry/commit tuple. Before Exchange's durable decision provider I/O uncertainty is
resolved through state; after the decision it is `query_receipt`/`same_proposal`, recovery repeats
commit and never aborts or edits the proposal. Canonical positive fixtures use non-zero generations
and valid unique nonces.

### Publication readiness is derived from the tagged tree

Binary and crates.io tag workflows share one permanent fail-closed readiness check. It has no
network access, Cargo execution, environment override, mutable marker or credential-based bypass.
It accepts only when the tagged tree directly selects one registry/checksummed
`codewandler-connector-secrets` 0.20.0; no obsolete channel, manifest, compatibility or readiness v1
producer remains; no tracked `tests/fixtures/exchange-release-v1/` candidate remains; and the v2
fixture-set manifest exactly inventories and authenticates its recursive files. Canonical positives
retain `exchange.release-trust.v1` while channel, manifest, compatibility and readiness are v2 and
carry exactly the eight protocols above. The v2 inventory also retains every trust-v1 refusal case
and X-128's nine native cases mapped to fourteen exact native test bindings.

The check is the first post-checkout action of the local-release preflight and crates.io workflow,
before secrets, metadata, downloads, builds, artifacts, tokens or release mutation. The publish-mode
crate script repeats it before metadata or network access; dry-run remains available. CI exercises a
synthetic self-test while X-134 is blocked, and repository checks enforce presence and ordering at
all three seams. Its ordinary mode becomes green only because X-134's actual released dependency,
production identities and regenerated fixtures satisfy the contract—never because an operator sets
a flag. All five binary jobs depend on that preflight, so one refusal closes the entire joint tag
boundary.

The release chain is strictly `connectors/C-515 -> exchange/X-134 -> exchange/X-126`: first the
five-target C-515 evidence and checksummed 0.20.0 registry artifact, then X-134's implementation and
provider fixtures, then regenerated candidate-bound v2 release evidence and public X-126
verification.

### Hosted FXLM WebSocket binding

Hosted operators use the same `exchange.local-management.v1` FXLM state machines through a
WebSocket upgrade at exactly `GET /api/onboarding/frames`. This is a transport binding of the
existing `local_management` protocol field, never a ninth release-inventory field. The successful
upgrade echoes the exact, case-sensitive sole subprotocol `exchange.local-management.v1`, includes
`Cache-Control: no-store`, returns no `Sec-WebSocket-Extensions` and negotiates no compression. An
offered `permessage-deflate` extension is ignored rather than accepted or treated as malformed.

The upgrade request has no query string or body. Authentication, tenant derivation and the existing
hosted operator policy are revalidated before upgrade; a Service Account cannot pass the operator
gate. `Origin` must exactly equal startup-bound `FLUX_EXCHANGE_CONSOLE_ORIGIN` under the canonical
serializer above: default `:443`/`:80` is omitted and an explicit default port is noncanonical.
Production requires HTTPS. `--dev` alone may use HTTP with a literal loopback IP and, only when the
setting is absent, derives the same serialization from its explicit loopback listener
configuration. A hosted route with no usable configured origin is unavailable; an invalid explicit
setting fails startup. The origin is never derived from an OIDC redirect URI, `Host`,
`Forwarded` or any `X-Forwarded-*` header, and a missing, `null`, malformed or mismatched request
origin refuses. Pre-upgrade outcomes are closed: missing or invalid authentication is 401; a
non-operator or unacceptable origin is 403; malformed upgrade, query, body or subprotocol input is
400; another method is 405 with `Allow: GET` before body decoding; an unsupported WebSocket version
is 426 with `Sec-WebSocket-Version: 13`; exhaustion of either the exact 32-process/4-tenant slot
counter is 429 with `Retry-After: 5`; and unavailable identity, audit, coordinator or
configured-origin dependencies are 503. Every refusal is value-free and
`Cache-Control: no-store`.

Native byte-stream reads may split or coalesce bytes arbitrarily; the 12-byte header and declared
payload length delimit each successive FXLM frame. Only hosted message boundaries equal complete
frames: after transport reassembly, each WebSocket binary message contains exactly one complete
FXLM frame. WebSocket fragmentation is transport-only; splitting one frame across messages is a
truncated frame and coalescing two frames in one message is surplus data. Text is never JSON-decoded.
The maximum binary message is 65,548 bytes, while the existing 65,536-byte canonical-control,
8,192-byte-secret, 64-secret-frame and 1-MiB cumulative-payload bounds all remain. One WebSocket
carries one logical operation. Hosted connect, credential rotation and password acquisition
preserve the interactive `BEGIN -> NEED_SECRETS -> ordered SECRET... -> COMMIT -> RECEIPT|ERROR`
state machine and use the same server-owned transaction coordinator as native FXLM. Exchange
allocates and associates the opaque transaction id only after an admitted `BEGIN`; no transaction
or receipt id appears in a URL, header or log. Query and same-proposal replay each use a separate
WebSocket. Replay may return the existing receipt before a prompt; a changed proposal refuses.

Hosted terminal framing and close behavior is exhaustive:

| Terminal category | Last FXLM frame when safely writable | WebSocket close code |
|---|---|---:|
| successful receipt or grant candidate response | the applicable canonical server frame | 1000 |
| any listed application error except the categories below, including every post-decision error | `ERROR` with its one exact tuple | 1000 |
| `invalid_frame`, `unsupported_version`, `wrong_direction`, `unexpected_frame`, `truncated_frame` or `surplus_data` | `ERROR` with that code | 1002 |
| text message | no FXLM frame and no JSON decoding | 1003 |
| `frame_too_large`, or any message/control/secret/count/cumulative bound excess | `ERROR frame_too_large` only when the header/state makes that safe | 1009 |
| pre-decision `deadline_exceeded` | `ERROR deadline_exceeded/408/refresh/none` when safe | 1008 |

No error belongs to two rows: the code/category rows override the generic application-error row.
If the transport cannot safely carry the listed final frame, Exchange sends only that row's close.
Every close reason is empty. A valid but idle state expires as `deadline_exceeded`, never
`unexpected_frame`. Before a durable decision, disconnect, deadline or protocol failure zeroizes
transient buffers and aborts or tombstones an allocated provider transaction. After the decision
Exchange never aborts: its exact 30-second response budget uses the applicable post-decision error
and close 1000, while recovery, query or same-proposal replay rolls forward.

Hosted conformance fixtures cover valid-credential cross-origin requests; missing, malformed,
`null`, sibling and mismatched request origins; OIDC redirect, `Host`, `Forwarded` and
`X-Forwarded-*` spoofing; missing, wrong, differently cased and multiple subprotocols;
offered-but-not-negotiated compression; text and binary JSON shapes; WebSocket fragmentation, frame
coalescing and message splitting; deceptive lengths and every FXLM control, secret, count, message
and cumulative bound; wrong direction, opcode, state and ordinal; surplus and second operations;
cross-tenant transaction and receipt ids; disconnects before prepare, after prepare, before decision,
after decision and after receipt; and lost-receipt query plus same-proposal replay. Startup-setting
fixtures admit one canonical production HTTPS origin; refuse userinfo, non-root paths, queries,
fragments and noncanonical or non-HTTPS production forms; admit HTTP only for `--dev` plus a literal
loopback IP; bind absent-setting derivation only to the explicit loopback listener; prove a missing
usable hosted origin makes the route unavailable; and prove an invalid explicit setting fails
startup. The pairs `https://example.com`/`https://example.com:443` and
`http://127.0.0.1`/`http://127.0.0.1:80` prove default-port omission; non-default canonical ports
remain admitted. Counter-boundary fixtures hold exactly 32 process slots, exactly 4 tenant slots and
one below each, asserting no queue and the byte-exact `Retry-After: 5`. Injected monotonic-clock
fixtures prove 299/300-second pre-decision and 29/30-second post-decision boundaries without reset.
Native-stream fixtures split headers and payloads at every boundary, read byte-by-byte and
coalesce successive frames, proving only the header plus declared payload length delimit frames.
Raw, JSON-escaped, percent-encoded and base64 sentinels are absent from logs, audit, journals, URLs,
headers, close reasons and persisted files.

The existing `POST /api/service-accounts` remains the distinct one-shot hosted operation. It accepts
only the strict non-secret id/expiry request and returns exactly one FXSA frame with
`application/vnd.flux-exchange.service-account-handoff-v1` and `Cache-Control: no-store`; metadata
is read through the existing list route. Every former create/rotate/acquire/mint secret JSON shape
refuses with status 415 and value-free `secret_json_forbidden` before body decoding or mutation.

### HTTP v1 compatible-change policy

The four HTTP v1 identities are strict wire identities, not an additive schema promise. A change is
compatible under an existing id only when every checked request receives the same status and a body
which round-trips through the same production type, with the same members, member types,
omission/null rules, bounds and authority derivation. Internal implementation changes, diagnostics
whose text stays within the declared bounded class, and catalogue *content* changes represented by a
new stable `generation` are compatible. Adding provider fixtures or adversarial cases without
weakening an existing outcome is compatible too.

Adding even an optional response member, accepting an unknown request/query member, changing a
status/body pairing, widening a diagnostic, adding an envelope around operation arguments, changing
authentication, or adding any tenant, authority, credential, endpoint, host, runtime or UUID axis is
not compatible. Such a change receives a new protocol id and is served in parallel with v1 until
every supported consumer can select the new id. Release automation may advertise the new id only
after its own provider fixture and production serializer/deserializer gate exist; it never changes a
v1 fixture to make an incompatible implementation appear compatible.

## Trust delegation

The root-signed document is named `flux-exchange-release-trust.json`, is at most 64 KiB, and has
schema identity `exchange.release-trust.v1`:

```json
{
  "schema": "exchange.release-trust.v1",
  "origin": "https://github.com/codewandler/flux-exchange",
  "version": 1,
  "issued_at": "<UTC seconds>",
  "expires_at": "<UTC seconds>",
  "root_signing_key_ids": ["flux-exchange-root-2026-01"],
  "roles": {
    "channel": {
      "threshold": 1,
      "keys": [
        {
          "key_id": "flux-exchange-channel-2026-01",
          "minisign_public_key": "<base64 minisign public key>",
          "not_before": "<UTC seconds>",
          "not_after": "<UTC seconds>"
        }
      ]
    },
    "release": {
      "threshold": 1,
      "keys": [
        {
          "key_id": "flux-exchange-release-2026-01",
          "minisign_public_key": "<base64 minisign public key>",
          "not_before": "<UTC seconds>",
          "not_after": "<UTC seconds>"
        }
      ]
    }
  }
}
```

`version` is `1..=9007199254740991`. Issuance may be at most five minutes in the future; expiry is
later, at most 366 days after issuance, and every delegated interval lies within it.
`root_signing_key_ids` contains 1..=4 unique lexically sorted basename-safe ids admitted by Flux's
pinned offline-root policy. Each role contains 1..=4 keys sorted by `key_id`; ids are globally
unique and cannot cross roles. A threshold is `1..=keys.len()`.

`minisign_public_key` is exactly 56 characters of canonical RFC 4648 standard base64 without
whitespace or padding. It decodes to the 42-byte minisign public-key packet: ASCII `Ed`, eight key-id
bytes and 32 Ed25519 public-key bytes; re-encoding must be byte-identical. A signature must use the
prehashed minisign `ED` algorithm and its embedded eight-byte key id must equal the public packet's;
legacy non-prehashed `Ed` signatures refuse. Reusing the same 32-byte Ed25519 key under another
metadata id, elsewhere in one role, across channel/release roles, or as an online and offline root
key refuses even if the packet's eight key-id bytes differ.

For every listed root id there is exactly one signature named
`flux-exchange-release-trust.json.<root-key-id>.minisig`, at most 4 KiB. The signature set must meet
the pinned root threshold. Owner-only state retains the greatest accepted `{version, sha256}`
globally: a lower version, or different bytes at the same version, refuses before an online signer
is trusted.

The mutable `exchange-trust-v1` release is a client discovery head, not the only retained evidence.
Before an externally authorized offline-root operation advances it, the candidate document and its
exact required root-signature set are published under
`exchange-trust-v1-version-<canonical-version>`. That immutable history tag is never deleted,
recreated or clobbered. A partial private draft may add only missing expected files after every
present name, byte size and SHA-256 agrees; public bytes are then fetched through the closed transport
and root-verified before the mutable head changes. This workflow neither chooses the root policy nor
automates that external operation. The production root policy remains non-authoritative until the
final verifier schema and independently reviewed non-test values are approved by owner/security.

## Stable channel

The channel document is named `flux-exchange-release-channel.json`, is at most 256 KiB, and has
schema identity `exchange.release-channel.v2`:

```json
{
  "schema": "exchange.release-channel.v2",
  "channel": "stable",
  "origin": "https://github.com/codewandler/flux-exchange",
  "generation": 1,
  "issued_at": "<UTC seconds>",
  "expires_at": "<UTC seconds>",
  "signing_key_ids": ["flux-exchange-channel-2026-01"],
  "releases": [
    {
      "tag": "refs/tags/vX.Y.Z",
      "version": "X.Y.Z",
      "source_commit": "<40 lowercase hex>",
      "build_id": "<1..128 printable ASCII bytes>",
      "manifest_sha256": "<SHA-256 of canonical manifest bytes>",
      "release_key_ids": ["flux-exchange-release-2026-01"],
      "protocols": {
        "exchange_api": "exchange.api.v1",
        "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
        "invoke_request": "exchange.invoke-request.v1",
        "invoke_response": "exchange.invoke-response.v1",
        "connection_plan": "exchange.connection-plan.v2",
        "local_management": "exchange.local-management.v1",
        "service_account_handoff": "exchange.service-account-handoff.v1",
        "supervisor": "exchange.supervisor-ready.v2"
      }
    }
  ]
}
```

`generation` is `1..=9007199254740991`; issuance has the same five-minute future allowance, expiry
is later and at most seven days after issuance, and the document must be unexpired at use.
`signing_key_ids`
is the unique lexically sorted set whose signatures are present and must meet the currently valid
channel-role threshold. There are 1..=128 unique stable SemVer releases in ascending order, without
prerelease/build metadata or duplicate tag, version, manifest or release identity. `tag` is exactly
`refs/tags/v<version>`. `release_key_ids` is sorted, names currently valid release-role delegates,
and meets that role's threshold.

Each channel signature is named
`flux-exchange-release-channel.json.<channel-key-id>.minisig` and is at most 4 KiB. Owner-only state
retains one global greatest `{generation, sha256}` for `stable`; a trust version or signer rotation
never creates a new channel-generation namespace or lowers that floor. After that authenticated
metadata floor advances, selection filters only by Flux's compiled support for all eight protocol
fields, then chooses the greatest compatible SemVer. A valid channel with no compatible release is
a named incompatibility after its higher generation/hash has been durably accepted; it cannot be
followed by a lower generation merely because selection failed.

Before CI advances `exchange-stable-v1`, it publishes the exact trust document/root signatures used
for verification and the new channel document/channel signatures under
`exchange-stable-v1-generation-<canonical-generation>`. The history release targets the immutable
source commit. Once public, its tag and closed asset set are never deleted, recreated or clobbered;
different bytes are publication equivocation. A partial private draft may only fill missing expected
assets after present names, sizes and SHA-256 values agree. CI attests the newly produced channel
document and signatures, not the externally root-signed trust bytes, then bounded-downloads the
public snapshot, verifies its exact target/set/bytes, trust and channel signatures, and channel
provenance. Only after that succeeds may mutable-head signatures be exposed followed by the canonical
channel index. These immutable history releases are audit/recovery evidence; Flux clients continue to
use the fixed mutable-head URL and signed rollback floors above.

Rollback state advances by authenticated metadata layer, not by successful download:

1. after a higher root-threshold-valid trust document passes its own canonicalization, time and key
   checks, Flux atomically persists its trust `{version, sha256}` before reading a channel;
2. after a higher channel passes delegated threshold, canonicalization, time and global rollback,
   Flux atomically persists `{trust version/hash, channel generation/hash}` before compatibility
   selection or any manifest/target fetch;
3. no compatible release, or a later manifest, signature, archive, compatibility or network
   failure, keeps the previous verified install byte-identical but never rolls either metadata
   floor back and never falls back to an older channel generation or entry for a new start. A retry
   must use the same channel bytes or greater global values.

Each state change is one fsync-and-atomic-replace transaction in the owner-only lifecycle store. A
crash leaves either the complete prior record or complete advanced record, never a mixed tuple. A
valid higher trust followed by an invalid channel advances trust only; a valid higher channel
followed by no compatible selection or any target failure advances both metadata floors but not the
install pointer.

Expiry gates starting, not an already-owned process. Metadata verification reads `now` after the
complete bounded metadata set arrives; `now == expires_at` refuses. A stopped/new `start`, import,
reinstall or target commit rechecks that both trust and channel satisfy `now < expires_at` before it
launches. If either expires during a target download, floors remain advanced and staging is removed,
but no process starts. A healthy child whose verified readiness was accepted before expiry remains
healthy and is not killed merely because update metadata ages out: `status` is local, reports the
trust/channel expiry diagnostic beside `healthy`, repeated `start` is idempotent and returns the
same owned child, and `stop` always works. Once stopped, it cannot start again until fresh online
metadata or a fully verified unexpired offline set passes. Expiry is update freshness, not a hidden
remote process-revocation channel.

## Release manifest

The immutable-tag document is named `flux-exchange-release-manifest.json`, is at most 256 KiB, and
has schema identity `exchange.release-manifest.v2`:

```json
{
  "schema": "exchange.release-manifest.v2",
  "origin": "https://github.com/codewandler/flux-exchange",
  "tag": "refs/tags/vX.Y.Z",
  "version": "X.Y.Z",
  "source_commit": "<40 lowercase hex>",
  "build_id": "<1..128 printable ASCII bytes>",
  "protocols": {
    "exchange_api": "exchange.api.v1",
    "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
    "invoke_request": "exchange.invoke-request.v1",
    "invoke_response": "exchange.invoke-response.v1",
    "connection_plan": "exchange.connection-plan.v2",
    "local_management": "exchange.local-management.v1",
    "service_account_handoff": "exchange.service-account-handoff.v1",
    "supervisor": "exchange.supervisor-ready.v2"
  },
  "signing_key_ids": ["flux-exchange-release-2026-01"],
  "assets": [
    {
      "target": "<one closed supported target>",
      "archive": "<basename>",
      "format": "tar.zst|zip",
      "archive_bytes": 1,
      "archive_sha256": "<SHA-256>",
      "executable": {
        "path": "<single-root relative path ending flux-exchange|flux-exchange.exe>",
        "bytes": 1,
        "sha256": "<SHA-256>"
      },
      "other_members": [
        {
          "path": "<single-root relative path>",
          "kind": "documentation|license",
          "bytes": 1,
          "sha256": "<SHA-256>"
        }
      ]
    }
  ]
}
```

The tag/version/source/build/protocol/release-key values agree exactly with the selected channel
entry. `signing_key_ids` is sorted, meets the current release-role threshold and equals the channel
entry's `release_key_ids`. Its signature files are
`flux-exchange-release-manifest.json.<release-key-id>.minisig`, each at most 4 KiB.

There are exactly five assets sorted by target, one for each X-126 target. Archive/member byte values
are `1..=268435456`; an archive has at most 16 regular-file members and 536870912 expanded bytes in
total. `other_members` contains 0..=15 entries sorted by path and, with the executable, is the exact
archive member set. Paths are relative single-root UTF-8 paths of at most 240 bytes. The complete
path, collision, format and digest rules remain acceptance criteria in X-126.

Provenance is deliberately absent from the v2 manifest and client/offline set. Minisign threshold
signatures authenticate canonical metadata and the signed archive/executable SHA-256 values close
client identity. Exchange CI still emits and verifies bounded repository/workflow provenance as
publication evidence tied to the immutable tag and source commit, but Flux neither downloads,
parses, trusts nor persists it. A provenance verifier therefore cannot become a second underspecified
client trust root.

## Compatibility and readiness

`flux-exchange compatibility --json` emits only this side-effect-free object:

```json
{
  "schema": "exchange.compatibility.v2",
  "release": {
    "tag": "refs/tags/vX.Y.Z",
    "version": "X.Y.Z",
    "source_commit": "<40 lowercase hex>",
    "build_id": "<1..128 printable ASCII bytes>"
  },
  "protocols": {
    "exchange_api": "exchange.api.v1",
    "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
    "invoke_request": "exchange.invoke-request.v1",
    "invoke_response": "exchange.invoke-response.v1",
    "connection_plan": "exchange.connection-plan.v2",
    "local_management": "exchange.local-management.v1",
    "service_account_handoff": "exchange.service-account-handoff.v1",
    "supervisor": "exchange.supervisor-ready.v2"
  }
}
```

X-128's one-shot record is at most 16 KiB. This Linux example shows the exact common shape:

```json
{
  "schema": "exchange.supervisor-ready.v2",
  "release": {
    "tag": "refs/tags/vX.Y.Z",
    "version": "X.Y.Z",
    "source_commit": "<40 lowercase hex>",
    "build_id": "<1..128 printable ASCII bytes>",
    "executable_sha256": "<SHA-256>"
  },
  "protocols": {
    "exchange_api": "exchange.api.v1",
    "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
    "invoke_request": "exchange.invoke-request.v1",
    "invoke_response": "exchange.invoke-response.v1",
    "connection_plan": "exchange.connection-plan.v2",
    "local_management": "exchange.local-management.v1",
    "service_account_handoff": "exchange.service-account-handoff.v1",
    "supervisor": "exchange.supervisor-ready.v2"
  },
  "bind": { "scheme": "http", "host": "127.0.0.1", "port": 1 },
  "process": {
    "pid": 1,
    "start_identity": {
      "kind": "linux-proc-start",
      "boot_id": "00000000-0000-0000-0000-000000000001",
      "ticks": "1"
    }
  }
}
```

The release and protocol fields shared by channel, manifest, compatibility and readiness agree
exactly. Readiness's executable digest agrees with the selected target's manifest entry and the
bytes Flux verified. The schema field agrees with `protocols.supervisor`.

`bind.scheme` is exactly `http`, `host` is exactly the JSON string `127.0.0.1` or `::1`, and `port`
is an integer `1..=65535`. `pid` is an integer `1..=4294967295`. `start_identity` is a closed tag;
unknown members/kinds or a kind not native to the selected target refuse:

- Linux is exactly
  `{kind:"linux-proc-start",boot_id:<lowercase RFC 4122 UUID>,ticks:<canonical 1..20 digit decimal string>}`.
  `ticks` is `1..=18446744073709551615` from `/proc/<pid>/stat` field 22 and `boot_id` is read from
  `/proc/sys/kernel/random/boot_id`.
- macOS is exactly
  `{kind:"macos-proc-start",seconds:<canonical 1..20 digit decimal string>,microseconds:<integer>}`.
  `seconds` is `1..=9223372036854775807`, `microseconds` is `0..=999999`, and both come from
  `proc_pidinfo(PROC_PIDTBSDINFO)`.
- Windows is exactly
  `{kind:"windows-process-creation",filetime:<canonical 1..20 digit decimal string>}` where
  `filetime` is `1..=18446744073709551615` 100-nanosecond ticks returned by `GetProcessTimes`.

Flux compares the record to the already-open child process handle using the same native source; a
PID lookup alone never proves identity.

### Supervised inherited-handle and liveness ABI

On Unix, Flux executes exactly `flux-exchange --supervised` after duplicating the readiness pipe's
write end to inherited FD 3 and the liveness pipe's read end to inherited FD 4. FD 3 is write-only,
FD 4 is read-only, both have close-on-exec cleared for this one spawn, and every other non-standard
descriptor is closed. Exchange refuses supervised mode when either fixed FD is absent, the same
object, not a pipe, or has the wrong usable direction. No descriptor number is accepted from argv,
environment, stdin or stdout.

On Windows, arbitrary inherited HANDLE values cannot be discovered implicitly. Flux therefore uses
exactly the hidden argv
`flux-exchange.exe --supervised --supervisor-readiness-handle <H> --supervisor-liveness-handle <H>`.
Each `<H>` is a canonical unsigned decimal `usize` string with no sign/leading zero, nonzero,
distinct, and names respectively a pipe write handle and pipe read handle. Flux sets inheritance
only for those handles through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`; Exchange checks both are
inherited `FILE_TYPE_PIPE` handles and refuses an absent, duplicate, malformed or unusable handle.
No undocumented `STARTUPINFO` reserved field is used. These two numeric non-secret capabilities are
the only supervised values permitted in argv; control credentials, Service Account tokens, vendor
values, addresses and lifecycle state remain forbidden from argv and environment on every platform.

Exchange starts a dedicated native liveness thread before it opens a store or binds. The supervisor
keeps only the liveness pipe's write end and never writes a byte. The thread blocks on the read end;
EOF, any byte, or any read error immediately terminates the Exchange process through the platform's
non-unwinding process-exit primitive. Normal supervised shutdown is the same close-and-wait path.
Thus supervisor exit or `SIGKILL` on Linux/macOS, and supervisor exit or `TerminateProcess` on
Windows, closes the writer and terminates Exchange even if the async runtime is wedged. Exchange
immediately restores close-on-exec on Unix
and clears `HANDLE_FLAG_INHERIT` on Windows after discovery, so readiness/liveness capabilities
never reach connector children.

After all startup checks and the OS-selected loopback bind succeed, Exchange writes the sole RFC
8785 readiness object (no prefix/suffix/newline), closes FD 3/readiness HANDLE, and never writes
readiness data elsewhere. EOF before one complete object, more than 16 KiB, trailing bytes, or a
second object refuses lifecycle ownership. The readiness channel carries no liveness/control data;
the liveness pipe carries no payload at all.

## Feasible GitHub release transport

The logical origin is signed, but GitHub serves release assets through a CDN redirect. Network
verification therefore constructs only these initial requests:

- trust: `https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1/flux-exchange-release-trust.json`
- channel: `https://github.com/codewandler/flux-exchange/releases/download/exchange-stable-v1/flux-exchange-release-channel.json`
- immutable release inputs:
  `https://github.com/codewandler/flux-exchange/releases/download/vX.Y.Z/<signed-basename>`

Publication/audit tooling additionally constructs only these immutable evidence requests; clients do
not use them for ordinary discovery:

- trust history:
  `https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1-version-<version>/<trust-basename>`
- channel history:
  `https://github.com/codewandler/flux-exchange/releases/download/exchange-stable-v1-generation-<generation>/<trust-or-channel-basename>`

Every metadata signature uses the same directory and its exact derived basename. The initial request
sends no authorization, proxy authorization or cookie, ignores proxy environment/configuration, and
accepts exactly HTTP 302 with one `Location`. That location is at most 8192 bytes and must be HTTPS
on default port with no userinfo or fragment, host exactly `release-assets.githubusercontent.com`,
and an ASCII path matching
`/github-production-release-asset/[1-9][0-9]{0,19}/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`.
Its query is at most 6144 bytes, has non-empty values of at most 2048 bytes, no duplicate names, and
only the following raw ASCII names. Names may not use percent encoding; values must be valid percent
encoding and contain no decoded control character.

```text
jwt response-content-disposition response-content-type rscd rsct se sig ske skoid sks skt sktid skv sp spr sr sv
```

After validation, the client makes a new credential-free GET to that exact URL. The final response
must be HTTP 200 and cannot redirect. The CDN URL/query is transient transport only: it is never
persisted, logged, signed, accepted from metadata or used as release identity. Declared byte bounds
apply while reading, and minisign plus the signed SHA-256 still decide authenticity and identity.

The historical `version` and `generation` tag suffixes are canonical decimal integers in
`1..=9007199254740991`. Trust-history releases admit only the trust document and its basename-derived
root signatures. Stable-history releases admit exactly one trust document, its required root
signatures, one channel document and its required channel signatures. No history tag admits a
manifest, archive, arbitrary URL or undeclared asset.

## Offline set and conformance ownership

An offline import is one closed set: trust document and root signatures; channel document and
channel-role signatures; selected manifest and release-role signatures; and the one target archive.
It runs the identical canonicalization, thresholds, time validity, rollback, newest-compatible
selection, bounds, signature, digest, archive and executable checks. It bypasses only the GitHub
transport. Provenance is publication evidence and is not an offline/client input.

X-126 materializes provider fixtures under `tests/fixtures/exchange-release-v2/`: canonical positive
trust/channel/manifest/compatibility/readiness bytes, test-only signatures and bounded archives;
plus a machine-readable mutation inventory covering duplicate/unknown/non-canonical fields,
threshold and role failures, expiry/future/rollback/equivocation, 129 releases, incompatible-newest
selection, manifest disagreement, archive/path/digest failures and every rejected redirect branch.
Flux vendors those exact fixture bytes with their Exchange commit and fixture-set SHA-256 and runs
the same expected outcomes. A byte or expected-outcome difference is a cross-repository contract
failure, not a reason to create another Flux-owned v2 fixture.

`fixture-set.json.exchange_commit` names the committed provider-behavior and native-binding
baseline whose generated bytes the inventory records. It does not claim to be the direct parent of
a later provenance-refresh commit; the separately recorded fixture-set SHA-256 identifies the exact
final manifest bytes. `native_cases` maps each platform-dependent verdict to its closed target set,
Cargo test target and exact test name. The five native release jobs first verify that mapping and
then list and execute each named test with `--exact`; a broad suite invocation or a zero-test filter
is not evidence for a fixture verdict.

The mutation inventory has these minimum named cases and closed outcomes (implementations may add
cases, never weaken these):

| Fixture id | Mutation/proof | Expected outcome |
|---|---|---|
| `positive-linux`, `positive-macos`, `positive-windows` | complete current trust/channel/manifest/archive plus native readiness/liveness | install/readiness accepted |
| `positive-signer-overlap` | higher trust, globally higher channel, old+new threshold signatures | newest compatible selected without Flux change |
| `integer-over-jcs-safe` | any JSON integer `9007199254740992` | document invalid |
| `decimal-noncanonical` | sign, leading zero, 21 digits or numeric overflow in a decimal-string field | document/readiness invalid |
| `id-or-basename-unsafe` | slash, `..`, non-ASCII, repeated hyphen/token separator, leading/trailing punctuation or case-fold collision | document invalid before path construction |
| `minisign-key-malformed` | noncanonical base64, wrong length/algorithm, embedded-id disagreement | trust invalid |
| `minisign-key-reused` | same Ed25519 bytes under two ids/roles/root+online packets | trust invalid |
| `channel-floor-survives-rotation` | higher trust with lower channel generation | channel rollback |
| `higher-channel-no-compatible` | globally higher valid channel with no compatible release | channel floor advances, incompatibility reported, old install retained, lower generation later refuses |
| `higher-channel-target-fails` | globally higher valid channel then manifest/download/archive/compatibility failure | floors advance, old install retained, new start refused |
| `same-number-different-bytes` | trust version or channel generation repeated with another digest | trust/channel equivocation refusal |
| `expiry-equality-stopped` | `now == expires_at` with no live child | expired; start/import/reinstall refused |
| `expiry-equality-live` | same boundary with an already-owned healthy child | healthy plus metadata-expired diagnostic; same-child start/stop works |
| `readiness-bind-domain` | other host spelling, scheme, zero/65536 port or zero/out-of-range PID | readiness invalid |
| `readiness-start-kind` | wrong-platform/unknown tag, malformed UUID/decimal or microseconds 1000000 | readiness invalid |
| `unix-inherited-abi` | missing/aliased/wrong-direction FD 3/4 or readiness on stdout/env/another FD | supervised startup refuses |
| `windows-inherited-abi` | malformed/zero/duplicate/unlisted/non-pipe HANDLE or reserved-field/env discovery | supervised startup refuses |
| `supervisor-death-*` | normal exit plus `SIGKILL` on Unix or `TerminateProcess` on Windows, with responsive and async-wedged child | liveness EOF exits Exchange and releases port |
| `provenance-client-input` | provenance member in manifest, network selection or offline set | unknown/disallowed client input |

Positive documents exercise boundary maxima and one value below expiry; adversarial pairs change one
fact at a time. The fixture manifest records each file SHA-256, mutation id, injected clock/platform,
prior rollback/install state and expected state/result so a test cannot claim a refusal without
proving which durable values survived.
