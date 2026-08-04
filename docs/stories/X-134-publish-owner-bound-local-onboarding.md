---
id: X-134
title: "Publish owner-bound local onboarding without secret JSON"
status: blocked
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

- [ ] This contract change amends X-126's frontmatter to depend on X-134 and replaces/supersedes
      every authoritative v1/six-protocol Acceptance clause with the final v2/eight-protocol
      contract below. X-126 stays `in-progress`; its merged v1 machinery remains unpublished
      implementation evidence only. X-134 stays `blocked` and no code wave starts until connectors
      C-515 publishes the prepared secret-transaction port to crates.io and the cross-repository
      schedule records `connectors/C-515 -> exchange/X-134 -> exchange/X-126`. A path, git
      dependency, copied batch representation or Exchange-owned credential schema refuses.
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
- [ ] `exchange.local-management.v1` uses one operation per native connection or hosted WebSocket
      and a 12-byte header: ASCII `FXLM`, version byte `1`, direction byte (`1` client-to-server or
      `2` server-to-client), big-endian `u16` opcode and big-endian `u32` payload length. Canonical
      control JSON is at most 65,536 bytes; each secret frame is an ordinal `u16` plus `1..=8192` raw
      bytes; at most 64 secret frames and 1 MiB cumulative payload are accepted. The header has no
      flag field. Native byte-stream reads may split or coalesce bytes arbitrarily; the 12-byte header
      and declared payload length delimit each successive FXLM frame. Only hosted transport binds
      message boundaries to frames: each reassembled WebSocket binary message contains exactly one
      complete FXLM frame and is at most 65,548 bytes. WebSocket fragmentation is transport-only;
      splitting one FXLM frame across messages is a truncated frame and coalescing frames in one
      message is surplus data. Compression, unknown opcodes, duplicate JSON members, trailing bytes
      and a second logical operation per native connection or hosted WebSocket refuse. Opcodes are
      connect begin/need-secrets/secret/commit/query/receipt `0x0001..0x0006`; grant preview/candidate/
      apply/query/receipt `0x0010..0x0014`; Service Account mint/query/receipt `0x0020..0x0022`;
      hosted credential rotate-or-acquire begin/commit/receipt/query `0x0030..0x0033`; and error
      `0x7fff`. Hosted credential ceremonies reuse `0x0002` NEED_SECRETS and `0x0003` SECRET;
      no additional opcode exists. Direction and state make each value exhaustive. Failing-first
      fixtures bind the linked design's exact closed payload objects: member names/types are not
      aliases, plan/target revisions and proposal digests are exactly 64 lowercase hex, non-zero
      transaction/receipt ids are opaque 256-bit values encoded as exactly 64 lowercase hex,
      store revisions/expiries are canonical decimal strings, and requested ordinals are exactly
      one-based contiguous plan order. Every omitted, renamed, added, nullable, mistyped,
      out-of-bound or noncanonical member refuses before mutation.
