---
id: X-134
title: "Publish owner-bound local onboarding without secret JSON"
status: in-progress
priority: 0
epic: connections
areas: [exchange-server, identity, connections, grants, protocol, tests]
depends_on: [X-125, X-127, X-128, X-129]
design: docs/designs/local-release-v1.md
note: "Milestone 1 — Linux OS-owner management, direct vendor-secret insertion, one-shot Service Account handoff and revisioned grants must ship before the first public Linux release"
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

## Child delivery sequence

X-134 remains the parent release blocker and is not complete until each child below is done, or an
item is explicitly retired with evidence satisfying the same X-134 Acceptance rows:

1. [X-135](X-135-close-local-management-deadlines.md) — hosted/native deadline and terminal
   behavior.
2. [X-136](X-136-bound-helper-plan-and-result-envelope.md) — helper plan validation and the one
   absolute setup/result envelope.
3. [X-137](X-137-constrain-exchange-runtime-and-release-to-linux.md) — remove non-Linux runtime/publication
   paths and establish the exact two-target Linux boundary.
4. [X-138](X-138-bind-provider-recovery-and-native-c515-evidence.md) — provider crash/replay and
   exact native C-515 bindings.
5. [X-139](X-139-canonicalize-release-native-evidence.md) — the sole canonical native-evidence
   authority, final fixtures and integrated closure proof.

These are sequencing boundaries inside this story, not permission to defer its blockers or ship a
partial X-134 contract. Their frontmatter records the serialized dependency order.

## Acceptance

### Decision 0012 platform amendment

Flux-roadmap Decision 0012 at `dc907fa` is the platform authority for every Acceptance row below.
It replaces every lower Unix/macOS/Windows split with Linux only. Production derives the owner root
with `getpwuid_r(geteuid())`, serves an owner-only Unix socket authenticated with Linux
`SO_PEERCRED`, uses readiness/liveness FD 3/4, FXSA writer FD 5, ceremony request/result FD 6/7,
opens `/dev/tty` for direct private input, transfers the writer once with `SCM_RIGHTS` and binds
process identity to the Linux proc start marker.

There is no macOS support root or `getpeereid` path and no Windows profile/DACL, named pipe,
impersonation, private `CONIN$`, HANDLE-list or FXHA path. Native FXLM begins directly with its
12-byte frame; Service Account MINT receives the separately transferred writer capability and no
descriptor appears in JSON. The hosted WebSocket contract and all platform-independent framing,
deadline, plan, grant, recovery, receipt, secret-exclusion and C-515 obligations remain unchanged.
Every lower mixed-platform row is authoritative only for that retained behavior executed on
`aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`; its non-Linux clauses and five-runner
counts are superseded and are not acceptance. The unpublished v2 protocol/schema identities remain
v2.

Authenticated Service Account catalogue/invocation HTTP and hosted FXLM WebSocket behavior remain
available on a Linux Exchange. This contract adds no remote lifecycle, native FXLM/FXSA,
connect/grant/mint or remote owner-management protocol for a non-Linux Flux client.

- [ ] This contract change amends X-126's frontmatter to depend on X-134 and replaces/supersedes
      every authoritative v1/six-protocol Acceptance clause with the final v2/eight-protocol
      contract below. X-126 stays `in-progress`; its merged v1 machinery remains unpublished
      implementation evidence only. X-134 closure requires the registry-resolved C-515 0.20.0 exact
      checksum/source and preserves the recorded order
      `connectors/C-515 -> exchange/X-134 -> exchange/X-126`. A path, git
      dependency, copied batch representation or Exchange-owned credential schema refuses.
- [ ] Production derives the native root without `HOME`, `XDG_*`, `USERPROFILE`, `LOCALAPPDATA` or
      an equivalent inherited variable. Linux uses
      `getpwuid_r(geteuid()).pw_dir/.local/state/flux-exchange`. Traversal refuses symlinks; the
      account home boundary belongs to the expected account; shared ancestors
      are not writable by untrusted accounts; the Exchange root and descendants are owner-only.
      Unsafe existing metadata refuses without chmod, chown, ACL repair or advice to narrow `/tmp`
      or another shared ancestor.
