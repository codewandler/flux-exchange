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
