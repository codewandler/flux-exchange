---
id: X-126
title: "Publish verified local Exchange binaries for every Flux platform"
status: ready
priority: 0
epic: remote-deployment
areas: [ci, release, supply-chain, exchange-server]
depends_on: [X-127, X-128]
design: docs/designs/local-release-v1.md
note: "Milestone 1 — Flux can manage a separately released, attested Exchange executable on a clean machine without bundling plugins or trusting PATH"
---

# Publish verified local Exchange binaries for every Flux platform

## Goal

Give `flux exchange local start` a separately released Exchange executable it can discover from a
signed stable update channel, identify, verify and run on a clean machine. Exchange owns the channel
and binary release; Flux owns compatible-version selection, download and lifecycle. No official
connector executable or plugin becomes part of either core distribution, and shipping a compatible
Exchange or connector update does not require shipping Flux again.

## Why this is Milestone 1 work

The Exchange server is currently `publish = false`, and the release path publishes only the reusable
host crate and the hosted image. A Flux installation therefore has no released local server to start.
Searching `PATH`, a sibling checkout or Cargo output would replace a release contract with mutable
machine state, while copying the server or connectors into Flux would recreate the artifact coupling
the Exchange migration is meant to remove.

Decision 0004 keeps Exchange a separate process and a separately versioned product artifact. Flux
therefore pins the long-lived offline Exchange release root, fixed channel origin, trust policy and
protocol versions it can speak — never one Exchange package version or routine online signing key.
This story makes the independently updated channel and product releases verifiable before Flux
implements its manager in C-510.

## Acceptance