- [ ] Linux creates `<native-root>/run/local-management-v1.sock` below an owner `0700` directory
      with an owner `0600` socket and authenticates the startup effective UID using `SO_PEERCRED`.
      The production endpoint implementation has no alternate macOS or Windows transport.
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
- [ ] `exchange.supervisor-ready.v2` changes only the schema identity and protocol inventory. Linux
      supervised readiness FD 3/liveness FD 4 retain their exact X-128 directions and value-free
      meanings. Local management has no lifecycle opcode
      and never shares readiness, liveness or C-510 lifecycle control.
- [ ] `exchange.local-management.v1` uses one operation per native connection or hosted WebSocket
      and a 12-byte header: ASCII `FXLM`, version byte `1`, direction byte (`1` client-to-server or
      `2` server-to-client), big-endian `u16` opcode and big-endian `u32` payload length. Canonical
      control JSON is at most 65,536 bytes; each secret frame is an ordinal `u16` plus `1..=8192` raw
      bytes; `NEED_SECRETS.secrets` has 0..=64 entries, at most 64 secret frames and 1 MiB cumulative
      payload are accepted. The header has no
      flag field. Native byte-stream reads may split or coalesce bytes arbitrarily; the 12-byte header
      and declared payload length delimit each successive FXLM frame. Every native operation begins
      with `FXLM`; the Service Account writer is transferred separately with `SCM_RIGHTS` alongside
      MINT and is never a frame, JSON member, opcode or ninth release protocol. Only hosted transport binds
      message boundaries to frames: each reassembled WebSocket binary message contains exactly one
      complete FXLM frame and is at most 65,548 bytes. WebSocket fragmentation is transport-only;
      splitting one FXLM frame across messages is a truncated frame and coalescing frames in one
      message is surplus data. Compression, unknown opcodes, duplicate JSON members, trailing bytes
      and a second logical operation per native connection or hosted WebSocket refuse. Opcodes are
      connect begin/need-secrets/secret/commit/query/receipt `0x0001..0x0006`; plan query/response
      `0x0007..0x0008`; grant preview/candidate/apply/query/receipt `0x0010..0x0014`; Service Account
      mint/query/receipt `0x0020..0x0022`; credential rotate-or-acquire begin/commit/receipt/query
      `0x0030..0x0033`; and error `0x7fff`. Credential ceremonies reuse `0x0002` NEED_SECRETS and
      `0x0003` SECRET; no additional opcode exists. Direction and state make each value exhaustive. Failing-first
      fixtures bind the linked design's exact closed payload objects: member names/types are not
      aliases, plan/target revisions and proposal digests are exactly 64 lowercase hex. A transaction
      id is exactly 64 lowercase hex whose first 16 digits decode as a nonzero big-endian u64
      Exchange-coordinator generation and whose remaining 48 encode its Exchange-coordinator-owned
      unique 192-bit nonce; C-515 validates/encodes that identity and owns provider state but
      generates neither component. A receipt id is a separate opaque
      64-lowerhex identity for which only the complete all-zero value refuses. Store
      revisions/expiries are canonical decimal strings, and requested ordinals are exactly
      one-based contiguous plan order. Every omitted, renamed, added, nullable, mistyped,
      out-of-bound or noncanonical member refuses before mutation. Plan/target revisions are
      reproducible domain-separated SHA-256 identities over the linked design's exact RFC 8785
      static plan/target preimages: target identity covers X-125's destination, choices and typed
      custom-origin policy; plan identity covers deterministic `connection.name`, released config
      order and remaining released auth order, including every static rendered field member and an
      unroutable field's deterministic reason. `routable` is exactly derived as `target != null`.
      Only live `set`, authority lifecycle state/revision/actions, `credential_revision`, labels,
      selection and aggregate state are excluded. Revisions exclude stored values and are never
      parsed or ordered. Decimal
      authority/grant store revisions remain mutable u64 CAS counters and cannot be substituted for
      either content identity.
