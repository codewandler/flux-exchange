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

- [ ] Production derives native roots without `HOME`, `XDG_*`, `USERPROFILE`, `LOCALAPPDATA` or an
      equivalent inherited variable. Linux uses
      `getpwuid_r(geteuid()).pw_dir/.local/state/flux-exchange`, macOS uses
      `getpwuid_r(geteuid()).pw_dir/Library/Application Support/Flux/Exchange`, and Windows uses
      `SHGetKnownFolderPath(FOLDERID_LocalAppData)/Flux/Exchange`. Traversal refuses symlinks/reparse
      points; the account home/profile boundary belongs to the expected account; shared ancestors
      are not writable by untrusted accounts; the Exchange root and descendants are owner-only.
      Unsafe existing metadata refuses without chmod, chown, ACL repair or advice to narrow `/tmp`
      or another shared ancestor.
- [ ] Unix creates `<native-root>/run/local-management-v1.sock` below an owner `0700` directory with
      an owner `0600` socket and authenticates the startup effective UID using `SO_PEERCRED` on Linux
      or `getpeereid` on macOS. Windows creates
      `\\.\pipe\flux-exchange-local-management-v1-<first-32-lowerhex-of-SHA256(TokenUser-SID-bytes)>`
      in the named-pipe namespace using byte mode, overlapped IO, `PIPE_REJECT_REMOTE_CLIENTS`,
      `FILE_FLAG_FIRST_PIPE_INSTANCE`, current-user ownership and a protected current-user/System
      DACL. It authenticates through named-pipe impersonation, `TokenUser` comparison and an
      unconditional `RevertToSelf`.
- [ ] The authenticated peer maps only inside the local-management dispatcher to tenant `local`,
      `PrincipalKind::User`, principal id `local-owner` and operator `true`. It is never installed as
      an HTTP identity provider and cannot authenticate loopback TCP, hosted mode, another account,
      Service Account runtime routes or `--dev`. Plan reads remain human management; connection,
      authority, grant and Service Account mutations remain operator management. A native adversary
      proves loopback TCP cannot reproduce the bootstrap.
- [ ] The first public release retains `exchange.release-trust.v1` and publishes
      `exchange.release-channel.v2`, `exchange.release-manifest.v2`,
      `exchange.compatibility.v2`, `exchange.connection-plan.v2` and
      `exchange.supervisor-ready.v2`. Its exact protocol object contains eight fields:
      `exchange_api`, `effective_catalogue_response`, `invoke_request` and `invoke_response` retain
      their v1 identities; `connection_plan` is v2; `local_management` is
      `exchange.local-management.v1`; `service_account_handoff` is
      `exchange.service-account-handoff.v1`; and `supervisor` is v2. Deny-unknown serializers,
      parsers, signed fixtures and release checks reject the old six-field object, added fields or a
      changed contract under any existing v1 identity.
- [ ] `exchange.supervisor-ready.v2` changes only the schema identity and protocol inventory. Unix
      supervised readiness FD 3/liveness FD 4 and the existing two Windows HANDLE arguments retain
      their exact X-128 directions and value-free meanings. Local management has no lifecycle opcode
      and never shares readiness, liveness or C-510 lifecycle control.
- [ ] `exchange.local-management.v1` uses one operation per connection and a 12-byte header: ASCII
      `FXLM`, version byte `1`, direction byte (`1` client-to-server or `2` server-to-client),
      big-endian `u16` opcode and big-endian `u32` payload length. Canonical control JSON is at most
      65,536 bytes; each secret frame is an ordinal `u16` plus `1..=8192` raw bytes; at most 64 secret
      frames and 1 MiB total payload are accepted. Compression, nonzero flags, unknown opcodes,
      duplicate JSON members, trailing bytes and a second operation refuse.
- [ ] The provider fixture owns the exact `CONNECT_BEGIN -> CONNECT_RECEIPT|ERROR` or
      `NEED_SECRETS -> SECRET* -> CONNECT_COMMIT -> CONNECT_RECEIPT|ERROR -> EOF` state machine.
      It publishes `exchange.connect-receipt.v1`, `exchange.grant-apply-receipt.v1`,
      `exchange.service-account-mint-receipt.v1` and `exchange.local-management-error.v1` with the
      closed error codes `invalid_frame`, `unsupported_version`, `wrong_direction`,
      `unexpected_frame`, `frame_too_large`, `truncated_frame`, `surplus_data`, `peer_unverified`,
      `unsafe_root`, `local_management_unavailable`, `invalid_request`, `unknown_connector`,
      `invalid_label`, `secret_json_forbidden`, `unknown_target`, `stale_plan`, `proposal_conflict`,
      `connect_busy`, `grant_stale`, `grant_digest_mismatch`, `service_account_conflict`,
      `writer_invalid`, `writer_closed`, `store_unavailable`, `audit_unavailable` and
      `internal_refusal`; retry is one of `never|refresh|same_proposal|operator`, and commit is one of
      `none|committed|query_receipt`.
- [ ] Plan, connection-create, credential-rotation/acquisition and Service Account mint JSON reject
      every secret identity, alias, value and unknown target before mutation without reflecting it.
      Hosted onboarding remains complete through bounded Exchange-owned binary paths only:
      `application/vnd.flux-exchange.vendor-secret-v1` for vendor input and
      `application/vnd.flux-exchange.service-account-handoff-v1` for a minted token, with
      `Cache-Control: no-store` and no secret in URL, query, header, JSON response or error.
- [ ] The verified `flux-exchange` executable owns local TTY/browser secret input. Flux may supply
      connector, label, exact plan/target revision and non-secret transaction metadata, but does not
      read, proxy, inherit, log or render the bytes and cannot redirect the helper to another
      endpoint, tenant, credential address or instance. This is a testable software/dataflow
      invariant, not an OS isolation claim against a malicious same-user debugger.