- [ ] Each native FXLM connection or hosted WebSocket carries one exact logical operation. Connect is
      client `BEGIN`, server `RECEIPT|ERROR` or server `NEED_SECRETS`, client one `SECRET` for each
      requested ordinal in request order, client `COMMIT`, server `RECEIPT|ERROR`; a separate client
      `QUERY` yields server `RECEIPT|ERROR`. Grant preview is client `PREVIEW`, server
      `CANDIDATE|ERROR`; grant apply is client `APPLY`, server `RECEIPT|ERROR`; a separate client
      `QUERY` yields server `RECEIPT|ERROR`. Service Account mint is client `MINT` plus its transferred
      writer capability, server `RECEIPT|ERROR`; a separate client `QUERY` yields server
      `RECEIPT|ERROR`. Hosted rotate/acquire is client `BEGIN`, server `NEED_SECRETS|ERROR`, client one
      `SECRET` for each requested ordinal in request order, client `COMMIT`, server `RECEIPT|ERROR`;
      a separate WebSocket carrying client `QUERY` yields server `RECEIPT|ERROR`. A query names
      exactly the receipt id; response loss before receipt-id delivery uses byte-identical proposal
      replay, never a client-manufactured query key. No other direction, repetition, omission or
      transition is valid. Closed control objects bind the canonical proposal, opaque 256-bit
      transaction/receipt ids and ordered ordinal/target pairs. Provider fixtures publish
      `exchange.connect-receipt.v1`, `exchange.grant-apply-receipt.v1`,
      `exchange.service-account-mint-receipt.v1` and `exchange.local-management-error.v1` with the
      closed error codes `invalid_frame`, `unsupported_version`, `wrong_direction`,
      `unexpected_frame`, `frame_too_large`, `truncated_frame`, `surplus_data`, `peer_unverified`,
      `unsafe_root`, `local_management_unavailable`, `invalid_request`, `unknown_connector`,
      `invalid_label`, `secret_json_forbidden`, `unknown_target`, `stale_plan`, `proposal_conflict`,
      `connect_busy`, `grant_stale`, `grant_digest_mismatch`, `service_account_conflict`,
      `writer_invalid`, `writer_closed`, `store_unavailable`, `audit_unavailable` and
      `internal_refusal`. The linked design's table is the exhaustive byte contract: status is a
      JSON integer; before decision every error has exactly `commit=none` and its single listed
      `never|refresh|operator` retry. Only `store_unavailable`/503,
      `audit_unavailable`/503 and `internal_refusal`/500 exist after decision, with the opaque
      receipt id and exactly `commit=query_receipt,retry=same_proposal`. Canonical receipts have the
      design's exact `schema`, public resource identity, receipt id, boolean `replayed` and closed
      commit object—never a setting, stored proposal digest, target, expiry, secret presence or
      secret-derived fact. The fixture enumerates every opcode/state/status/code/retry/commit row and
      rejects the complement; a new tuple requires a new protocol identity.
- [ ] Plan, connection-create, credential-rotation/acquisition and Service Account mint JSON reject
      every secret identity, alias, value and unknown target before mutation without reflecting it.
      Hosted operators use the same FXLM connect/rotate/acquire state machines only through a
      WebSocket upgrade at exactly `GET /api/onboarding/frames` with exact, case-sensitive sole
      subprotocol `exchange.local-management.v1` and `Cache-Control: no-store`. The request has no
      query string or body. Authentication, tenant derivation and the existing hosted operator
      policy are revalidated before upgrade; Service Accounts fail the operator gate. `Origin` must
      exactly equal startup-bound `FLUX_EXCHANGE_CONSOLE_ORIGIN`. The explicit setting is one
      canonical ASCII origin containing only scheme, host and effective port, with no userinfo,
      path slash, query or fragment. Default ports are omitted: canonical HTTPS/HTTP omit
      `:443`/`:80`, while a non-default port is present as decimal `1..=65535` without a leading
      zero. An explicit default port, trailing slash, uppercase scheme/host, noncanonical IP or
      leading-zero port fails startup rather than being normalized; request Origin comparison is
      byte-exact and performs no normalization. Production requires HTTPS. `--dev` alone may use
      HTTP with a literal loopback IP and, only when the setting is absent, derives the same
      canonical serialization from the explicit loopback listener configuration. A hosted route
      with no usable configured origin is unavailable; an invalid explicit setting fails startup.
      Exchange never derives the origin from an OIDC
      redirect URI, `Host`, `Forwarded` or any `X-Forwarded-*` header; missing, `null`, malformed and
      mismatched request origins refuse. Success echoes the exact subprotocol, returns no
      `Sec-WebSocket-Extensions`, and never negotiates compression; offered `permessage-deflate` is
      ignored rather than accepted or treated as malformed. Missing or invalid authentication is
      401; a non-operator or unacceptable origin is 403; malformed upgrade, query, body or
      subprotocol input is 400; another method is 405 with `Allow: GET` before body decoding; an
      unsupported WebSocket version is 426 with `Sec-WebSocket-Version: 13`. Hosted ceremony
      occupancy is exactly 32 live WebSockets process-wide and 4 per resolved tenant, including
      query/preview/replay, with no queue or override; either exhausted counter is 429 with the exact
      delta-seconds header `Retry-After: 5`. Unavailable identity, audit, coordinator or
      configured-origin dependencies are 503. Every handshake refusal is value-free and
      `Cache-Control: no-store`.