- [ ] `PLAN_QUERY 0x0007` is exactly `{"connector":Connector,"selection":Label|null}` and is admitted
      only on the owner-authenticated native endpoint. `PLAN_RESPONSE 0x0008` is the complete RFC
      8785 `exchange.connection-plan.v2` object, not an identity-only acknowledgment. Its required
      top-level members are exactly `connector`, `credential_revision`, `fields`, `labels`,
      `plan_revision`, `selection`, `state`, `vendor` and `version`; v2 has no `apply`, `submission`,
      endpoint or tenant member. `credential_revision` is null exactly for `selection:null`; every
      selected held label has an always-present opaque nonzero 64-lowerhex head independent of
      credential presence, including when its complete credential set is absent. It is excluded
      from static plan/target preimages and never encodes a presence bit, count, generation or time.
      Every field requires the design's exact aliases, also-binds, authority, binds, choices, help,
      identity, input, label, name, provenance, reason, required/routable/secret/set, service and
      target members, with explicit null/empty forms. `set` is exactly null for every `secret:true`
      field regardless of stored presence; for every non-secret field it is the closed boolean live
      fact. Aggregate state requires required routability and required non-secret `set:true` only,
      and never reads or derives secret presence. A target is exactly id plus target revision;
      authority is one of the four closed state/revision/action objects. The authenticated human
      `GET /api/connections/{connector}/plan?version=exchange.connection-plan.v2[&name=<Label>]`
      returns byte-for-byte the same canonical body for the same resolved owner and state snapshot;
      Service Accounts and onboarding browser capabilities cannot read it. Failing-first fixtures
      bind complete, incomplete, unselected and all authority-state positives, including selected
      empty-credential and present-credential plans with identical secret nulls/state, full revision input/preimage bytes,
      the `0x0008` frame and identical HTTP body, then reject every member/type/null/bound/order,
      digest/preimage, static reason, state/target/authority/credential-head and secret/endpoint
      mutation enumerated by the design; changing an unroutable reason must change PlanRevision,
      while changing only secret presence must not change `set` or aggregate state. Secret booleans,
      non-secret nulls and presence-derived state are contradiction fixtures.
