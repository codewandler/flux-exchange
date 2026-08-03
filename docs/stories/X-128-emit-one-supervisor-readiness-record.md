---
id: X-128
title: "Emit one trusted readiness record for a supervised Exchange"
status: ready
priority: 0
epic: remote-deployment
areas: [exchange-server, lifecycle, linux, macos, protocol, windows]
design: docs/designs/local-release-v1.md
note: "Milestone 1 — Flux owns a child only after that exact process reports its bound address and compiled identity over a dedicated one-shot channel"
---

# Emit one trusted readiness record for a supervised Exchange

## Goal

Give Flux's local supervisor one versioned, bounded proof that the exact Exchange child it spawned
has bound its loopback listener and is compatible. The proof travels over a dedicated inherited
one-shot pipe or handle; neither a PID nor an HTTP health response can substitute for it.

## Why this is Milestone 1 work

`flux exchange local start` is short-lived, while the Exchange process has to survive it. Decision
0004 assigns lifetime ownership to a detached Flux supervisor and requires that supervisor to
commit ownership only after the child identifies itself. Polling `/health` can answer from a foreign
listener, and a PID can be stale or reused. Mixing readiness with stdout also lets ordinary logs
become lifecycle protocol by accident.

Exchange therefore owns a narrow supervised launch mode and readiness writer. Flux C-510 owns the
supervisor, authenticated control channel and later start/status/stop behavior.

## Acceptance

- [ ] The server has one documented supervised launch mode distinct from ordinary interactive and
      deployment startup. It requires a loopback bind with port `0`, lets the operating system
      choose the port, and refuses a non-loopback or preselected nonzero port before listening. The
      published address is read from the bound socket; there is no reserve-then-rebind port race.
- [ ] The inherited-handle discovery ABI is exact. Unix runs only `--supervised`: readiness is the
      write-only pipe at FD 3, liveness is the read-only pipe at FD 4, and every other nonstandard FD
      is closed. Windows runs `--supervised` with exactly
      `--supervisor-readiness-handle <H>` and `--supervisor-liveness-handle <H>`, where the two
      distinct nonzero HANDLE values are canonical decimal `usize` strings and the parent supplies
      exactly them through
      `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. Exchange validates the pipe kind/usable direction and
      refuses absent, duplicate, malformed, non-inherited or wrong-kind capabilities. No reserved
      `STARTUPINFO` field, caller path/shared named pipe, env/stdin/stdout discovery or generic
      arbitrary handle/FD option exists. The numeric Windows handles are the sole non-secret argv
      exception; no control/vendor/Service Account value is one.
- [ ] After every required store and safety check succeeds and the socket has bound, Exchange writes
      exactly one UTF-8 JSON record of at most 16 KiB and closes the channel. The record has schema
      identity `exchange.supervisor-ready.v1` and the exact provider-owned shape in
      `docs/designs/local-release-v1.md`: actual loopback socket address, OS process-start identity,
      release identity and executable digest, plus the six exact protocol fields shared by channel,
      manifest and compatibility output. EOF before one complete record, a second record, trailing
      bytes, an unknown field or a record over the bound is a refusal for the supervisor.
- [ ] The process identity contains both the PID as a diagnostic and an OS-derived start identity
      that distinguishes PID reuse. The readiness contract never claims a PID alone proves
      ownership, and Exchange emits no PID file for Flux to trust. `pid` is `1..=u32::MAX`; bind host
      is the literal `127.0.0.1` or `::1` and port is `1..=65535`. The closed tagged identities are
      exactly Linux `{kind:"linux-proc-start",boot_id,ticks}`, macOS
      `{kind:"macos-proc-start",seconds,microseconds}` and Windows
      `{kind:"windows-process-creation",filetime}`, with native sources, decimal encodings and bounds
      from the design. Native Linux, macOS and Windows tests compare the record with the already-open
      child handle using the same source.
- [ ] The release/build and protocol portion is produced from the same typed source as
      `flux-exchange compatibility --json` and agrees exactly for Exchange API, effective-catalogue,
      invoke request, invoke response, connection plan and supervisor versions. Their serialized keys
      are exactly `exchange_api`, `effective_catalogue_response`, `invoke_request`, `invoke_response`,
      `connection_plan` and `supervisor`. Supervised startup does not infer a protocol from the
      package version, and the side-effect-free compatibility command still binds no listener and
      opens no store.
- [ ] The one-shot channel carries no log line, progress event, HTTP byte or later control traffic.
      Application stdout/stderr remain ordinary log streams, the bound socket carries application
      traffic, and Flux C-510's owner-only control channel is separate. Closing or losing the
      readiness reader cannot redirect the record to either stream.
- [ ] A separate inherited liveness pipe makes Exchange cooperate with owner death on every target.
      The supervisor retains the write end and writes nothing; before any store/listener work,
      Exchange starts a native thread blocking on the read end. EOF, any byte or read error invokes
      immediate non-unwinding process exit. Supervisor normal exit or `SIGKILL` on Linux/macOS, and
      supervisor normal exit or `TerminateProcess` on Windows, therefore cannot leave Exchange
      running even when Tokio is wedged. Liveness descriptors never reach connector children and
      carry no readiness/control payload.
- [ ] No credential, setting, grant body, Service Account token/verifier, session value or control
      credential is serializable in the readiness type. A sentinel test drives success and every
      startup refusal and proves the readiness bytes and captured stdout/stderr contain none of
      them.
- [ ] **Failing-first process tests:** a successful child emits one record only after bind; a store
      refusal or bind failure emits none; a foreign process answering `/health`, a planted PID file
      and a reused PID cannot satisfy the parent-side fixture; corrupt schema, address,
      process-start, release or protocol identity is rejected before lifecycle ownership is
      committed. Wrong/aliased Unix FDs, each malformed Windows handle flag, handles outside the
      explicit list, stdout readiness and every start-identity tag/encoding/domain mutation fail.
- [ ] Native Linux, macOS and Windows tests spawn the real server through the exact inherited ABI,
      validate the one-shot record against `compatibility --json`, connect to its reported address,
      then use `SIGKILL` for the Unix supervisor or `TerminateProcess` for the Windows supervisor and
      prove the Exchange process/port disappear within a bounded deadline. A fixture wedges the
      async runtime while the native liveness thread still exits.
      No test treats `/health` as readiness, though health remains independent liveness after
      ownership exists.

## Progress

- 2026-08-04: Filed from the supervision amendment to cross-repository Decision 0004. The audit
  separated Exchange's launch/readiness protocol from Flux C-510's process supervisor and control
  channel.
- 2026-08-04: Reconciled the exact readiness record with X-126's provider-owned local-release v1
  contract after Flux C-510's independently written shape diverged. Exchange owns the fixture;
  channel, manifest, compatibility and readiness now use the same six protocol keys.
- 2026-08-04: The implementation audit replaced generic "pipe or handle" prose with fixed Unix FDs,
  an explicit Windows inherited HANDLE-list ABI, three closed native process-start identities and a
  provider-owned liveness pipe/thread so supervisor death cannot orphan Exchange on macOS or any
  other supported platform.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0004-flux-manages-a-verified-local-exchange.md` at `013a2ab`.
- X-126 depends on this story so the released executable and signed manifest carry the same
  compatibility identity the supervisor validates. Flux C-510 consumes the record and owns the
  long-lived supervisor; this story does not add a downloader, daemon manager or stop command.
- `/health` remains useful after identity is established. It is deliberately not process identity
  and cannot select which process Flux owns.