- [ ] Hosted Exchange allocates and associates the server-owned opaque transaction id only after an
      admitted `BEGIN`, returns ordered `NEED_SECRETS`, then accepts exactly those `SECRET` ordinals
      before `COMMIT`. No transaction or receipt id appears in a URL, header or log. Connect, rotate
      and password acquisition use the same coordinator and interactive state machine as native
      FXLM. Query and same-proposal replay each use a separate WebSocket; replay may return the
      existing receipt before a prompt, while a changed proposal refuses. A successful receipt or
      well-formed FXLM error is followed by close code 1000. Malformed FXLM, wrong direction
      or state, surplus data or a second operation uses 1002 after a binary FXLM error when one can
      safely be emitted; text uses 1003 without JSON decoding; any declared frame, message, control,
      secret, count or cumulative bound excess uses 1009. The absolute pre-decision deadline is
      exactly 300 monotonic seconds from hosted slot reservation before `101`, or native peer
      authentication before the first header; traffic never resets it and expiry uses 1008. After
      decision there is a separate exact 30-second response budget: expiry returns the applicable
      `query_receipt` error, releases the transport/slot and leaves recovery rolling forward. Close
      reasons are always empty and neither deadline is configurable. Before a durable decision,
      disconnect, timeout or protocol failure zeroizes transient buffers and aborts or tombstones
      an allocated provider transaction. After the decision it never aborts: recovery, query or
      same-proposal replay rolls forward.
- [ ] The existing one-shot `POST /api/service-accounts` is unchanged: it accepts only a strict
      non-secret id/expiry object and returns exactly one FXSA body as
      `application/vnd.flux-exchange.service-account-handoff-v1` with `Cache-Control: no-store`;
      metadata is obtained through list. Every former create/rotate/acquire/mint secret JSON shape
      returns status 415 plus value-free `secret_json_forbidden` before body decoding or mutation.
      No other method, path, query, header or response shape can select a secret target, creation,
      rotation, acquisition or mint operation. The hosted WebSocket binds the existing
      `exchange.local-management.v1` protocol and never adds a ninth release inventory field.
- [ ] The verified `flux-exchange` executable owns local TTY/browser secret input. Flux may supply
      connector, label and exact plan/target revision, but never supplies, chooses, parses or orders
      the provider transaction identity. Flux does not read, proxy, inherit, log or render the bytes
      and cannot redirect the helper to another endpoint, tenant, credential address or instance.
      This is a testable software/dataflow invariant, not an OS isolation claim against a malicious
      same-user debugger.
- [ ] The exact helper grammar is provider-fixtured and closed. Vendor input is
      `flux-exchange local vendor-secret --connector <id> --label <label> --plan-revision
      <revision>`; Service Account mint is
      `flux-exchange local service-account-mint --id <id> --expires-at <canonical-decimal>
      --writer-fd 5` on Unix or the same command with `--writer-handle <canonical-decimal>` on
      Windows. No endpoint, tenant, credential address, program, cwd or arbitrary argument is
      caller-selectable. Stdin is null; the helper opens `/dev/tty` or the current Windows console
      directly. Stdout/stderr and exit status are bounded and value-free.
- [ ] Supervised local connect uses one Exchange-server transaction coordinator, not the current plan
      handler's separate writes, `partial` outcome or an Exchange-owned credential database. It
      registry-resolves released `codewandler-connector-secrets` 0.20.0 with its crates.io checksum
      and consumes C-515 through `Arc<dyn PreparedSecretStore>`. The only provider operations are
      `prepare`, `state`, `commit`, `abort` and `reclaim`; Exchange cannot emulate them with point
      writes. Prepare durably stages the checked `SecretBatch` invisibly inside
      `connector-secrets`; public state is only `Absent|Prepared|Committed`. Same-id/same-digest
      prepare returns its existing prepared or committed state without inspecting the supplied
      batch; a different digest refuses. Repeated commit of committed state and repeated abort of an
      aborted tombstone are idempotent; abort of committed returns `AlreadyCommitted`, and commit or
      prepare after abort returns `TransactionIdReused`. Aborted ids remain internal terminal
      tombstones reported as absent, and reclaimed ids return `Retired`. Exchange never inspects
      crate-private mutations, staged paths or values and never persists a credential byte.
- [ ] Exchange durably allocates every provider transaction id in its value-free journal root as one
      non-zero generation followed by a unique 192-bit nonce and treats the resulting 256 bits as
      opaque outside the provider API. Generation zero, wrap and nonce reuse refuse. It acknowledges
      `reclaim(G)` only after every transaction through `G` is terminal and no journal, recovery,
      receipt query or same-proposal replay can ask the provider about those generations. No timer,
      ledger pressure, count threshold or ordering over an opaque id permits reclamation.