- [ ] Each native FXLM connection or hosted WebSocket carries one exact logical operation. Plan is
      native client `PLAN_QUERY`, server `PLAN_RESPONSE|ERROR`. Connect is
      client `BEGIN`, server `RECEIPT|ERROR` or server `NEED_SECRETS`, client one `SECRET` for each
      requested ordinal in request order, client `COMMIT`, server `RECEIPT|ERROR`; a separate client
      `QUERY` yields server `RECEIPT|ERROR`. Grant preview is client `PREVIEW`, server
      `CANDIDATE|ERROR`; grant apply is client `APPLY`, server `RECEIPT|ERROR`; a separate client
      `QUERY` yields server `RECEIPT|ERROR`. Native Service Account mint is client `MINT` plus its
      transferred writer capability, server `RECEIPT|ERROR`; a separate native client `QUERY` yields
      server `RECEIPT|ERROR`. Credential rotate/acquire is client `BEGIN`, server
      `NEED_SECRETS|ERROR`, client one
      `SECRET` for each requested ordinal in request order, client `COMMIT`, server `RECEIPT|ERROR`;
      a separate native connection or WebSocket carrying client `QUERY` yields server
      `RECEIPT|ERROR`. Connect, grant and credential query opcodes are native and hosted; plan query
      is native-only; every Service Account opcode is native-only. Hosted mint remains HTTP FXSA and
      the WebSocket has no generic writer capability. The design's opcode-by-transport matrix is
      exhaustive; a known opcode in a prohibited transport is `unexpected_frame`. A query names
      exactly the receipt id; response loss before receipt-id delivery uses byte-identical proposal
      replay, never a client-manufactured query key. No other direction, repetition, omission or
      transition is valid. `0x0001` creates only: an unheld label starts create, a held label returns
      replay only for its byte-identical durable proposal, a changed/unrecorded proposal is
      `proposal_conflict/409/refresh/none`, an active same-digest unresolved transaction is
      `connect_busy/409/refresh/none`, and an active different digest remains conflict, all before
      transaction allocation/prompt. `0x0030` requires a
      held label or returns `unknown_label/404/refresh/none`. Target closure is derived only from the
      current X-125 `TargetSpec`: `connection.name` first, first byte-identical shared target from
      config declaration order, then remaining auth declaration order. Every target is exactly one
      of connection-name, setting, typed custom-origin authority or credential. Connect includes
      connection.name, every required routable target and exactly the optional targets selected by
      their PlanTarget; setting/authority values, authority revisions and requested secret ordinals
      are exact ordered projections, and shared targets occur once. Credential acquire/rotate names
      the complete credential partition in plan order and no name/setting/authority target. Acquire
      requires that complete set absent; rotate requires it present; mixed/opposite state is only
      `credential_state_conflict/409/refresh/none` without a target, count or presence fact.
      Invented, required-omitted, extra unpaired optional, duplicate, reordered, cross-partition or
      one-revision-changed targets refuse before allocation/prompt under the linked exact mapping.
      Credential BEGIN additionally carries the selected plan's exact `credential_revision`.
      Terminal same-proposal replay is checked before current plan/head/action validation; otherwise
      a head mismatch is `stale_credential_revision/409/refresh/none`. A committed credential
      mutation atomically publishes a new unique opaque head, so successive rotations have different
      BEGIN/digest bytes while byte-identical replay of an older committed proposal remains
      idempotent. Every new or zero-secret connection initializes a head. Legacy held labels receive
      heads in one exclusive atomic migration independent of presence; crash before publish retries,
      while missing/reset/corrupt state after the migration marker refuses without regeneration.
      For zero secret needs the server still
      allocates and prepares an empty
      C-515 batch and sends `NEED_SECRETS` with the transaction/digest and `secrets:[]`; the helper
      sends COMMIT directly without a prompt. For `N>0`, all `1..N` SECRET frames precede COMMIT.
      A SECRET at `N=0`, early/skipped COMMIT at `N>0`, an empty raw secret or a direct server
      decision is `unexpected_frame`/the applicable framing refusal before decision. Closed control
      objects bind the canonical proposal, opaque 256-bit
      transaction/receipt ids and ordered ordinal/target pairs. Provider fixtures publish
      `exchange.connect-receipt.v1`, `exchange.grant-apply-receipt.v1`,
      `exchange.service-account-mint-receipt.v1` and `exchange.local-management-error.v1` with the
      closed error codes `invalid_frame`, `unsupported_version`, `wrong_direction`,
      `unexpected_frame`, `frame_too_large`, `truncated_frame`, `surplus_data`,
      `deadline_exceeded`, `peer_unverified`, `unsafe_root`, `local_management_unavailable`,
      `invalid_request`, `unknown_connector`, `unknown_label`,
      `invalid_label`, `secret_json_forbidden`, `unknown_target`, `stale_plan`,
      `stale_credential_revision`, `credential_state_conflict`, `proposal_conflict`,
      `connect_busy`, `grant_unexpressible`, `grant_stale`, `grant_digest_mismatch`,
      `service_account_conflict`,
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
- [ ] Hosted Exchange has its transaction coordinator allocate and associate both components of the
      server-owned opaque transaction id only after an
      admitted `BEGIN`, returns ordered `NEED_SECRETS`, then accepts exactly those `SECRET` ordinals
      before `COMMIT`. No transaction or receipt id appears in a URL, header or log. Connect, rotate
      and password acquisition use the same coordinator and interactive state machine as native
      FXLM. Query and same-proposal replay each use a separate WebSocket; replay may return the
      existing receipt before a prompt, while a changed proposal refuses. A successful server result
      closes 1000. The error-code exceptions are exact: `invalid_frame`, `unsupported_version`,
      `wrong_direction`, `unexpected_frame`, `truncated_frame` and `surplus_data` close 1002;
      `frame_too_large` and every message/control/secret/count/cumulative bound excess close 1009;
      text closes 1003 without an FXLM frame or JSON decoding; and `deadline_exceeded` closes 1008.
      Every other well-formed pre- or post-decision FXLM error closes 1000. The category's canonical
      error precedes the close when safe; otherwise only that close is sent, and every reason is
      empty. The absolute pre-decision deadline is
      exactly 300 monotonic seconds from hosted slot reservation before `101`, or native peer
      authentication before the first header; traffic never resets it and expiry is the distinct
      `deadline_exceeded/408/refresh/none` outcome plus hosted close 1008, never
      `unexpected_frame`. Five minutes covers bounded TTY/browser entry while the 4-tenant/32-process
      occupancy counters bound idle peers. After decision no human input remains and there is a
      separate exact 30-second response budget: expiry returns `store_unavailable` while
      provider/store outcome is unresolved, `audit_unavailable` while audit is unresolved, or
      `internal_refusal` for any other incomplete roll-forward, always with the fixed
      `query_receipt/same_proposal` tuple; it releases the transport/slot and leaves recovery
      rolling forward. Close reasons are always empty and neither deadline is configurable. Before a durable decision,
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
- [ ] The verified `flux-exchange` helper—not Flux—owns the native secret-bearing FXLM connection and
      local TTY/browser input. Flux writes exactly one canonical non-secret `0x0001` or `0x0030`
      initiating frame then EOF to the inherited request capability. The helper parses it and opens
      owner-authenticated native connection 1 solely for `PLAN_QUERY -> PLAN_RESPONSE|ERROR`; it
      closes connection 1 at terminal after validating connector/label, every plan/target revision,
      exact opcode-specific target closure/partition/order, authority revision, every secret
      `set:null`, non-secret setting target/value and credential-head null/value rule. For `0x0001`
      that query has `selection:null` and requires `credential_revision:null`; the proposed label
      remains only in BEGIN, and a held label proceeds for the
      server's replay/conflict decision. For `0x0030` it selects exactly `BEGIN.label`, so an unknown
      label is terminal. It requires both current selected head and `BEGIN.credential_revision` to
      have the exact nonzero 64-lowerhex grammar but does not require equality: an old byte-identical
      replay must reach the server's terminal lookup before current-head validation. Only then it
      opens distinct
      owner-authenticated native connection 2 and forwards the byte-identical BEGIN as that
      connection's sole operation, receives `NEED_SECRETS`, privately reads and sends every ordered
      `SECRET`/`COMMIT`, and owns the ceremony through its terminal result. It writes exactly one
      value-free `0x0006`, `0x0032` or `0x7fff` frame then EOF to the distinct result capability.
      Flux never receives the transaction id, secret ordinals/targets or any secret-derived fact and
      never supplies, chooses, parses or orders either Exchange-coordinator-generated component of
      provider transaction identity. Non-secret settings
      remain inside that one atomic BEGIN rather than being prewritten. This is a testable
      software/dataflow invariant, not OS isolation against a malicious same-user debugger.
      With `secrets:[]` the helper opens no TTY/browser, emits no SECRET and sends COMMIT directly;
      Flux's request/result view remains identical and value-free.
