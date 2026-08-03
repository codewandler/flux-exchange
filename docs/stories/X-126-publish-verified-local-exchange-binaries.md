---
id: X-126
title: "Publish verified local Exchange binaries for every Flux platform"
status: ready
priority: 0
epic: remote-deployment
areas: [ci, release, supply-chain, exchange-server]
depends_on: [X-127, X-128]
note: "Milestone 1 — Flux can manage a separately released, attested Exchange executable on a clean machine without bundling plugins or trusting PATH"
---

# Publish verified local Exchange binaries for every Flux platform

## Goal

Give `flux exchange local start` a separately released Exchange executable it can identify, verify
and run on a clean machine. Exchange owns the binary release; Flux owns download and lifecycle. No
official connector executable or plugin becomes part of either core distribution.

## Why this is Milestone 1 work

The Exchange server is currently `publish = false`, and the release path publishes only the reusable
host crate and the hosted image. A Flux installation therefore has no released local server to start.
Searching `PATH`, a sibling checkout or Cargo output would replace a release contract with mutable
machine state, while copying the server or connectors into Flux would recreate the artifact coupling
the Exchange migration is meant to remove.

Decision 0004 keeps Exchange a separate process and a separately versioned product artifact. This
story makes that product release verifiable before Flux implements its manager in C-510.

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
      `exchange.release-manifest.v1`. Its top level names the immutable repository origin, exact
      `refs/tags/vX.Y.Z` tag, Exchange version, 40-hex source commit, build identity, compatibility
      protocols, signing key id and complete closed asset set. Duplicate keys, non-canonical JSON,
      unknown schema, a tag/version/SHA mismatch or a mutable/latest origin refuses verification.
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
- [ ] The manifest is signed with minisign in a dedicated Exchange-release trust domain. The
      committed public key and stable key id are reviewed independently and pinned by Flux; no
      crates.io, Flux-release, connector, GitHub token or provenance identity substitutes for this
      signature. The production workflow performs an initial preflight that requires the exact
      Exchange minisign secret key and proves its derived public key/key id match the committed
      production trust root before building or exposing any release file. This story creates or
      configures no secret.
- [ ] Key rotation is explicit and overlapping: a Flux release trusts the new key id before
      Exchange signs with it; the transition release carries valid signatures from the active and
      successor Exchange keys; only a later Flux release may remove the retired public key. An
      unannounced key id, substituted signature, key-id/signature disagreement or single-signed
      transition release refuses. Every published asset also has repository/workflow provenance
      bound to the immutable tag and source SHA; provenance complements and never replaces minisign.
- [ ] The manifest contains no download URL. Verifiers are configured with the one immutable
      Exchange release origin and construct exact tag-and-filename inputs from signed fields. They
      reject a different repository/host, mutable `latest` endpoint and every HTTP redirect rather
      than letting transport choose a new origin or asset. Offline import is the same closed set of
      bytes and passes the identical verifier without a network exception.
- [ ] The released executable answers a side-effect-free compatibility command with JSON only (for
      example `flux-exchange compatibility --json`) without binding a listener, opening a store or
      requiring identity configuration. It reports the exact binary/release identity and supported
      versions for the Exchange API, effective-catalogue response, invoke request/response and
      `exchange.connection-plan`; the same values are in the signed release manifest.
- [ ] Compatibility identities are versioned protocols, not the workspace package version used as
      a guess. A release check executes every platform artifact it can run directly or under the
      declared cross-platform harness and proves its JSON agrees with the manifest; a missing,
      malformed or contradictory compatibility field refuses publication.
- [ ] Publication is CI-only from an immutable `vX.Y.Z` tag whose version matches the workspace and
      whose exact commit passes the full repository gate. All third-party actions are SHA-pinned,
      signing/provenance happens before exposure, retries are idempotent, and no local/manual command
      can be the documented normal publish path.
- [ ] **Failing-first staged-release checks:** corrupting one archive after digest generation,
      deleting one supported-platform asset, adding one undeclared asset, renaming an executable, or
      inserting a plugin/connector executable makes the release verifier fail before publication.
      Oversizing the manifest/archive/member set, changing the executable while retaining the archive
      entry, substituting a key id, adding a redirect and changing the immutable origin also fail.
      Its self-test demonstrates each failure against fixtures rather than asserting only the happy
      path. The staged verifier is the same program and policy used for live and offline verification.
- [ ] A post-publication verifier downloads the release by immutable tag, requires the exact closed
      asset set, recomputes every digest, verifies the manifest signature and provenance, checks
      archive contents and runs the host-platform compatibility command. Missing provenance, a
      substituted signature/key, an extra public asset or any staged/live shape difference leaves
      the release visibly failed.
- [ ] X-126 is not marked `done` when workflow code, fixtures or uploaded draft assets are green. Its
      first real immutable `vX.Y.Z` production tag must complete the post-publication verifier over
      the public five-target release with the exact production minisign key, no redirect, and the
      staged/live byte-identical manifest. The tag, release URL, verifier run and source SHA are
      recorded in Progress before the story closes; a failed live verification leaves the release
      unusable by Flux and this story open.
- [ ] Public release and operator documentation explain the trust root, supported platforms,
      compatibility JSON and offline verification/import path. They state explicitly that these
      binaries are not crates.io artifacts, Flux release artifacts, official integration plugins or
      connector runtimes.

## Progress

- 2026-08-04: Filed from cross-repository Decision 0004 after the Milestone 1 seam audit found that
  `flux exchange local start` had no executable available from a clean Flux installation.
- 2026-08-04: Decision 0004's supervision/trust amendment split true five-platform persistence into
  X-127 and the inherited one-shot readiness protocol into X-128. Both are prerequisites: X-126
  publishes only targets whose local storage and supervised process identity have already been
  proved natively.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0004-flux-manages-a-verified-local-exchange.md`.
- Flux C-510 consumes this release and owns verified cache/install/start/status/stop. X-126 does not
  add a downloader or lifecycle manager to Exchange.
- X-127 owns native five-target persistence and owner-only Windows DACLs. X-128 owns Exchange's
  supervised launch/readiness record. Neither is satisfied by adding an archive to this workflow.
- X-125 owns the connection-plan protocol that the compatibility output identifies. X-113 owns the
  effective catalogue and invoke contracts. Version reporting binds those contracts; it does not
  redefine them in release automation.
- This is distinct from X-119. X-126 releases the Exchange host process; X-119 later installs
  connector-declared rich-runtime artifacts inside Exchange. Neither permits a plugin executable in
  the Flux core release.