- [ ] The coordinator preserves C-515's bounded state machine rather than weakening it: one prepared
      slot reserves ordinary mutations; abort-before-prepare durably fences delayed prepare; a
      cross-id abort that would rewrite the ledger while another id is prepared returns `Busy`; the
      4096-terminal-record and 1 MiB bounds return `Capacity` without eviction, and abort of an
      unseen id at capacity returns `Capacity` without mutation; successful prepare has already
      staged the complete next image, so commit has no later deterministic validation failure.
      Provider `Busy`, `DigestMismatch`, `TransactionIdReused`, `NotPrepared`,
      `AlreadyCommitted`, `Retired`, `Capacity`, `InvalidBatch`, `Unsupported` and `Backend` map to
      the linked design's one exhaustively fixtured closed value-free tuple beside successful
      `Absent|Prepared|Committed` outcomes: `Unsupported` is
      `local_management_unavailable/503/operator`; `Busy` is `connect_busy/409/refresh`;
      `DigestMismatch` is `proposal_conflict/409/refresh`; `Capacity` is
      `store_unavailable/503/operator`; and pre-decision `TransactionIdReused`, `NotPrepared`,
      `AlreadyCommitted`, `Retired` or `InvalidBatch` is `internal_refusal/500/operator`. Each has
      `commit=none`. Provider `Backend`/I/O first resolves through `state`: `Absent` retries the
      same prepare, `Prepared` continues, and pre-decision `Committed` is the invariant refusal,
      never a synthesized decision. If unresolved before decision it is
      `store_unavailable/503/operator/none`; after decision, `Backend`, `NotPrepared`, `Retired` or
      impossible `Absent` carries the receipt id with the design's fixed 503 or 500 status and
      `commit=query_receipt,retry=same_proposal`. Recovery queries state and repeats commit, never
      aborts, re-prepares or edits the proposal. The fixture rejects every unlisted phase/result
      pairing.
- [ ] The connect proposal digest is SHA-256 over canonical non-secret connector, label, plan
      revision, ordered target identities, settings and the ordered collection of every `(authority
      target identity, canonical revision)` pair. It excludes secret bytes, lengths, hashes,
      presence facts and fingerprints. Exchange writes a value-free coordinator journal, prepares
      the secret transaction, then durably records one commit decision. Without that decision,
      recovery aborts staging and removes the journal. With it, recovery idempotently commits the
      secret batch, rolls forward label/instance/settings/authority, drains audit, publishes the
      connection head/receipt and closes the journal. Relevant readers/mutations are gated while
      unresolved, and startup recovers before readiness/routes. A pre-decision crash exposes none;
      a post-decision crash converges to one complete result; query/replay never prompts or writes a
      second time. A changed proposal refuses naming only connector and label.