- [ ] The exact helper grammar and capability ABI are provider-fixtured and closed. Linux vendor input
      is `flux-exchange local vendor-secret` with no arguments: request-read FD 6 and terminal-
      response-write FD 7 are distinct anonymous pipes, FD 5 is closed/reserved for FXSA, every other
      FD at or above 3 is closed, and the helper sets `FD_CLOEXEC` on 6/7. Each pipe is capped at one 65,548-byte FXLM frame
      plus EOF. Request completion is bounded at 5 seconds from spawn. Endpoint discovery/pinning,
      both native connect/auth handshakes, complete connection-1 plan query/response/close and
      revalidation, and readiness to write BEGIN on connection 2 are bounded together at 5 seconds
      from request EOF. Connection 1's generic server deadline is bounded earlier by that helper
      cap and has no post-decision phase. Connection 2 alone owns the following 300-second server
      pre-decision and, after its durable decision, 30-second post-decision budgets. Flux result
      completion is therefore exactly bounded at `5 + 300 + 30 = 335` seconds from request EOF;
      traffic resets no budget.
      Service Account mint is exactly
      `flux-exchange local service-account-mint --id <id> --expires-at <canonical-decimal>
      --writer-fd 5`. No endpoint, tenant, credential address, program, cwd or arbitrary argument is
      caller-selectable; the helper rederives the owner-private root/endpoint. Standard streams are
      null and the helper opens `/dev/tty` directly. Stdout/stderr are empty;
      exit 0 means one terminal receipt/error was written and closed, while exit 1 is the sole
      value-free capability/transport failure. No other status exists.
