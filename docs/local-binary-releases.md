# Verified local binary releases

This is the operator contract for publishing and consuming the separately released local
`flux-exchange` server. The provider-owned wire details live in
[`designs/local-release-v1.md`](designs/local-release-v1.md); this document explains how to operate
that contract without turning the workflow, crates.io, or a local checkout into another trust root.

The contract and its fixtures do not by themselves make a production channel live. The first usable
release requires one real immutable `vX.Y.Z` tag, all five public assets, production trust metadata,
and a successful post-publication verification of the downloaded release. Draft assets or a green
staging run are not release evidence.

## What is released

A tag release contains exactly one server archive for each supported Flux target:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`

Each target reaches the signed manifest only after its native persistence and supervised-readiness
proof passes. Cross-compilation alone is not support. Unix targets use deterministic `tar.zst`
archives and Windows uses a deterministic zip. An archive contains the server executable plus only
declared documentation or licence members.

These binaries are a separate Exchange product artifact. They are **not**:

- the `codewandler-flux-exchange-host` crates.io artifact;
- part of a Flux release;
- an official integration plugin; or
- a connector runtime.

The Exchange executable may embed a released connector catalogue, but neither the Flux distribution
nor these archives contain connector/plugin executables or separately downloaded runtime helpers.

## Fixed origin and trust domains

The signed logical origin is exactly:

```text
https://github.com/codewandler/flux-exchange
```

Clients construct only these initial request shapes; metadata never supplies an arbitrary URL:

```text
https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1/flux-exchange-release-trust.json
https://github.com/codewandler/flux-exchange/releases/download/exchange-stable-v1/flux-exchange-release-channel.json
https://github.com/codewandler/flux-exchange/releases/download/vX.Y.Z/<signed-basename>
```

The first two documents are the root-signed trust document and delegated stable channel. Immutable
release manifests, signatures and archives use the third shape. GitHub may answer an initial
credential-free request with exactly one validated HTTPS redirect to
`release-assets.githubusercontent.com`; the final response must be `200` and cannot redirect again.
Proxy configuration, authorization, proxy authorization and cookies are not sent. The transient CDN
URL is transport, never signed identity or persisted state.

Exchange release signing is an independent trust domain:

- Flux pins the long-lived, independently reviewed **offline root** public key and closed root
  policy. The offline private key stays outside GitHub Actions.
- Root-threshold-signed `flux-exchange-release-trust.json` delegates separate short-lived `channel`
  and `release` roles. Those online signers satisfy the threshold, role and validity interval in the
  current trust document; one role cannot substitute for another.
- The channel and manifest are accepted only through their delegated signatures, canonical bytes,
  fixed origin and declared bounds. A crates.io identity, Flux signature, connector signature,
  GitHub token or provenance record cannot substitute.

X-126 creates or configures no production signing secret. Production requires exactly these two
delegated signing secrets in the GitHub Actions `local-release` environment (not as repository
variables or general repository secrets):

- `FLUX_EXCHANGE_CHANNEL_SIGNING_KEY_B64`
- `FLUX_EXCHANGE_RELEASE_SIGNING_KEY_B64`

Each value is one-line canonical RFC 4648 base64, with normal `=` padding and no whitespace, of the
complete minisign secret-key file bytes. Preflight decodes the material canonically, derives its
embedded key id and public packet, and proves that it holds the corresponding channel or release
role and is currently valid under root-signed trust metadata. It refuses before any build or exposed
file if the secret is missing, malformed, has the wrong role, or disagrees with that metadata.

An authorized operator resumes an existing release only by dispatching the workflow at that same
immutable tag ref (for example, `gh workflow run local-release.yml --ref vX.Y.Z -f tag=vX.Y.Z`). The
workflow refuses a branch-ref dispatch even when its input names a tag, because GitHub provenance is
bound to the dispatch ref and source SHA.

Production also requires the checked `.github/release-root-policy.json` containing the independently
reviewed root public-key ids and packets. The policy and both delegated secrets are intentionally
absent until operators supply them; their absence means preflight refusal, not an unpublished default
trust policy. The offline root secret never enters CI. Repository/workflow provenance remains
separate post-publication evidence; it is neither a client trust mechanism nor an online/offline
import input.

### Production authorization is still a hard stop

As of 2026-08-04 there is no `local-release` environment, neither delegated secret is configured,
`.github/release-root-policy.json` is absent, neither `exchange-trust-v1` nor
`exchange-stable-v1` exists, and `main` is not protected. This implementation creates none of those
objects and does not turn an unprotected merge into release authorization. The eventual policy must
be a separately reviewed, non-test policy matching the final integrated verifier schema; the
repository does not choose production root ids, packets, threshold or custodians in advance.

The next unused version is `v0.18.0`. Pushing that tag starts both this five-target binary workflow
and the crates.io workflow, whose publications are irreversible. Creating or pushing the tag is
therefore one explicit release-operator authorization boundary for both products, after owner and
security approval confirms every production hard stop above has been resolved. Workflow dispatch is
only a byte-idempotent resume at an already authorized immutable tag; it is not permission to create
the tag. X-126 remains active after the implementation merge until that production release passes
the public five-target verifier.

Decision 0007 adds another independent stop. A permanent content-derived preflight closes both tag
workflows before credentials or publication unless the tree consumes the checksummed registry
release of `codewandler-connector-secrets` 0.20.0, contains only the final v2/eight-protocol
producers and has a complete digest-checked v2 fixture inventory. There is no override flag or
readiness marker. The six-protocol compatibility shape previously implemented and fixture-tested is
unpublished implementation evidence only. Until X-134's owner-bound local management, direct vendor credential
insertion and one-shot Service Account credential-handoff pass their native fixtures, there is no production
trust ceremony, tag, crates.io publication, public binary asset, stable-channel update or X-126
completion.

## Compatibility identity

Inspect a built executable without opening a store, binding a listener or requiring identity
configuration:

```sh
flux-exchange compatibility --json
```

The first public release writes JSON only, with this exact v2 shape and eight protocol keys. The
current six-field implementation is deliberately unpublished and cannot pass release preflight:

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

Package version is audit identity, not a compatibility guess. A client selects the greatest stable
SemVer channel entry for which all eight signed protocol ids are supported, then requires the manifest,
executable output and supervised readiness record to agree exactly. A newer incompatible release is
skipped. If none is compatible, selection refuses; it does not try `latest`, `PATH`, a sibling
checkout or Cargo output.

This is what keeps releases independent. A compatible Exchange release, including one that embeds a
newly released connector catalogue, reaches existing Flux installations through the signed channel
without a Flux release. A new Flux release is required only when it must learn an unsupported
protocol/client contract or change the pinned offline-root/trust policy.

## Update, rollback and expiry

Trust and channel rollback floors are global and monotonic, including across delegated signer
rotation:

1. After a higher root-valid trust document passes canonicalization, time, key and threshold checks,
   persist its `{version, sha256}` in owner-only state before reading a channel.
2. After a higher delegated-valid channel passes canonicalization, time, threshold and rollback
   checks, persist `{trust version/hash, channel generation/hash}` before compatibility selection or
   any target fetch.
3. If selection finds no compatible release, or a later manifest, signature, download, archive,
   executable or compatibility check fails, retain the previous verified install byte-for-byte.
   Never lower either floor and never launch or fall back to an older channel generation or entry
   for a new start.

A lower number is rollback. Different authenticated bytes at the same trust version or channel
generation are equivocation. Both refuse. Each floor update is one owner-only fsync plus atomic
replacement, so a crash leaves either the complete prior record or complete advanced record.

Trust delegates and the stable channel expire. Validity is half-open: equality with `not_after` or
`expires_at` is expired. A new or stopped start, import, reinstall, and launch after a download all
require current trust and channel metadata. Expiry during a download removes staging and prevents a
start without reversing accepted floors.

Expiry is update freshness, not remote revocation. An already-owned healthy process stays healthy;
local status reports the metadata-expiry diagnostic, repeated start returns that same process, and
stop still works. Once stopped, it cannot restart until fresh online metadata or a fully verified,
unexpired offline set passes.

## Signer rotation

Routine delegated rotation is an Exchange-only operation and does not require a Flux release:

1. Publish new root-signed trust metadata that delegates the successor while the old signer remains
   valid.
2. Leave enough overlap for clients with cached trust metadata to refresh. During overlap, sign the
   channel or manifest with the threshold-satisfying old/new set declared by the metadata.
3. Remove the old delegate only in a later root-signed trust version, after the overlap.

A client refreshes and verifies trust metadata before rejecting an otherwise unknown online signer.
The one stable channel-generation floor does not reset when the trust version or delegate changes.
Unannounced ids, role confusion, signature substitution and key-id/signature disagreement refuse.

Rotating the pinned offline root is exceptional and coordinated: a Flux release must trust the
successor before it becomes required, transition metadata must satisfy both root policies, and only a
later Flux release may remove the retired root.

Replacing the mutable `exchange-trust-v1` release is an external offline-root operation, not a step
in `local-release.yml`. Before a trust document becomes the mutable head, owner/security operators
publish its exact canonical document and required root signatures under the immutable tag
`exchange-trust-v1-version-<version>`. The version is canonical decimal in the signed v1 domain. An
existing public history release is never deleted, recreated or uploaded with clobber semantics; its
tag target, exact asset names, sizes and bytes must agree or rotation refuses. A partial private draft
may only fill missing expected assets after matching every present size and SHA-256. The complete
public archive is then re-downloaded through the bounded one-redirect transport and verified against
the reviewed policy before `exchange-trust-v1` changes. No online workflow generates that trust
document or attests externally root-signed bytes as if CI produced them.

## Online update and offline import

The online path fetches root-signed trust metadata, the delegated signed stable channel, the selected
immutable signed manifest and the one archive for the current target through the closed transport
above. It validates freshness and advances rollback floors before selection/download at the points
described above. The archive is staged under declared byte/member/path limits; the archive and
extracted executable SHA-256 values, exact member set, compatibility JSON and readiness identity all
have to agree before installation can commit.

An **offline import** is one closed set:

- `flux-exchange-release-trust.json` and its root signatures;
- `flux-exchange-release-channel.json` and its delegated channel signatures;
- the selected `flux-exchange-release-manifest.json` and its delegated release signatures; and
- the one target archive named by that manifest.

Import uses the identical canonicalization, root/delegate thresholds, freshness, global rollback,
newest-compatible selection, manifest agreement, size/path/member bounds, signatures, digests,
archive extraction and compatibility checks. It bypasses only GitHub transport. An offline bundle
is not allowed to supply provenance, a mutable URL, an exact version override, an expired snapshot or
a different trust root.

## Publication evidence

Publication is CI-only from an immutable `vX.Y.Z` tag whose version matches the workspace and whose
exact commit passes the full repository gate. Publication is incomplete until the post-publication
verifier downloads the public release by immutable tag, requires the closed five-target asset set,
verifies signatures and every digest/member, runs the host-platform compatibility command, verifies
the separate bounded provenance evidence, and proves that the signed channel selects the release
without an exact-version input.

Record the tag, public release URL, verifier run and source SHA in X-126 before closing it. A failed
live verification leaves the release unusable and the story open.

Before the mutable `exchange-stable-v1` head advances, the workflow publishes
`exchange-stable-v1-generation-<generation>`. That immutable release contains exactly the verified
trust document and its required root signatures plus the new channel document and its required
channel signatures. The workflow attests the new channel bytes, not the externally supplied trust
bytes, then re-downloads the complete snapshot through the same bounded one-redirect transport,
checks its tag target, names, sizes and bytes, verifies root and channel signatures, and verifies the
channel provenance. Only then does it upload stable signatures followed by the mutable canonical
index.

Public history is append-only: an existing version release or stable-generation snapshot is never
deleted, recreated or clobbered. A private partial draft may only fill absent expected assets after
the present names, sizes and SHA-256 digests agree; an extra or changed asset refuses. Immutable
version releases, trust-version and stable-generation snapshots, their attestations, and successful
workflow runs are retained as audit evidence. The mutable trust and stable heads remain discovery
pointers and cannot erase or substitute for that history.
