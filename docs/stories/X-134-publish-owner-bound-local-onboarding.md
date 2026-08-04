---
id: X-134
title: "Publish owner-bound local onboarding without secret JSON"
status: ready
priority: 0
epic: connections
areas: [exchange-server, identity, connections, grants, protocol, tests, windows]
depends_on: [X-125, X-127, X-128, X-129]
design: docs/designs/local-release-v1.md
note: "Milestone 1 — OS-owner management, direct vendor-secret insertion, one-shot Service Account handoff and revisioned grants must ship before the first public local release"
---

# Publish owner-bound local onboarding without secret JSON

## Goal

Give the verified local Exchange one authenticated, owner-bound onboarding surface that Flux can
use without receiving vendor credentials or a one-time Service Account token. The local OS owner
can create a complete labelled connection, grant it and install Flux's runtime credential while
ordinary HTTP JSON, argv, environment, lifecycle state and diagnostics remain secret-free.

## Why this precedes the first public local release

X-125's plan describes every field, but its submission currently mixes secret and non-secret
values in JSON. Service Account mint currently returns its token in JSON. X-127 gives the supervised
binary conventional persistent stores, but `--supervised` neither enables `--dev` nor establishes a
human or operator, and X-128 deliberately allows only readiness and liveness capabilities. Flux
C-509 cannot repair any of those provider boundaries without becoming the credential proxy
Decision 0001 forbids.

The first public X-126 channel must therefore describe the final local-management and credential-
handoff bytes, not publish an exact six-protocol schema and silently change it afterward.

## Acceptance

- [ ] A supervised single-user Exchange derives its conventional state root from native OS account
      APIs rather than `HOME`, `XDG_*`, `LOCALAPPDATA` or another inherited environment variable.
      It creates one owner-only local-management endpoint below that root: a Unix-domain socket with
      owner-only ancestry on Unix and a current-user-owned, protected-DACL named pipe on Windows.
      Foreign ownership, group/world access, symlink/reparse traversal, unavailable account data or
      ambiguous peer identity refuses without changing metadata or recommending changes to a shared
      ancestor.
- [ ] The endpoint authenticates the connecting OS account as one local human/operator only for the
      single-user supervised deployment. Its authority cannot reach the TCP router, hosted mode,
      another account, a Service Account runtime route or `--dev` roster authentication. Plan reads
      remain human management; connection, custom-origin approval, grant and Service Account
      mutations remain operator management. A native adversarial test proves a loopback TCP caller
      cannot reproduce the OS-owner bootstrap.
- [ ] Exchange publishes one closed provider-owned local-management protocol and one closed one-shot
      credential-handoff protocol with exact schema identities, bounds, error vocabulary and
      positive/adversarial fixtures. X-126's channel, manifest, compatibility and readiness contract
      advertises them through a reviewed release-schema revision; it does not add an unknown field
      to the existing exact protocol object, reuse a v1 identity for changed bytes or derive an id
      from a package version. Flux can consume the fixture inventory byte-for-byte.
- [ ] The connection-plan JSON submission accepts only plan-published non-secret targets. A secret
      identity, alias, value or unknown target in JSON refuses before mutation and the refusal never
      reflects the value. The ordinary hosted console retains a direct Exchange-owned secret path
      through a bounded non-JSON body; URLs, browser history/navigation, responses and plan documents
      remain value-free.
- [ ] The verified `flux-exchange` executable provides the local TTY secret-input mode consumed by
      Flux C-509. Connector, label, exact plan/target revision and non-secret transaction metadata
      may be supplied; the vendor secret is read by Exchange itself and sent over only the owner-
      bound management endpoint immediately before `SecretStore::put`. The parent Flux process
      cannot inherit, proxy, read, log or render the input, and no caller can redirect the helper to
      a different endpoint, tenant, credential address or connection instance.
- [ ] One connect attempt atomically binds the label, canonical non-secret settings, required
      credentials and approved authority revision. Its bounded value-free receipt can be replayed:
      repeating the committed proposal succeeds without a second instance or credential write;
      failure before commit leaves no partial state; failure after commit resumes from the receipt.
      A different proposal for the same connector/label refuses naming only those two identities.
      The first-run path exposes no edit-by-retry behavior or secret-derived definition fingerprint.
- [ ] The local operator can mint a Service Account directly to an inherited one-shot writer without
      serializing the token into an HTTP/local-management response. The Exchange helper receives
      only the pipe write end; on Unix it is one explicit close-on-exec descriptor and on Windows it
      is one explicit inheritable HANDLE in a closed handle list. X-128 readiness FD 3, liveness FD 4
      and their Windows equivalents remain value-free and unchanged.
- [ ] The handoff writes exactly one versioned length-bounded binary frame containing the opaque
      token and then closes. Truncation, surplus bytes, a second frame, wrong direction/version,
      inherited extra capabilities, sink failure and early close all refuse with only a value-free
      receipt/diagnostic. The Service Account verifier store continues to persist only the verifier;
      the raw token and `Bearer ` spelling are absent from it and every Exchange persistence file.
- [ ] Grant preview and whole-set apply gain an exact revision/ETag and proposal digest. Apply is a
      compare-and-swap against the previewed revision, preserves unrelated connector grants and is
      idempotent for the same committed digest; a stale or concurrently changed set refuses before
      write. Grants remain tenant-and-connector scoped metadata selectors with no connection-label
      or operation-id authority axis.
- [ ] Failing-first integration tests scan raw/escaped/percent/base64 vendor and Service Account
      sentinels across JSON bodies, URLs, argv, environment, local-management diagnostics, stdout,
      stderr, tracing, audit, readiness, liveness, lifecycle/control state and every persisted file.
      The only allowlisted locations are the Exchange vendor credential store and the receiving
      host's test credential sink; captured Flux-facing traffic may contain the Service Account
      token only in the dedicated handoff frame and later sensitive Authorization transport.
- [ ] Native CI executes the owner endpoint, TTY input, pipe/HANDLE inheritance, descriptor closure,
      restart, grant CAS and unsafe-root refusals on every platform advertised by X-126:
      `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`,
      `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. Cross-compilation, fixture parsing or
      one architecture standing in for another is not acceptance evidence.
- [ ] `docs/designs/local-release-v1.md`, operator/user documentation, the public website and the
      X-126 fixture/check pipeline describe the final boundary and its honest hard stops. The full
      repository gate and public-site build pass. X-126 remains active until this story is merged,
      the final provider fixtures are reverified from the candidate commit and the separately
      authorized public five-target release verifier succeeds.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0007-local-onboarding-uses-owner-bound-capabilities.md`.
- This story owns provider bytes and fixtures. Flux C-509 owns the receiving credential writer,
  owner-only Flux store, opaque resolver, management/runtime client split, CLI projection, concrete
  write approval and retry suppression.
- The local-management endpoint is not C-510's lifecycle control channel and never carries start,
  status, stop, readiness or liveness frames. C-510 remains secret-free.
- Existing hosted authentication stays authoritative for hosted Exchange. OS-owner bootstrap is
  deliberately unavailable outside the supervised single-user composition.
