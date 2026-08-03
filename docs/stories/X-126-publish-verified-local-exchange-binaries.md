---
id: X-126
title: "Publish verified local Exchange binaries for every Flux platform"
status: ready
priority: 0
epic: remote-deployment
areas: [ci, release, supply-chain, exchange-server]
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

- [ ] A tag release builds one Exchange server archive for each platform Flux supports:
      `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`,
      `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. The target list is a checked closed
      set matching Flux's `dist-workspace.toml`; changing either list requires one coordinated
      contract update rather than silently omitting a platform.
- [ ] Unix assets are deterministic archives and Windows is a deterministic zip, each named with
      the exact Exchange version and target triple. Each contains the server executable and only
      explicitly allowed runtime documentation/licence material: no connector/plugin executable,
      dynamic download helper, credential, deployment config or sibling checkout content.
- [ ] The release carries one canonical machine-readable manifest naming its schema, release tag,
      Exchange version, source commit, build identity and complete asset set. Every asset entry names
      the target, archive, executable identity, byte size and SHA-256 digest. The manifest itself is
      covered by an offline-verifiable signature whose public trust material can be pinned by Flux,
      and every published asset has repository/workflow provenance bound to the tag and source SHA.
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
      Its self-test demonstrates each failure against fixtures rather than asserting only the happy
      path.
- [ ] A post-publication verifier downloads the release by immutable tag, requires the exact closed
      asset set, recomputes every digest, verifies the manifest signature and provenance, checks
      archive contents and runs the host-platform compatibility command. Missing provenance, a
      substituted signature/key, an extra public asset or any staged/live shape difference leaves
      the release visibly failed.
- [ ] Public release and operator documentation explain the trust root, supported platforms,
      compatibility JSON and offline verification/import path. They state explicitly that these
      binaries are not crates.io artifacts, Flux release artifacts, official integration plugins or
      connector runtimes.

## Progress

- 2026-08-04: Filed from cross-repository Decision 0004 after the Milestone 1 seam audit found that
  `flux exchange local start` had no executable available from a clean Flux installation.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0004-flux-manages-a-verified-local-exchange.md`.
- Flux C-510 consumes this release and owns verified cache/install/start/status/stop. X-126 does not
  add a downloader or lifecycle manager to Exchange.
- X-125 owns the connection-plan protocol that the compatibility output identifies. X-113 owns the
  effective catalogue and invoke contracts. Version reporting binds those contracts; it does not
  redefine them in release automation.
- This is distinct from X-119. X-126 releases the Exchange host process; X-119 later installs
  connector-declared rich-runtime artifacts inside Exchange. Neither permits a plugin executable in
  the Flux core release.
