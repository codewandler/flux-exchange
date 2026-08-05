---
id: X-137
title: "Constrain Exchange runtime and release to Linux"
status: in-progress
epic: connections
areas: [exchange-server, exchange-release, protocol, tests, workflows]
depends_on: [X-136]
design: docs/designs/local-release-v1.md
note: "X-134 child — Decision 0012 replaces the stopped Windows proof with the exact two-target Linux product boundary"
---

# Constrain Exchange runtime and release to Linux

## Goal

Make Exchange an explicitly Linux-only runtime and separately released product. Remove macOS and
Windows production/runtime/publication paths, retain the Linux owner-bound protocol, and make every
release inventory and target selector close over exactly the two supported Linux GNU targets.

## Acceptance

- [ ] The Exchange release target authority contains exactly
      `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm` and
      `x86_64-unknown-linux-gnu` on `ubuntu-24.04`. Both assets are deterministic `tar.zst`
      archives containing the `flux-exchange` executable. Manifest, compatibility, channel,
      readiness and native-evidence projections contain no Darwin or MSVC target, runner, archive,
      capability or positive fixture. Unknown/non-Linux target or platform input refuses before
      staging, signing, download or publication.
- [ ] The production server/helper native boundary is Linux-only: `getpwuid_r(geteuid())` derives
      the root; the owner endpoint is an owner-only Unix socket authenticated with Linux
      `SO_PEERCRED`; readiness/liveness and ceremony/result capabilities use the fixed inherited
      descriptors; the Service Account writer crosses once with `SCM_RIGHTS`; and process identity
      uses the Linux proc start marker. These remain the existing v2 release and FXLM/FXSA schema
      identities; removing unpublished platform members does not invent v3.
- [ ] macOS support roots and `getpeereid`, plus every Windows profile/ACL, named-pipe,
      impersonation, HANDLE-list, private `CONIN$` and FXHA production path are removed from or made
      structurally unreachable in the production binary. Non-Linux server/helper builds or package
      requests fail explicitly; no environment, feature or hidden target restores support.
- [ ] X-137 contracts the supported-target policy, `release-targets.tsv`, distribution target set,
      `SUPPORTED_TARGETS`, `Platform::from_target`, production server/helper cfg and non-Linux
      build/package refusal to the exact two-target set. Release staging/publication remains stopped
      if its interim fixtures or reports do not match that set. X-139, after X-138, exclusively owns
      the final `native-evidence-v1.json`, generator/frozen fixture projection and digest,
      publication/readiness derivation, workflow native matrix/reports and candidate identity.
- [ ] Any mechanical fixture regeneration needed to keep the repository gate green is explicitly an
      interim platform contraction. It cannot claim X-139's final semantic obligation inventory,
      freeze new family/binding counts or close publication before X-138's recovery tests exist.
      Ordinary content-derived publication readiness remains unconditionally fail-closed after
      X-137 until the authority contains X-138's exact recovery/lease obligations and X-139's final
      candidate/fixture identity. No flag, marker, credential or operator action bypasses that stop;
      repository self-tests may pass without making a tag publishable.
- [ ] The X-134, X-138, X-139 and X-126 contracts and the linked design are committed on canonical
      `main` before dispatch and remain reconciled with Decision 0012 through closure. Historical
      X-127/X-128/C-515 portability evidence remains attributable; only its Linux subset is consumed
      as Exchange publication evidence.
- [ ] Changes are limited to Exchange-owned runtime/release paths. The published portable
      `codewandler-connector-secrets` 0.20.0 release remains identified by checksum
      `edf98bece86f6364aba3e7dd48c3b7e161146942e9e8450d5dc286143b627717` and source commit
      `c764f5c3b8e745cc65e90a298b04851647b76778`; no upstream portable source, evidence or support
      claim is removed or reinterpreted. Exchange projects only the two supported Linux rows.
- [ ] Targeted Linux tests cover owner root/peer refusal, supervised readiness/liveness, helper
      descriptor closure, `SCM_RIGHTS` FXSA handoff, process identity and release projection on both
      supported runners. The repository gate and release self-tests are green without running or
      claiming macOS/Windows Exchange evidence.

## Progress

- 2026-08-05: The previous Windows implementation branch
  `story/X-137-fxha-native` was stopped after three native runs crossed the same opaque diagnostic
  boundary. Its clean pushed head `a92161ed90f620628f1c77e627763557b91e9fa1` is preserved as
  historical evidence and must not be merged into this story.
- 2026-08-05: Flux-roadmap Decision 0012 at `dc907fa` superseded the non-Linux platform clauses of
  Decisions 0004, 0007 and 0011. This story restarts from canonical Exchange `main` rather than the
  preserved Windows branch.
- 2026-08-05: Failing-first contracts rejected the previous five-target `Platform` and the absence
  of a non-Linux server build refusal. The implementation now closes release/package selection over
  the two Linux GNU targets and makes the Linux FD/socket/helper graph the only production entry.
  The mechanically contracted native authority and fixture projection are explicitly interim;
  ordinary publication readiness refuses until X-138 and X-139 are both `done`.

## Notes

- Child of X-134, sequenced after X-136. X-138 and X-139 remain blocked until this exact product
  boundary lands.
- Authenticated catalogue/invocation HTTP from any Flux platform to an independently provisioned
  Linux Exchange remains unchanged. This story adds no remote lifecycle, FXLM, FXSA,
  connect/grant/mint or remote owner-management protocol; hosted WebSocket support does not become
  Flux remote onboarding.
