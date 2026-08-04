# Verified local releases

The local Exchange server has its own release contract so Flux can discover, verify and supervise a
compatible executable without bundling that executable or trusting mutable machine state. The
contract alone is not a claim that a production channel is live: a usable release is evidenced only
after its immutable tag and complete public asset set pass post-publication verification.

## Supported platforms

The signed manifest's platform set is closed:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`

A target enters that set only after native owner-only persistence and supervised-readiness proofs
pass. Cross-compilation by itself is not evidence that Exchange runs safely on a target.

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

## Compatibility is a protocol set

The side-effect-free inspection command opens no store and binds no listener:

```sh
flux-exchange compatibility --json
```

It emits JSON only:

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
    "exchange_api": "exchange.api.v1",
    "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
    "invoke_request": "exchange.invoke-request.v1",
    "invoke_response": "exchange.invoke-response.v1",
    "connection_plan": "exchange.connection-plan.v1",
    "supervisor": "exchange.supervisor-ready.v1"
  }
}
```

Flux verifies an unexpired signed channel, filters its entries by all six protocol identities, and
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

The complete wire-level contract, bounds and operator publication procedure remain in the
[repository's release design](https://github.com/codewandler/flux-exchange/blob/main/docs/designs/local-release-v1.md).
