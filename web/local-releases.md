# Verified local releases

The local Exchange server has its own release contract so Flux can discover, verify and supervise a
compatible executable without bundling that executable or trusting mutable machine state. The
contract alone is not a claim that a production channel is live: a usable release is evidenced only
after its immutable tag and complete public asset set pass post-publication verification.
Before either binary or crates.io publication, a permanent content-derived publication preflight
requires the released connector-secrets 0.20.0 dependency and complete v2/eight-protocol fixtures;
there is no environment or marker override.

## Supported platforms

The signed manifest's platform set is closed:

- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`

A target enters that set only after native owner-only persistence and supervised-readiness proofs
pass. Both archives are deterministic `tar.zst` files containing `flux-exchange`.
Cross-compilation by itself is not evidence that Exchange runs safely on a target; non-Linux server
and package requests refuse before staging or signing.

These server binaries are not a crates.io artifact, a Flux release artifact, an official integration
plugin, or a connector runtime. They are independently versioned Exchange product artifacts. A
released executable may embed a newly released connector catalogue, but its archive contains no
connector/plugin executable or runtime download helper.

## One origin, two signing layers

The signed origin is fixed at
`https://github.com/codewandler/flux-exchange`. Clients derive the trust and channel requests rather
than accepting URLs from metadata:

```text
https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1/flux-exchange-release-trust.json
https://github.com/codewandler/flux-exchange/releases/download/exchange-stable-v1/flux-exchange-release-channel.json
```

Immutable manifest, signature and archive requests use the same repository's
`releases/download/vX.Y.Z/<signed-basename>` path. GitHub may use one tightly validated HTTPS asset
redirect; no authorization, cookie or proxy credential follows it, and a second redirect refuses.
The redirect URL is transport only. Signed identity and digests still decide what is accepted.

Flux pins a long-lived **offline root** public key and a closed trust policy, never a routine online
signer or Exchange package version. Root-signed `flux-exchange-release-trust.json` delegates separate
short-lived channel and release roles. Their threshold signatures authenticate
`flux-exchange-release-channel.json` and each immutable release manifest. The offline root private
key stays outside CI; delegated online signers cannot substitute for one another.

Routine delegated signer rotation publishes an overlapping old-and-new delegation before the old key
is retired. Clients refresh root-signed trust metadata, and the stable channel's rollback floor
continues across that rotation. This needs no Flux release. Replacing the pinned offline root is
exceptional: Flux must first ship support for the successor root and overlapping transition policy.

Mutable trust and channel releases are discovery heads, not the only audit record. Each externally
authorized trust update first retains its exact root-signed set under
`exchange-trust-v1-version-<version>`. Each channel update first retains the verified trust/channel
snapshot under `exchange-stable-v1-generation-<generation>`, and CI attests the new channel bytes.
Public history is never clobbered or deleted; it is bounded-downloaded and signature-verified before
the corresponding mutable head changes. Immutable version releases, metadata snapshots,
attestations, and successful workflow runs remain the rollback/audit evidence.

## Compatibility is a protocol set

The side-effect-free inspection command opens no store and binds no listener:

```sh
flux-exchange compatibility --json
```

The first public release emits JSON only. The older six-field implementation is unpublished and
cannot pass the content-derived publication preflight:

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

Flux verifies an unexpired signed channel, filters its entries by all eight protocol identities, and
selects the greatest compatible stable semantic version. It never guesses compatibility from a
package version and never falls back to `latest`, `PATH`, a checkout or Cargo output. A newer
incompatible release is skipped; no compatible entry is a named refusal.

That selection rule is what permits independent releases. A compatible Exchange update, including
one that embeds a newer released connector catalogue, reaches an existing Flux installation through
the signed channel without another Flux release. A Flux release is needed only for an unsupported
protocol/client change or a pinned offline-root/trust-policy change.

## Updates refuse rollback