- [ ] Supervised local connect uses one Exchange-server transaction coordinator, not the current plan
      handler's separate writes, `partial` outcome or an Exchange-owned credential database. It
      registry-resolves released `codewandler-connector-secrets` 0.20.0 with its crates.io checksum
      and consumes C-515 through `Arc<dyn PreparedSecretStore>`. The only provider operations are
      `prepare`, `state`, `commit`, `abort` and `reclaim`; Exchange cannot emulate them with point
      writes. Prepare durably stages the checked `SecretBatch`, including a valid empty batch for a
      credential-free settings-only create, invisibly inside
      `connector-secrets`; public state is only `Absent|Prepared|Committed`. Same-id/same-digest
      prepare returns its existing prepared or committed state without inspecting the supplied
      batch; a different digest refuses. Repeated commit of committed state and repeated abort of an
      aborted tombstone are idempotent; abort of committed returns `AlreadyCommitted`, and commit or
      prepare after abort returns `TransactionIdReused`. Aborted ids remain internal terminal
      tombstones reported as absent, and reclaimed ids return `Retired`. Exchange never inspects
      crate-private mutations, staged paths or values and never persists a credential byte.
- [ ] The Exchange transaction coordinator durably allocates both components of every provider
      transaction id in its value-free journal root: one non-zero generation followed by its unique
      192-bit nonce. C-515 validates/encodes the identity and owns provider transaction state but
      generates neither component; the resulting 256 bits are opaque outside its API. Generation
      zero, wrap and nonce reuse refuse. Exchange acknowledges
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
- [ ] Proposal digest bytes are closed. Connect hashes UTF-8
      `exchange.local-management.v1.connect-proposal`, one `0x00`, then RFC 8785 of exactly
      `{"authorities":[...],"connector":...,"label":...,"plan_revision":...,"settings":[...],"targets":[...]}`.
      Credential acquire/rotate hashes UTF-8
      `exchange.local-management.v1.credential-proposal`, one `0x00`, then RFC 8785 of exactly
      `{"action":...,"connector":...,"credential_revision":...,"label":...,"plan_revision":...,"targets":[...]}`.
      The result
      is lowerhex SHA-256. There is no wrapper, extra separator, length, newline or integer
      substitution; arrays retain control-object order. These exact BEGIN objects exclude secret
      bytes, lengths, hashes, presence facts and fingerprints. Exchange writes a value-free
      coordinator journal, prepares
      the secret transaction, then durably records one commit decision. Without that decision,
      recovery aborts staging and removes the journal. With it, recovery idempotently commits the
      secret batch, rolls forward label/instance/settings/authority and the initialized/next opaque
      credential head, drains audit, publishes the connection head/receipt and closes the journal.
      Relevant readers/mutations are gated while
      unresolved, and startup recovers before readiness/routes. A pre-decision crash exposes none;
      a post-decision crash converges to one complete result; query/replay never prompts or writes a
      second time. A changed proposal refuses naming only connector and label.
- [ ] The journal's value-free audit outbox uses one stable transaction-derived event id.
      `AuditJournal` append is idempotent by that id, restart drains every committed outbox entry
      before readiness or route service, and a committed receipt is not returned until canonical
      audit delivery succeeds. An unavailable audit sink after the decision or a lost response
      returns `query_receipt`; retry drains/deduplicates and returns the same receipt. No committed
      connection can become visible or replayable without one queryable canonical audit event.
