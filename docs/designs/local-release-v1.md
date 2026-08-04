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
| `PlanRevision`, `TargetRevision`, `ProposalDigest` | string of exactly 64 lowercase hexadecimal characters |
| `TransactionId`, `ReceiptId` | opaque 256-bit value encoded as exactly 64 lowercase hexadecimal characters; all-zero refuses |
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
`targets` has 1..=64 entries, `settings` and `authorities` have 0..=64, and `secrets` has 1..=64.
Every `settings`/`authorities` target occurs exactly once in `targets`; every `secrets` target occurs
exactly once in the initiating `targets`. Secret ordinals are exactly `1, 2, ..., secrets.len()` in
array order. There is no ordinal zero and no sparse or caller-chosen ordinal.

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
inbound bindings in lexical binding order.

The 65,536-byte control bound is measured over the complete canonical JSON payload. The cumulative
1,048,576-byte ceremony bound is the sum of every FXLM payload length in the one logical operation,
including control and raw-secret payloads but excluding the 12-byte headers. The limit does not
replace the per-control, per-secret or 64-secret limits; the first limit crossed refuses.

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
| grant `PREVIEW` `0x0010`, client | `{"connector":Connector,"selector":Selector}` |
| grant `CANDIDATE` `0x0011`, server | `{"candidate":GrantCandidate,"proposal_digest":ProposalDigest,"revision":StoreRevision}` |
| grant `APPLY` `0x0012`, client | `{"candidate":GrantCandidate,"proposal_digest":ProposalDigest,"revision":StoreRevision}` |
| grant `QUERY` `0x0013`, client | `{"receipt_id":ReceiptId}` |
| grant `RECEIPT` `0x0014`, server | the exact grant receipt below |
| Service Account `MINT` `0x0020`, client | `{"expires_at":ExpiresAt,"id":ServiceAccountId}` plus the separately transferred writer capability; no descriptor/HANDLE number occurs in JSON |
| Service Account `QUERY` `0x0021`, client | `{"receipt_id":ReceiptId}` |
| Service Account `RECEIPT` `0x0022`, server | the exact Service Account receipt below |
| hosted credential `BEGIN` `0x0030`, client | `{"action":"acquire"|"rotate","connector":Connector,"label":Label,"plan_revision":PlanRevision,"targets":[PlanTarget...]}` |
| hosted credential `COMMIT` `0x0031`, client | `{"proposal_digest":ProposalDigest,"transaction_id":TransactionId}` |
| hosted credential `RECEIPT` `0x0032`, server | the exact connect receipt below, whose `operation` agrees with `action` |
| hosted credential `QUERY` `0x0033`, client | `{"receipt_id":ReceiptId}` |
| `ERROR` `0x7fff`, server | one exact error object below |

Hosted credential ceremonies reuse shared `NEED_SECRETS` `0x0002` and `SECRET` `0x0003`; there are
no hidden `0x0034`/`0x0035` aliases. `proposal_digest` in `NEED_SECRETS` is the server's SHA-256 over
the canonical non-secret proposal defined below. `COMMIT` must echo the byte-identical digest and
transaction id. Receipt query always names a receipt id and never a transaction, connector, label,
Service Account id or proposal. A response-loss caller that never received a receipt id replays the
byte-identical initiating proposal on a new connection/WebSocket; it does not manufacture a query.

The state table is exhaustive. `S` is the initial state and `T` is terminal:

| Operation | Valid frames and states |
|---|---|
| connect/replay | `S --client 0x0001--> B`; `B --server 0x0006--> T` for same-proposal replay, `B --server 0x0002--> N`, or `B --server 0x7fff--> T`; `N --client 0x0003 ordinals 1..N in order--> C`; `C --client 0x0004--> D`; `D --server 0x0006|0x7fff--> T` |
| connect query | `S --client 0x0005--> Q --server 0x0006|0x7fff--> T` |
| grant preview | `S --client 0x0010--> P --server 0x0011|0x7fff--> T` |
| grant apply | `S --client 0x0012--> D --server 0x0014|0x7fff--> T` |
| grant query | `S --client 0x0013--> Q --server 0x0014|0x7fff--> T` |
| Service Account mint | `S --client 0x0020 plus one writer--> D --server 0x0022|0x7fff--> T` |
| Service Account query | `S --client 0x0021--> Q --server 0x0022|0x7fff--> T` |
| hosted credential/replay | `S --client 0x0030--> B`; `B --server 0x0032--> T` for same-proposal replay, `B --server 0x0002--> N`, or `B --server 0x7fff--> T`; `N --client 0x0003 ordinals 1..N in order--> C`; `C --client 0x0031--> D`; `D --server 0x0032|0x7fff--> T` |
| hosted credential query | `S --client 0x0033--> Q --server 0x0032|0x7fff--> T` |

A known opcode with the wrong header direction is `wrong_direction`; a known opcode in any other
state, a skipped/repeated/out-of-order ordinal, omitted writer or second logical operation is
`unexpected_frame`; an unknown opcode is `invalid_frame`. Native EOF before the declared frame or
before `T` is `truncated_frame`; bytes after one complete JSON/raw payload are `surplus_data`.
Hosted close-code mapping remains the transport mapping below and does not change these FXLM codes.

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
| `peer_unverified` | 403 | `never` | `none` |
| `unsafe_root` | 503 | `operator` | `none` |
| `local_management_unavailable` | 503 | `operator` | `none` |
| `invalid_request` | 400 | `never` | `none` |
| `unknown_connector` | 404 | `refresh` | `none` |
| `invalid_label` | 422 | `never` | `none` |
| `secret_json_forbidden` | 415 | `never` | `none` |
| `unknown_target` | 422 | `refresh` | `none` |
| `stale_plan` | 409 | `refresh` | `none` |
| `proposal_conflict` | 409 | `refresh` | `none` |
| `connect_busy` | 409 | `refresh` | `none` |
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
already specified; hosted closes 1008 with an empty reason after an FXLM
`unexpected_frame/409/never/none` error when safe, while native returns that exact error then EOF
when safe.

After the durable decision, the connection gets a separate 30-second monotonic response budget to
complete roll-forward and canonical audit delivery. Its expiry never aborts, rolls back or edits the
proposal: it returns the applicable post-decision `query_receipt/same_proposal` error when safe,
closes/releases the hosted slot, and leaves recovery to roll forward. Neither deadline appears in a
JSON member, close reason, header, argv, environment or log value, and neither is configurable in v1.

### Prepared credential transaction ownership

`connector-secrets` owns the prepared credential representation, terminal ledger, inclusive
retired-through fence and native lifetime lease. Exchange owns only its value-free coordinator
journal, metadata/audit roll-forward and durable allocation of non-zero generations plus unique
192-bit nonces. It constructs the opaque 256-bit transaction id through the released provider API;
Flux never chooses, parses, orders, generates or logs either component.

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

A successful receipt or well-formed FXLM error is followed by close code 1000. Malformed FXLM,
wrong direction or state, surplus data or a second logical operation uses 1002 after a binary FXLM
error when one can safely be emitted. Text uses 1003; any declared frame, message, control, secret,
count or cumulative bound excess uses 1009; and expiry of the exact 300-second absolute
pre-decision deadline uses 1008. Close reasons are always empty. Before a durable decision,
disconnect, timeout or protocol failure zeroizes transient buffers and aborts or tombstones an
allocated provider transaction. After the decision Exchange never aborts: its exact 30-second
response budget may end the transport with `query_receipt`, while recovery, query or same-proposal
replay rolls forward.

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
