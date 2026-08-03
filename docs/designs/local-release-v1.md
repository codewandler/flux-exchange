# Local Exchange release protocol v1

This document is the provider-owned wire contract for X-126 and X-128. Exchange owns these names,
fields, bounds and conformance cases. Flux C-510 consumes them verbatim; a Flux document or type may
describe how it responds, but cannot define another shape under the same or a competing v1 name.

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

`version` is `1..=u64::MAX`. Issuance may be at most five minutes in the future; expiry is later,
at most 366 days after issuance, and every delegated interval lies within it. `root_signing_key_ids`
contains 1..=4 unique lexically sorted ids admitted by Flux's pinned offline-root policy. Each role
contains 1..=4 keys sorted by `key_id`; ids are globally unique and cannot cross roles. A threshold
is `1..=keys.len()`.

For every listed root id there is exactly one signature named
`flux-exchange-release-trust.json.<root-key-id>.minisig`, at most 4 KiB. The signature set must meet
the pinned root threshold. Owner-only state retains the greatest accepted `{version, sha256}`: a
lower version, or different bytes at the same version, refuses before an online signer is trusted.

## Stable channel

The channel document is named `flux-exchange-release-channel.json`, is at most 256 KiB, and has
schema identity `exchange.release-channel.v1`:

```json
{
  "schema": "exchange.release-channel.v1",
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
        "exchange_api": "<versioned id>",
        "effective_catalogue_response": "<versioned id>",
        "invoke_request": "<versioned id>",
        "invoke_response": "<versioned id>",
        "connection_plan": "exchange.connection-plan.v1",
        "supervisor": "exchange.supervisor-ready.v1"
      }
    }
  ]
}
```

`generation` is `1..=u64::MAX`; issuance has the same five-minute future allowance, expiry is later
and at most seven days after issuance, and the document must be unexpired at use. `signing_key_ids`
is the unique lexically sorted set whose signatures are present and must meet the currently valid
channel-role threshold. There are 1..=128 unique stable SemVer releases in ascending order, without
prerelease/build metadata or duplicate tag, version, manifest or release identity. `tag` is exactly
`refs/tags/v<version>`. `release_key_ids` is sorted, names currently valid release-role delegates,
and meets that role's threshold.

Each channel signature is named
`flux-exchange-release-channel.json.<channel-key-id>.minisig` and is at most 4 KiB. Owner-only state
retains the greatest accepted `{generation, sha256}` under the accepted trust version and applies the
same rollback/equivocation rule as trust metadata. Selection filters only by Flux's compiled support
for all six protocol fields, then chooses the greatest compatible SemVer.

## Release manifest

The immutable-tag document is named `flux-exchange-release-manifest.json`, is at most 256 KiB, and
has schema identity `exchange.release-manifest.v1`:

```json
{
  "schema": "exchange.release-manifest.v1",
  "origin": "https://github.com/codewandler/flux-exchange",
  "tag": "refs/tags/vX.Y.Z",
  "version": "X.Y.Z",
  "source_commit": "<40 lowercase hex>",
  "build_id": "<1..128 printable ASCII bytes>",
  "protocols": {
    "exchange_api": "<versioned id>",
    "effective_catalogue_response": "<versioned id>",
    "invoke_request": "<versioned id>",
    "invoke_response": "<versioned id>",
    "connection_plan": "exchange.connection-plan.v1",
    "supervisor": "exchange.supervisor-ready.v1"
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
      ],
      "provenance": "<archive basename>.intoto.jsonl"
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
path, collision, format, digest and provenance rules remain acceptance criteria in X-126.

## Compatibility and readiness

`flux-exchange compatibility --json` emits only this side-effect-free object:

```json
{
  "schema": "exchange.compatibility.v1",
  "release": {
    "tag": "refs/tags/vX.Y.Z",
    "version": "X.Y.Z",
    "source_commit": "<40 lowercase hex>",
    "build_id": "<1..128 printable ASCII bytes>"
  },
  "protocols": {
    "exchange_api": "<versioned id>",
    "effective_catalogue_response": "<versioned id>",
    "invoke_request": "<versioned id>",
    "invoke_response": "<versioned id>",
    "connection_plan": "exchange.connection-plan.v1",
    "supervisor": "exchange.supervisor-ready.v1"
  }
}
```

X-128's one-shot record is at most 16 KiB and has this exact shape:

```json
{
  "schema": "exchange.supervisor-ready.v1",
  "release": {
    "tag": "refs/tags/vX.Y.Z",
    "version": "X.Y.Z",
    "source_commit": "<40 lowercase hex>",
    "build_id": "<1..128 printable ASCII bytes>",
    "executable_sha256": "<SHA-256>"
  },
  "protocols": {
    "exchange_api": "<versioned id>",
    "effective_catalogue_response": "<versioned id>",
    "invoke_request": "<versioned id>",
    "invoke_response": "<versioned id>",
    "connection_plan": "exchange.connection-plan.v1",
    "supervisor": "exchange.supervisor-ready.v1"
  },
  "bind": { "scheme": "http", "host": "127.0.0.1|::1", "port": 1 },
  "process": { "pid": 1, "start_identity": "<OS process-start identity>" }
}
```

The release and protocol fields shared by channel, manifest, compatibility and readiness agree
exactly. Readiness's executable digest agrees with the selected target's manifest entry and the
bytes Flux verified. The schema field agrees with `protocols.supervisor`.

## Feasible GitHub release transport

The logical origin is signed, but GitHub serves release assets through a CDN redirect. Network
verification therefore constructs only these initial requests:

- trust: `https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1/flux-exchange-release-trust.json`
- channel: `https://github.com/codewandler/flux-exchange/releases/download/exchange-stable-v1/flux-exchange-release-channel.json`
- immutable release inputs:
  `https://github.com/codewandler/flux-exchange/releases/download/vX.Y.Z/<signed-basename>`

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

## Offline set and conformance ownership

An offline import is one closed set: trust document and root signatures; channel document and
channel-role signatures; selected manifest and release-role signatures; the one target archive and
its provenance. It runs the identical canonicalization, thresholds, time validity, rollback,
newest-compatible selection, bounds, signature, digest, archive and executable checks. It bypasses
only the GitHub transport.

X-126 materializes provider fixtures under `tests/fixtures/exchange-release-v1/`: canonical positive
trust/channel/manifest/compatibility/readiness bytes, test-only signatures and archive/provenance;
plus a machine-readable mutation inventory covering duplicate/unknown/non-canonical fields,
threshold and role failures, expiry/future/rollback/equivocation, 129 releases, incompatible-newest
selection, manifest disagreement, archive/path/digest failures and every rejected redirect branch.
Flux vendors those exact fixture bytes with their Exchange commit and fixture-set SHA-256 and runs
the same expected outcomes. A byte or expected-outcome difference is a cross-repository contract
failure, not a reason to create another Flux-owned v1 fixture.