- [ ] For Service Account mint, the Linux helper receives only write FD 5 and transfers exactly that
      write-only pipe to the authenticated running server through one `SCM_RIGHTS` message. Exchange
      rejects `MSG_CTRUNC`, multiple descriptors and the wrong pipe kind/direction and sets
      `FD_CLOEXEC`. Missing, multiple, truncated-control, wrong-kind/direction and unrelated planted
      descriptors refuse and close every received capability before mutation. No descriptor number
      enters FXLM JSON, receipts, persistence, audit or logs.
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
      and-connector scoped metadata selectors with no label or operation-id authority axis. The
      candidate round-trips current `Grant` exactly when expressible: parent `connector`; all three
      proposed selector axes; and every held inbound binding/event, reconstructing each redundant
      `InboundGrant.connector` from the parent. The selected stored `Grant.inbound` Vec order is
      preserved exactly rather than lexically sorted; each typed BTreeSet event collection retains
      its canonical lexical order. A nonlexical unique selected inbound fixture round-trips through
      PREVIEW/APPLY/replay/QUERY without reordering. Zero selected-connector grants creates an empty-inbound
      candidate; exactly one may be projected; duplicate selected-connector grants are never picked
      or merged. Duplicates, nonempty current `allow_ids`/`deny_ids`, an invalid inbound declaration,
      empty events, duplicate inbound binding, 65 bindings, 257 events on one binding or any future
      unrepresented provider field refuse PREVIEW as `grant_unexpressible/409/operator/none`; refresh cannot make
      manual authority expressible, and nothing is dropped or defaulted. Typed whole-store
      reserialization preserves every unrelated grant's decoded vector position/multiplicity and
      connector/selector/inbound Vec order and values identically in canonical form—including
      unrelated duplicates, empty events, over-bound sets and legacy omitted-inbound-as-empty—not
      necessarily original whitespace, member order or omission spelling.
      The per-tenant high-water mark is stored atomically with the whole set, is restart-stable and
      never resets. Under exclusive mutation authority an unmarked legacy/absent tenant is atomically
      initialized at revision 1 before CANDIDATE without changing grants; a pre-publish crash retries
      revision 1, while missing/corrupt revision state after the migration marker refuses. Every
      successful whole-set mutation increments once. CANDIDATE/APPLY carry the precondition revision;
      the first, replayed and queried receipt carries the same post-commit revision. Whole set,
      increment and terminal receipt record commit atomically, so crash retry cannot double-increment.
      The grant digest hashes UTF-8 `exchange.local-management.v1.grant-proposal`, one `0x00`, then
      RFC 8785 of exactly `{"candidate":GrantCandidate,"revision":StoreRevision}`, where revision is
      the canonical decimal JSON string. There is no wrapper, extra separator, length, newline or
      decoded-u64 substitution. Changed candidate bytes are `grant_digest_mismatch` and a changed
      high-water mark is `grant_stale`.
- [ ] Positive/adversarial provider fixtures and tests cover native root poisoning, unsafe metadata,
      peer authentication, loopback TCP, every secret JSON path, connect crash/response-loss/replay/
      conflict, exact digest preimages, zero-generation/nonzero-nonce transaction ids, handoff
      framing/capability closure, split receipts, grant CAS/preservation, the complete plan-v2
      positive/adversarial corpus, every opcode/transport cell and the unchanged X-128 capability
      ABI. Vendor-helper fixtures prove Linux 6/7 with planted 3/4/5/8 FDs and an unrelated
      inheritable descriptor canary; one initiating/result frame plus EOF; swapped,
      truncated, surplus, second-frame and wrong-opcode cases; 4/5 and 334/335-second boundaries;
      empty stdout/stderr and exact 0/1 exits; and that transaction ids/ordinals never cross to Flux.
      Plan-validation fixtures cover new create and held-label same-proposal replay with null
      selection/credential revision, rotate/acquire with the exact held label and head, changed
      create conflict, and adversarial create-with-new-label selection plus credential-with-null or
      malformed head. Old committed replay with a now-stale well-formed head reaches connection 2
      and returns its receipt without prompting; an unknown stale-head proposal returns
      `stale_credential_revision` without prompting. Plan bytes prove every secret `set:null`, non-secret boolean set, aggregate-state
      independence from secret presence, and selected/unselected credential-revision null rules.
      Target fixtures bind the exact X-125 connection-name/setting/authority/credential partition,
      required and optional connect selection, complete acquire/rotate credential set, shared-target
      deduplication and every one-fact omission/extra/duplicate/order/partition/revision refusal.
      Head fixtures cover new/legacy initialization, migration crash boundaries, missing/reset/corrupt
      refusal, two successive rotations with different proposal digests, and replay of the first
      proposal after the head advances.
      A credential-free settings-only positive proves empty prepare, empty NEED_SECRETS, no prompt,
      direct COMMIT and ordinary metadata/audit commit; adversarial fixtures cover SECRET at `N=0`,
      skipped/early COMMIT at `N>0`, empty SECRET and a direct non-replay decision without COMMIT.
      Typed-store grant fixtures cover empty selected events, duplicate selected inbound binding and
      duplicate selected connector as exact `grant_unexpressible` rows, then place each shape on an
      unrelated connector and prove value-identical semantic preservation through canonical CAS. A
      nonlexical unique selected inbound Vec round-trips without sorting. Revision fixtures cover
      atomic legacy/empty revision-1 initialization, restart, both migration/commit crash boundaries,
      stale concurrent APPLY, missing/corrupt post-migration refusal and post-commit receipt revision.
      Hosted handshake evidence covers a
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
- [ ] Native CI executes the complete evidence on `ubuntu-24.04-arm`
      (`aarch64-unknown-linux-gnu`) and `ubuntu-24.04` (`x86_64-unknown-linux-gnu`). Every row runs
      root poisoning, owner endpoint/TCP adversary, real `/dev/tty` input, handoff closure plus an extra-capability canary, crash injection before and
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
      regenerated from the candidate commit, both native Linux jobs pass and the separately authorized
      public release verifier succeeds.