Accepted trust versions and stable-channel generations only move forward. The trust floor advances
as soon as a higher root-valid trust document passes. The global channel floor advances as soon as a
higher delegated-valid channel passes, before compatibility selection or target download. It does
not reset for a trust version or signer rotation.

If the higher channel has no compatible release, or its later manifest, signature, download, archive
or executable checks fail, the previous verified install stays byte-identical. The accepted floors
do not roll back, and a new start does not launch the older channel entry. A lower number is rollback;
different bytes at the same number are equivocation. Both refuse.

Trust and channel metadata expires. Equality with an expiry is expired: new starts, imports,
reinstalls and launch after download require current metadata. A healthy process already owned by
Flux is not killed merely because update metadata ages out. Status reports the expiry beside the
healthy process, repeated start returns that same process, and stop works. Once stopped, fresh online
metadata or a fully verified unexpired offline set is required before it can start again.

## Online verification and offline import

Online verification fetches the root-signed trust document, delegated stable channel, selected
signed immutable manifest, and one platform archive from the fixed origin. Canonical JSON,
thresholds, time validity, rollback floors, supported protocols, byte and member bounds, archive and
executable digests, compatibility output and supervised identity all have to agree before the
install commits.

An **offline import** supplies one closed set: current root-signed trust metadata and signatures, the
signed channel snapshot and signatures, the same selected signed manifest and signatures, and the
one target archive. Import runs the identical freshness, rollback, threshold, authenticity,
newest-compatible selection, compatibility, bounds, digest and archive checks. It bypasses only the
GitHub transport; provenance is not an import input and offline is not an authenticity exception.

The release machinery is intentionally not a claim of current production availability. A release is
usable only after an authorized immutable tag, its closed two-target Linux public asset set and the
post-publication verifier all agree. Tagging starts both binary and crates.io publication, so the
release operator treats them as one irreversible authorization boundary rather than inferring
permission from a green staging run.

## The local owner boundary

The supervised single-user Linux contract has one native management endpoint. It is an owner-only
Unix socket below the native state root and authenticates the connecting account with
`SO_PEERCRED` before granting local operator authority inside that dispatcher.
It is not an HTTP identity provider: loopback TCP, hosted traffic, another account and `--dev` cannot
reuse the bootstrap. Readiness, liveness and lifecycle control remain separate value-free
capabilities.

Production discovers the default state root from the authenticated native Linux account with
`getpwuid_r(geteuid())`, not from inherited `HOME`, `XDG_STATE_HOME` or an equivalent variable.
Every path component is checked before use. A symlink, foreign ownership, an
untrusted-writable ancestor or widened owner-only metadata refuses; Exchange does not repair the
path or fall back to memory.

The verified `flux-exchange` helper owns native secret input and the credential-handoff boundary.
Flux sends a bounded non-secret
request, while the helper opens `/dev/tty` directly only when input is needed
and sends that value to Exchange over the authenticated owner endpoint. Flux receives only a
value-free receipt or refusal. Service Account mint uses a separate one-way writer capability, so
the one-time credential does not enter ordinary JSON, argv, environment, standard output,
diagnostics or supervisor state. The receiver, not Exchange, decides when its own credential store
has committed that handoff.

Supervision uses fixed inherited FDs 3 and 4, helper request/result use FDs 6 and 7, and Service
Account mint transfers its one writer as FD 5 with `SCM_RIGHTS`. The signed readiness identity uses
the Linux proc start marker; no alternate native capability profile is part of this release set.

The v2/eight-protocol schema shown above is the first public contract: four unchanged HTTP
identities plus the v2 connection-plan and supervisor identities, local management and the
Service Account handoff. The older six-protocol shape is unpublished implementation evidence and
cannot pass the content-derived publication preflight.

The complete wire-level contract, bounds and operator publication procedure remain in the
[repository's release design](https://github.com/codewandler/flux-exchange/blob/main/docs/designs/local-release-v1.md).