- [ ] Supervised local connect uses one new Exchange-server transaction owner, not the current plan
      handler's separate credential/settings/registry writes and `partial` outcome. A server-owned
      `<native-root>/local-connections/store.sqlite3` with `synchronous=FULL` implements
      `SecretStore`, `ConnectionSettings`, `ConnectionRegistry` and local receipts. Credential
      columns are raw binary/text values, never JSON; SQLite journal files are credential-store
      sinks. Hosted routes may retain separate composition but may not claim local crash atomicity.
- [ ] The connect proposal digest is SHA-256 over canonical non-secret connector, label, plan
      revision, ordered target identities, settings and authority revision. It excludes secret
      bytes, lengths, hashes, presence facts and fingerprints. Under `BEGIN IMMEDIATE`, one process-
      wide connection lock rechecks plan/authority/occupancy and one transaction writes label,
      instance, settings, authority, credentials, a value-free audit outbox entry and receipt before
      one commit. A pre-commit crash exposes none; response loss or uncertain commit yields
      `query_receipt`; same-proposal replay returns the receipt before prompting or writing; a
      changed proposal refuses naming only connector and label. Retry is never edit-by-retry.
- [ ] For Service Account mint, Unix helper mode receives only write FD 5 and transfers exactly that
      write-only pipe to the authenticated running server through one `SCM_RIGHTS` message. Exchange
      rejects `MSG_CTRUNC`, multiple descriptors and the wrong pipe kind/direction and sets
      `FD_CLOEXEC`. Windows launches the helper with a closed
      `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, revalidates the helper process token SID, pins the process
      and transfers the declared write HANDLE with `DuplicateHandle`; a native planted-handle canary
      proves no unrelated inheritable handle arrived. No receiver claims to enumerate the complete
      Windows process handle table.
- [ ] `exchange.service-account-handoff.v1` is exactly one frame followed by EOF: ASCII `FXSA`,
      version byte `1`, direction byte `1` (Exchange to writer), two zero flag bytes, a big-endian
      `u32` length of `1..=512`, then opaque token bytes. Truncation, surplus bytes, a second frame,
      wrong version/direction, wrong capability, sink failure and early close refuse. Receivers do
      not depend on the current token prefix. Exchange persists only the verifier and its receipt may
      claim only verifier commit plus `frame_written`; Flux C-509 alone derives `credential_stored`
      after its receiving store commits. The one-way pipe never implies receiver persistence.
- [ ] Grant preview accepts one connector-scoped selector change and returns the complete candidate,
      an exact revision/ETag and a canonical proposal digest. Compare-and-swap apply preserves every
      unrelated connector, the selected connector's inbound authority and all provider-owned
      unmodified fields. Same-digest replay returns the committed receipt; a stale revision,
      mismatched digest or unexpressible stored authority refuses before write. Grants remain tenant-
      and-connector scoped metadata selectors with no label or operation-id authority axis.
- [ ] Positive/adversarial provider fixtures and tests cover native root poisoning, unsafe metadata,
      peer authentication, loopback TCP, every secret JSON path, bounded hosted binary input,
      connect crash/response-loss/replay/conflict, secret-free digests, handoff framing/capability
      closure, split receipts, grant CAS/preservation and the unchanged X-128 capability ABI. Raw,
      JSON-escaped, percent-encoded and base64 sentinels are scanned across JSON, URLs, argv,
      environment, local-management diagnostics, stdout/stderr, tracing, audit, readiness, liveness,
      lifecycle/control state and every persisted file. Only the Exchange credential database and
      journals, the dedicated frame/test sink and later Authorization transport are allowlisted.
- [ ] Native CI executes the complete evidence on `macos-15` (`aarch64-apple-darwin`),
      `macos-15-intel` (`x86_64-apple-darwin`), `ubuntu-24.04-arm`
      (`aarch64-unknown-linux-gnu`), `ubuntu-24.04` (`x86_64-unknown-linux-gnu`) and `windows-2025`
      (`x86_64-pc-windows-msvc`). Every row runs root poisoning, owner endpoint/TCP adversary, real
      TTY/console input, handoff closure plus an extra-capability canary, crash injection before and
      after connect commit, restart receipt replay, concurrent grant CAS, four-form sentinel scans
      and existing native X-128 tests. Cross-compilation, fixture parsing or another architecture is
      not evidence.
- [ ] `docs/designs/local-release-v1.md` is reconciled into the v2 provider contract, operator/user
      documentation and the public website describe the final boundary, and X-126 release fixtures
      and checks consume it exactly. The full repository gate and public-site build pass. Any
      corresponding Flux PR runs Flux's `scripts/build-embedded-docs.sh` before its gate so embedded
      documentation cannot be stale. X-126 stays active until X-134 is merged, final v2 fixtures are
      regenerated from the candidate commit, all five native jobs pass and the separately authorized
      public release verifier succeeds.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0007-local-onboarding-uses-owner-bound-capabilities.md`.
- This story owns provider bytes and fixtures. Flux C-509 owns the receiving credential writer,
  owner-only Flux store, opaque resolver, management/runtime client split, CLI projection, concrete
  write approval and retry suppression. Exchange fixtures may use a test sink but never certify
  Flux's production receiver store.
- The local-management endpoint is not C-510's lifecycle control channel and never carries start,
  status, stop, readiness or liveness frames. C-510 remains secret-free.
- Existing hosted authentication stays authoritative for hosted Exchange. OS-owner bootstrap is
  deliberately unavailable outside the supervised single-user composition.