- [ ] After X-127 and X-128 are complete, a tag release builds one Exchange server archive for each
      platform Flux supports:
      `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`,
      `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. The target list is a checked closed
      set matching Flux's `dist-workspace.toml`; changing either list requires one coordinated
      contract update rather than silently omitting a platform. A target enters the manifest only
      after X-127's native owner-only restart proof and X-128's native supervised-readiness proof
      pass for it; cross-compilation alone is insufficient.
- [ ] Unix assets are deterministic archives and Windows is a deterministic zip, each named with
      the exact Exchange version and target triple. Each contains the server executable and only
      explicitly allowed runtime documentation/licence material: no connector/plugin executable,
      dynamic download helper, credential, deployment config or sibling checkout content.
- [ ] The release carries one UTF-8 canonical-JSON manifest with schema identity
      `exchange.release-manifest.v1` and the exact fields, names, ordering and bounds in
      `docs/designs/local-release-v1.md`. Its top level names the immutable repository origin, exact
      `refs/tags/vX.Y.Z` tag, Exchange version, 40-hex source commit, build identity, six exact
      compatibility protocol fields, delegated signing key ids and complete closed asset set.
      Duplicate keys, non-canonical JSON, unknown schema, a tag/version/SHA mismatch or a
      mutable/latest origin refuses verification.
- [ ] The same release workflow publishes a UTF-8 canonical-JSON stable-channel index with schema
      identity `exchange.release-channel.v1`, exact filename
      `flux-exchange-release-channel.json`, and the provider-owned v1 shape in the linked design.
      Flux pins the fixed logical request origin plus the long-lived offline Exchange signing root,
      trust policy and protocol/schema versions that Flux implements; it does **not** pin an
      Exchange version or routine channel/release signer. The signed index names a monotonically
      increasing generation, issued-at and expiry instants, threshold-satisfying delegated signer
      ids and 1..=128 releases through immutable identity and manifest SHA-256 values. The index is
      at most 256 KiB, valid for at most seven days and allows issuance at most five minutes in the
      future. Every other field, order, type and bound comes from the one provider contract, not a
      second Flux-owned v1.
- [ ] Channel selection is deterministic and compatibility-led. From a valid unexpired index, the
      verifier chooses the highest stable semantic version whose signed manifest declares every
      `exchange_api`, `effective_catalogue_response`, `invoke_request`, `invoke_response`,
      `connection_plan` and `supervisor` version Flux requires. Package version never substitutes
      for protocol compatibility; a newer incompatible release is skipped, while no compatible
      entry is a named refusal rather than permission to use `latest`, `PATH` or a sibling checkout.
- [ ] Channel rollback is fail-closed. Flux persists the highest accepted generation for this
      channel and refuses an older generation, a changed payload at the same generation, an
      issued-at value unreasonably in the future, an expired index and a selected manifest whose
      digest or identity differs from the signed index. The exact installed Exchange version,
      source SHA, trust-metadata version, manifest digest, executable digest and channel generation
      are retained as lifecycle/audit state; none becomes a compatibility pin that blocks a later
      valid update.
- [ ] Every asset entry names one target, exact archive filename, archive byte size and SHA-256,
      exact executable member path, executable byte size and independent SHA-256, archive format and
      permitted documentation/licence members. The executable digest is over the extracted bytes,
      not inferred from the archive digest. Archive and executable names are relative single-root
      paths; absolute paths, `..`, alternate separators, links, devices and duplicate normalized
      member names refuse.
- [ ] The version-one bounds are part of the signed schema and the verifier: manifest at most
      256 KiB; exactly five asset entries; each archive at most 256 MiB; at most 16 members and
      512 MiB total expanded bytes per archive; each member at most 256 MiB; each member path at most
      240 UTF-8 bytes. Staged and live verification apply the bounds before allocation/extraction and
      refuse integer overflow, size disagreement, trailing data or decompression past a declared
      bound.
- [ ] Minisign authentication has a dedicated Exchange-release trust domain with a long-lived
      offline root. Flux pins that independently reviewed root public key and a closed trust policy,
      never an online CI key. Canonical root-signed `exchange.release-trust.v1` metadata delegates
      separate channel and release roles through the exact provider-owned shape: filename
      `flux-exchange-release-trust.json`, `version` (not channel `generation`), root signer ids, and
      role objects containing `threshold` plus 1..=4 ordered keys and validity intervals. It is at
      most 64 KiB, valid for at most 366 days and monotonically versioned; an unknown role, untrusted
      root, expired delegation, threshold failure, rollback or changed payload at one version
      refuses before the channel is read.
- [ ] The manifest and channel index carry minisign signatures satisfying their currently valid
      delegated roles. No crates.io, Flux-release, connector, GitHub token or provenance identity
      substitutes for a role signature. The production workflow preflight requires only the exact
      delegated CI signer secrets it needs and proves each derived public key/id, role and validity
      against current root-signed trust metadata before building or exposing a release file. The
      offline root secret never enters CI. This story creates or configures no secret.
- [ ] Routine signer rotation is explicit and overlapping under new root-signed trust metadata: the
      successor delegate is published before use, old and new delegates overlap long enough for
      cached clients to refresh trust metadata, and the old delegate is removed only after the
      overlap. A client refreshes and verifies trust metadata before rejecting an otherwise unknown
      online signer. This rotation needs no Flux release. Rotating the pinned offline root itself is
      exceptional: a Flux release trusts the successor root before it is required, transition trust
      metadata satisfies both root policies, and only a later Flux release may remove the retired
      root. An unannounced key id, role confusion, substituted signature or key-id/signature
      disagreement refuses. Repository/workflow provenance remains bound to the immutable tag and
      source SHA; it complements and never replaces minisign.
- [ ] Neither manifest nor channel entry contains an arbitrary download URL. Verifiers construct
      only the three exact `github.com/codewandler/flux-exchange/releases/download/...` request
      shapes in the provider contract. GitHub's real asset transport may return exactly one HTTP 302
      to `release-assets.githubusercontent.com`; the client admits it only after the exact HTTPS
      host, port, release-asset path grammar, closed query-name set and URL/query/value bounds pass,
      sends no credential/cookie/proxy authorization to either request, and refuses a second
      redirect. The transient CDN URL is never identity or state; signed minisign evidence, digest
      and bounds still decide acceptance. A different repository/host, mutable latest/API endpoint,
      unvalidated redirect or proxy-selected replacement refuses.
- [ ] Offline import supplies current root-signed trust metadata, a signed channel snapshot and the
      same selected signed manifest, one-platform archive and provenance to the identical bounded
      trust, channel-selection and release verifier. Being offline bypasses only the closed GitHub
      transport, not freshness, rollback, thresholds, authenticity, newest-compatible selection or
      compatibility.
- [ ] The released executable answers a side-effect-free compatibility command with JSON only (for
      example `flux-exchange compatibility --json`) without binding a listener, opening a store or
      requiring identity configuration. It reports the exact binary/release identity and supported
      versions under exactly six protocol keys: `exchange_api`, `effective_catalogue_response`,
      `invoke_request`, `invoke_response`, `connection_plan` and `supervisor`; the exact
      `exchange.compatibility.v1` shape comes from the provider design. The same values and field
      names occur in the channel, signed manifest and X-128 readiness record.
- [ ] Compatibility identities are versioned protocols, not the workspace package version used as
      a guess. A release check executes every platform artifact it can run directly or under the
      declared cross-platform harness and proves its JSON agrees with the manifest; a missing,
      malformed or contradictory compatibility field refuses publication.
- [ ] Publication is CI-only from an immutable `vX.Y.Z` tag whose version matches the workspace and
      whose exact commit passes the full repository gate. All third-party actions are SHA-pinned,
      signing/provenance happens before exposure, and no local/manual command can be the documented
      normal publish path. Channel publication is serialized: it compares the currently signed
      generation before replacing the index, allocates exactly its successor, and makes a retry of
      the same release byte-idempotent rather than producing a second generation or equivocation.
- [ ] **Failing-first staged-release checks:** corrupting one archive after digest generation,
      deleting one supported-platform asset, adding one undeclared asset, renaming an executable, or
      inserting a plugin/connector executable makes the release verifier fail before publication.
      Oversizing the manifest/archive/member set, changing the executable while retaining the archive
      entry, substituting a key id, adding an unadmitted redirect and changing the logical origin
      also fail. One exact GitHub 302 satisfying the provider transport fixture succeeds; changing
      its status, scheme, host, port, path grammar, query-name set or bounds, forwarding a credential,
      or returning a second redirect fails.
      Trust/channel fixtures fail first for expired or rolled-back delegation, role confusion,
      expired index, rollback generation, equivocation at one generation, manifest-digest
      substitution, foreign origin, unsupported protocol set and a higher but incompatible release;
      the positive selection fixture chooses the newest compatible release. The self-test
      demonstrates each failure against fixtures rather than asserting only the happy path. The
      staged verifier is the same program and policy used for live and offline release verification.
- [ ] X-126 materializes the provider conformance set named by the design under
      `tests/fixtures/exchange-release-v1/`: canonical positive trust, channel, manifest,
      compatibility and readiness bytes with test-only threshold signatures and bounded
      archive/provenance, plus the machine-readable adversarial mutation inventory. A checked
      fixture-set manifest records every relative filename and SHA-256. Exchange's staged verifier
      and Flux C-510's vendored byte-identical copy run the same expected outcome for every case;
      either repository changing a v1 byte, bound or verdict alone fails its contract gate.
- [ ] A post-publication verifier downloads the release by immutable tag, requires the exact closed
      asset set, recomputes every digest, verifies the manifest signature and provenance, checks
      archive contents and runs the host-platform compatibility command. It then reads the signed
      trust metadata and stable channel from their pinned origin and proves that a client with the
      declared protocol set selects this release without an exact-version input. Missing provenance,
      a substituted signature/key, an extra public asset or any staged/live shape difference leaves
      the release visibly failed.
- [ ] X-126 is not marked `done` when workflow code, fixtures or uploaded draft assets are green. Its
      first real immutable `vX.Y.Z` production tag must complete the post-publication verifier over
      the public five-target release with the exact production trust/delegation policy, the one
      admitted GitHub asset redirect and the staged/live byte-identical manifest. The tag, release
      URL, verifier run and source SHA are recorded in Progress before the story closes; a failed
      live verification leaves the release unusable by Flux and this story open.
- [ ] Public release and operator documentation explain the trust root, supported platforms,
      fixed channel origin, update/rollback behavior, compatibility JSON and offline
      verification/import path. They state explicitly that these binaries are not crates.io
      artifacts, Flux release artifacts, official integration plugins or connector runtimes. They
      also state the release independence rule: a compatible Exchange release — including one that
      embeds a newly released connector catalogue — reaches existing Flux installations through the
      signed channel without a Flux release, and routine delegated signer rotation does too; only an
      offline-root/trust-policy or unsupported protocol/client change needs a coordinated Flux
      update.

## Progress

- 2026-08-04: Filed from cross-repository Decision 0004 after the Milestone 1 seam audit found that
  `flux exchange local start` had no executable available from a clean Flux installation.
- 2026-08-04: Decision 0004's supervision/trust amendment split true five-platform persistence into
  X-127 and the inherited one-shot readiness protocol into X-128. Both are prerequisites: X-126
  publishes only targets whose local storage and supervised process identity have already been
  proved natively.
- 2026-08-04: Corrected the release coupling: Flux pins the long-lived offline Exchange root, trust
  policy, channel origin and supported protocol set, not an Exchange version or routine signer. The
  root-delegated, signed expiry-bounded monotonic channel can therefore deliver compatible Exchange
  and connector updates — and rotate online signers — independently of a Flux release.
- 2026-08-04: The cross-repository completion audit found that Flux C-510 had independently invented
  competing v1 names, fields, a 64-release bound and impossible no-redirect GitHub URLs. The linked
  design is now the single Exchange-owned trust/channel/manifest/compatibility/readiness contract;
  it keeps X-126's 128-release bound and describes GitHub's actual one-hop asset transport.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0004-flux-manages-a-verified-local-exchange.md` at `013a2ab`.
- Flux C-510 consumes this channel and owns rollback state, compatible selection, verified
  cache/install/start/status/stop and audit of the installed exact identity. X-126 does not add a
  downloader or lifecycle manager to Exchange.
- `docs/designs/local-release-v1.md` is the provider source of truth. X-126 materializes its positive
  and adversarial fixtures; Flux may vendor those exact bytes with the Exchange commit and fixture
  digest, but may not restate a different schema, bound or transport as another v1.
- X-127 owns native five-target persistence and owner-only Windows DACLs. X-128 owns Exchange's
  supervised launch/readiness record. Neither is satisfied by adding an archive to this workflow.
- X-125 owns the connection-plan protocol that the compatibility output identifies. X-113 owns the
  effective catalogue and invoke contracts. Version reporting binds those contracts; it does not
  redefine them in release automation.
- This is distinct from X-119. X-126 releases the Exchange host process; X-119 later installs
  connector-declared rich-runtime artifacts inside Exchange. Neither permits a plugin executable in
  the Flux core release.