## Progress

- 2026-08-05: Flux-roadmap Decision 0012 at `dc907fab219d67f80bf08311ebdfdeb766f1e8d7`
  contracted the unpublished local Exchange runtime to the two Linux GNU targets. X-137 now owns
  removal of the non-Linux runtime/release paths; all platform-independent FXLM/FXSA, hosted,
  recovery, grant and secret-exclusion obligations remain open until the amended child sequence
  closes.
- 2026-08-04: X-134 remained blocked until C-515 0.20.0 published; that prerequisite is now
  satisfied and retained as the exact registry identity used by the in-progress child sequence.

- 2026-08-04T18:11:23+02:00 — The coordinator admitted X-134 for implementation after the accepted
  contract head `9dc414c76f231bd179358fd526019a16872a7be1` merged as
  `3b16bcb5b1c52984449118775125fe66da1686da` and Connectors `v0.20.0` published from exact commit
  `c764f5c3b8e745cc65e90a298b04851647b76778`. Tag-triggered crates.io run
  `30927493484` completed successfully, and API, sparse-index and downloaded-byte SHA-256 values
  agree for address `bdee7fb0d488de4ed97dbd3b8414e04138c122ee36b6f9c97a174bb317913d8c`,
  catalog `9a7737659b74876b09ff6e09b253402c5bdfcafcbde89373cb76f689bd8ffed2`,
  secrets `edf98bece86f6364aba3e7dd48c3b7e161146942e9e8450d5dc286143b627717` and pack
  `8e858a844dab8324d42bb83c98c4ffb6823681eb1157ddb96a79d5d7a42cff48`; all four are unyanked.
  The public release is `https://github.com/codewandler/flux-connectors/releases/tag/v0.20.0`.
  X-134 is ready; every Acceptance item remains open for failing-first implementation evidence.
- 2026-08-04T17:17:52+02:00 — Reconciled the frozen provider contract to roadmap Decision 0007 at
  `4511f44b4defcb6de92ab8fc1b56bd5b4356ca78` after the final byte audit: secret live presence is
  always null; selected plans and credential proposals carry an opaque credential-head CAS;
  X-125-derived target partitions are exhaustive; transaction identity ownership, selected inbound
  order and durable grant revision migration are closed. X-134 remains blocked and every Acceptance
  item remains open pending independent re-audit.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0007-local-onboarding-uses-owner-bound-capabilities.md` at roadmap
  commit `4511f44b4defcb6de92ab8fc1b56bd5b4356ca78`.
- External implementation dependency satisfied by Connectors `v0.20.0`: implementation must still
  resolve `codewandler-connector-secrets` 0.20.0 and its transitive address crate from crates.io
  with the checksums recorded above. A path/git dependency, copied batch representation or
  unmatched lockfile resolution remains a contract failure.
- This story owns provider bytes and fixtures. Flux C-509 owns the receiving credential writer,
  owner-only Flux store, opaque resolver, management/runtime client split, CLI projection, concrete
  write approval and retry suppression. Exchange fixtures may use a test sink but never certify
  Flux's production receiver store.
- The local-management endpoint is not C-510's lifecycle control channel and never carries start,
  status, stop, readiness or liveness frames. C-510 remains secret-free.
- Existing hosted authentication stays authoritative for hosted Exchange. OS-owner bootstrap is
  deliberately unavailable outside the supervised single-user composition.