- [ ] The journal's value-free audit outbox uses one stable transaction-derived event id.
      `AuditJournal` append is idempotent by that id, restart drains every committed outbox entry
      before readiness or route service, and a committed receipt is not returned until canonical
      audit delivery succeeds. An unavailable audit sink after the decision or a lost response
      returns `query_receipt`; retry drains/deduplicates and returns the same receipt. No committed
      connection can become visible or replayable without one queryable canonical audit event.
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
      peer authentication, loopback TCP, every secret JSON path, connect crash/response-loss/replay/
      conflict, secret-free digests, handoff framing/capability closure, split receipts, grant CAS/
      preservation and the unchanged X-128 capability ABI. Hosted handshake evidence covers a
      cross-origin request even with valid credentials; missing, malformed, `null`, sibling and
      mismatched origins; OIDC redirect, `Host`, `Forwarded` and `X-Forwarded-*` spoofing; missing,
      wrong, differently cased and multiple subprotocols; offered-but-not-negotiated compression;
      and the exact 400/401/403/405+`Allow: GET`/426+version/429+`Retry-After: 5`/503 value-free
      no-store outcomes. Occupancy fixtures hold 31/32 process slots and 3/4 tenant slots and prove
      immediate no-queue refusal at each inclusive bound. Startup-setting fixtures admit a
      canonical production HTTPS origin; refuse userinfo, non-root paths, queries, fragments and
      noncanonical or non-HTTPS production forms; specifically admit omitted default ports, refuse
      explicit `:443`/`:80`, and admit a canonical non-default port. They admit HTTP only for
      `--dev` plus a literal loopback IP; prove absent-setting derivation uses only the explicit
      loopback listener; prove no usable hosted origin makes the route unavailable; and prove an
      invalid explicit `FLUX_EXCHANGE_CONSOLE_ORIGIN` fails startup. Injected-clock fixtures prove
      the 299/300-second pre-decision and 29/30-second post-decision boundaries without traffic
      reset. Native-stream fixtures
      split headers and payloads at every boundary, read byte-by-byte and coalesce successive frames,
      proving only header plus payload length delimit them. Hosted message/state evidence covers text
      and binary JSON shapes, WebSocket fragmentation, frame coalescing and message splitting,
      deceptive lengths, every frame/message/control/secret/count/cumulative bound, wrong direction/
      opcode/state/ordinal, surplus and second operations, cross-tenant transaction and receipt ids,
      and empty close reasons with exact close codes. Crash/disconnect evidence covers before
      prepare, after prepare, before decision, after decision and after receipt; lost-receipt query
      and same-proposal replay prove roll-forward without a second prompt. Raw, JSON-escaped,
      percent-encoded and base64 sentinels are scanned across JSON, URLs, headers, argv, environment,
      local-management diagnostics, close reasons,
      stdout/stderr, tracing, audit, journals, readiness, liveness, lifecycle/control state and every
      persisted file. Only connector-secrets' committed credential store and C-515 staging sink, the
      dedicated frame/test sink and later Authorization transport are allowlisted; Exchange journals
      remain value-free.
- [ ] Native CI executes the complete evidence on `macos-15` (`aarch64-apple-darwin`),
      `macos-15-intel` (`x86_64-apple-darwin`), `ubuntu-24.04-arm`
      (`aarch64-unknown-linux-gnu`), `ubuntu-24.04` (`x86_64-unknown-linux-gnu`) and `windows-2025`
      (`x86_64-pc-windows-msvc`). Every row runs root poisoning, owner endpoint/TCP adversary, real
      TTY/console input, handoff closure plus an extra-capability canary, crash injection before and
      after connect commit, restart receipt replay, concurrent grant CAS, four-form sentinel scans
      and existing native X-128 tests. The registry-resolved 0.20.0 FileStore holds C-515's exclusive
      lifetime writer/recovery lease throughout each native process test; a second opener refuses,
      abrupt exit releases it, and no test repairs, replaces or reaps the lease. Exchange opens that
      one store before recovery/readiness and retains the same object for the server lifetime. Every
      0.19 writer is quiesced before the first 0.20 open. Cross-compilation, fixture parsing or
      another architecture is not evidence.
- [ ] X-126's story Acceptance and dependency graph plus `docs/designs/local-release-v1.md` are
      amended/superseded by this contract PR into the v2 provider contract. Operator/user
      documentation and
      the public website describe the final boundary, and X-126 release fixtures and checks consume
      it exactly. The full repository gate and public-site build pass. Any
      corresponding Flux PR runs Flux's `scripts/build-embedded-docs.sh` before its gate so embedded
      documentation cannot be stale. X-126 stays active until X-134 is merged, final v2 fixtures are
      regenerated from the candidate commit, all five native jobs pass and the separately authorized
      public release verifier succeeds.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0007-local-onboarding-uses-owner-bound-capabilities.md` at roadmap
  commit `77a0d69b3eb938f6d055650cefc4ba2153228776`.
- External implementation dependency: connectors C-515 and the checksummed crates.io release of
  `codewandler-connector-secrets` 0.20.0. X-134 remains blocked and no code wave starts from an
  unmerged provider port, unpublished commit, path/git dependency or unmatched lockfile resolution.
- This story owns provider bytes and fixtures. Flux C-509 owns the receiving credential writer,
  owner-only Flux store, opaque resolver, management/runtime client split, CLI projection, concrete
  write approval and retry suppression. Exchange fixtures may use a test sink but never certify
  Flux's production receiver store.
- The local-management endpoint is not C-510's lifecycle control channel and never carries start,
  status, stop, readiness or liveness frames. C-510 remains secret-free.
- Existing hosted authentication stays authoritative for hosted Exchange. OS-owner bootstrap is
  deliberately unavailable outside the supervised single-user composition.
